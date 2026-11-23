-- Kids Guardian (client decision 2026-11-20; on 2026-11-23 VARA carries no men, but women may
-- bring young children, baby boys included): a rider declares the children riding with her and
-- may ask for a child seat, which adds a flat surcharge (F_kids, setting
-- 'pricing.kids_surcharge_cents', N$0 until an admin sets it). The surcharge is locked with the
-- quote and goes to the driver in full. Children never ride alone: unaccompanied minors stay
-- disabled until legal and insurance sign-off, and the age rules wait for the same.
ALTER TABLE rides
  ADD COLUMN children smallint NOT NULL DEFAULT 0 CHECK (children BETWEEN 0 AND 3),
  ADD COLUMN child_seat boolean NOT NULL DEFAULT false,
  ADD COLUMN kids_surcharge_cents bigint NOT NULL DEFAULT 0 CHECK (kids_surcharge_cents >= 0),
  ADD CONSTRAINT rides_children_with_an_adult CHECK (children < passengers),
  ADD CONSTRAINT rides_child_seat_needs_a_child CHECK (NOT child_seat OR children > 0),
  ADD CONSTRAINT rides_kids_surcharge_only_with_seat CHECK (child_seat OR kids_surcharge_cents = 0);
