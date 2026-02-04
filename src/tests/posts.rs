use crate::tests::util::*;

use chrono::{DateTime, Duration, Timelike, Utc};
use rocket::http::Status;
use rocket::serde::{Deserialize, Serialize};

use crate::db;

const POSTS_BASE: &str = "/api/posts";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", crate = "rocket::serde")]
struct PostListResponse {
    items: Vec<db::Post>,
    has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", crate = "rocket::serde")]
struct CreatePostPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<DateTime<Utc>>,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<DateTime<Utc>>,
    variant: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", crate = "rocket::serde")]
struct UpdatePostPayload {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", crate = "rocket::serde")]
struct UpsertPostPayload {
    id: String,
    created_at: DateTime<Utc>,
    content: String,
    updated_at: DateTime<Utc>,
    variant: String,
}

#[test]
fn posts_list_filter_after() {
    let client = ClientAuthenticated::new();
    // Ensure the list is initially empty
    assert!(fetch_posts(&client, POSTS_BASE).items.is_empty());

    let start = Utc::now().with_nanosecond(0).unwrap();
    let mut timestamps = Vec::new();

    for offset in 0..3 {
        let stamp = start + Duration::seconds(offset as i64);
        let payload = CreatePostPayload {
            id: Some(format!("list-{}", offset)),
            created_at: Some(stamp),
            content: format!("List post {}", offset),
            updated_at: Some(stamp),
            variant: "note".into(),
        };
        // Insert posts with different timestamps
        assert_success(client.post_json(POSTS_BASE, &payload), Status::Created);
        timestamps.push(stamp.naive_utc());
    }

    // Verify all posts are listed
    let list = fetch_posts(&client, POSTS_BASE);
    assert_eq!(list.items.len(), 3);
    // Ensure `has_more` is false since there are no more posts beyond the limit
    assert!(!list.has_more);

    let threshold = timestamps[1];
    let filtered_uri = format!("{}?after={}", POSTS_BASE, threshold.to_rfc3339());
    let filtered = fetch_posts(&client, &filtered_uri);
    let expected = timestamps.iter().filter(|&&ts| ts >= threshold).count();
    // Ensure the filtered list contains only posts after the threshold
    assert_eq!(filtered.items.len(), expected);
    // Verify all posts in the filtered list have `updated_at` >= threshold
    assert!(filtered.items.iter().all(|post| post.updated_at >= threshold));
}

#[test]
fn posts_read_by_id() {
    let client = ClientAuthenticated::new();
    let now = Utc::now().with_nanosecond(0).unwrap();

    let payload = CreatePostPayload {
        id: Some("read-test".into()),
        created_at: Some(now),
        content: "Reads fine".into(),
        updated_at: Some(now),
        variant: "note".into(),
    };

    // Create a post to read later
    assert_success(client.post_json(POSTS_BASE, &payload), Status::Created);

    let read_uri = format!("{}/{}", POSTS_BASE, "read-test");
    let fetched = fetch_post(&client, &read_uri);
    // Ensure the fetched post matches the created post
    assert_eq!(fetched.content, "Reads fine");

    let missing_uri = format!("{}/{}", POSTS_BASE, "missing-read");
    let response = client.get(&missing_uri);
    // Ensure fetching a non-existent post returns 404
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn posts_create_upsert() {
    let client = ClientAuthenticated::new();
    let now = Utc::now().with_nanosecond(0).unwrap();

    let id = "create-upsert";
    let initial_payload = CreatePostPayload {
        id: Some(id.into()),
        created_at: Some(now),
        content: "Initial content".into(),
        updated_at: Some(now),
        variant: "note".into(),
    };

    // Create a new post
    assert_success(client.post_json(POSTS_BASE, &initial_payload), Status::Created);

    let read_uri = format!("{}/{}", POSTS_BASE, id);
    let fetched = fetch_post(&client, &read_uri);
    // Ensure the created post matches the payload
    assert_eq!(fetched.content, "Initial content");

    let older_payload = CreatePostPayload {
        id: Some(id.into()),
        created_at: Some(now),
        content: "Older content".into(),
        updated_at: Some(now - Duration::seconds(30)),
        variant: "note".into(),
    };
    // Attempt to upsert with an older timestamp (should not update)
    assert_success(client.post_json(POSTS_BASE, &older_payload), Status::Created);

    let not_updated_post = fetch_post(&client, &read_uri);
    // Ensure the post was not updated with older content
    assert_eq!(not_updated_post.content, "Initial content");

    let newer_payload = CreatePostPayload {
        id: Some(id.into()),
        created_at: Some(now),
        content: "Updated content".into(),
        updated_at: Some(now + Duration::seconds(30)),
        variant: "note".into(),
    };
    // Upsert with a newer timestamp (should update)
    assert_success(client.post_json(POSTS_BASE, &newer_payload), Status::Created);

    let updated_post = fetch_post(&client, &read_uri);
    // Ensure the post was updated with newer content
    assert_eq!(updated_post.content, "Updated content");
    assert_eq!(updated_post.updated_at, (now + Duration::seconds(30)).naive_utc());
}

#[test]
fn posts_update_by_id() {
    let client = ClientAuthenticated::new();
    let now = Utc::now().with_nanosecond(0).unwrap();
    let id = "update-me";

    let payload = CreatePostPayload {
        id: Some(id.into()),
        created_at: Some(now),
        content: "Before update".into(),
        updated_at: Some(now),
        variant: "note".into(),
    };

    // Create a post to update later
    assert_success(client.post_json(POSTS_BASE, &payload), Status::Created);

    let update_uri = format!("{}/{}", POSTS_BASE, id);
    let update_at = now + Duration::seconds(30);
    let update_payload = UpdatePostPayload {
        content: "After update".into(),
        updated_at: Some(update_at),
    };
    // Update the post with a newer timestamp
    assert_success(client.put_json(&update_uri, &update_payload), Status::Ok);

    let updated = fetch_post(&client, &update_uri);
    // Ensure the post was updated with the new content and timestamp
    assert_eq!(updated.content, "After update");
    assert_eq!(updated.updated_at, update_at.naive_utc());

    let stale_payload = UpdatePostPayload {
        content: "Stale".into(),
        updated_at: Some(now),
    };
    // Attempt to update with an older timestamp (should fail)
    let response = client.put_json(&update_uri, &stale_payload);
    assert_eq!(response.status(), Status::NotFound);

    let missing_uri = format!("{}/{}", POSTS_BASE, "missing-update");
    // Attempt to update a non-existent post (should fail)
    let response = client.put_json(&missing_uri, &update_payload);
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn posts_delete_all() {
    let client = ClientAuthenticated::new();
    let now = Utc::now().with_nanosecond(0).unwrap();

    for offset in 0..2 {
        let payload = CreatePostPayload {
            id: Some(format!("delete-all-{}", offset)),
            created_at: Some(now),
            content: format!("Delete all {}", offset),
            updated_at: Some(now),
            variant: "note".into(),
        };
        // Create multiple posts to delete later
        assert_success(client.post_json(POSTS_BASE, &payload), Status::Created);
    }

    // Delete all posts
    assert_success(client.delete(POSTS_BASE), Status::Ok);
    // Ensure the list is empty after deletion
    assert!(fetch_posts(&client, POSTS_BASE).items.is_empty());
}

#[test]
fn posts_delete_by_id() {
    let client = ClientAuthenticated::new();
    let now = Utc::now().with_nanosecond(0).unwrap();

    let payload = CreatePostPayload {
        id: Some("delete-one".into()),
        created_at: Some(now),
        content: "Delete me".into(),
        updated_at: Some(now),
        variant: "note".into(),
    };
    // Create a post to delete later
    assert_success(client.post_json(POSTS_BASE, &payload), Status::Created);

    let delete_uri = format!("{}/{}", POSTS_BASE, "delete-one");
    // Delete the post by ID
    assert_success(client.delete(&delete_uri), Status::Ok);

    let response = client.get(&delete_uri);
    // Ensure the post no longer exists
    assert_eq!(response.status(), Status::NotFound);
}

#[test]
fn posts_upsert_many() {
    let client = ClientAuthenticated::new();
    let now = Utc::now().with_nanosecond(0).unwrap();

    let upsert_uri = format!("{}/upsert-many", POSTS_BASE);
    // Initial bulk insert for two posts
    let payloads = vec![
        UpsertPostPayload {
            id: format!("bulk-{}", db::id_gen()),
            created_at: now,
            content: "bulk one".into(),
            updated_at: now,
            variant: "note".into(),
        },
        UpsertPostPayload {
            id: format!("bulk-{}", db::id_gen()),
            created_at: now,
            content: "bulk two".into(),
            updated_at: now,
            variant: "note".into(),
        },
    ];

    // Insert multiple posts
    assert_success(client.post_json(&upsert_uri, &payloads), Status::Ok);
    let list = fetch_posts(&client, POSTS_BASE);
    // Ensure all posts were inserted
    assert_eq!(list.items.len(), 2);

    let mut updated_payloads = payloads.clone();
    let newer = (now + Duration::seconds(30)).with_nanosecond(0).unwrap();
    updated_payloads[0].content = "bulk one updated".into();
    updated_payloads[0].updated_at = newer;

    // Upsert with a newer timestamp (should update)
    assert_success(client.post_json(&upsert_uri, &updated_payloads), Status::Ok);

    let updated = fetch_post(&client, &format!("{}/{}", POSTS_BASE, updated_payloads[0].id));
    // Ensure the post was updated with newer content
    assert_eq!(updated.content, "bulk one updated");
    assert_eq!(updated.updated_at, newer.naive_utc());

    let older = (now - Duration::seconds(30)).with_nanosecond(0).unwrap();
    let stale_payloads = vec![UpsertPostPayload {
        id: updated_payloads[0].id.clone(),
        created_at: now,
        content: "should skip".into(),
        updated_at: older,
        variant: "note".into(),
    }];

    // Attempt to upsert with an older timestamp (should not update)
    assert_success(client.post_json(&upsert_uri, &stale_payloads), Status::Ok);
    let skipped = fetch_post(&client, &format!("{}/{}", POSTS_BASE, updated_payloads[0].id));
    // Ensure the post was not updated with older content
    assert_eq!(skipped.content, "bulk one updated");
    assert_eq!(skipped.updated_at, newer.naive_utc());
}

fn fetch_posts(client: &ClientAuthenticated, uri: &str) -> PostListResponse {
    let response = client.get(uri);
    assert_eq!(response.status(), Status::Ok);
    response.into_json::<PostListResponse>().expect("posts response")
}

fn fetch_post(client: &ClientAuthenticated, uri: &str) -> db::Post {
    let response = client.get(uri);
    assert_eq!(response.status(), Status::Ok);
    response.into_json::<db::Post>().expect("post response")
}
