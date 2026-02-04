use argon2::password_hash::{PasswordHash, SaltString, rand_core::OsRng};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
pub use chrono::NaiveDateTime;
pub use chrono::{DateTime, Utc};
pub use futures::{future::TryFutureExt, stream::TryStreamExt};
use mail_struct::Mail;
use once_cell::sync::Lazy;
use regex::Regex;
use rocket::http;
use rocket::outcome::IntoOutcome;
use rocket::request;
use rocket::serde::{self, Deserialize, Serialize};
use rocket::tokio::sync::Semaphore;
use rocket::tokio::task::spawn_blocking;
use rocket::tokio::time::{Duration, timeout};
use rocket::{Request, futures};
use smtp_send::Send;
use std::{env, sync::OnceLock};

/// Returns the application mode as a string: "debug" if the profile is "debug", otherwise "production".
pub fn app_mode() -> &'static str {
    static MODE: OnceLock<&'static str> = OnceLock::new();
    *MODE.get_or_init(|| {
        let profile = rocket::Config::figment().profile().to_string();
        if profile == "debug" { "debug" } else { "production" }
    })
}

pub fn auth_cookie(user_id: i64) -> http::Cookie<'static> {
    http::Cookie::build(("user_id", user_id.to_string()))
        .http_only(false)
        .build()
}

/// Validates if the given email is in a valid format.
pub fn email_is_valid(email: &str) -> bool {
    static EMAIL_RE: OnceLock<Regex> = OnceLock::new();
    let regex =
        EMAIL_RE.get_or_init(|| Regex::new(r"^[^\s@]+@[^\s@]+\.[^\s@]+$").expect("failed to compile email regex"));

    regex.is_match(email)
}

/// Struct to hold required environment variables.
#[derive(Debug)]
pub struct EnvVars {
    pub database_url: String,
    pub rocket_databases: String,
    pub dkim_key_public: String,
    pub dkim_key_private: String,
    pub rocket_secret_key: String,
}

/// Loads and validates required environment variables into an `EnvVars` struct.
/// Throws an error if any required variable is missing.
pub fn env_get() -> &'static EnvVars {
    static ENV_VARS: OnceLock<EnvVars> = OnceLock::new();

    ENV_VARS.get_or_init(|| EnvVars {
        database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
        rocket_databases: env::var("ROCKET_DATABASES").expect("ROCKET_DATABASES must be set"),
        dkim_key_public: env::var("DKIM_KEY_PUBLIC")
            .expect("DKIM_KEY_PUBLIC must be set")
            .replace("\\n", "\n"),
        dkim_key_private: env::var("DKIM_KEY_PRIVATE")
            .expect("DKIM_KEY_PRIVATE must be set")
            .replace("\\n", "\n"),
        rocket_secret_key: env::var("ROCKET_SECRET_KEY").expect("ROCKET_SECRET_KEY must be set"),
    })
}

/// Validates if the given code is a 8-digit numeric string.
pub fn code_is_valid(code: &str) -> bool {
    code.len() == 8 && code.chars().all(|c| c.is_ascii_digit())
}

/// Sends an email using the `smtp_send` crate with DKIM signing.
pub async fn email_send(from: &str, to: &str, subject: &str, body: &str) {
    if app_mode() == "debug" {
        info!(
            "Email send simulated (debug mode): from={}, to={}, subject={}, body={}",
            from, to, subject, body
        );
        return;
    }

    let sk = env_get().dkim_key_private.as_bytes().to_vec();

    // Create sender with DKIM selector
    let sender = Send::new("default", &sk);

    // Build email
    let mut mail = Mail::new(
        from,
        [to],
        // b"Subject: Test\r\n\r\nHello".to_vec(),
        format!("Subject: {}\r\n\r\n{}", subject, body).into_bytes(),
    )
    .unwrap();

    // Send email
    let result = sender.send(&mut mail).await;

    println!("sent: {}, errors: {}", result.success, result.error_li.len());
}

/// Returns a static semaphore for limiting concurrent hashing operations.
fn hash_semaphore() -> &'static Semaphore {
    static SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| Semaphore::const_new(8))
}

/// Hashes the given code using the Argon2 algorithm.
/// Returns the hashed code as a `String` or an error message.
pub async fn hash_code(code: &str) -> Result<String, &'static str> {
    let _permit = hash_semaphore().acquire().await.map_err(|_| "semaphore closed")?;
    let salt = SaltString::generate(&mut OsRng);
    // Here we reduce memory cost because the default is much higher than we need for a temporal code
    // and we don't have a big server
    let params = argon2::Params::new(3000, 3, 4, None).unwrap();
    let argon2 = Argon2::new(argon2::Algorithm::Argon2i, argon2::Version::V0x13, params);
    // let argon2 = Argon2::default(); // defaults, which are more resource intensive
    let code = code.as_bytes().to_vec();
    let result = timeout(
        Duration::from_secs(5),
        spawn_blocking(move || argon2.hash_password(&code, &salt).map(|hash| hash.to_string())),
    )
    .await
    .map_err(|_| "hash timeout")?;

    result.map_err(|_| "hash join error")?.map_err(|_| "hash error")
}

