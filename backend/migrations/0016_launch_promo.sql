-- Launch promotion (client decision 2026-11-20): a 13 % service fee on every ride type, Map
-- Experts included, for 90 days from the official market launch. An admin sets the launch date
-- in the ops console (settings key 'pricing.launch_promo', value {"starts_at": <rfc3339>}); after
-- 90 days the 15–18 % tiers and the 15 % Map Expert fee apply again with no manual step. Like the
-- Map Expert fee, it is decided when a driver accepts, so the offer and the booked split agree.
CREATE FUNCTION launch_promo_active(at timestamptz DEFAULT now()) RETURNS boolean
LANGUAGE sql STABLE AS $$
  SELECT coalesce(
    (SELECT at >= (value->>'starts_at')::timestamptz
            AND at < (value->>'starts_at')::timestamptz + interval '90 days'
     FROM settings WHERE key = 'pricing.launch_promo'),
    false)
$$;
