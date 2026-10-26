//! Priority points (PRD D-5). Hidden from riders and drivers; only ops sees them.

pub const START: i32 = 100;
pub const MAX: i32 = 150;
/// The nth cancellation costs n × this many points.
pub const CANCEL_STEP: i32 = 5;
pub const TRIP_GAIN: i32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Priority {
    pub points: i32,
    pub cancels: i32,
}

impl Default for Priority {
    fn default() -> Self {
        Priority { points: START, cancels: 0 }
    }
}

impl Priority {
    /// Records a cancellation (or a rider no-show) and returns the points lost.
    pub fn penalise(&mut self) -> i32 {
        self.cancels += 1;
        let lost = CANCEL_STEP * self.cancels;
        self.points = (self.points - lost).clamp(0, MAX);
        lost
    }

    pub fn reward_trip(&mut self) {
        self.points = (self.points + TRIP_GAIN).clamp(0, MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cancellation_costs_more() {
        let mut p = Priority::default();
        assert_eq!([p.penalise(), p.penalise(), p.penalise()], [5, 10, 15]);
        assert_eq!(p.points, 70);
    }

    #[test]
    fn clamped_to_range() {
        let mut p = Priority { points: 149, cancels: 0 };
        p.reward_trip();
        assert_eq!(p.points, MAX);
        let mut p = Priority { points: 3, cancels: 9 };
        p.penalise();
        assert_eq!(p.points, 0);
    }
}
