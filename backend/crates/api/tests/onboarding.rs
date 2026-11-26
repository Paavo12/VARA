//! Phase 1 end-to-end tests against a real PostGIS database: sign-in, rider and driver
//! onboarding through to an ops decision, uploads through the worker, account deletion, and
//! the "wrong user" and validation-failure checks required for every endpoint.
//!
//! Runs only when TEST_DATABASE_URL is set (CI provides one). Each test binary creates its own
//! scratch database next to it and migrates it.

#[macro_use]
mod common;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use fuber_db::sqlx;
use serde_json::{Value, json};
use time::OffsetDateTime;
use tower::ServiceExt;
use uuid::Uuid;

use common::*;
use fuber_api::rate_limit::RateLimits;

// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn otp_sign_in_and_refresh_rotation_with_reuse_detection() {
    let t = setup!();
    let phone = new_phone();
    let challenge = t.request_code(&phone).await;
    let code = t.last_code(&phone);
    let wrong = if code == "000000" { "111111" } else { "000000" };

    let verify = |code: &str, phone: &str| json!({ "challenge_id": challenge, "phone": phone, "code": code, "app": "rider" });
    let (status, body) = t.call(Method::POST, "/v1/auth/otp/verify", None, Some(verify(wrong, &phone))).await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("otp_invalid")));
    // The right code for a different number fails too.
    let (status, _) = t.call(Method::POST, "/v1/auth/otp/verify", None, Some(verify(&code, &new_phone()))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = t.call(Method::POST, "/v1/auth/otp/verify", None, Some(verify(&code, &phone))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["new_user"], true);
    let access = body["access_token"].as_str().unwrap();
    let refresh = body["refresh_token"].as_str().unwrap().to_owned();
    // A code works once.
    assert_eq!(t.call(Method::POST, "/v1/auth/otp/verify", None, Some(verify(&code, &phone))).await.0, StatusCode::BAD_REQUEST);

    let (status, me) = t.get("/v1/me", access).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["phone"], phone.as_str());
    assert_eq!(me["verification"], "not_started");
    assert_eq!(me["profile_complete"], false);

    // Rotation: the new pair works, the old access token's session is closed.
    let (status, pair) = t.call(Method::POST, "/v1/auth/refresh", None, Some(json!({ "refresh_token": refresh }))).await;
    assert_eq!(status, StatusCode::OK, "{pair}");
    let access2 = pair["access_token"].as_str().unwrap().to_owned();
    assert_eq!(t.get("/v1/me", &access2).await.0, StatusCode::OK);
    assert_eq!(t.get("/v1/me", access).await.0, StatusCode::UNAUTHORIZED);

    // Reusing the spent refresh token revokes the whole family, including the new session.
    let (status, _) = t.call(Method::POST, "/v1/auth/refresh", None, Some(json!({ "refresh_token": refresh }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(t.get("/v1/me", &access2).await.0, StatusCode::UNAUTHORIZED);
    let (status, _) = t.call(Method::POST, "/v1/auth/refresh", None, Some(json!({ "refresh_token": pair["refresh_token"] }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Signing in again finds the same account.
    let again = t.sign_in(&phone, "rider").await;
    assert!(!again.new_user);
    assert_eq!(t.post("/v1/auth/logout", &again.access, json!({})).await.0, StatusCode::NO_CONTENT);
    assert_eq!(t.get("/v1/me", &again.access).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn otp_challenge_locks_after_five_wrong_codes_and_resend_has_cooldown() {
    let t = setup!();
    let phone = new_phone();
    let challenge = t.request_code(&phone).await;
    let code = t.last_code(&phone);
    let wrong = if code == "999999" { "888888" } else { "999999" };
    for i in 0..5 {
        let (status, body) = t
            .call(
                Method::POST,
                "/v1/auth/otp/verify",
                None,
                Some(json!({ "challenge_id": challenge, "phone": phone, "code": wrong, "app": "rider" })),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], if i == 4 { "otp_locked" } else { "otp_invalid" });
    }
    let (_, body) = t
        .call(
            Method::POST,
            "/v1/auth/otp/verify",
            None,
            Some(json!({ "challenge_id": challenge, "phone": phone, "code": code, "app": "rider" })),
        )
        .await;
    assert_eq!(body["error"]["code"], "otp_locked", "the right code no longer works");

    let (status, body) = t.call(Method::POST, "/v1/auth/otp/request", None, Some(json!({ "phone": phone }))).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
}

#[tokio::test]
async fn rider_onboarding_through_ops_approval() {
    let t = setup!();
    let rider = t.sign_in(&new_phone(), "rider").await;

    // Verification needs the profile first.
    let id_doc = t.photo(&rider.access, "identity").await;
    let selfie = t.photo(&rider.access, "selfie").await;
    let submit = json!({ "id_document_upload_id": id_doc, "selfie_upload_id": selfie, "national_id": "94031200123" });
    let (status, body) = t.post("/v1/me/verification", &rider.access, submit.clone()).await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("profile_incomplete")));

    let (status, me) = t
        .patch(
            "/v1/me",
            &rider.access,
            json!({ "full_name": "Ndeshi  Shikongo", "date_of_birth": "1994-03-12", "email": "Ndeshi@Example.com" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["display_name"], "Ndeshi");
    assert_eq!(me["email"], "ndeshi@example.com");
    assert_eq!(me["profile_complete"], true);
    let kinds: Vec<&str> = me["pending_consents"].as_array().unwrap().iter().map(|d| d["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["privacy", "terms"]);
    let ids: Vec<Value> = me["pending_consents"].as_array().unwrap().iter().map(|d| d["id"].clone()).collect();
    assert_eq!(t.post("/v1/consents", &rider.access, json!({ "document_ids": ids })).await.0, StatusCode::NO_CONTENT);

    // Date of birth is stored encrypted, never as text.
    let raw: Vec<u8> =
        sqlx::query_scalar("SELECT date_of_birth FROM users WHERE id = $1").bind(rider.user_id).fetch_one(&t.state.db).await.unwrap();
    assert!(!raw.windows(4).any(|w| w == b"1994"));

    let (status, body) = t.post("/v1/me/verification", &rider.access, submit.clone()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let verification = body["verification_id"].as_str().unwrap().to_owned();
    let (_, me) = t.get("/v1/me", &rider.access).await;
    assert_eq!(
        (me["verification"].as_str(), me["identity_locked"].as_bool(), me["pending_consents"].as_array().unwrap().len()),
        (Some("pending"), Some(true), 0)
    );
    // Name can't change while under review; the second submission is refused.
    assert_eq!(t.patch("/v1/me", &rider.access, json!({ "full_name": "Someone Else" })).await.0, StatusCode::CONFLICT);
    assert_eq!(t.post("/v1/me/verification", &rider.access, submit).await.0, StatusCode::CONFLICT);

    // Ops review.
    let reviewer = t.staff("ops_reviewer").await;
    let (status, queue) = t.get("/v1/ops/verifications", &reviewer).await;
    assert_eq!(status, StatusCode::OK);
    assert!(queue.as_array().unwrap().iter().any(|q| q["id"] == verification.as_str()));
    let (status, detail) = t.get(&format!("/v1/ops/verifications/{verification}"), &reviewer).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["national_id"], "94031200123");
    assert_eq!(detail["date_of_birth"], "1994-03-12");
    let image_path = detail["id_document_url"].as_str().unwrap().strip_prefix(PUBLIC_URL).unwrap().to_owned();
    let res = t.app.clone().oneshot(Request::get(&image_path).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "image/jpeg");
    // A tampered image link is refused.
    let tampered = image_path.replace("sig=", "sig=x");
    assert_eq!(t.app.clone().oneshot(Request::get(&tampered).body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::FORBIDDEN);

    // Rejecting needs a reason.
    let (status, _) = t.post(&format!("/v1/ops/verifications/{verification}/decision"), &reviewer, json!({ "decision": "reject" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, body) =
        t.post(&format!("/v1/ops/verifications/{verification}/decision"), &reviewer, json!({ "decision": "approve" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, me) = t.get("/v1/me", &rider.access).await;
    assert_eq!((me["verification"].as_str(), me["status"].as_str()), (Some("approved"), Some("active")));
    // Deciding twice is refused.
    assert_eq!(
        t.post(&format!("/v1/ops/verifications/{verification}/decision"), &reviewer, json!({ "decision": "approve" })).await.0,
        StatusCode::CONFLICT
    );

    let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_log WHERE user_id = $1 ORDER BY id")
        .bind(rider.user_id)
        .fetch_all(&t.state.db)
        .await
        .unwrap();
    assert!(actions.contains(&"identity.view".to_owned()) && actions.contains(&"identity.approve".to_owned()), "{actions:?}");

    // The same national ID can't verify a second account.
    let other = t.onboarded("rider").await;
    let (a, b) = (t.photo(&other.access, "identity").await, t.photo(&other.access, "selfie").await);
    let (status, body) = t
        .post(
            "/v1/me/verification",
            &other.access,
            json!({ "id_document_upload_id": a, "selfie_upload_id": b, "national_id": "940312 00123" }),
        )
        .await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("duplicate_identity")));
}

#[tokio::test]
async fn bad_files_are_rejected_and_reject_the_verification_waiting_on_them() {
    let t = setup!();
    let rider = t.onboarded("rider").await;
    // An HTML file declared as PNG.
    let fake = t.send_upload(&rider.access, "identity", "image/png", b"<html><script>alert(1)</script></html>").await;
    let selfie = t.send_upload(&rider.access, "selfie", "image/png", &png()).await;
    // Submitted before the worker has checked the files: accepted and pending.
    let (status, body) =
        t.post("/v1/me/verification", &rider.access, json!({ "id_document_upload_id": fake, "selfie_upload_id": selfie })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    t.work().await;

    let (_, upload) = t.get(&format!("/v1/uploads/{fake}"), &rider.access).await;
    assert_eq!(upload["status"], "rejected");
    assert!(upload["reject_reason"].as_str().unwrap().contains("not a JPEG"));
    let (_, me) = t.get("/v1/me", &rider.access).await;
    assert_eq!(me["verification"], "rejected");
    assert!(me["verification_reject_reason"].as_str().is_some());
    // The rejected upload can't be reused; a new photo works.
    let (status, _) =
        t.post("/v1/me/verification", &rider.access, json!({ "id_document_upload_id": fake, "selfie_upload_id": selfie })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let good = t.photo(&rider.access, "identity").await;
    assert_eq!(
        t.post("/v1/me/verification", &rider.access, json!({ "id_document_upload_id": good, "selfie_upload_id": selfie })).await.0,
        StatusCode::ACCEPTED
    );

    // Declared size is enforced by the signed URL.
    let (_, target) =
        t.post("/v1/uploads", &rider.access, json!({ "purpose": "selfie", "content_type": "image/png", "size_bytes": 10 })).await;
    let path = target["url"].as_str().unwrap().strip_prefix(PUBLIC_URL).unwrap().to_owned();
    let res = t
        .app
        .clone()
        .oneshot(Request::put(&path).header(header::CONTENT_TYPE, "image/png").body(Body::from(png())).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    // And the content type it was signed for.
    let res =
        t.app.clone().oneshot(Request::put(&path).header(header::CONTENT_TYPE, "text/html").body(Body::from("x")).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    // Identity photos can't be PDFs; driver documents can't be uploaded from the rider app.
    let (status, _) =
        t.post("/v1/uploads", &rider.access, json!({ "purpose": "identity", "content_type": "application/pdf", "size_bytes": 10 })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) =
        t.post("/v1/uploads", &rider.access, json!({ "purpose": "licence", "content_type": "image/png", "size_bytes": 10 })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn driver_onboarding_approval_and_document_expiry() {
    let t = setup!();
    let driver = t.onboarded("driver").await;
    let (status, app) = t.get("/v1/driver/application", &driver.access).await;
    assert_eq!(status, StatusCode::OK, "{app}");
    assert_eq!(app["status"], "not_started");
    assert_eq!(app["missing_documents"].as_array().unwrap().len(), 5);

    let application = json!({
        "requested_tiers": ["classic", "comfort", "classic"],
        "vehicle": { "make": "Toyota", "model": "Corolla", "colour": "Silver", "year": 2019, "plate": "n9090w", "seats": 4, "wheelchair_accessible": false },
        "payout_account": { "bank": "Bank Windhoek", "account_holder": "Ndeshi Shikongo", "account_number": "8001 234 567", "branch_code": "483872" }
    });
    let (status, app) = t.put("/v1/driver/application", &driver.access, application.clone()).await;
    assert_eq!(status, StatusCode::OK, "{app}");
    assert_eq!(app["vehicle"]["plate"], "N 9090 W");
    assert_eq!(app["requested_tiers"], json!(["classic", "comfort"]));
    assert_eq!(app["payout_account_saved"], true);
    assert_eq!(app["ready_to_submit"], false);
    assert_eq!(t.post("/v1/driver/application/submit", &driver.access, json!({})).await.0, StatusCode::CONFLICT);

    let next_year = format!("{}-06-30", OffsetDateTime::now_utc().year() + 1);
    for kind in ["identity", "selfie", "licence", "insurance", "police_clearance"] {
        let upload = t.photo(&driver.access, kind).await;
        let mut body = json!({ "kind": kind, "upload_id": upload });
        if !matches!(kind, "identity" | "selfie") {
            // Missing and past expiry dates are refused first.
            assert_eq!(t.post("/v1/driver/documents", &driver.access, body.clone()).await.0, StatusCode::UNPROCESSABLE_ENTITY);
            body["expires_on"] = json!("2020-01-01");
            assert_eq!(t.post("/v1/driver/documents", &driver.access, body.clone()).await.0, StatusCode::UNPROCESSABLE_ENTITY);
            body["expires_on"] = json!(next_year);
        }
        let (status, doc) = t.post("/v1/driver/documents", &driver.access, body).await;
        assert_eq!((status, doc["status"].as_str()), (StatusCode::CREATED, Some("in_review")), "{doc}");
    }
    // Replacing a document supersedes the old one.
    let licence2 = t.photo(&driver.access, "licence").await;
    assert_eq!(
        t.post("/v1/driver/documents", &driver.access, json!({ "kind": "licence", "upload_id": licence2, "expires_on": next_year }))
            .await
            .0,
        StatusCode::CREATED
    );

    let (status, app) = t.post("/v1/driver/application/submit", &driver.access, json!({})).await;
    assert_eq!((status, app["status"].as_str()), (StatusCode::OK, Some("pending")), "{app}");
    assert_eq!(app["documents"].as_array().unwrap().len(), 5);
    // Locked while under review.
    assert_eq!(t.put("/v1/driver/application", &driver.access, application).await.0, StatusCode::CONFLICT);

    let reviewer = t.staff("ops_reviewer").await;
    let (_, queue) = t.get("/v1/ops/drivers", &reviewer).await;
    assert!(queue.as_array().unwrap().iter().any(|q| q["user_id"] == driver.user_id.to_string()));
    let uri = format!("/v1/ops/drivers/{}", driver.user_id);
    // Approving before the parts are approved is refused, naming what's missing.
    let (status, body) = t.post(&format!("{uri}/decision"), &reviewer, json!({ "decision": "approve" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"]["message"].as_str().unwrap().contains("documents, vehicle, tiers"));

    let (_, review) = t.get(&uri, &reviewer).await;
    for doc in review["documents"].as_array().unwrap() {
        assert!(doc["url"].as_str().is_some());
        let (status, _) = t
            .post(&format!("/v1/ops/documents/{}/decision", doc["id"].as_str().unwrap()), &reviewer, json!({ "decision": "approve" }))
            .await;
        assert_eq!(status, StatusCode::OK);
    }
    let vehicle = review["vehicle"]["id"].as_str().unwrap();
    assert_eq!(
        t.post(&format!("/v1/ops/vehicles/{vehicle}/decision"), &reviewer, json!({ "decision": "approve" })).await.0,
        StatusCode::OK
    );
    let (status, review) = t.put(&format!("{uri}/tiers"), &reviewer, json!({ "tiers": ["classic"] })).await;
    assert_eq!((status, review["approved_tiers"].clone()), (StatusCode::OK, json!(["classic"])));
    let (status, review) = t.post(&format!("{uri}/decision"), &reviewer, json!({ "decision": "approve" })).await;
    assert_eq!(status, StatusCode::OK, "{review}");
    assert_eq!((review["application_status"].as_str(), review["can_go_online"].as_bool()), (Some("approved"), Some(true)));
    let (_, app) = t.get("/v1/driver/application", &driver.access).await;
    assert_eq!((app["status"].as_str(), app["can_go_online"].as_bool()), (Some("approved"), Some(true)));

    // Expiry (PRD A-5): a warning inside 7 days, then expiry takes the driver offline.
    let today = OffsetDateTime::now_utc().date();
    sqlx::query("UPDATE driver_documents SET expires_on = $2 WHERE driver_id = $1 AND kind = 'insurance' AND superseded_by IS NULL")
        .bind(driver.user_id)
        .bind(today + time::Duration::days(5))
        .execute(&t.state.db)
        .await
        .unwrap();
    fuber_worker::check_documents(&t.ctx, today).await.unwrap();
    fuber_worker::check_documents(&t.ctx, today).await.unwrap();
    let warnings = t.ctx.sms.outbox().iter().filter(|m| m.body.contains("insurance expires")).count();
    assert_eq!(warnings, 1, "one warning, even when the check runs twice");
    fuber_worker::check_documents(&t.ctx, today + time::Duration::days(6)).await.unwrap();
    let (_, app) = t.get("/v1/driver/application", &driver.access).await;
    assert_eq!(app["can_go_online"], false);
    assert!(app["missing_documents"].as_array().unwrap().contains(&json!("insurance")));
    assert!(t.ctx.sms.outbox().iter().any(|m| m.body.contains("insurance has expired")));
}

#[tokio::test]
async fn emergency_contacts_are_encrypted_and_validated() {
    let t = setup!();
    let rider = t.sign_in(&new_phone(), "rider").await;
    let contacts = json!({ "contacts": [{ "name": "Martha Shikongo", "phone": "081 222 3344" }, { "name": "Anna Nghifikwa", "phone": "+264 85 111 2222" }] });
    let (status, body) = t.put("/v1/me/emergency-contacts", &rider.access, contacts).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = t.get("/v1/me/emergency-contacts", &rider.access).await;
    assert_eq!(body["contacts"][0]["phone"], "+264812223344");
    assert_eq!(body["contacts"][1]["name"], "Anna Nghifikwa");
    let raw: Vec<Vec<u8>> = sqlx::query_scalar("SELECT phone FROM emergency_contacts WHERE user_id = $1")
        .bind(rider.user_id)
        .fetch_all(&t.state.db)
        .await
        .unwrap();
    assert!(raw.iter().all(|p| !p.windows(7).any(|w| w == b"2223344")));

    let (_, me) = t.get("/v1/me", &rider.access).await;
    let own = me["phone"].as_str().unwrap();
    let c = |phone: &str| json!({ "name": "Someone", "phone": phone });
    for bad in [
        json!({ "contacts": [c(own)] }),
        json!({ "contacts": [c("+264812223344"), c("0812223344")] }),
        json!({ "contacts": [c("+264811111111"), c("+264812222222"), c("+264813333333"), c("+264814444444")] }),
        json!({ "contacts": [c("+27821234567")] }),
        json!({ "contacts": [{ "name": "<script>", "phone": "+264811111111" }] }),
    ] {
        assert_eq!(t.put("/v1/me/emergency-contacts", &rider.access, bad.clone()).await.0, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
}

#[tokio::test]
async fn account_deletion_signs_out_and_erases_after_grace() {
    let t = setup!();
    let phone = new_phone();
    let rider = t.sign_in(&phone, "rider").await;
    t.patch("/v1/me", &rider.access, json!({ "full_name": "Ndeshi Shikongo", "date_of_birth": "1994-03-12" })).await;
    t.put("/v1/me/emergency-contacts", &rider.access, json!({ "contacts": [{ "name": "Martha", "phone": "+264812223344" }] })).await;
    t.photo(&rider.access, "identity").await;
    let favourite = t.approved_driver("Hilma Nangolo", "classic", false).await;
    sqlx::query("INSERT INTO favourite_drivers (rider_id, driver_id) VALUES ($1, $2)")
        .bind(rider.user_id)
        .bind(favourite.user_id)
        .execute(&t.state.db)
        .await
        .unwrap();

    let (status, body) = t.call(Method::DELETE, "/v1/me", Some(&rider.access), None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(t.get("/v1/me", &rider.access).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        t.call(Method::POST, "/v1/auth/refresh", None, Some(json!({ "refresh_token": rider.refresh }))).await.0,
        StatusCode::UNAUTHORIZED
    );

    // Signing in during the grace period is refused with an explanation.
    t.age_codes(&phone).await;
    let challenge = t.request_code(&phone).await;
    let code = t.last_code(&phone);
    let (status, body) = t
        .call(
            Method::POST,
            "/v1/auth/otp/verify",
            None,
            Some(json!({ "challenge_id": challenge, "phone": phone, "code": code, "app": "rider" })),
        )
        .await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("account_deleted")));

    // The erase job does nothing early, then erases after 30 days.
    fuber_worker::erase_account(&t.ctx, rider.user_id).await.unwrap();
    let still: Option<String> =
        sqlx::query_scalar("SELECT phone_e164 FROM users WHERE id = $1").bind(rider.user_id).fetch_one(&t.state.db).await.unwrap();
    assert!(still.is_some());
    sqlx::query("UPDATE users SET deleted_at = now() - interval '31 days' WHERE id = $1")
        .bind(rider.user_id)
        .execute(&t.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE jobs SET run_at = now() WHERE kind = 'account.erase' AND payload->>'user_id' = $1")
        .bind(rider.user_id.to_string())
        .execute(&t.state.db)
        .await
        .unwrap();
    t.work().await;
    let (phone_after, name, contacts, uploads, favourites): (Option<String>, Option<String>, i64, i64, i64) = sqlx::query_as(
        "SELECT phone_e164, full_name, (SELECT count(*) FROM emergency_contacts WHERE user_id = $1), (SELECT count(*) FROM uploads WHERE user_id = $1),
                (SELECT count(*) FROM favourite_drivers WHERE rider_id = $1)
         FROM users WHERE id = $1",
    )
    .bind(rider.user_id)
    .fetch_one(&t.state.db)
    .await
    .unwrap();
    assert_eq!((phone_after, name, contacts, uploads, favourites), (None, None, 0, 0, 0));
    // The number is free to sign up again as a new person.
    let fresh = t.sign_in(&phone, "rider").await;
    assert!(fresh.new_user);
    assert_ne!(fresh.user_id, rider.user_id);
}

#[tokio::test]
async fn one_driver_device_at_a_time_but_rider_and_driver_together() {
    let t = setup!();
    let phone = new_phone();
    let first = t.sign_in(&phone, "driver").await;
    let rider = t.sign_in(&phone, "rider").await;
    assert_eq!(rider.user_id, first.user_id, "one person, both apps (PRD A-9)");
    let second = t.sign_in(&phone, "driver").await;
    assert_eq!(t.get("/v1/driver/application", &first.access).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(t.get("/v1/driver/application", &second.access).await.0, StatusCode::OK);
    assert_eq!(t.get("/v1/me", &rider.access).await.0, StatusCode::OK);
}

/// Every Phase 1 endpoint refuses missing or forged credentials, the wrong app, and other
/// people's objects (TRD §7.2: "wrong user" test for every endpoint).
#[tokio::test]
async fn wrong_user_and_unauthenticated_access_is_refused_everywhere() {
    let t = setup!();
    let alice = t.onboarded("rider").await;
    let mallory = t.onboarded("rider").await;
    let mallory_driver = t.onboarded("driver").await;
    let alice_upload = t.send_upload(&alice.access, "identity", "image/png", &png()).await;
    let any = Uuid::new_v4();

    let protected: Vec<(Method, String)> = vec![
        (Method::POST, "/v1/auth/logout".into()),
        (Method::POST, "/v1/consents".into()),
        (Method::GET, "/v1/me".into()),
        (Method::PATCH, "/v1/me".into()),
        (Method::DELETE, "/v1/me".into()),
        (Method::GET, "/v1/me/emergency-contacts".into()),
        (Method::PUT, "/v1/me/emergency-contacts".into()),
        (Method::POST, "/v1/me/verification".into()),
        (Method::POST, "/v1/uploads".into()),
        (Method::GET, format!("/v1/uploads/{alice_upload}")),
        (Method::POST, format!("/v1/uploads/{alice_upload}/complete")),
        (Method::GET, "/v1/driver/application".into()),
        (Method::PUT, "/v1/driver/application".into()),
        (Method::POST, "/v1/driver/application/submit".into()),
        (Method::POST, "/v1/driver/documents".into()),
    ];
    let forged = {
        // A well-formed token signed with another key.
        let other = fuber_common::crypto::TokenSigner::new("k1", &"another-secret-0123456789abcdef0123456".to_owned().into(), &[]);
        other.sign(&json!({ "iss": "fuber-api", "sub": alice.user_id, "role": "rider", "sid": any, "iat": 0, "exp": 4_000_000_000_i64 }))
    };
    for (m, uri) in &protected {
        assert_eq!(t.call(m.clone(), uri, None, Some(json!({}))).await.0, StatusCode::UNAUTHORIZED, "{m} {uri} without token");
        assert_eq!(t.call(m.clone(), uri, Some(&forged), Some(json!({}))).await.0, StatusCode::UNAUTHORIZED, "{m} {uri} forged token");
    }

    // Other people's uploads look like missing ones.
    assert_eq!(t.get(&format!("/v1/uploads/{alice_upload}"), &mallory.access).await.0, StatusCode::NOT_FOUND);
    assert_eq!(t.post(&format!("/v1/uploads/{alice_upload}/complete"), &mallory.access, json!({})).await.0, StatusCode::NOT_FOUND);
    t.work().await;
    let mine = t.photo(&mallory.access, "selfie").await;
    let (status, _) =
        t.post("/v1/me/verification", &mallory.access, json!({ "id_document_upload_id": alice_upload, "selfie_upload_id": mine })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "someone else's ID photo");
    let (status, _) =
        t.post("/v1/driver/documents", &mallory_driver.access, json!({ "kind": "identity", "upload_id": alice_upload })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "someone else's ID photo as a driver document");

    // Wrong app.
    for (m, uri) in [
        (Method::GET, "/v1/driver/application"),
        (Method::PUT, "/v1/driver/application"),
        (Method::POST, "/v1/driver/documents"),
        (Method::POST, "/v1/driver/application/submit"),
    ] {
        assert_eq!(t.call(m, uri, Some(&mallory.access), Some(json!({}))).await.0, StatusCode::FORBIDDEN, "rider token on {uri}");
    }
    assert_eq!(t.post("/v1/me/verification", &mallory_driver.access, json!({})).await.0, StatusCode::FORBIDDEN);

    // Ops endpoints: no token, a rider token, and a staff role without review rights.
    let support = t.staff("ops_support").await;
    let ops: Vec<(Method, String)> = vec![
        (Method::GET, "/v1/ops/verifications".into()),
        (Method::GET, format!("/v1/ops/verifications/{any}")),
        (Method::POST, format!("/v1/ops/verifications/{any}/decision")),
        (Method::GET, "/v1/ops/drivers".into()),
        (Method::GET, format!("/v1/ops/drivers/{any}")),
        (Method::PUT, format!("/v1/ops/drivers/{any}/tiers")),
        (Method::POST, format!("/v1/ops/drivers/{any}/decision")),
        (Method::POST, format!("/v1/ops/documents/{any}/decision")),
        (Method::POST, format!("/v1/ops/vehicles/{any}/decision")),
    ];
    for (m, uri) in &ops {
        assert_eq!(t.call(m.clone(), uri, None, None).await.0, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(t.call(m.clone(), uri, Some(&alice.access), None).await.0, StatusCode::UNAUTHORIZED, "rider token on {uri}");
        let body = if *m == Method::GET {
            None
        } else if uri.ends_with("tiers") {
            Some(json!({ "tiers": [] }))
        } else {
            Some(json!({ "decision": "approve" }))
        };
        assert_eq!(t.call(m.clone(), uri, Some(&support), body).await.0, StatusCode::FORBIDDEN, "ops_support on {uri}");
    }
    assert_eq!(t.call(Method::POST, "/v1/ops/auth/logout", None, None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn validation_failures_on_every_new_body() {
    let t = setup!();
    let rider = t.onboarded("rider").await;
    let driver = t.onboarded("driver").await;
    let unknown = |mut v: Value| {
        v["admin"] = json!(true);
        v
    };
    let cases: Vec<(&str, Method, &str, Value)> = vec![
        ("public", Method::POST, "/v1/auth/otp/request", json!({ "phone": "+27821234567" })),
        ("public", Method::POST, "/v1/auth/otp/request", unknown(json!({ "phone": new_phone() }))),
        (
            "public",
            Method::POST,
            "/v1/auth/otp/verify",
            json!({ "challenge_id": Uuid::new_v4(), "phone": new_phone(), "code": "12345", "app": "rider" }),
        ),
        (
            "public",
            Method::POST,
            "/v1/auth/otp/verify",
            json!({ "challenge_id": Uuid::new_v4(), "phone": new_phone(), "code": "123456", "app": "admin" }),
        ),
        ("public", Method::POST, "/v1/auth/refresh", json!({ "refresh_token": "short" })),
        ("public", Method::POST, "/v1/ops/auth/login", json!({ "email": "not-an-email", "password": "x", "totp": "123456" })),
        ("rider", Method::PATCH, "/v1/me", json!({ "date_of_birth": "2015-01-01" })),
        ("rider", Method::PATCH, "/v1/me", json!({ "full_name": "R2-D2 <b>" })),
        ("rider", Method::PATCH, "/v1/me", unknown(json!({}))),
        ("rider", Method::POST, "/v1/consents", json!({ "document_ids": [Uuid::new_v4()] })),
        ("rider", Method::POST, "/v1/consents", json!({ "document_ids": [] })),
        ("rider", Method::POST, "/v1/uploads", json!({ "purpose": "selfie", "content_type": "image/gif", "size_bytes": 10 })),
        ("rider", Method::POST, "/v1/uploads", json!({ "purpose": "selfie", "content_type": "image/png", "size_bytes": 20_000_000 })),
        ("rider", Method::POST, "/v1/me/verification", json!({ "id_document_upload_id": "x", "selfie_upload_id": Uuid::new_v4() })),
        (
            "driver",
            Method::PUT,
            "/v1/driver/application",
            json!({ "requested_tiers": [], "vehicle": { "make": "Toyota", "model": "Corolla", "colour": "Silver", "year": 2019, "plate": "N 1 W", "seats": 4, "wheelchair_accessible": false } }),
        ),
        (
            "driver",
            Method::PUT,
            "/v1/driver/application",
            json!({ "requested_tiers": ["classic"], "vehicle": { "make": "Toyota", "model": "Corolla", "colour": "Silver", "year": 2019, "plate": "CA 123-456", "seats": 4, "wheelchair_accessible": false } }),
        ),
        (
            "driver",
            Method::PUT,
            "/v1/driver/application",
            json!({ "requested_tiers": ["classic"], "vehicle": { "make": "Toyota", "model": "Corolla", "colour": "Silver", "year": 2019, "plate": "N 1 W", "seats": 2, "wheelchair_accessible": false } }),
        ),
        ("driver", Method::PUT, "/v1/driver/application", json!({ "requested_tiers": ["helicopter"], "vehicle": {} })),
        ("driver", Method::POST, "/v1/driver/documents", json!({ "kind": "passport", "upload_id": Uuid::new_v4() })),
    ];
    for (who, m, uri, body) in cases {
        let token = match who {
            "rider" => Some(rider.access.as_str()),
            "driver" => Some(driver.access.as_str()),
            _ => None,
        };
        let (status, resp) = t.call(m.clone(), uri, token, Some(body.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{m} {uri} {body} → {resp}");
        assert!(resp["error"]["code"].is_string());
    }
}

#[tokio::test]
async fn legal_documents_are_public_and_rate_limited_routes_answer_429() {
    let t = setup!();
    let (status, doc) = t.call(Method::GET, "/v1/legal/terms/latest", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(doc["version"].as_str().unwrap().starts_with("draft-"));
    assert_eq!(t.call(Method::GET, "/v1/legal/driver_agreement/latest", None, None).await.0, StatusCode::NOT_FOUND, "not drafted yet");
    assert_eq!(t.call(Method::GET, "/v1/legal/nonsense/latest", None, None).await.0, StatusCode::BAD_REQUEST);

    // Three codes per number per 10 minutes, whatever the cooldown.
    let phone = new_phone();
    for _ in 0..3 {
        t.request_code(&phone).await;
        t.age_codes(&phone).await;
    }
    let (status, _) = t.call(Method::POST, "/v1/auth/otp/request", None, Some(json!({ "phone": phone }))).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

/// An approved licence uploaded `age` ago (a Postgres interval).
async fn old_document(db: &sqlx::PgPool, driver: Uuid, age: &str) {
    sqlx::query(
        "INSERT INTO driver_documents (driver_id, kind, storage_key, mime_type, size_bytes, sha256, status, created_at)
         VALUES ($1, 'licence', 'docs/' || gen_random_uuid(), 'image/jpeg', 1000, '\\x00', 'approved', now() - $2::interval)",
    )
    .bind(driver)
    .bind(age)
    .execute(db)
    .await
    .unwrap();
}

/// An 8-hour shift that started `ago` (a Postgres interval).
async fn old_shift(db: &sqlx::PgPool, driver: Uuid, ago: &str) {
    sqlx::query(
        "INSERT INTO driver_shifts (driver_id, vehicle_id, tier, started_at, ended_at)
         SELECT $1, active_vehicle_id, 'classic', now() - $2::interval, now() - $2::interval + interval '8 hours'
         FROM driver_profiles WHERE user_id = $1",
    )
    .bind(driver)
    .bind(ago)
    .execute(db)
    .await
    .unwrap();
}

/// Her documents left, and whether she may still go online.
async fn documents_left(db: &sqlx::PgPool, driver: Uuid) -> (i64, bool) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM driver_documents WHERE driver_id = $1), can_go_online FROM driver_profiles WHERE user_id = $1",
    )
    .bind(driver)
    .fetch_one(db)
    .await
    .unwrap()
}

#[tokio::test]
async fn driver_documents_go_a_year_after_she_stops_driving() {
    let t = setup!();
    let db = &t.state.db;
    // Stopped driving two years ago, drove last month, and never drove since applying 18 months ago.
    let stopped = t.approved_driver("Selma Nghipandulwa", "classic", false).await;
    let active = t.approved_driver("Hilma Nangolo", "classic", false).await;
    let never = t.approved_driver("Ester Shilongo", "classic", false).await;
    old_document(db, stopped.user_id, "2 years").await;
    old_document(db, active.user_id, "2 years").await;
    old_document(db, never.user_id, "18 months").await;
    old_shift(db, stopped.user_id, "2 years").await;
    old_shift(db, active.user_id, "30 days").await;

    fuber_worker::cleanup(&t.ctx).await.unwrap();

    assert_eq!(documents_left(db, stopped.user_id).await, (0, false));
    assert_eq!(documents_left(db, never.user_id).await, (0, false));
    assert_eq!(documents_left(db, active.user_id).await, (1, true));
}

#[tokio::test]
async fn erased_riders_rides_are_anonymised() {
    let t = setup!();
    let db = &t.state.db;
    let trip = t.completed().await;
    let ride: Uuid = trip.id.parse().unwrap();
    sqlx::query("UPDATE rides SET guest_name = 'Martha', guest_phone = decode('00', 'hex') WHERE id = $1")
        .bind(ride)
        .execute(db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO ride_track_points (ride_id, at, location) VALUES ($1, now(), 'POINT(17.0870 -22.5750)')")
        .bind(ride)
        .execute(db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET status = 'deleted', deleted_at = now() - interval '31 days' WHERE id = $1")
        .bind(trip.rider.user_id)
        .execute(db)
        .await
        .unwrap();

    fuber_worker::erase_account(&t.ctx, trip.rider.user_id).await.unwrap();

    let (guest, address, pickup, tracks): (Option<String>, String, String, i64) = sqlx::query_as(
        "SELECT guest_name, pickup_address, ST_AsText(pickup, 6), (SELECT count(*) FROM ride_track_points WHERE ride_id = $1)
         FROM rides WHERE id = $1",
    )
    .bind(ride)
    .fetch_one(db)
    .await
    .unwrap();
    assert_eq!((guest, address.as_str(), tracks), (None, "", 0));
    // The ride is still there for the accounts, its pickup only to ~1 km.
    assert_eq!(pickup, "POINT(17.08 -22.57)");
}
