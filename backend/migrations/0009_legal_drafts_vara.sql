-- The product is now VARA, so the draft legal documents from docs/legal/ are published again under
-- the new name and the placeholder domain vara.example. content_sha256 is the SHA-256 of each
-- Markdown file as committed (LF line endings).
--
-- New rows rather than updates: consents keep pointing at the version the user accepted. These
-- are still DRAFTS awaiting Namibian counsel; see 0004.
INSERT INTO legal_documents (kind, version, url, content_sha256, published_at, requires_reacceptance) VALUES
  ('terms',   'draft-2026-11-05', 'https://vara.example/legal/terms',   '\xc55148f3d864c6da28c8bf9a8ef42be98b75f2579199c10872059f86d21bdf49', '2026-11-05T00:00:00Z', true),
  ('privacy', 'draft-2026-11-05', 'https://vara.example/legal/privacy', '\xb49b2a5ef125c080479fc810ec681f2dc24f482571a91e320e065ab37fcc174e', '2026-11-05T00:00:00Z', true),
  -- Informational only: the cookie policy is published but never needs acceptance.
  ('cookies', 'draft-2026-11-05', 'https://vara.example/legal/cookies', '\x319913bb7fd6cb5405903a74bacfaaf829c13688cb947f835dae3d49cf2cbadf', '2026-11-05T00:00:00Z', false);
