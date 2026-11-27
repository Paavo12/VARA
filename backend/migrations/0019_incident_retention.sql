-- Safety incidents are kept 5 years after they close, then reviewed for deletion (schema §12,
-- Privacy Policy §7). An admin either keeps one for another year, with a reason, or releases
-- it: it stops holding its ride's data (the clean-up job then anonymises the ride and deletes
-- its track and chat), and its free text, notes and location are removed. Kind and dates stay,
-- so the safety statistics still count it.
ALTER TABLE safety_incidents
  ADD COLUMN retain_until timestamptz,   -- kept after a review until then
  ADD COLUMN released_at  timestamptz;

CREATE INDEX ON safety_incidents (closed_at) WHERE hold_data AND status = 'closed';
