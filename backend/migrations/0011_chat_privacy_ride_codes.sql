-- The privacy draft now says the safety team can read a trip's in-app messages when a safety issue
-- is raised about it (each view is audited, see ops::incidents::messages). content_sha256 is the
-- SHA-256 of docs/legal/privacy-policy.md as committed (LF line endings). Still a DRAFT for
-- Namibian counsel; see 0004 and 0009. Re-acceptance because staff access to messages is new.
INSERT INTO legal_documents (kind, version, url, content_sha256, published_at, requires_reacceptance) VALUES
  ('privacy', 'draft-2026-11-09', 'https://vara.example/legal/privacy', '\x7b0104d78502c34063451be71d26d14584b09505c170e93e4b774806339e00fe', '2026-11-09T00:00:00Z', true);

-- Ride codes follow the VARA name. Existing rides keep their FU- codes; the sequence carries on,
-- so a VA- code never repeats the number of an FU- one.
ALTER TABLE rides ALTER COLUMN code SET DEFAULT ('VA-' || nextval('ride_code_seq'));
