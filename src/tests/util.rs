use std::env;
use std::fs;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use rocket::http::Status;
use rocket::local::blocking::{Client, LocalRequest, LocalResponse};
use rocket::serde::Serialize;
use rocket::tokio::runtime::Runtime;
use rocket_db_pools::Database;

use crate::db;
use crate::handlers;
pub use crate::util::*;

static DB_ENV_MUTEX: Mutex<()> = Mutex::new(());
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

pub(super) struct ClientAuthenticated {
    inner: Client,
    user_id: i64,
}

impl ClientAuthenticated {
    pub(super) fn new() -> Self {
        let client = client_tracked_get();
        let email = format!("user+{}@example.com", next_sequence());
        let user_id = seed_user(&client, &email);
        Self { inner: client, user_id }
    }

    pub(super) fn get<'c>(&'c self, uri: &'c str) -> LocalResponse<'c> {
        self.with_auth(self.inner.get(uri)).dispatch()
    }

    pub(super) fn post_json<'c, T>(&'c self, uri: &'c str, body: &T) -> LocalResponse<'c>
    where
        T: Serialize,
    {
        self.with_auth(self.inner.post(uri).json(body)).dispatch()
    }

    pub(super) fn put_json<'c, T>(&'c self, uri: &'c str, body: &T) -> LocalResponse<'c>
    where
        T: Serialize,
    {
        self.with_auth(self.inner.put(uri).json(body)).dispatch()
    }

    pub(super) fn delete<'c>(&'c self, uri: &'c str) -> LocalResponse<'c> {
        self.with_auth(self.inner.delete(uri)).dispatch()
    }

    fn with_auth<'c>(&'c self, request: LocalRequest<'c>) -> LocalRequest<'c> {
        request.private_cookie(auth_cookie(self.user_id))
    }
}

pub(super) fn client_tracked_get() -> Client {
    // setup env
    let lock = DB_ENV_MUTEX.lock().unwrap();
    let seq = next_sequence();
    let db_path = format!("/tmp/test_db_{}.sqlite", seq);

    // Clean up if file exists from previous run
    let _ = fs::remove_file(&db_path);
    let db_config = format!("{{sqlx={{url=\"sqlite://{}\"}}}}", db_path);
    unsafe {
        env::set_var("DATABASE_URL", format!("sqlite://{}", db_path));
        env::set_var("ROCKET_DATABASES", &db_config);
        env::set_var("ROCKET_PROFILE", "debug");
        env::set_var("ROCKET_SECRET_KEY", "5Z4RZccfO6oVLQj86VXLxCaX/xyGq5wixH4hWsLve0s=");
        env::set_var("DKIM_KEY_PRIVATE", "test_key");
        env::set_var("DKIM_KEY_PUBLIC", "test_public_key");
        env::set_var("EMAIL_FROM", "test@example.com");
    }
    env_get(); // asserts all are there

    // env ready
    let rocket = rocket::build()
        .attach(db::stage())
        .attach(handlers::posts::stage())
        .attach(handlers::session::stage());
    let client = Client::tracked(rocket).expect("valid rocket instance");
    drop(lock);
    client
}

pub(super) const CODE_EXAMPLE: &str = "12345678"; // Updated to 8 digits

pub(super) fn email_for_session() -> String {
    format!("session+{}@example.com", next_sequence())
}

pub(super) fn fetch_user_by_email(client: &Client, email: &str) -> db::User {
    let pool = pool_cloned_get(client);
    let email_owned = email.to_owned();
    block_on(async move {
        sqlx::query_as!(db::User, "SELECT * FROM users WHERE email = ?", email_owned)
            .fetch_one(&pool)
            .await
            .expect("fetch user by email")
    })
}

pub(super) fn next_sequence() -> usize {
    NEXT_ID.fetch_add(1, Ordering::SeqCst)
}

pub(super) fn pool_cloned_get(client: &Client) -> sqlx::SqlitePool {
    let pool = db::Db::fetch(client.rocket()).expect("database pool");
    (**pool).clone()
}

pub(super) fn seed_user(client: &Client, email: &str) -> i64 {
    let pool = pool_cloned_get(client);
    let email_owned = email.to_owned();
    block_on(async move {
        sqlx::query("INSERT INTO users (email) VALUES (?)")
            .bind(email_owned)
            .execute(&pool)
            .await
            .expect("insert user")
            .last_insert_rowid()
    })
}

pub(super) fn seed_user_with_code(
    client: &Client,
    email: &str,
    code: &str,
    attempts: Option<i64>,
    code_created_at: NaiveDateTime,
) -> (i64, String) {
    let pool = pool_cloned_get(client);
    let email_owned = email.to_owned();
    let code_owned = code.to_owned();
    block_on(async move {
        let hash = hash_code(&code_owned).await.expect("hash code");
        let id =
            sqlx::query("INSERT INTO users (email, code_attempts, code_created_at, code_hash) VALUES (?, ?, ?, ?)")
                .bind(email_owned)
                .bind(attempts)
                .bind(code_created_at)
                .bind(&hash)
                .execute(&pool)
                .await
                .expect("insert user with code")
                .last_insert_rowid();
        (id, hash)
    })
}

pub(super) fn assert_success(response: LocalResponse, expected: Status) {
    assert_eq!(response.status(), expected);
    if expected == Status::Ok || expected == Status::Created {
        let body = response.into_json::<MessageResponse>().expect("message response");
        assert_eq!(body.message, "success");
    }
}

pub(super) fn block_on<F, T>(future: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    Runtime::new().expect("tokio runtime").block_on(future)
}
