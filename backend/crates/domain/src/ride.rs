//! The ride state machine (App Flow §3). The only way to change a ride's state is
//! [`RideState::apply`], so an invalid transition can never be persisted.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RideState {
    Scheduled,
    Requested,
    Searching,
    NoDrivers,
    DriverAssigned,
    DriverEnRoute,
    DriverArrived,
    PinVerified,
    InTrip,
    Completed,
    Rated,
    Cancelled,
    /// Five wrong PINs at pickup. Only the safety desk can close it.
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Party {
    Rider,
    Driver,
    System,
    Staff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RideEvent {
    /// Scheduled ride reaches its dispatch time, or a requested ride starts matching.
    StartSearch,
    DriverAccepted,
    NoDriverLeft,
    RetrySearch,
    RouteComputed,
    DriverArrived,
    PinCorrect,
    PinLockout,
    TripStarted,
    TripCompleted,
    /// Both parties rated, or the driver skipped rating.
    RatingsDone,
    Cancel {
        by: Party,
    },
    /// Driver no-show cancel after the free wait; counts against the rider.
    RiderNoShow,
    SafetyDeskClosed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cannot apply {event:?} to a ride in state {state:?}")]
pub struct TransitionError {
    pub state: RideState,
    pub event: RideEvent,
}

impl RideState {
    pub const ALL: [RideState; 13] = [
        RideState::Scheduled,
        RideState::Requested,
        RideState::Searching,
        RideState::NoDrivers,
        RideState::DriverAssigned,
        RideState::DriverEnRoute,
        RideState::DriverArrived,
        RideState::PinVerified,
        RideState::InTrip,
        RideState::Completed,
        RideState::Rated,
        RideState::Cancelled,
        RideState::Blocked,
    ];

    /// The Postgres `ride_state` label (same as the JSON name).
    pub fn as_str(self) -> &'static str {
        match self {
            RideState::Scheduled => "SCHEDULED",
            RideState::Requested => "REQUESTED",
            RideState::Searching => "SEARCHING",
            RideState::NoDrivers => "NO_DRIVERS",
            RideState::DriverAssigned => "DRIVER_ASSIGNED",
            RideState::DriverEnRoute => "DRIVER_EN_ROUTE",
            RideState::DriverArrived => "DRIVER_ARRIVED",
            RideState::PinVerified => "PIN_VERIFIED",
            RideState::InTrip => "IN_TRIP",
            RideState::Completed => "COMPLETED",
            RideState::Rated => "RATED",
            RideState::Cancelled => "CANCELLED",
            RideState::Blocked => "BLOCKED",
        }
    }

    pub fn parse(s: &str) -> Option<RideState> {
        RideState::ALL.into_iter().find(|r| r.as_str() == s)
    }

    pub fn apply(self, event: RideEvent) -> Result<RideState, TransitionError> {
        use RideEvent as E;
        use RideState as S;
        let next = match (self, event) {
            (S::Scheduled | S::Requested, E::StartSearch) => S::Searching,
            (S::Searching, E::DriverAccepted) => S::DriverAssigned,
            (S::Searching, E::NoDriverLeft) => S::NoDrivers,
            (S::NoDrivers, E::RetrySearch) => S::Searching,
            (S::DriverAssigned, E::RouteComputed) => S::DriverEnRoute,
            (S::DriverEnRoute, E::DriverArrived) => S::DriverArrived,
            (S::DriverArrived, E::PinCorrect) => S::PinVerified,
            (S::DriverArrived, E::PinLockout) => S::Blocked,
            (S::PinVerified, E::TripStarted) => S::InTrip,
            (S::InTrip, E::TripCompleted) => S::Completed,
            (S::Completed, E::RatingsDone) => S::Rated,
            // A driver cancelling after accepting sends the request back to the pool.
            (S::DriverAssigned | S::DriverEnRoute | S::DriverArrived, E::Cancel { by: Party::Driver }) => S::Searching,
            (S::DriverArrived, E::RiderNoShow) => S::Cancelled,
            (
                S::Scheduled | S::Requested | S::Searching | S::NoDrivers | S::DriverAssigned | S::DriverEnRoute | S::DriverArrived,
                E::Cancel { by: Party::Rider | Party::System | Party::Staff },
            ) => S::Cancelled,
            (S::Blocked, E::SafetyDeskClosed) => S::Cancelled,
            (state, event) => return Err(TransitionError { state, event }),
        };
        Ok(next)
    }

    /// The ride occupies its rider: she cannot request another.
    pub fn is_active(self) -> bool {
        !matches!(self, RideState::Scheduled | RideState::Completed | RideState::Rated | RideState::Cancelled)
    }

    /// The ride occupies its driver.
    pub fn locks_driver(self) -> bool {
        matches!(
            self,
            RideState::DriverAssigned
                | RideState::DriverEnRoute
                | RideState::DriverArrived
                | RideState::PinVerified
                | RideState::InTrip
                | RideState::Blocked
        )
    }

    /// The driver may see the exact pickup address (PRD T-2).
    pub fn driver_sees_exact_pickup(self) -> bool {
        matches!(self, RideState::DriverArrived | RideState::PinVerified | RideState::InTrip | RideState::Completed | RideState::Rated)
    }

    /// The driver may see the destination: only after the correct PIN (PRD T-3).
    pub fn driver_sees_destination(self) -> bool {
        matches!(self, RideState::PinVerified | RideState::InTrip | RideState::Completed | RideState::Rated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RideEvent as E;
    use RideState as S;

    #[test]
    fn labels_round_trip_and_match_serde() {
        for st in RideState::ALL {
            assert_eq!(RideState::parse(st.as_str()), Some(st));
            assert_eq!(serde_json::to_value(st).unwrap(), st.as_str());
        }
    }

    #[test]
    fn happy_path() {
        let mut s = S::Requested;
        for e in [
            E::StartSearch,
            E::DriverAccepted,
            E::RouteComputed,
            E::DriverArrived,
            E::PinCorrect,
            E::TripStarted,
            E::TripCompleted,
            E::RatingsDone,
        ] {
            s = s.apply(e).unwrap();
        }
        assert_eq!(s, S::Rated);
    }

    #[test]
    fn trip_cannot_start_without_pin() {
        assert!(S::DriverArrived.apply(E::TripStarted).is_err());
    }

    #[test]
    fn driver_cancel_returns_to_pool_rider_cancel_ends_ride() {
        assert_eq!(S::DriverEnRoute.apply(E::Cancel { by: Party::Driver }), Ok(S::Searching));
        assert_eq!(S::DriverEnRoute.apply(E::Cancel { by: Party::Rider }), Ok(S::Cancelled));
    }

    #[test]
    fn no_cancel_once_in_trip() {
        assert!(S::InTrip.apply(E::Cancel { by: Party::Rider }).is_err());
        assert!(S::PinVerified.apply(E::Cancel { by: Party::Driver }).is_err());
    }

    #[test]
    fn pin_lockout_needs_safety_desk() {
        let s = S::DriverArrived.apply(E::PinLockout).unwrap();
        assert!(s.apply(E::Cancel { by: Party::Rider }).is_err());
        assert_eq!(s.apply(E::SafetyDeskClosed), Ok(S::Cancelled));
    }

    #[test]
    fn destination_hidden_until_pin_verified() {
        for s in [S::Searching, S::DriverAssigned, S::DriverEnRoute, S::DriverArrived, S::Blocked] {
            assert!(!s.driver_sees_destination(), "{s:?}");
        }
        assert!(S::PinVerified.driver_sees_destination());
        assert!(!S::DriverEnRoute.driver_sees_exact_pickup());
        assert!(S::DriverArrived.driver_sees_exact_pickup());
    }
}
