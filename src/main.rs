#[macro_use]
extern crate rocket;

use chrono::{DateTime, Utc};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Status;
use rocket::serde::json;
use rocket::{Data, Request, Response};
use rocket_sqlx::{db, handlers, util::*};

#[launch]
fn rocket() -> _ {
    dotenv::dotenv().expect("Failed to load .env file");
    env_get(); // asserts all are there

    rocket::build()
        .attach(RequestLogger)
        .register("/", catchers![c401, c404, c422, c500])
        .attach(db::stage())
        .attach(handlers::posts::stage())
        .attach(handlers::session::stage())
}

#[catch(401)]
fn c401() -> (Status, json::Value) {
    (Status::Unauthorized, json::json!({ "message": "Unauthorized" }))
}

#[catch(404)]
fn c404() -> (Status, json::Value) {
    (Status::NotFound, json::json!({ "message": "Not found" }))
}

#[catch(422)]
fn c422() -> (Status, json::Value) {
    (
        Status::UnprocessableEntity,
        json::json!({ "message": "Inputs are invalid" }),
    )
}

#[catch(500)]
fn c500() -> (Status, json::Value) {
    (
        Status::InternalServerError,
        json::json!({ "message": "Internal Server Error" }),
    )
}

struct RequestLogger;
struct RequestLoggerCache {
    start: DateTime<Utc>,
    method: String,
    uri: String,
}
#[rocket::async_trait]
impl Fairing for RequestLogger {
    fn info(&self) -> Info {
        Info {
            name: "Request Logger",
            kind: Kind::Request | Kind::Response,
        }
    }

    async fn on_request(&self, request: &mut Request<'_>, _: &mut Data<'_>) {
        let method = request.method().to_string();
        let uri = request.uri().to_string();
        let start = Utc::now();
        request.local_cache(|| RequestLoggerCache { start, method, uri });
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        let local_cache = request.local_cache(|| RequestLoggerCache {
            start: Utc::now(),
            method: "UNKNOWN".to_string(),
            uri: "UNKNOWN".to_string(),
        });
        let duration = (Utc::now() - local_cache.start).num_milliseconds();

        println!(
            "{} {} {} {} {}ms",
            local_cache.start.to_rfc3339(),
            local_cache.method,
            local_cache.uri,
            response.status().code,
            duration
        );
    }
}