/// Verifies if the given code matches the provided hash using the Argon2 algorithm.
/// Returns `true` if the code matches, otherwise `false`.
pub async fn hash_code_verify(hash: &str, code: &str) -> Result<bool, &'static str> {
    let _permit = hash_semaphore().acquire().await.map_err(|_| "semaphore closed")?;
    // Here we reduce memory cost because the default is much higher than we need for a temporal code
    // and we don't have a big server
    let params = argon2::Params::new(3000, 3, 4, None).unwrap();
    let argon2 = Argon2::new(argon2::Algorithm::Argon2i, argon2::Version::V0x13, params);
    // let argon2 = Argon2::default(); // defaults, which are more resource intensive
    let hash = hash.to_owned();
    let code = code.as_bytes().to_vec();
    let result = timeout(
        Duration::from_secs(5),
        spawn_blocking(move || {
            let parsed_hash = match PasswordHash::new(&hash) {
                Ok(h) => h,
                Err(_) => return Ok(false),
            };
            Ok(argon2.verify_password(&code, &parsed_hash).is_ok())
        }),
    )
    .await
    .map_err(|_| "verify timeout")?;

    result.map_err(|_| "verify join error")?
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(crate = "rocket::serde")]
pub struct MessageResponse {
    pub message: String,
}

pub static MESSAGE_RESPONSE_SUCCESS: Lazy<MessageResponse> = Lazy::new(|| MessageResponse {
    message: "success".into(),
});

// MessageResponse { message: "success".into() }
// do teh above as a static var

/// Extension trait for `NaiveDateTime` providing additional utility methods.
pub trait NaiveDateTimeExt {
    fn now() -> NaiveDateTime;
    fn parse_from_rfc3339(timestamp: String) -> NaiveDateTime;
    fn serializer<S>(ndt: &NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer;
    fn deserializer<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
    where
        D: serde::Deserializer<'de>;
    fn serializer_option<S>(ndt: &Option<NaiveDateTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer;
    fn deserializer_option<'de, D>(deserializer: D) -> Result<Option<NaiveDateTime>, D::Error>
    where
        D: serde::Deserializer<'de>;
    fn to_datetime(self) -> DateTime<Utc>;
    fn to_rfc3339(self) -> String;
}

impl NaiveDateTimeExt for NaiveDateTime {
    /// Returns the current UTC time as a `NaiveDateTime`.
    fn now() -> NaiveDateTime {
        let now: DateTime<Utc> = chrono::Utc::now();
        now.naive_utc()
    }

    /// Parses a timestamp in RFC3339 format into a `NaiveDateTime`.
    fn parse_from_rfc3339(timestamp: String) -> NaiveDateTime {
        let dt = DateTime::parse_from_rfc3339(&timestamp)
            .unwrap_or_else(|_| panic!("Invalid timestamp format: {}", timestamp));
        dt.naive_utc()
    }

    /// Converts a `NaiveDateTime` to a `DateTime<Utc>`.
    fn to_datetime(self) -> DateTime<Utc> {
        self.and_utc()
    }

    /// Converts a `NaiveDateTime` to an RFC3339 formatted string.
    fn to_rfc3339(self) -> String {
        let str = self.to_datetime().to_rfc3339();
        // replace +00:00 with Z for UTC timezone
        str.replace("+00:00", "Z")
    }

    /// Serializes a `NaiveDateTime` into a string using RFC3339 format.
    fn serializer<S>(ndt: &NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = ndt.to_rfc3339();
        serializer.serialize_str(&s)
    }

    /// deserializes a `NaiveDateTime` from a string in RFC3339 format.
    /// The standard deserializer for NaiveDateTime does not support timezone
    /// '+08:00' or 'Z' suffix for UTC timezone, use the DateTime parser instead
    ///  and convert to NaiveDateTime
    fn deserializer<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        let dt = DateTime::parse_from_rfc3339(&s)
            .map_err(|_| serde::de::Error::custom(format!("Invalid timestamp format: {}", s)))?;
        Ok(dt.naive_utc())
    }

    fn serializer_option<S>(ndt: &Option<NaiveDateTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match ndt {
            Some(value) => {
                let s = value.to_rfc3339();
                serializer.serialize_some(&s)
            }
            None => serializer.serialize_none(),
        }
    }

    fn deserializer_option<'de, D>(deserializer: D) -> Result<Option<NaiveDateTime>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value: Option<String> = serde::Deserialize::deserialize(deserializer)?;
        value
            .map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.naive_utc())
                    .map_err(|_| serde::de::Error::custom(format!("Invalid timestamp format: {}", s)))
            })
            .transpose()
    }
}

/// Represents the user context extracted from request cookies.
#[derive(Debug, serde::Serialize)]
#[serde(crate = "rocket::serde")]
pub struct UserCtx {
    pub id: i64,
}

/// Extracts the user context from the request cookies for convenient access.
#[rocket::async_trait]
impl<'r> request::FromRequest<'r> for UserCtx {
    type Error = std::convert::Infallible;

    async fn from_request(request: &'r Request<'_>) -> request::Outcome<UserCtx, Self::Error> {
        request
            .cookies()
            .get_private("user_id")
            .and_then(|cookie| cookie.value().parse().ok())
            .map(|id| UserCtx { id })
            .or_forward(http::Status::Unauthorized)
    }
}
