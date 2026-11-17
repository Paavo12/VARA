-- Weekly settlement (PRD P-6). Every Monday the worker closes the week before (Monday to
-- Sunday, Windhoek time) for each driver: the ledger entries not yet settled become one
-- `payouts` row, positive when VARA owes her (card trips, with the payment provider), negative
-- when she owes VARA (service fees on cash trips). Until a payout provider exists, ops settles
-- each row by hand with a reference (bank transfer or deposit slip); that books a `payout`
-- ledger entry which brings the settled part of her balance back to zero.
ALTER TABLE payouts DROP CONSTRAINT payouts_status_check;
ALTER TABLE payouts ADD CONSTRAINT payouts_status_check CHECK (status IN ('pending', 'sent', 'received', 'failed'));
ALTER TABLE payouts ADD COLUMN settled_at timestamptz;
ALTER TABLE payouts ADD COLUMN settled_by uuid REFERENCES staff_users(id);
ALTER TABLE payouts ADD COLUMN reference text CHECK (char_length(reference) <= 100);
CREATE INDEX ON payouts (status, period_start) WHERE status = 'pending';
CREATE INDEX ON driver_ledger (driver_id) WHERE payout_id IS NULL;
