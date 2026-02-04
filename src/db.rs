use rocket::fairing::{self, AdHoc};
use rocket::serde::{Deserialize, Serialize};
use rocket::{Build, Rocket};

use nanoid::nanoid;
pub use rocket_db_pools::{Connection, Database, sqlx};

use crate::util::*;

#[derive(Database)]
#[database("sqlx")]
pub struct Db(sqlx::SqlitePool);

/// A generic database table that can hold multiple types of data, distinguished by the `variant` field.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(crate = "rocket::serde")]
pub struct Post {
    pub id: String,
    pub content: String,
    #[serde(
        serialize_with = "NaiveDateTime::serializer",
        deserialize_with = "NaiveDateTime::deserializer"
    )]
    pub created_at: NaiveDateTime,
    #[serde(
        serialize_with = "NaiveDateTime::serializer",
        deserialize_with = "NaiveDateTime::deserializer"
    )]
    pub updated_at: NaiveDateTime,
    #[serde(skip)]
    #[allow(dead_code)]
    pub user_id: i64,
    pub variant: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(crate = "rocket::serde")]
pub struct User {
    pub id: i64,
    #[serde(
        serialize_with = "NaiveDateTime::serializer",
        deserialize_with = "NaiveDateTime::deserializer"
    )]
    pub created_at: NaiveDateTime,
    pub email: String,
    pub code_hash: Option<String>,
    pub code_attempts: Option<i64>,
    #[serde(
        serialize_with = "NaiveDateTime::serializer_option",
        deserialize_with = "NaiveDateTime::deserializer_option"
    )]
    pub code_created_at: Option<NaiveDateTime>,
}

/// Generates a unique ID using the `nanoid` crate with a custom alphabet and length.
pub fn id_gen() -> String {
    const ALPHABET: [char; 62] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', // Digits
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v',
        'w', 'x', 'y', 'z', // Lowercase
        'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V',
        'W', 'X', 'Y', 'Z', // Uppercase
    ];

    nanoid!(21, &ALPHABET)
}

/// Runs database migrations using SQLx when the Rocket application is launched.
async fn migrations_run(rocket: Rocket<Build>) -> fairing::Result {
    match Db::fetch(&rocket) {
        Some(db) => match sqlx::migrate!().run(&**db).await {
            Ok(_) => Ok(rocket),
            Err(e) => {
                error!("Failed to initialize SQLx database: {}", e);
                Err(rocket)
            }
        },
        None => Err(rocket),
    }
}

pub fn stage() -> AdHoc {
    AdHoc::on_ignite("SQLx Stage", |rocket| async {
        rocket
            .attach(Db::init())
            .attach(AdHoc::try_on_ignite("SQLx Migrations", migrations_run))
    })
}
