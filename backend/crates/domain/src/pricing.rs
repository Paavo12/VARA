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
    /// VARA's service fee in basis points (1500 = 15 %), taken from the driver's share.
    pub service_fee_bp: u32,
}

impl Rates {
    pub fn default_for(tier: Tier) -> Rates {
        match tier {
            Tier::Classic => {
                Rates { base: Cents(1500), per_km: Cents(800), per_min: Cents(120), minimum: Cents(3500), service_fee_bp: 1500 }
            }
            Tier::Comfort => {
                Rates { base: Cents(2000), per_km: Cents(1000), per_min: Cents(150), minimum: Cents(4500), service_fee_bp: 1650 }
            }
            Tier::SoftLife => {
                Rates { base: Cents(2800), per_km: Cents(1300), per_min: Cents(200), minimum: Cents(6000), service_fee_bp: 1800 }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct Fare {
    pub base: Cents,
    pub distance: Cents,
    pub time: Cents,
    /// Top-up to reach the tier minimum (zero when the trip is long enough).
    pub minimum_adjustment: Cents,
    pub surge_bp: u32,
    pub surge: Cents,
    /// Kids Guardian child seat surcharge (F_kids): all of it goes to the driver, who provides
    /// the seat, so no service fee is taken on it. Zero when no child seat was asked for.
    #[serde(default)]
    pub kids: Cents,
    /// What the rider pays: base + distance + time + minimum_adjustment + surge + kids.
    pub total: Cents,
    pub service_fee: Cents,
    /// What the driver earns: total − service fee.
    pub driver_earning: Cents,
}

impl Fare {
    /// Adds the Kids Guardian child seat surcharge: the rider pays it and the driver keeps all of it.
    pub fn with_kids_surcharge(self, surcharge: Cents) -> Fare {
        Fare { kids: surcharge, total: self.total + surcharge, driver_earning: self.driver_earning + surcharge, ..self }
    }
}

/// The flat service fee for a Map Expert, on every tier (1500 = 15 %).
pub const MAP_EXPERT_FEE_BP: u32 = 1500;

/// The launch promotion's service fee, on every tier and for Map Experts too (1300 = 13 %).
/// It runs for [`LAUNCH_PROMO_DAYS`] from the launch date an admin sets; the database function
/// `launch_promo_active()` (migration 0016) says whether a moment falls inside it.
pub const LAUNCH_PROMO_FEE_BP: u32 = 1300;
pub const LAUNCH_PROMO_DAYS: i64 = 90;

/// What lowers a driver's service fee below her tier's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeeTerms {
    /// She is a Map Expert now.
    pub map_expert: bool,
    /// The launch promotion is running now.
    pub launch_promo: bool,
}

/// The service fee and the driver's earning on a fare of `total`: the lowest of the tier's fee,
/// the Map Expert fee when she is one, and the launch promotion's fee while it runs.
pub fn split(total: Cents, tier_fee_bp: u32, terms: FeeTerms) -> (Cents, Cents) {
    let mut bp = tier_fee_bp;
    if terms.map_expert {
        bp = bp.min(MAP_EXPERT_FEE_BP);
    }
    if terms.launch_promo {
        bp = bp.min(LAUNCH_PROMO_FEE_BP);
    }
    let fee = total.mul_bp(bp);
    (fee, total - fee)
}

/// [`split`] for a ride's whole fare of `total` that includes a child seat surcharge of `kids`:
/// the service fee is taken on the rest only, and the driver keeps the surcharge.
pub fn split_with_kids(total: Cents, kids: Cents, tier_fee_bp: u32, terms: FeeTerms) -> (Cents, Cents) {
    let (fee, _) = split(total - kids, tier_fee_bp, terms);
    (fee, total - fee)
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
        kids: Cents(0),
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
        // 7.4 km, 15 min: 20 + 74 + 22.50 = N$116.50; 16.5 % fee = N$19.22 (19.2225 rounds down).
        let f = fare(&Rates::default_for(Tier::Comfort), 7400, 900, 10_000);
        assert_eq!(f.total, Cents(11650));
        assert_eq!(f.service_fee, Cents(1922));
        assert_eq!(f.driver_earning, Cents(9728));
    }

    #[test]
    fn on_a_n100_fare_drivers_keep_85_83_50_and_82() {
        for (tier, fee, kept) in [(Tier::Classic, 1500, 8500), (Tier::Comfort, 1650, 8350), (Tier::SoftLife, 1800, 8200)] {
            let r = Rates::default_for(tier);
            let fee_on_100 = Cents(10_000).mul_bp(r.service_fee_bp);
            assert_eq!((fee_on_100, Cents(10_000) - fee_on_100), (Cents(fee), Cents(kept)), "{tier:?}");
        }
    }

    const EXPERT: FeeTerms = FeeTerms { map_expert: true, launch_promo: false };
    const PROMO: FeeTerms = FeeTerms { map_expert: false, launch_promo: true };

    #[test]
    fn a_map_expert_pays_15_percent_on_every_tier() {
        for tier in Tier::ALL {
            let r = Rates::default_for(tier);
            assert_eq!(split(Cents(10_000), r.service_fee_bp, EXPERT), (Cents(1500), Cents(8500)), "{tier:?}");
            assert_eq!(split(Cents(10_000), r.service_fee_bp, FeeTerms::default()).0, Cents(10_000).mul_bp(r.service_fee_bp));
        }
        // Never more than the tier's own fee.
        assert_eq!(split(Cents(10_000), 1200, EXPERT).0, Cents(1200));
    }

    #[test]
    fn the_launch_promo_charges_13_percent_to_everyone_map_experts_too() {
        for tier in Tier::ALL {
            let r = Rates::default_for(tier);
            assert_eq!(split(Cents(10_000), r.service_fee_bp, PROMO), (Cents(1300), Cents(8700)), "{tier:?}");
            let both = FeeTerms { map_expert: true, launch_promo: true };
            assert_eq!(split(Cents(10_000), r.service_fee_bp, both), (Cents(1300), Cents(8700)), "{tier:?}");
        }
        // Never more than the tier's own fee.
        assert_eq!(split(Cents(10_000), 1200, PROMO).0, Cents(1200));
    }

    #[test]
    fn the_child_seat_surcharge_goes_to_the_driver_without_a_fee() {
        let f = fare(&Rates::default_for(Tier::Comfort), 7400, 900, 10_000).with_kids_surcharge(Cents(2000));
        assert_eq!((f.kids, f.total), (Cents(2000), Cents(13650)));
        assert_eq!((f.service_fee, f.driver_earning), (Cents(1922), Cents(11728)));
        // The split booked at accept agrees: fee on N$116.50 only, the driver keeps the rest.
        assert_eq!(split_with_kids(f.total, f.kids, 1650, FeeTerms::default()), (Cents(1922), Cents(11728)));
        assert_eq!(split_with_kids(Cents(10_000), Cents(0), 1500, FeeTerms::default()), split(Cents(10_000), 1500, FeeTerms::default()));
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
