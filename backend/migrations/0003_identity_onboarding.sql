-- Phase 1: identity and onboarding. Documented in docs/05-backend-schema.md §3, §4 and §9.

-- Accounts are created at the first successful OTP sign-in, before the person has entered
-- their details; names are filled in by PATCH /me. Erasing a deleted account (30-day grace)
-- clears the phone number so the number can sign up again.
ALTER TABLE users ALTER COLUMN phone_e164 DROP NOT NULL;
ALTER TABLE users ALTER COLUMN full_name DROP NOT NULL;
ALTER TABLE users ALTER COLUMN display_name DROP NOT NULL;
ALTER TABLE users ADD CONSTRAINT users_live_have_phone CHECK (phone_e164 IS NOT NULL OR status = 'deleted');

-- Refresh-token rotation keeps one row per issued token. The reason tells reuse of a rotated
-- token (revoke the family) apart from an ordinary sign-out.
ALTER TABLE sessions ADD COLUMN revoked_reason text
  CHECK (revoked_reason IN ('rotated', 'logout', 'reuse_detected', 'signed_in_elsewhere', 'account_deleted', 'suspended'));
ALTER TABLE sessions ADD CONSTRAINT sessions_revoked_has_reason CHECK ((revoked_at IS NULL) = (revoked_reason IS NULL));
ALTER TABLE sessions ADD COLUMN app_version text CHECK (char_length(app_version) <= 40);
CREATE INDEX ON sessions (family_id);
ALTER TABLE sessions ADD CONSTRAINT sessions_device_name_len CHECK (char_length(device_name) <= 80);

-- Uploads: the client sends the bytes straight to object storage with a short-lived signed
-- URL; the worker then checks magic bytes, scans for viruses, re-encodes images (dropping
-- EXIF/GPS) and only then marks the upload clean (TRD §7.2).
CREATE TYPE upload_status AS ENUM ('awaiting_upload', 'processing', 'clean', 'rejected');

CREATE TABLE uploads (
  id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id        uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  purpose        document_kind NOT NULL,
  declared_type  text NOT NULL CHECK (declared_type IN ('image/jpeg', 'image/png', 'application/pdf')),
  declared_size  int NOT NULL CHECK (declared_size BETWEEN 1 AND 10485760),
  status         upload_status NOT NULL DEFAULT 'awaiting_upload',
  storage_key    text,                    -- the clean object, set when status = 'clean'
  mime_type      text,
  size_bytes     int,
  sha256         bytea,
  reject_reason  text,
  url_expires_at timestamptz NOT NULL,
  processed_at   timestamptz,
  created_at     timestamptz NOT NULL DEFAULT now(),
  CHECK ((status = 'clean') = (storage_key IS NOT NULL))
);
CREATE INDEX ON uploads (user_id, created_at DESC);

-- Identity verification references uploads; the clean keys are copied in once processed.
ALTER TABLE identity_verifications ALTER COLUMN id_document_key DROP NOT NULL;
ALTER TABLE identity_verifications ALTER COLUMN selfie_key DROP NOT NULL;
ALTER TABLE identity_verifications ADD COLUMN id_upload_id uuid REFERENCES uploads(id) ON DELETE SET NULL;
ALTER TABLE identity_verifications ADD COLUMN selfie_upload_id uuid REFERENCES uploads(id) ON DELETE SET NULL;
-- At most one verification waiting for review per person.
CREATE UNIQUE INDEX one_pending_verification ON identity_verifications (user_id) WHERE status = 'pending';

-- Driver documents are created when submitted and move from 'uploading' to 'in_review'
-- once their upload is clean.
ALTER TABLE driver_documents ALTER COLUMN storage_key DROP NOT NULL;
ALTER TABLE driver_documents ALTER COLUMN mime_type DROP NOT NULL;
ALTER TABLE driver_documents ALTER COLUMN size_bytes DROP NOT NULL;
ALTER TABLE driver_documents ALTER COLUMN sha256 DROP NOT NULL;
ALTER TABLE driver_documents ADD COLUMN upload_id uuid REFERENCES uploads(id) ON DELETE SET NULL;
ALTER TABLE driver_documents ADD CONSTRAINT driver_documents_reviewable_has_file
  CHECK (status IN ('uploading', 'rejected') OR storage_key IS NOT NULL);
-- Smallest expiry warning already sent (30, 7 or 1 days), so each is sent once.
ALTER TABLE driver_documents ADD COLUMN expiry_warned_days smallint CHECK (expiry_warned_days IN (30, 7, 1));
-- Replacing a document marks the old one superseded and inserts the new one in one
-- transaction; deferring the check lets the old row point at the new one first.
ALTER TABLE driver_documents ALTER CONSTRAINT driver_documents_superseded_by_fkey DEFERRABLE INITIALLY DEFERRED;
CREATE INDEX ON driver_documents (upload_id);

ALTER TABLE driver_profiles ADD COLUMN requested_tiers ride_tier[] NOT NULL DEFAULT '{}';
ALTER TABLE driver_profiles ADD COLUMN submitted_at timestamptz;
ALTER TABLE driver_profiles ADD COLUMN decision_reason text CHECK (char_length(decision_reason) <= 500);
CREATE INDEX ON driver_profiles (submitted_at) WHERE application_status = 'pending';

ALTER TABLE vehicles ADD COLUMN reject_reason text CHECK (char_length(reject_reason) <= 500);
ALTER TABLE vehicles ADD COLUMN reviewed_by uuid REFERENCES staff_users(id);
ALTER TABLE vehicles ADD COLUMN reviewed_at timestamptz;

-- Staff sign-in (bearer token until the ops console moves to cookies in Phase 5).
ALTER TABLE staff_users ADD COLUMN totp_last_step bigint;   -- blocks replay of a used TOTP code
CREATE TABLE staff_sessions (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  staff_id    uuid NOT NULL REFERENCES staff_users(id) ON DELETE CASCADE,
  token_hash  bytea NOT NULL UNIQUE,
  ip          inet,
  expires_at  timestamptz NOT NULL,
  revoked_at  timestamptz,
  created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON staff_sessions (staff_id) WHERE revoked_at IS NULL;

-- Job queue: unique keys stop the same scheduled job (e.g. one account erasure) being queued twice.
ALTER TABLE jobs ADD COLUMN dedupe_key text;
CREATE UNIQUE INDEX jobs_dedupe ON jobs (dedupe_key) WHERE done_at IS NULL AND dedupe_key IS NOT NULL;
