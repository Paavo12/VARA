-- Draft legal documents from docs/legal/, so sign-up can record consent end to end in
-- development and staging. content_sha256 is the SHA-256 of each Markdown file as committed (LF line endings).
--
-- These are DRAFTS awaiting Namibian counsel (Implementation Plan §6, item 14). Before launch,
-- a new migration inserts the approved versions with their real dates; because they have a
-- newer published_at, every user is asked to accept them (PRD A-7). The Driver Agreement is
-- not drafted yet, so drivers currently accept the Terms and Privacy Policy only.
INSERT INTO legal_documents (kind, version, url, content_sha256, published_at, requires_reacceptance) VALUES
  ('terms',   'draft-2026-10-26', 'https://fuber.app/legal/terms',   '\x16a36d5bf2e8827beb4f4dfc42fdc8c5fbf69d26d13eb628cc6aff6e3d7fa0df', '2026-10-26T00:00:00Z', true),
  ('privacy', 'draft-2026-10-26', 'https://fuber.app/legal/privacy', '\x1461e6f8763b66a4003e271c18d6f51e4f999f673cb6ce7d408c58e54be582c4', '2026-10-26T00:00:00Z', true),
  -- Informational only: the cookie policy is published but never needs acceptance.
  ('cookies', 'draft-2026-10-26', 'https://fuber.app/legal/cookies', '\x7d6688972f76acafb3d1ec90d00d61945564d729fce5e76ab0e0fe88ed1c43a6', '2026-10-26T00:00:00Z', false);
