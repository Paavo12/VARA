//! Rules at pickup and during the trip (PRD T-2 – T-6).

/// The driver counts as arrived, and sees the exact pickup, within this distance of it (T-2).
pub const ARRIVE_RADIUS_M: f64 = 50.0;
/// Wrong PINs allowed before the trip is blocked and the safety desk alerted (T-3).
pub const PIN_ATTEMPTS: i16 = 5;
/// Free wait at pickup; after it the driver may cancel as a rider no-show (T-5).
pub const FREE_WAIT_S: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinCheck {
    Correct,
    Wrong {
        attempts_left: i16,
    },
    /// This was the last allowed attempt.
    LockedOut,
}

/// Checks a PIN typed at pickup, given how many wrong ones came before. Compares in constant
/// time so response timing says nothing about which digits were right.
pub fn check_pin(expected: &str, given: &str, wrong_so_far: i16) -> PinCheck {
    let (e, g) = (expected.as_bytes(), given.as_bytes());
    let same = e.len() == g.len() && e.iter().zip(g).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
    if same {
        return PinCheck::Correct;
    }
    let wrong = wrong_so_far + 1;
    if wrong >= PIN_ATTEMPTS { PinCheck::LockedOut } else { PinCheck::Wrong { attempts_left: PIN_ATTEMPTS - wrong } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_wrong_pins_lock_the_trip() {
        assert_eq!(check_pin("0427", "0427", 4), PinCheck::Correct);
        assert_eq!(check_pin("0427", "0428", 0), PinCheck::Wrong { attempts_left: 4 });
        assert_eq!(check_pin("0427", "7240", 3), PinCheck::Wrong { attempts_left: 1 });
        assert_eq!(check_pin("0427", "1111", 4), PinCheck::LockedOut);
        assert_eq!(check_pin("0427", "042", 0), PinCheck::Wrong { attempts_left: 4 });
    }
}
