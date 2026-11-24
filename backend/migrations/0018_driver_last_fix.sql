-- Plausible driver positions (internal security review 2026-11-24). The exact pickup unlocks
-- when the driver's reported position is within 50 m of it, and that position comes from her
-- phone. A position no car could have reached since her last accepted one is ignored. The last
-- accepted position is kept here rather than in driver_presence, which is deleted when she goes
-- offline, so going offline and back online can't be used to jump.
ALTER TABLE driver_profiles
  ADD COLUMN last_fix    geography(Point, 4326),
  ADD COLUMN last_fix_at timestamptz;
