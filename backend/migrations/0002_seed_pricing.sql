-- Launch rates (PRD §8), in cents. Service fee in basis points (200 = 2 %).
INSERT INTO pricing_tiers (tier, base_cents, per_km_cents, per_min_cents, minimum_cents, service_fee_bp, effective_from) VALUES
  ('classic',  1500,  800, 120, 3500, 200, '2026-01-01T00:00:00Z'),
  ('comfort',  2000, 1000, 150, 4500, 250, '2026-01-01T00:00:00Z'),
  ('softlife', 2800, 1300, 200, 6000, 300, '2026-01-01T00:00:00Z');

INSERT INTO settings (key, value) VALUES
  ('dispatch.radius_m',        '3000'),
  ('dispatch.offer_ms',        '15000'),
  ('dispatch.arrive_m',        '50'),
  ('safety.deviation_m',       '150'),
  ('safety.unexpected_stop_s', '120'),
  ('trip.free_wait_s',         '300');
