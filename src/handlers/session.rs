use chrono::{Duration, Utc};
use rocket::fairing::AdHoc;
use rocket::http::{CookieJar, Status};
use rocket::serde::{Deserialize, json};

use crate::db::*;
use crate::util::*;

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct SendCodeRequestBody<'r> {
    email: &'r str,
}

#[derive(Deserialize)]
#[serde(crate = "rocket::serde")]
struct LoginRequestBody<'r> {
    code: &'r str,
    email: &'r str,
}

#[get("/")]
fn index(user: UserCtx) -> json::Value {
    json::json!(user)
}

#[post("/login", data = "<body>")]
async fn login(
    jar: &CookieJar<'_>,
    mut db: Connection<Db>,
    body: json::Json<LoginRequestBody<'_>>,
) -> (Status, json::Value) {
    let unauthorized = (
        Status::Unauthorized,
        json::json!({ "message": "invalid email or password" }),
    );

    if !code_is_valid(body.code) {
        info!("login:code-invalid");
        return unauthorized;
    }

    if !email_is_valid(body.email) {
        info!("login:email-invalid");
        return unauthorized;
    }

    let user = sqlx::query!("SELECT * FROM users WHERE email = ?", body.email)
        .fetch_one(&mut **db)
        .await;

    let user = match user {
        Ok(user) => user,
        Err(_) => {
            return unauthorized;
        }
    };

    if user.code_hash.is_none() {
        info!("login:unavailable:{}", user.id);
        return unauthorized;
    }

    let code_attempts = user.code_attempts.expect("code_attempts is unexpectedly NULL");
    if code_attempts > 2 {
        info!("login:exhuasted:{}", user.id);
        return unauthorized;
    }

    let code_created_at = user
        .code_created_at
        .expect("code_created_at is unexpectedly NULL")
        .to_datetime();
    let ten_minutes_ago = Utc::now() - Duration::minutes(10);
    if code_created_at < ten_minutes_ago {
        info!("login:expired:{}", user.id);
        return unauthorized;
    }

    let code_verified = hash_code_verify(user.code_hash.as_deref().expect("unreachable"), body.code)
        .await
        .unwrap_or(false);

    if !code_verified {
        let new_attempts = user.code_attempts.unwrap_or(0) + 1;
        sqlx::query!("UPDATE users SET code_attempts = ? WHERE id = ?", new_attempts, user.id)
            .execute(&mut **db)
            .await
            .expect("Failed to increment code attempts");
        info!("login:bad-code:{}", user.id);
        return unauthorized;
    }

    // clear the code_hash on the user
    sqlx::query!(
        "UPDATE users SET code_attempts = NULL, code_created_at = NULL, code_hash = NULL WHERE id = ?",
        user.id
    )
    .execute(&mut **db)
    .await
    .expect("Failed to clear user code");

    jar.add_private(auth_cookie(user.id));

    (Status::Ok, json::json!({ "message": "success" }))
}

#[post("/logout")]
fn logout(jar: &CookieJar<'_>) -> (Status, json::Value) {
    jar.remove_private("user_id");
    (Status::Ok, json::json!({ "message": "success" }))
}

#[post("/send-code", data = "<body>")]
async fn send_code(mut db: Connection<Db>, body: json::Json<SendCodeRequestBody<'_>>) -> (Status, json::Value) {
    if !email_is_valid(body.email) {
        return (Status::Unauthorized, json::json!({ "message": "invalid email" }));
    }

    let code: String = (0..8)
        .map(|_| rand::random::<u8>() % 10)
        .map(|digit| digit.to_string())
        .collect();

    let code_hash = match hash_code(&code).await {
        Ok(hash) => hash,
        Err(e) => {
            return (Status::InternalServerError, json::json!({ "error": e }));
        }
    };

    let user_partial = sqlx::query!("SELECT id, code_created_at FROM users WHERE email = ?", body.email)
        .fetch_one(&mut **db)
        .await;

    match user_partial {
        Ok(record) => {
            if let Some(code_created_at) = record.code_created_at {
                let code_created_at = code_created_at.to_datetime();
                let two_minutes_ago: chrono::DateTime<Utc> = Utc::now() - Duration::minutes(2);
                if code_created_at > two_minutes_ago {
                    return (
                        Status::TooManyRequests,
                        json::json!({ "message": "Wait 2 minutes after requesting a code to try again." }),
                    );
                }
            }

            let now = NaiveDateTime::now();
            sqlx::query!(
                "UPDATE users SET code_attempts = 0, code_created_at = ?, code_hash = ? WHERE id = ?",
                now,
                code_hash,
                record.id
            )
            .execute(&mut **db)
            .await
            .expect("Failed to update user code");
        }
        Err(sqlx::Error::RowNotFound) => {
            let now = NaiveDateTime::now();
            sqlx::query!(
                "INSERT INTO users (code_attempts, code_created_at, code_hash, email) VALUES (0, ?, ?, ?)",
                now,
                code_hash,
                body.email,
            )
            .execute(&mut **db)
            .await
            .expect("Failed to insert new user");
        }
        Err(e) => {
            return (
                Status::InternalServerError,
                json::json!({ "error": format!("{:?}", e) }),
            );
        }
    }

    email_send(
        "codes@example.com",
        body.email,
        "[ROCKET] Your login code",
        &format!("Your login code is: {}. It will expire in 5 minutes.", code),
    )
    .await;
    (Status::Ok, json::json!({ "message": "success" }))
}

pub fn stage() -> AdHoc {
    AdHoc::on_ignite("Session stage", |rocket| async {
        rocket.mount("/api/session", routes![index, login, logout, send_code])
    })
}
