use chrono::Timelike;
use rocket::fairing::AdHoc;
use rocket::form::FromForm;
use rocket::http::Status;
use rocket::serde::{Deserialize, json};

use crate::db::*;
use crate::util::*;

#[derive(FromForm)]
struct QueryParams {
    after: Option<String>,
    limit: Option<i64>,
}

#[get("/?<qp..>")]
async fn list(mut db: Connection<Db>, user: UserCtx, qp: QueryParams) -> (Status, json::Value) {
    // info!("list:params:limit={:?}:after={:?}", qp.limit, qp.after);

    let limit = qp.limit.unwrap_or(10).min(1000);
    let limit_plus_one = limit + 1;

    let posts = match qp.after {
        Some(after) => {
            let after = NaiveDateTime::parse_from_rfc3339(after);
            sqlx::query_as!(
                Post,
                "SELECT * FROM posts WHERE user_id = ? AND updated_at >= ? ORDER BY updated_at DESC LIMIT ?",
                user.id,
                after,
                limit_plus_one
            )
            .fetch(&mut **db)
            .try_collect::<Vec<_>>()
            .await
            .expect("Failed to fetch posts")
        }
        None => sqlx::query_as!(Post, "SELECT * FROM posts WHERE user_id = ? LIMIT ?", user.id, limit)
            .fetch(&mut **db)
            .try_collect::<Vec<_>>()
            .await
            .expect("Failed to fetch posts"),
    };

    let has_more = posts.len() as i64 > limit;
    let posts = if has_more {
        posts.into_iter().take(limit as usize).collect()
    } else {
        posts
    };

    (
        Status::Ok,
        json::json!({
            "items": posts,
            "hasMore": has_more,
        }),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(crate = "rocket::serde")]
pub struct CreateRequestBody {
    pub id: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub content: String,
    pub updated_at: Option<DateTime<Utc>>,
    pub variant: String,
}

#[post("/", data = "<body>")]
async fn create(mut db: Connection<Db>, user: UserCtx, body: json::Json<CreateRequestBody>) -> (Status, json::Value) {
    let now = Utc::now().with_nanosecond(0).unwrap();

    let id = body.id.clone().unwrap_or_else(|| id_gen());
    let created_at = body.created_at.unwrap_or_else(|| now).naive_utc();
    let updated_at = body.updated_at.unwrap_or_else(|| now).naive_utc();

    sqlx::query!(
        "INSERT INTO posts (created_at, id, content, updated_at, user_id, variant) \
        VALUES (?, ?, ?, ?, ?, ?) \
        ON CONFLICT(id) DO UPDATE SET \
        content = excluded.content, \
        variant = excluded.variant, \
        updated_at = excluded.updated_at \
        WHERE posts.updated_at < excluded.updated_at AND posts.user_id = excluded.user_id",
        created_at,
        id,
        body.content,
        updated_at,
        user.id,
        body.variant,
    )
    .execute(&mut **db)
    .await
    .expect("Failed to upsert post");

    (Status::Created, json::json!(MESSAGE_RESPONSE_SUCCESS.clone()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(crate = "rocket::serde")]
pub struct UpsertPostPayload {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub content: String,
    pub updated_at: DateTime<Utc>,
    pub variant: String,
}

#[post("/upsert-many", data = "<body>")]
/// Upsert multiple posts in a single request. The client must provide the full post
/// data for each post, and the server will insert or update each post based on the ID.
/// For updates, the server will only apply the update if the provided updated_at is
/// greater than the existing updated_at to prevent overwriting newer data with older
/// data.
async fn upsert_many(
    mut db: Connection<Db>,
    user: UserCtx,
    body: json::Json<Vec<UpsertPostPayload>>,
) -> (Status, json::Value) {
    if body.is_empty() {
        return (Status::Ok, json::json!(MESSAGE_RESPONSE_SUCCESS.clone()));
    }

    let mut builder =
        sqlx::QueryBuilder::new("INSERT INTO posts (created_at, id, content, updated_at, user_id, variant) ");

    builder.push_values(body.iter(), |mut row, post| {
        row.push_bind(post.created_at.naive_utc())
            .push_bind(&post.id)
            .push_bind(&post.content)
            .push_bind(post.updated_at.naive_utc())
            .push_bind(user.id)
            .push_bind(&post.variant);
    });

    builder.push(
        " ON CONFLICT(id) DO UPDATE SET content = excluded.content, variant = excluded.variant, updated_at = excluded.updated_at"
    );
    builder.push(" WHERE posts.updated_at < excluded.updated_at AND posts.user_id = excluded.user_id");

    builder
        .build()
        .execute(&mut **db)
        .await
        .expect("Failed to upsert posts");

    (Status::Ok, json::json!(MESSAGE_RESPONSE_SUCCESS.clone()))
}

#[delete("/")]
async fn delete_all(mut db: Connection<Db>, user: UserCtx) -> (Status, json::Value) {
    sqlx::query!("DELETE FROM posts WHERE user_id = ?", user.id)
        .execute(&mut **db)
        .await
        .expect("Failed to delete posts");

    (Status::Ok, json::json!({ "message": "success" }))
}

#[get("/<id>")]
async fn read(mut db: Connection<Db>, user: UserCtx, id: String) -> (Status, json::Value) {
    let post = sqlx::query_as!(Post, "SELECT * FROM posts WHERE id = ? AND user_id = ?", id, user.id)
        .fetch_optional(&mut **db)
        // .map_ok(|r| {
        //     Post {
        //         id: r.id,
        //         // created_at: r.created_at,
        //         content: r.content,
        //         // updated_at: r.updated_at,
        //         variant: r.variant,
        //     }
        //     // r
        // })
        .await
        .expect("Failed to fetch post");

    if let Some(post) = post {
        (Status::Ok, json::json!(post))
    } else {
        (Status::NotFound, json::json!({ "error": "Post not found" }))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(crate = "rocket::serde")]
pub struct UpdateRequestBody {
    pub content: String,
    pub updated_at: Option<DateTime<Utc>>,
}

#[put("/<id>", data = "<body>")]
async fn update(
    mut db: Connection<Db>,
    user: UserCtx,
    id: String,
    body: json::Json<UpdateRequestBody>,
) -> (Status, json::Value) {
    let now = Utc::now().with_nanosecond(0).unwrap();
    let updated_at = body.updated_at.unwrap_or_else(|| now).naive_utc();

    let result = sqlx::query!(
        "UPDATE posts SET content = ?, updated_at = ? WHERE id = ? AND user_id = ? AND updated_at < ?",
        body.content,
        updated_at,
        id,
        user.id,
        updated_at,
    )
    .execute(&mut **db)
    .await
    .expect("Failed to update post");

    if result.rows_affected() == 0 {
        return (
            Status::NotFound,
            json::json!({ "error": "Post not found or supplied update_at is less than existing" }),
        );
    }

    (Status::Ok, json::json!({ "message": "success" }))
}

#[delete("/<id>")]
async fn delete(mut db: Connection<Db>, user: UserCtx, id: String) -> (Status, json::Value) {
    let result = sqlx::query!("DELETE FROM posts WHERE id = ? AND user_id = ?", id, user.id)
        .execute(&mut **db)
        .await
        .expect("Failed to delete post");

    if result.rows_affected() == 0 {
        return (Status::NotFound, json::json!({ "error": "Post not found" }));
    }

    (Status::Ok, json::json!({ "message": "success" }))
}

pub fn stage() -> AdHoc {
    AdHoc::on_ignite("Posts stage", |rocket| async {
        rocket.mount(
            "/api/posts",
            routes![list, create, upsert_many, delete_all, read, update, delete],
        )
    })
}
