//! Safety rules with no I/O (PRD S-1).

/// Pressing SOS again within this window returns the same incident and texts nobody again.
pub const SOS_REPEAT_S: i64 = 120;
/// Namibian Police, offered as a one-tap call after every SOS.
pub const POLICE_PHONE: &str = "10111";

/// The text her emergency contacts receive. With a ride it carries the live trip link;
/// without one (a driver online between trips) her last known position.
pub fn sos_text(name: &str, link: Option<&str>, at: Option<(f64, f64)>) -> String {
    let follow = match (link, at) {
        (Some(link), _) => format!(" Follow her trip live: {link}"),
        (None, Some((lat, lon))) => format!(" Her location: https://www.openstreetmap.org/?mlat={lat:.5}&mlon={lon:.5}"),
        (None, None) => String::new(),
    };
    format!(
        "VARA SOS: {name} pressed the emergency button.{follow} Our safety team is responding. If you think she is in danger, call the police on {POLICE_PHONE}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_links_the_trip_or_else_gives_her_position() {
        let t = sos_text("Ndeshi", Some("https://vara.example/t/abc"), Some((-22.57, 17.08)));
        assert!(t.contains("Ndeshi pressed") && t.contains("https://vara.example/t/abc") && !t.contains("openstreetmap"));
        let t = sos_text("Ndeshi", None, Some((-22.570001, 17.083799)));
        assert!(t.contains("mlat=-22.57000&mlon=17.08380"), "{t}");
        let t = sos_text("Ndeshi", None, None);
        assert!(t.contains("10111") && !t.contains("http"));
        // Fits one long SMS with a full-length link (the gateway caps bodies at 320).
        let link = format!("https://vara.example/t/{}", "x".repeat(43));
        assert!(sos_text(&"N".repeat(80), Some(&link), None).chars().count() <= 320);
    }
}
