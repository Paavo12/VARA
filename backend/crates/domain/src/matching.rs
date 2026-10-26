//! Driver ranking (PRD D-2, D-5), ported from the demo's `rankDrivers`.
//!
//! Callers filter drivers first (online, free, tier, wheelchair, not excluded, within the
//! radius) and supply road-distance ETAs; this module only scores and orders them.

use crate::{Demand, priority};

pub const ETA_NORM_MIN: f64 = 12.0;
pub const FAVOURITE_BONUS: f64 = 0.08;
/// A rider loses her nearest driver for every this many points below the start value.
pub const HOLD_PER_POINTS: i32 = 15;
/// Driver score shift per 100 points away from the start value.
pub const DRIVER_PRIORITY_WEIGHT: f64 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub eta: f64,
    pub acceptance: f64,
}

impl Weights {
    pub fn for_demand(d: Demand) -> Weights {
        match d {
            Demand::Normal => Weights { eta: 0.7, acceptance: 0.3 },
            Demand::High => Weights { eta: 0.5, acceptance: 0.5 },
            Demand::Peak => Weights { eta: 0.45, acceptance: 0.55 },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate<Id> {
    pub driver_id: Id,
    /// Straight-line distance to pickup in metres (used for holding the nearest drivers).
    pub straight_m: f64,
    /// Road-distance ETA to pickup in minutes.
    pub eta_min: f64,
    /// Rolling acceptance rate, 0–1.
    pub acceptance: f64,
    pub favourite: bool,
    pub priority_points: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked<Id> {
    /// Best first.
    pub offer_order: Vec<(Id, f64)>,
    /// Nearest drivers withheld because the rider's priority is low.
    pub held: Vec<Id>,
}

pub fn score<Id>(c: &Candidate<Id>, w: Weights, rider_points: i32) -> f64 {
    // High-priority riders get closeness weighted up; never down.
    let eta_boost = (rider_points as f64 / priority::START as f64).max(1.0);
    let eta_score = (1.0 - c.eta_min / ETA_NORM_MIN).max(0.0);
    let prio = DRIVER_PRIORITY_WEIGHT * (c.priority_points - priority::START) as f64 / priority::START as f64;
    let fav = if c.favourite { FAVOURITE_BONUS } else { 0.0 };
    w.eta * eta_boost * eta_score + w.acceptance * c.acceptance + fav + prio
}

pub fn rank<Id: Clone>(mut candidates: Vec<Candidate<Id>>, demand: Demand, rider_points: i32) -> Ranked<Id> {
    let w = Weights::for_demand(demand);
    // Hold the nearest drivers for higher-priority riders, but always leave at least one.
    let deficit = (priority::START - rider_points).max(0);
    let hold = ((deficit / HOLD_PER_POINTS) as usize).min(candidates.len().saturating_sub(1));
    let mut held = Vec::new();
    if hold > 0 {
        candidates.sort_by(|a, b| a.straight_m.total_cmp(&b.straight_m));
        held = candidates.drain(..hold).map(|c| c.driver_id).collect();
    }
    let mut scored: Vec<(Id, f64)> = candidates.iter().map(|c| (c.driver_id.clone(), score(c, w, rider_points))).collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ranked { offer_order: scored, held }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: u32, straight_m: f64, eta_min: f64, acceptance: f64) -> Candidate<u32> {
        Candidate { driver_id: id, straight_m, eta_min, acceptance, favourite: false, priority_points: 100 }
    }

    #[test]
    fn closer_driver_wins_in_normal_demand() {
        let r = rank(vec![c(1, 2000.0, 8.0, 0.9), c(2, 500.0, 2.0, 0.85)], Demand::Normal, 100);
        assert_eq!(r.offer_order[0].0, 2);
    }

    #[test]
    fn score_matches_demo_formula() {
        // 0.7 × (1 − 3/12) + 0.3 × 0.9 = 0.525 + 0.27
        let s = score(&c(1, 0.0, 3.0, 0.9), Weights::for_demand(Demand::Normal), 100);
        assert!((s - 0.795).abs() < 1e-12);
    }

    #[test]
    fn low_priority_rider_loses_nearest_but_keeps_one() {
        let drivers = vec![c(1, 300.0, 1.0, 0.9), c(2, 900.0, 3.0, 0.9), c(3, 1500.0, 5.0, 0.9)];
        // 100 − 65 = 35 → 2 held.
        let r = rank(drivers.clone(), Demand::Normal, 65);
        assert_eq!(r.held, vec![1, 2]);
        assert_eq!(r.offer_order.len(), 1);
        // 0 points would hold 6, but one driver always stays.
        let r = rank(drivers, Demand::Normal, 0);
        assert_eq!(r.offer_order.len(), 1);
    }

    #[test]
    fn unreliable_driver_ranks_lower() {
        let mut a = c(1, 500.0, 2.0, 0.9);
        a.priority_points = 40;
        let r = rank(vec![a, c(2, 500.0, 2.0, 0.9)], Demand::Normal, 100);
        assert_eq!(r.offer_order[0].0, 2);
    }
}
