-- Service fee raised to 15 / 16.5 / 18 % (Classic / Comfort / Soft Life) so the platform covers
-- its per-trip costs (SMS, maps, payments) and fixed overhead (PRD §8). Fares are unchanged:
-- only the split between FUber and the driver moves.
--
-- New rows rather than updates: rides keep the `pricing_tier_id` they were quoted with, so
-- earlier trips still show the fee they were charged.
INSERT INTO pricing_tiers (tier, base_cents, per_km_cents, per_min_cents, minimum_cents, service_fee_bp, effective_from) VALUES
  ('classic',  1500,  800, 120, 3500, 1500, '2026-11-02T00:00:00Z'),
  ('comfort',  2000, 1000, 150, 4500, 1650, '2026-11-02T00:00:00Z'),
  ('softlife', 2800, 1300, 200, 6000, 1800, '2026-11-02T00:00:00Z');
