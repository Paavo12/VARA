use std::fmt;

/// An amount of Namibian dollars in cents. Money is never a float.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(transparent)]
pub struct Cents(pub i64);

impl Cents {
    pub const ZERO: Cents = Cents(0);

    /// `self × bp / 10 000`, rounded half away from zero.
    pub fn mul_bp(self, bp: u32) -> Cents {
        let p = self.0 as i128 * bp as i128;
        let q = if p >= 0 { (p + 5_000) / 10_000 } else { (p - 5_000) / 10_000 };
        Cents(q as i64)
    }
}

impl std::ops::Add for Cents {
    type Output = Cents;
    fn add(self, o: Cents) -> Cents {
        Cents(self.0 + o.0)
    }
}

impl std::ops::Sub for Cents {
    type Output = Cents;
    fn sub(self, o: Cents) -> Cents {
        Cents(self.0 - o.0)
    }
}

/// Formats as `N$123.40`.
impl fmt::Display for Cents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let a = self.0.unsigned_abs();
        write!(f, "{sign}N${}.{:02}", a / 100, a % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul_bp_rounds_half_away_from_zero() {
        assert_eq!(Cents(1000).mul_bp(14_000), Cents(1400));
        assert_eq!(Cents(5).mul_bp(5_000), Cents(3)); // 2.5 → 3
        assert_eq!(Cents(-5).mul_bp(5_000), Cents(-3));
    }

    #[test]
    fn display() {
        assert_eq!(Cents(12340).to_string(), "N$123.40");
        assert_eq!(Cents(-5).to_string(), "-N$0.05");
    }
}
