//! FUber business rules with no I/O: pricing, the ride state machine, matching scores and
//! priority points. Ported from the demo engine (`demo/public/js/engine.js`), which is the
//! behavioural reference; tests here pin the same numbers.

pub mod matching;
pub mod money;
pub mod pricing;
pub mod priority;
pub mod ride;

pub use money::Cents;
pub use ride::{Party, RideEvent, RideState, TransitionError};

/// Ride types, cheapest first. The order is significant: a higher tier charges more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Classic,
    Comfort,
    #[serde(rename = "softlife")]
    SoftLife,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Classic, Tier::Comfort, Tier::SoftLife];
}

/// Demand level. Drives the surge multiplier, the base traffic factor used for pricing,
/// and the matching weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Demand {
    Normal,
    High,
    Peak,
}

impl Demand {
    /// Surge multiplier in basis points (10 000 = 1.0×).
    pub fn surge_bp(self) -> u32 {
        match self {
            Demand::Normal => 10_000,
            Demand::High => 14_000,
            Demand::Peak => 18_000,
        }
    }

    /// Base traffic factor: free-flow speed is divided by this. Prices use it instead of live
    /// traffic so the quote never drifts between the options screen and Request.
    pub fn traffic(self) -> f64 {
        match self {
            Demand::Normal => 1.2,
            Demand::High => 1.4,
            Demand::Peak => 1.6,
        }
    }
}
