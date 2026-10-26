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

    let tiers: i64 = sqlx::query_scalar("SELECT count(*) FROM pricing_tiers").fetch_one(&pool).await.unwrap();
    assert_eq!(tiers, 3);

    // Phone numbers outside +264 are rejected by the database too, not just the API.
    let bad = sqlx::query("INSERT INTO users (phone_e164, full_name, display_name) VALUES ('+27821234567', 'X', 'X')").execute(&pool).await;
    assert!(bad.is_err());
}
