//! Safety rules with no I/O (PRD S-1 – S-3).

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

/// A shared trip link keeps working this long after the ride ends (PRD S-2).
pub const SHARE_AFTER_END_S: i64 = 30 * 60;

/// RideCheck (PRD S-3): further than this from the planned route, two samples in a row.
pub const RIDECHECK_OFF_ROUTE_M: f64 = 150.0;
/// RideCheck: no movement beyond `RIDECHECK_STOP_RADIUS_M` for this long, or no position at all.
pub const RIDECHECK_STOP_S: i64 = 120;
pub const RIDECHECK_STOP_RADIUS_M: f64 = 30.0;
/// A stop this close to the destination is the drop-off, not a reason to ask.
pub const RIDECHECK_NEAR_DESTINATION_M: f64 = 150.0;
/// Seconds she has to answer before the safety desk is alerted.
pub const RIDECHECK_ANSWER_S: i64 = 60;
/// After a check, the same kind is not asked again on that ride for this long.
pub const RIDECHECK_AGAIN_S: i64 = 300;

/// The text sent when she taps *Send to my emergency contacts* on Share trip.
pub fn share_text(name: &str, link: &str) -> String {
    format!("{name} is sharing her VARA trip with you. Follow it live: {link} The link stops working 30 minutes after the trip ends.")
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
        assert!(share_text(&"N".repeat(80), &link).chars().count() <= 320);
    }
}
