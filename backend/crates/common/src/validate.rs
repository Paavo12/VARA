//! Validated input types (TRD §7.2: parse, don't validate).
//!
//! Each type can only be constructed through its validating constructor, and deserialises
//! through it (`#[serde(try_from = ...)]`), so a handler that receives one knows the value is
//! clean. Request structs combine these with `#[serde(deny_unknown_fields)]`.

use std::fmt;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Invalid(pub &'static str);

/// Unicode NFC, trimmed, internal whitespace collapsed, control characters rejected.
fn clean_line(raw: &str) -> Result<String, Invalid> {
    if raw.chars().any(char::is_control) {
        return Err(Invalid("must not contain control characters"));
    }
    let nfc: String = raw.nfc().collect();
    Ok(nfc.split_whitespace().collect::<Vec<_>>().join(" "))
}

// ---------------------------------------------------------------------------------------------

/// A Namibian mobile number in E.164 form (`+264` and 8–9 digits). Accepts spaces, hyphens
/// and the national `0` prefix on input.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PhoneE164(String);

impl PhoneE164 {
    pub fn parse(raw: &str) -> Result<Self, Invalid> {
        let compact: String = raw.chars().filter(|c| !matches!(c, ' ' | '-' | '(' | ')')).collect();
        let national = if let Some(rest) = compact.strip_prefix("+264") {
            rest
        } else if let Some(rest) = compact.strip_prefix("00264") {
            rest
        } else if let Some(rest) = compact.strip_prefix('0') {
            rest
        } else {
            return Err(Invalid("phone number must be a Namibian number (+264)"));
        };
        if !(8..=9).contains(&national.len()) || !national.bytes().all(|b| b.is_ascii_digit()) || national.starts_with('0') {
            return Err(Invalid("phone number is not valid"));
        }
        Ok(PhoneE164(format!("+264{national}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Never prints the full number.
impl fmt::Debug for PhoneE164 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::telemetry::mask_phone(&self.0))
    }
}

impl TryFrom<String> for PhoneE164 {
    type Error = Invalid;
    fn try_from(s: String) -> Result<Self, Invalid> {
        Self::parse(&s)
    }
}

impl From<PhoneE164> for String {
    fn from(p: PhoneE164) -> String {
        p.0
    }
}

// ---------------------------------------------------------------------------------------------

/// A person's name: 1–80 characters of letters (any script), spaces, and `-'.`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PersonName(String);

impl PersonName {
    pub fn parse(raw: &str) -> Result<Self, Invalid> {
        let s = clean_line(raw)?;
        let len = s.chars().count();
        if len == 0 || len > 80 {
            return Err(Invalid("name must be 1–80 characters"));
        }
        if !s.chars().all(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '\'' | '.' | '’')) {
            return Err(Invalid("name may contain only letters, spaces, hyphens, apostrophes and full stops"));
        }
        if !s.chars().any(char::is_alphabetic) {
            return Err(Invalid("name must contain letters"));
        }
        Ok(PersonName(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PersonName {
    type Error = Invalid;
    fn try_from(s: String) -> Result<Self, Invalid> {
        Self::parse(&s)
    }
}

impl From<PersonName> for String {
    fn from(p: PersonName) -> String {
        p.0
    }
}

// ---------------------------------------------------------------------------------------------

/// User-written text (chat, reports, comments) up to `MAX` characters. Stored and returned as
/// plain text; front ends must render it as text, never as HTML. Line breaks and tabs are
/// kept; other control characters are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlainText<const MAX: usize>(String);

impl<const MAX: usize> PlainText<MAX> {
    pub fn parse(raw: &str) -> Result<Self, Invalid> {
        let raw = raw.replace("\r\n", "\n");
        if raw.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
            return Err(Invalid("text must not contain control characters"));
        }
        let s: String = raw.nfc().collect::<String>().trim().to_owned();
        let len = s.chars().count();
        if len == 0 {
            return Err(Invalid("text must not be empty"));
        }
        if len > MAX {
            return Err(Invalid("text is too long"));
        }
        Ok(PlainText(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<const MAX: usize> TryFrom<String> for PlainText<MAX> {
    type Error = Invalid;
    fn try_from(s: String) -> Result<Self, Invalid> {
        Self::parse(&s)
    }
}

impl<const MAX: usize> From<PlainText<MAX>> for String {
    fn from(p: PlainText<MAX>) -> String {
        p.0
    }
}

// ---------------------------------------------------------------------------------------------

/// A point inside the service area. v1 serves Windhoek plus about 20 km; the bounding box is
/// replaced by the `service_areas` polygon once the database is wired in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawLatLng")]
pub struct LatLng {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLatLng {
    lat: f64,
    lon: f64,
}

pub const SERVICE_AREA_LAT: (f64, f64) = (-22.80, -22.35);
pub const SERVICE_AREA_LON: (f64, f64) = (16.85, 17.35);

impl LatLng {
    pub fn new(lat: f64, lon: f64) -> Result<Self, Invalid> {
        if !lat.is_finite() || !lon.is_finite() {
            return Err(Invalid("coordinates must be numbers"));
        }
        if !(SERVICE_AREA_LAT.0..=SERVICE_AREA_LAT.1).contains(&lat) || !(SERVICE_AREA_LON.0..=SERVICE_AREA_LON.1).contains(&lon) {
            return Err(Invalid("location is outside the FUber service area"));
        }
        Ok(LatLng { lat, lon })
    }

    /// Great-circle distance in metres.
    pub fn distance_m(&self, o: &LatLng) -> f64 {
        const R: f64 = 6_371_008.8;
        let (p1, p2) = (self.lat.to_radians(), o.lat.to_radians());
        let dp = p2 - p1;
        let dl = (o.lon - self.lon).to_radians();
        let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
        2.0 * R * a.sqrt().asin()
    }
}

impl TryFrom<RawLatLng> for LatLng {
    type Error = Invalid;
    fn try_from(r: RawLatLng) -> Result<Self, Invalid> {
        LatLng::new(r.lat, r.lon)
    }
}

// ---------------------------------------------------------------------------------------------

/// An integer in `MIN..=MAX`, e.g. passengers (1–4) or a rating (1–5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub struct Bounded<const MIN: i64, const MAX: i64>(i64);

impl<const MIN: i64, const MAX: i64> Bounded<MIN, MAX> {
    pub fn get(self) -> i64 {
        self.0
    }
}

impl<const MIN: i64, const MAX: i64> TryFrom<i64> for Bounded<MIN, MAX> {
    type Error = Invalid;
    fn try_from(v: i64) -> Result<Self, Invalid> {
        if (MIN..=MAX).contains(&v) { Ok(Bounded(v)) } else { Err(Invalid("number is out of range")) }
    }
}

impl<const MIN: i64, const MAX: i64> From<Bounded<MIN, MAX>> for i64 {
    fn from(b: Bounded<MIN, MAX>) -> i64 {
        b.0
    }
}

pub type Passengers = Bounded<1, 4>;
pub type Stars = Bounded<1, 5>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_normalises_namibian_formats() {
        for raw in ["+264 81 123 4567", "081-123-4567", "00264811234567", "+264811234567"] {
            assert_eq!(PhoneE164::parse(raw).unwrap().as_str(), "+264811234567", "{raw}");
        }
    }

    #[test]
    fn phone_rejects_foreign_and_malformed() {
        for raw in ["+27821234567", "+2648112", "+264 81 123 456a", "", "+264081234567"] {
            assert!(PhoneE164::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn phone_debug_is_masked() {
        assert!(!format!("{:?}", PhoneE164::parse("+264811234567").unwrap()).contains("1234567"));
    }

    #[test]
    fn names() {
        assert_eq!(PersonName::parse("  Ndeshi   Shikongo ").unwrap().as_str(), "Ndeshi Shikongo");
        assert!(PersonName::parse("Martha van Wyk").is_ok());
        assert!(PersonName::parse("O'Neill-Hamutenya").is_ok());
        assert!(PersonName::parse("Zoë").is_ok());
        for bad in ["", "   ", "<script>", "Robert'); DROP TABLE users;--", "a\u{0000}b", "---", &"a".repeat(81)] {
            assert!(PersonName::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn plain_text_keeps_newlines_rejects_controls() {
        type Chat = PlainText<500>;
        assert_eq!(Chat::parse(" I'm at the gate\r\nblue door ").unwrap().as_str(), "I'm at the gate\nblue door");
        assert!(Chat::parse("bell\u{0007}").is_err());
        assert!(Chat::parse(&"x".repeat(501)).is_err());
        assert!(Chat::parse("  ").is_err());
    }

    #[test]
    fn latlng_inside_windhoek_only() {
        assert!(LatLng::new(-22.5667, 17.0813).is_ok()); // Post Street Mall
        assert!(LatLng::new(-33.92, 18.42).is_err()); // Cape Town
        assert!(LatLng::new(f64::NAN, 17.0).is_err());
    }

    #[test]
    fn latlng_rejects_unknown_fields() {
        assert!(serde_json::from_str::<LatLng>(r#"{"lat":-22.56,"lon":17.08,"x":1}"#).is_err());
    }

    #[test]
    fn distance_post_street_to_maerua() {
        let a = LatLng::new(-22.56665, 17.08126).unwrap();
        let b = LatLng::new(-22.58359, 17.09373).unwrap();
        let d = a.distance_m(&b);
        assert!((2200.0..2300.0).contains(&d), "{d}");
    }

    #[test]
    fn bounded() {
        assert!(serde_json::from_str::<Passengers>("4").is_ok());
        assert!(serde_json::from_str::<Passengers>("5").is_err());
        assert!(serde_json::from_str::<Passengers>("0").is_err());
    }
}
