use crate::tests::util::*;

use chrono::Duration;
use rocket::http::Status;
use rocket::serde::json;

#[test]
fn session_index_requires_auth() {
    let client = client_tracked_get();
    let response = client.get("/api/session/").dispatch();
    assert_eq!(response.status(), Status::Unauthorized);

    let email = email_for_session();
    let user_id = seed_user(&client, &email);
    let response = client
        .get("/api/session/")
        .private_cookie(auth_cookie(user_id))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let body = response.into_json::<json::Value>().unwrap();
    assert_eq!(body, json::json!({ "id": user_id }));
}

#[test]
fn session_login_success_sets_cookie_and_clears_metadata() {
    let client = client_tracked_get();
    let email = email_for_session();
    let code = CODE_EXAMPLE; // Use shared constant
    let created_at = NaiveDateTime::now();
    let (user_id, _) = seed_user_with_code(&client, &email, code, Some(0), created_at);

    let response = client
        .post("/api/session/login")
        .json(&json::json!({ "email": email, "code": code }))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let cookie = response.cookies().get_private("user_id").map(|c| c.value().to_string());
    assert_eq!(cookie, Some(user_id.to_string()));

    let user = fetch_user_by_email(&client, &email);
    assert_eq!(user.id, user_id);
    assert!(user.code_hash.is_none());
    assert!(user.code_created_at.is_none());
    assert!(user.code_attempts.is_none());
}

#[test]
fn session_login_rejects_invalid_code_format() {
    let client = client_tracked_get();
    let email = email_for_session();
    seed_user_with_code(&client, &email, CODE_EXAMPLE, Some(0), NaiveDateTime::now()); // Use shared constant

    let response = client
        .post("/api/session/login")
        .json(&json::json!({ "email": email, "code": "12ab" }))
        .dispatch();
    assert_eq!(response.status(), Status::Unauthorized);
}

#[test]
fn session_login_rejects_expired_code() {
    let client = client_tracked_get();
    let email = email_for_session();
    let expired_at = NaiveDateTime::now() - Duration::minutes(11);
    seed_user_with_code(&client, &email, CODE_EXAMPLE, Some(0), expired_at); // Use shared constant

    let response = client
        .post("/api/session/login")
        .json(&json::json!({ "email": email, "code": CODE_EXAMPLE })) // Use shared constant
        .dispatch();
    assert_eq!(response.status(), Status::Unauthorized);
}

#[test]
fn session_login_increments_attempts_on_failure() {
    let client = client_tracked_get();
    let email = email_for_session();
    seed_user_with_code(&client, &email, CODE_EXAMPLE, Some(0), NaiveDateTime::now()); // Use shared constant

    let response = client
        .post("/api/session/login")
        .json(&json::json!({ "email": email, "code": "99999999" })) // Updated invalid code to 8 digits
        .dispatch();
    assert_eq!(response.status(), Status::Unauthorized);

    let user = fetch_user_by_email(&client, &email);
    assert_eq!(user.code_attempts, Some(1));
}

#[test]
fn session_login_rejects_exhausted_attempts() {
    let client = client_tracked_get();
    let email = email_for_session();
    seed_user_with_code(&client, &email, CODE_EXAMPLE, Some(3), NaiveDateTime::now()); // Use shared constant

    let response = client
        .post("/api/session/login")
        .json(&json::json!({ "email": email, "code": CODE_EXAMPLE })) // Use shared constant
        .dispatch();
    assert_eq!(response.status(), Status::Unauthorized);
}

#[test]
fn session_logout_clears_cookie() {
    let client = client_tracked_get();
    let email = email_for_session();
    let user_id = seed_user(&client, &email);
    client.cookies().add_private(auth_cookie(user_id));

    let response = client.post("/api/session/logout").dispatch();
    assert_success(response, Status::Ok);
    assert!(client.cookies().get_private("user_id").is_none());

    let follow_up = client.get("/api/session/").dispatch();
    assert_eq!(follow_up.status(), Status::Unauthorized);
}

#[test]
fn session_send_code_updates_existing_user() {
    let client = client_tracked_get();
    let email = email_for_session();
    let old_time = NaiveDateTime::now() - Duration::minutes(5);
    let (_, old_hash) = seed_user_with_code(&client, &email, CODE_EXAMPLE, Some(1), old_time); // Use shared constant

    let response = client
        .post("/api/session/send-code")
        .json(&json::json!({ "email": email }))
        .dispatch();
    assert_success(response, Status::Ok);

    let user = fetch_user_by_email(&client, &email);
    assert_eq!(user.code_attempts, Some(0));
    let updated_at = user.code_created_at.expect("code_created_at");
    assert!(updated_at > old_time);
    let new_hash = user.code_hash.expect("code_hash");
    assert_ne!(new_hash, old_hash);
}

#[test]
fn session_send_code_rate_limits_recent_requests() {
    let client = client_tracked_get();
    let email = email_for_session();
    let recent = NaiveDateTime::now();
    seed_user_with_code(&client, &email, CODE_EXAMPLE, Some(0), recent); // Use shared constant

    let response = client
        .post("/api/session/send-code")
        .json(&json::json!({ "email": email }))
        .dispatch();
    assert_eq!(response.status(), Status::TooManyRequests);

    let user = fetch_user_by_email(&client, &email);
    assert_eq!(user.code_created_at, Some(recent));
}

#[test]
fn session_send_code_creates_user() {
    let client = client_tracked_get();
    let email = email_for_session();

    let response = client
        .post("/api/session/send-code")
        .json(&json::json!({ "email": email }))
        .dispatch();
    assert_success(response, Status::Ok);

    let user = fetch_user_by_email(&client, &email);
    assert_eq!(user.email, email);
    assert_eq!(user.code_attempts, Some(0));
    assert!(user.code_hash.is_some());
}
