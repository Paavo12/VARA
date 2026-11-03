-- Phase 4: cash confirmation, ratings, trip history and earnings. Documented in
-- docs/05-backend-schema.md ("Phase 4 schema changes"). Numbered 0008: 0007 is the service-fee
-- change on its own branch.

-- The driver may skip rating the passenger (App Flow §8, "Back to requests"); the ride is RATED
-- once the rider has rated and the driver has rated or skipped.
ALTER TABLE rides ADD COLUMN driver_rating_skipped boolean NOT NULL DEFAULT false;

-- A trip's ledger entries are booked once, however often "Cash received" is tapped.
CREATE UNIQUE INDEX driver_ledger_once_per_ride ON driver_ledger (ride_id, kind) WHERE ride_id IS NOT NULL;

-- Trip history, newest first.
CREATE INDEX rides_rider_history ON rides (rider_id, requested_at DESC);
CREATE INDEX rides_driver_history ON rides (driver_id, requested_at DESC) WHERE driver_id IS NOT NULL;
