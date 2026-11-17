-- Share trip (PRD S-2). A link is tied to the driver who had the ride when it was made: if she
-- cancels and the ride goes to someone else, a link she shared stops working, so her contacts
-- never follow another driver's car. A rider's link follows the ride to the next driver.
-- Links expire 30 minutes after the ride ends; that is worked out from the ride, so
-- `expires_at` stays for links ended early by other means.
ALTER TABLE trip_shares ADD COLUMN driver_id uuid REFERENCES driver_profiles(user_id);
UPDATE trip_shares s SET driver_id = r.driver_id FROM rides r WHERE r.id = s.ride_id;
CREATE INDEX ON trip_shares (ride_id);
