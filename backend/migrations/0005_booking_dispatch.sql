-- Phase 2: booking and dispatch. Documented in docs/05-backend-schema.md §5 and §6.

CREATE EXTENSION IF NOT EXISTS unaccent;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- ---------------------------------------------------------------------------------------------
-- Places search (PRD B-2): landmarks, suburbs and streets. Matching ignores case, accents and
-- punctuation, like the demo's `fold`. Seeded by 0006_seed_places.sql.

-- Lower case, accents removed, anything that is not a letter or digit collapsed to one space.
-- IMMUTABLE so it can back a generated column: the unaccent dictionary is named explicitly.
CREATE FUNCTION fold_text(t text) RETURNS text
  LANGUAGE sql IMMUTABLE PARALLEL SAFE STRICT
  RETURN trim(regexp_replace(lower(public.unaccent('public.unaccent'::regdictionary, t)), '[^a-z0-9]+', ' ', 'g'));

CREATE TYPE place_kind AS ENUM ('landmark', 'suburb', 'street');

CREATE TABLE places (
  id        uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  kind      place_kind NOT NULL,
  name      text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
  -- Second line in the results: "Shopping · CBD · 19 Mandume Ndemufayo Avenue", "Street · Khomasdal".
  detail    text NOT NULL CHECK (char_length(detail) <= 200),
  -- Extra words that find a landmark ("hospital", "shopping") without being part of its name.
  keywords  text NOT NULL DEFAULT '',
  location  geography(Point, 4326) NOT NULL,
  rank      smallint NOT NULL,                -- landmarks 0, suburbs 1, streets 2 (results order)
  search    text GENERATED ALWAYS AS (fold_text(name || ' ' || keywords)) STORED,
  active    boolean NOT NULL DEFAULT true,
  UNIQUE (kind, name)
);
CREATE INDEX places_search_trgm ON places USING gin (search gin_trgm_ops);
CREATE INDEX ON places USING gist (location);

