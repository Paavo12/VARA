-- RideCheck (PRD S-3, App Flow §6.2). The worker watches every trip in progress:
--   - off the planned route by more than 150 m (two samples in a row, against `planned_route`);
--   - stopped for more than 2 minutes away from the destination, or no position from the car
--     for 2 minutes (a phone switched off mid-trip looks the same to the rider's contacts).
-- The rider gets "Are you okay?". "I'm fine" closes it; "Call for help" raises an SOS; no answer
-- in 60 s opens a ridecheck_* incident for the safety desk. The driver is never told.

-- The road route from pickup to destination, filled by the worker when the trip starts
-- (straight line with the local routing estimate).
ALTER TABLE rides ADD COLUMN planned_route geography(LineString, 4326);

CREATE TABLE ride_checks (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  ride_id     uuid NOT NULL REFERENCES rides(id) ON DELETE CASCADE,
  kind        text NOT NULL CHECK (kind IN ('deviation', 'stop')),
  location    geography(Point, 4326),        -- the car when the check was raised
  asked_at    timestamptz NOT NULL DEFAULT now(),
  answer      text CHECK (answer IN ('fine', 'help')),
  answered_at timestamptz,
  incident_id uuid REFERENCES safety_incidents(id),
  CHECK ((answer IS NULL) = (answered_at IS NULL))
);
-- At most one question waiting per ride.
CREATE UNIQUE INDEX one_open_ride_check ON ride_checks (ride_id) WHERE answered_at IS NULL AND incident_id IS NULL;
CREATE INDEX ON ride_checks (ride_id, asked_at);
