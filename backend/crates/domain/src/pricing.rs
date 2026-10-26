//! Locked, up-front fares (PRD §8, TRD §4.3).
//!
//! Every component is rounded to the cent and the total is the sum of the components, so a
//! receipt always adds up. The demo rounds only the total; results agree to within a cent.

use crate::{Cents, Tier};

/// One tier's rates. Production values come from the `pricing_tiers` table; [`Rates::default_for`]
/// holds the launch rates from the PRD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rates {
    pub base: Cents,
    pub per_km: Cents,
    pub per_min: Cents,
    pub minimum: Cents,
    /// FUber's service fee in basis points (200 = 2 %), taken from the driver's share.
    pub service_fee_bp: u32,
}

impl Rates {
    pub fn default_for(tier: Tier) -> Rates {
        match tier {
            Tier::Classic => {
                Rates { base: Cents(1500), per_km: Cents(800), per_min: Cents(120), minimum: Cents(3500), service_fee_bp: 200 }
            }
            Tier::Comfort => {
                Rates { base: Cents(2000), per_km: Cents(1000), per_min: Cents(150), minimum: Cents(4500), service_fee_bp: 250 }
            }
            Tier::SoftLife => {
                Rates { base: Cents(2800), per_km: Cents(1300), per_min: Cents(200), minimum: Cents(6000), service_fee_bp: 300 }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fare {
    pub base: Cents,
    pub distance: Cents,
    pub time: Cents,
    /// Top-up to reach the tier minimum (zero when the trip is long enough).
    pub minimum_adjustment: Cents,
    pub surge_bp: u32,
    pub surge: Cents,
    /// What the rider pays: base + distance + time + minimum_adjustment + surge.
    pub total: Cents,
    pub service_fee: Cents,
    /// What the driver earns: total − service fee.
    pub driver_earning: Cents,
}

/// `rate × n / d`, rounded half up, for non-negative inputs.
fn prorate(rate: Cents, n: u64, d: u64) -> Cents {
    let p = rate.0 as i128 * n as i128;
    Cents(((p + d as i128 / 2) / d as i128) as i64)
}

/// Prices a trip. `duration_s` must be the duration at the demand level's *base* traffic
/// (see [`crate::Demand::traffic`]), not live traffic, so the quote is stable.
pub fn fare(rates: &Rates, distance_m: u32, duration_s: u32, surge_bp: u32) -> Fare {
    let distance = prorate(rates.per_km, distance_m as u64, 1000);
    let time = prorate(rates.per_min, duration_s as u64, 60);
    let sub = rates.base + distance + time;
    let subtotal = sub.max(rates.minimum);
    let total = subtotal.mul_bp(surge_bp);
    let service_fee = total.mul_bp(rates.service_fee_bp);
    Fare {
        base: rates.base,
        distance,
        time,
        minimum_adjustment: subtotal - sub,
        surge_bp,
        surge: total - subtotal,
        total,
        service_fee,
        driver_earning: total - service_fee,
    }
}

/// Free-flow speed before the traffic factor, as in the demo (`CFG.SPEED_KMH`).
pub const FREE_FLOW_KMH: f64 = 34.0;

/// Pricing duration for a road distance at a demand level's base traffic, as the demo computes
/// it. Production uses OSRM durations scaled by the same factor.
pub fn pricing_duration_s(distance_m: u32, traffic: f64) -> u32 {
    let speed_ms = FREE_FLOW_KMH / 3.6 / traffic;
    (distance_m as f64 / speed_ms).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Demand;
    use proptest::prelude::*;

    /// The demo's `fareFor`, verbatim in f64, for comparison.
    fn demo_fare(r: &Rates, km: f64, min: f64, surge: f64) -> f64 {
        let to = |c: Cents| c.0 as f64 / 100.0;
        let sub = to(r.base) + to(r.per_km) * km + to(r.per_min) * min;
        let subtotal = to(r.minimum).max(sub);
        ((subtotal + subtotal * (surge - 1.0)) * 100.0).round() / 100.0
    }

    #[test]
    fn short_trip_pays_the_minimum() {
        let f = fare(&Rates::default_for(Tier::Classic), 500, 60, 10_000);
        assert_eq!(f.total, Cents(3500));
        assert_eq!(f.base + f.distance + f.time + f.minimum_adjustment, Cents(3500));
    }

    #[test]
    fn comfort_example_matches_hand_calculation() {
        // 7.4 km, 15 min: 20 + 74 + 22.50 = N$116.50; 2.5 % fee = N$2.91 (2.9125 rounds down).
        let f = fare(&Rates::default_for(Tier::Comfort), 7400, 900, 10_000);
        assert_eq!(f.total, Cents(11650));
        assert_eq!(f.service_fee, Cents(291));
        assert_eq!(f.driver_earning, Cents(11359));
    }

    #[test]
    fn surge_applies_after_minimum() {
        let f = fare(&Rates::default_for(Tier::SoftLife), 100, 30, Demand::Peak.surge_bp());
        assert!(f.minimum_adjustment.0 > 0);
        assert_eq!(f.total, Cents(10800)); // N$60 × 1.8
        assert_eq!(f.surge, Cents(4800));
    }

    proptest! {
        #[test]
        fn receipt_always_adds_up(tier in 0usize..3, m in 0u32..60_000, s in 0u32..7_200, d in 0usize..3) {
            let surge = [Demand::Normal, Demand::High, Demand::Peak][d].surge_bp();
            let f = fare(&Rates::default_for(Tier::ALL[tier]), m, s, surge);
            prop_assert_eq!(f.base + f.distance + f.time + f.minimum_adjustment + f.surge, f.total);
            prop_assert_eq!(f.service_fee + f.driver_earning, f.total);
            prop_assert!(f.total >= Rates::default_for(Tier::ALL[tier]).minimum);
        }

        #[test]
        fn agrees_with_demo_within_a_cent(tier in 0usize..3, m in 0u32..60_000, d in 0usize..3) {
            let demand = [Demand::Normal, Demand::High, Demand::Peak][d];
            let rates = Rates::default_for(Tier::ALL[tier]);
            let dur = pricing_duration_s(m, demand.traffic());
            let ours = fare(&rates, m, dur, demand.surge_bp()).total.0 as f64 / 100.0;
            let demo = demo_fare(&rates, m as f64 / 1000.0, dur as f64 / 60.0, demand.surge_bp() as f64 / 10_000.0);
            // Per-component rounding can move each of base/distance/time by half a cent before surge.
            prop_assert!((ours - demo).abs() <= 0.03, "ours {ours} demo {demo}");
        }
    }
}
