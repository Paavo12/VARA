-- Map Experts (stakeholder decision 2026-11-04, profitability analysis 2026-11-19): drivers who
-- work unmapped or newly developed zones (e.g. Katutura) and help verify wayfinding pay a flat
-- 15 % service fee on every tier, so a Comfort (16.5 %) or Soft Life (18 %) trip costs her less.
-- An admin grants the status until a date; it lapses on its own. The fee is set when she accepts
-- (and shown on the offer), so a ride that moves to another driver gets that driver's fee.
ALTER TABLE driver_profiles ADD COLUMN map_expert_until timestamptz;
