-- In-app chat (PRD T-8). A ride can pass to another driver when one cancels, so each message
-- records which driver's conversation it belongs to; the next driver never sees the earlier
-- one. `client_key` makes a resend after a lost response safe. Nothing has written to this
-- table before this migration, so the new required columns need no backfill.

ALTER TABLE ride_messages
  ADD COLUMN driver_id  uuid NOT NULL REFERENCES driver_profiles(user_id),
  ADD COLUMN client_key text NOT NULL,
  ADD CONSTRAINT one_message_per_client_key UNIQUE (ride_id, sender, client_key);

CREATE INDEX ride_messages_thread ON ride_messages (ride_id, driver_id, id);
CREATE INDEX ride_messages_sent_at ON ride_messages (sent_at);