-- ---------------------------------------------------------------------------------------------
-- Drivers online (PRD D-1). A row exists only while the driver is online, tied to the driver
-- session that went online: signing out (or signing in on another phone) revokes the session
-- and the matching query ignores the row, as in the demo where only a driver signed in on a
-- phone can receive requests. Positions arrive every 4 s over the WebSocket.
--
-- Kept in Postgres rather than Redis GEO (TRD §4.2) so going online, matching and accepting are
-- one consistent view; at launch scale (500 drivers × one write per 4 s) this is ~125 small
-- updates a second. The `presence` module is the only code that touches it, so it can move to
-- Redis if the load test says so.
CREATE TABLE driver_presence (
  driver_id    uuid PRIMARY KEY REFERENCES driver_profiles(user_id) ON DELETE CASCADE,
  session_id   uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  shift_id     uuid NOT NULL REFERENCES driver_shifts(id) ON DELETE CASCADE,
  vehicle_id   uuid NOT NULL REFERENCES vehicles(id),
  tiers        ride_tier[] NOT NULL CHECK (cardinality(tiers) > 0),
  wheelchair   boolean NOT NULL,
  location     geography(Point, 4326) NOT NULL,
  heading      smallint CHECK (heading BETWEEN 0 AND 359),
  speed_kmh    real CHECK (speed_kmh BETWEEN 0 AND 250),
  online_since timestamptz NOT NULL DEFAULT now(),
  updated_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON driver_presence USING gist (location);

-- One shift per online period; earnings per shift come in Phase 4.
ALTER TABLE driver_shifts ADD COLUMN end_reason text
  CHECK (end_reason IN ('went_offline', 'signed_out', 'stale', 'not_eligible'));

-- ---------------------------------------------------------------------------------------------
-- Rides

-- The rider app sends an Idempotency-Key with POST /rides, so a retried request never books twice.
ALTER TABLE rides ADD COLUMN idempotency_key text CHECK (char_length(idempotency_key) BETWEEN 8 AND 80);
CREATE UNIQUE INDEX rides_idempotency ON rides (rider_id, idempotency_key) WHERE idempotency_key IS NOT NULL;
-- Demand level at quote time: sets the matching weights for this ride.
ALTER TABLE rides ADD COLUMN demand_level text NOT NULL DEFAULT 'normal' CHECK (demand_level IN ('normal', 'high', 'peak'));
-- "Try again" after NO_DRIVERS starts a new round in which earlier drivers may be offered again.
ALTER TABLE rides ADD COLUMN search_round smallint NOT NULL DEFAULT 1;
ALTER TABLE rides ADD COLUMN search_started_at timestamptz;
-- Matching picks up rides waiting for an offer.
CREATE INDEX rides_searching ON rides (search_started_at) WHERE state = 'SEARCHING';

ALTER TABLE ride_offers ADD COLUMN round smallint NOT NULL DEFAULT 1;
ALTER TABLE ride_offers DROP CONSTRAINT ride_offers_ride_id_driver_id_key;
ALTER TABLE ride_offers ADD CONSTRAINT ride_offers_once_per_round UNIQUE (ride_id, round, driver_id);
-- A driver sees at most one request at a time.
CREATE UNIQUE INDEX one_pending_offer_per_driver ON ride_offers (driver_id) WHERE response = 'pending';
CREATE INDEX ride_offers_expiry ON ride_offers (expires_at) WHERE response = 'pending';
-- Rider's display rating and the driver's earning are frozen into the offer card.
ALTER TABLE ride_offers ADD COLUMN earning_cents bigint NOT NULL DEFAULT 0;

-- ---------------------------------------------------------------------------------------------
-- Change notifications (TRD §5). Every ride transition and offer change is announced on
-- commit with pg_notify; each API instance listens and pushes to the WebSockets it holds.
-- Payloads carry IDs only: the API loads the role-specific view (never the destination for a
-- driver before PIN_VERIFIED) when it sends.

CREATE FUNCTION notify_ride_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE r rides%ROWTYPE;
BEGIN
  SELECT * INTO r FROM rides WHERE id = NEW.ride_id;
  PERFORM pg_notify('fuber_events', json_build_object(
    'kind', 'ride', 'ride_id', NEW.ride_id, 'rider_id', r.rider_id, 'driver_id', r.driver_id,
    -- The driver who just lost the ride (driver cancel) still needs to hear about it.
    'previous_driver_id', NEW.detail->>'previous_driver_id', 'state', NEW.to_state)::text);
  RETURN NULL;
END $$;
CREATE TRIGGER ride_events_notify AFTER INSERT ON ride_events FOR EACH ROW EXECUTE FUNCTION notify_ride_event();

CREATE FUNCTION notify_offer() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  PERFORM pg_notify('fuber_events', json_build_object(
    'kind', 'offer', 'offer_id', NEW.id, 'ride_id', NEW.ride_id, 'driver_id', NEW.driver_id, 'response', NEW.response)::text);
  RETURN NULL;
END $$;
CREATE TRIGGER ride_offers_notify AFTER INSERT OR UPDATE OF response ON ride_offers FOR EACH ROW EXECUTE FUNCTION notify_offer();

-- Matching parameters (PRD D-6), alongside dispatch.radius_m and dispatch.offer_ms from 0002.
INSERT INTO settings (key, value) VALUES
  ('dispatch.location_stale_s', '60'),    -- ignore drivers whose last position is older
  ('dispatch.max_candidates',   '25'),    -- nearest drivers considered per offer
  ('dispatch.fuzz_min_m',       '100'),   -- fuzzy pickup offset (TRD §4.4)
  ('dispatch.fuzz_max_m',       '200'),
  ('booking.quote_ttl_s',       '300'),
  ('booking.schedule_min_s',    '1800'),  -- PRD B-6: 30 min – 7 days ahead
  ('booking.schedule_max_s',    '604800'),
  ('booking.schedule_lead_s',   '900');   -- dispatch starts 15 min before pickup
