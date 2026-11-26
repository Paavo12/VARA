//! Push notices (App Flow §10). They are queued as jobs in the same transaction as the change
//! they announce, so a notice goes out only if the change commits; the worker writes the text
//! and sends it to each of her signed-in devices.

use fuber_domain::RideState;
use serde_json::json;
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::jobs;

/// What happened. Ride notices go to the ride's rider; the others to `user_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    DriverAssigned,
    DriverArrived,
    TripStarted,
    TripCompleted,
    /// A scheduled ride has started looking for a driver (PRD B-6).
    ScheduledSearching,
    /// RideCheck is asking whether she is okay (PRD S-3).
    RideCheck,
    RiderVerified,
    RiderNotVerified,
    DriverApproved,
    DriverNotApproved,
}

impl Notice {
    pub const ALL: [Notice; 10] = [
        Notice::DriverAssigned,
        Notice::DriverArrived,
        Notice::TripStarted,
        Notice::TripCompleted,
        Notice::ScheduledSearching,
        Notice::RideCheck,
        Notice::RiderVerified,
        Notice::RiderNotVerified,
        Notice::DriverApproved,
        Notice::DriverNotApproved,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Notice::DriverAssigned => "driver_assigned",
            Notice::DriverArrived => "driver_arrived",
            Notice::TripStarted => "trip_started",
            Notice::TripCompleted => "trip_completed",
            Notice::ScheduledSearching => "scheduled_searching",
            Notice::RideCheck => "ridecheck",
            Notice::RiderVerified => "rider_verified",
            Notice::RiderNotVerified => "rider_not_verified",
            Notice::DriverApproved => "driver_approved",
            Notice::DriverNotApproved => "driver_not_approved",
        }
    }

    pub fn parse(s: &str) -> Option<Notice> {
        Notice::ALL.into_iter().find(|n| n.as_str() == s)
    }

    /// The app the notice is for.
    pub fn app(self) -> &'static str {
        match self {
            Notice::DriverApproved | Notice::DriverNotApproved => "driver",
            _ => "rider",
        }
    }

    /// The rider's notice for a ride moving `from` → `to`, if any. Waiting at pickup records
    /// the same state again and sends nothing.
    pub fn for_ride(from: Option<RideState>, to: RideState) -> Option<Notice> {
        if from == Some(to) {
            return None;
        }
        match (from, to) {
            (_, RideState::DriverAssigned) => Some(Notice::DriverAssigned),
            (_, RideState::DriverArrived) => Some(Notice::DriverArrived),
            (_, RideState::InTrip) => Some(Notice::TripStarted),
            (_, RideState::Completed) => Some(Notice::TripCompleted),
            (Some(RideState::Scheduled), RideState::Searching) => Some(Notice::ScheduledSearching),
            _ => None,
        }
    }
}

/// Queues a notice about a ride for its rider.
pub async fn ride<'e>(db: impl PgExecutor<'e>, notice: Notice, ride_id: Uuid) -> Result<(), sqlx::Error> {
    let key = format!("push:{}:{ride_id}", notice.as_str());
    jobs::enqueue(db, jobs::kind::PUSH, json!({ "notice": notice.as_str(), "ride_id": ride_id.to_string() }), None, Some(&key)).await
}

/// Queues a notice for one user (in the notice's app).
pub async fn user<'e>(db: impl PgExecutor<'e>, notice: Notice, user_id: Uuid) -> Result<(), sqlx::Error> {
    jobs::enqueue(db, jobs::kind::PUSH, json!({ "notice": notice.as_str(), "user_id": user_id.to_string() }), None, None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for n in Notice::ALL {
            assert_eq!(Notice::parse(n.as_str()), Some(n));
        }
    }

    #[test]
    fn ride_changes_that_notify_her() {
        use RideState::*;
        assert_eq!(Notice::for_ride(Some(Searching), DriverAssigned), Some(Notice::DriverAssigned));
        assert_eq!(Notice::for_ride(Some(DriverEnRoute), DriverArrived), Some(Notice::DriverArrived));
        assert_eq!(Notice::for_ride(Some(DriverArrived), DriverArrived), None, "waiting at pickup");
        assert_eq!(Notice::for_ride(Some(PinVerified), InTrip), Some(Notice::TripStarted));
        assert_eq!(Notice::for_ride(Some(InTrip), Completed), Some(Notice::TripCompleted));
        assert_eq!(Notice::for_ride(Some(Scheduled), Searching), Some(Notice::ScheduledSearching));
        assert_eq!(Notice::for_ride(Some(DriverAssigned), Searching), None, "her driver cancelled: the app says so");
        assert_eq!(Notice::for_ride(None, Requested), None);
        assert_eq!(Notice::for_ride(Some(Completed), Rated), None);
    }
}
