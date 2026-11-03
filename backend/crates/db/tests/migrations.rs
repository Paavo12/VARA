//! Applies every migration to a real PostGIS database and checks the seed data.
//! Runs only when TEST_DATABASE_URL is set (CI provides one; locally, start
//! infra/docker-compose.yml and point it at a scratch database).

use secrecy::SecretString;

#[tokio::test]
async fn migrations_apply_cleanly() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("skipped: TEST_DATABASE_URL not set");
        return;
    };
    let pool = fuber_db::pool(&SecretString::from(url)).unwrap();
    fuber_db::MIGRATOR.run(&pool).await.expect("migrations apply");

    // One rate in force per ride type, at the 15 / 16.5 / 18 % service fee (0007).
    let fees: Vec<(String, i32)> = sqlx::query_as(
        "SELECT DISTINCT ON (tier) tier::text, service_fee_bp FROM pricing_tiers
         WHERE effective_from <= now() ORDER BY tier, effective_from DESC",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let fees: Vec<(&str, i32)> = fees.iter().map(|(t, bp)| (t.as_str(), *bp)).collect();
    assert_eq!(fees, [("classic", 1500), ("comfort", 1650), ("softlife", 1800)]);

    // Phone numbers outside +264 are rejected by the database too, not just the API.
    let bad = sqlx::query("INSERT INTO users (phone_e164, full_name, display_name) VALUES ('+27821234567', 'X', 'X')").execute(&pool).await;
    assert!(bad.is_err());
}
