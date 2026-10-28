//! Small geometry helpers for privacy rules (TRD §4.4).

const EARTH_R: f64 = 6_371_008.8;

/// The point `distance_m` from `(lat, lon)` along `bearing_deg` (0 = north, 90 = east).
pub fn offset(lat: f64, lon: f64, bearing_deg: f64, distance_m: f64) -> (f64, f64) {
    let (p1, l1, b, d) = (lat.to_radians(), lon.to_radians(), bearing_deg.to_radians(), distance_m / EARTH_R);
    let p2 = (p1.sin() * d.cos() + p1.cos() * d.sin() * b.cos()).asin();
    let l2 = l1 + (b.sin() * d.sin() * p1.cos()).atan2(d.cos() - p1.sin() * p2.sin());
    (p2.to_degrees(), l2.to_degrees())
}

/// Great-circle distance in metres.
pub fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (p1, p2) = (a.0.to_radians(), b.0.to_radians());
    let dp = p2 - p1;
    let dl = (b.1 - a.1).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_R * h.sqrt().asin()
}

/// The fuzzy pickup shown to a driver before arrival: the real pickup moved `min_m..=max_m`
/// in a random direction. `u1`, `u2` are uniform in `[0, 1)` (the caller owns the randomness).
pub fn fuzz(lat: f64, lon: f64, min_m: f64, max_m: f64, u1: f64, u2: f64) -> (f64, f64) {
    offset(lat, lon, u1 * 360.0, min_m + u2 * (max_m - min_m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn offset_north_by_a_kilometre() {
        let (lat, lon) = offset(-22.56665, 17.08126, 0.0, 1000.0);
        assert!((distance_m((-22.56665, 17.08126), (lat, lon)) - 1000.0).abs() < 0.01);
        assert!(lat > -22.56665);
        assert!((lon - 17.08126).abs() < 1e-9);
    }

    proptest! {
        #[test]
        fn fuzzed_pickup_is_100_to_200_m_away(u1 in 0.0f64..1.0, u2 in 0.0f64..1.0) {
            let p = (-22.58359, 17.09373);
            let f = fuzz(p.0, p.1, 100.0, 200.0, u1, u2);
            let d = distance_m(p, f);
            prop_assert!((99.99..=200.01).contains(&d), "{d}");
        }
    }
}
