use chrono::{DateTime, Utc};
use pebesen_core::{Topic, TopicStatus};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn insert(
    pool: &PgPool,
    stream_id: Uuid,
    name: &str,
    created_by: Uuid,
) -> Result<Topic, sqlx::Error> {
    sqlx::query_as::<_, Topic>(
        r#"
        INSERT INTO topics (stream_id, name, created_by)
        VALUES ($1, $2, $3)
        RETURNING id, stream_id, name, status as "status: TopicStatus", created_by, created_at, last_active
        "#,
    )
    .bind(stream_id)
    .bind(name)
    .bind(created_by)
    .fetch_one(pool)
    .await
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Topic>, sqlx::Error> {
    sqlx::query_as::<_, Topic>(
        r#"
        SELECT id, stream_id, name, status as "status: TopicStatus", created_by, created_at, last_active
        FROM topics
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn list_by_stream(
    pool: &PgPool,
    stream_id: Uuid,
    status_filter: Option<TopicStatus>,
) -> Result<Vec<Topic>, sqlx::Error> {
    if let Some(status) = status_filter {
        let status_str = match status {
            TopicStatus::Open => "open",
            TopicStatus::Closed => "closed",
            TopicStatus::Archived => "archived",
        };
        sqlx::query_as::<_, Topic>(
            r#"
            SELECT id, stream_id, name, status as "status: TopicStatus", created_by, created_at, last_active
            FROM topics
            WHERE stream_id = $1 AND status = $2
            ORDER BY last_active DESC
            "#,
        )
        .bind(stream_id)
        .bind(status_str)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, Topic>(
            r#"
            SELECT id, stream_id, name, status as "status: TopicStatus", created_by, created_at, last_active
            FROM topics
            WHERE stream_id = $1
            ORDER BY last_active DESC
            "#,
        )
        .bind(stream_id)
        .fetch_all(pool)
        .await
    }
}

pub async fn search_by_name_prefix(
    pool: &PgPool,
    stream_id: Uuid,
    prefix: &str,
) -> Result<Vec<Topic>, sqlx::Error> {
    sqlx::query_as::<_, Topic>(
        r#"
        SELECT id, stream_id, name, status as "status: TopicStatus", created_by, created_at, last_active
        FROM topics
        WHERE stream_id = $1 AND lower(name) LIKE lower($2 || '%')
        ORDER BY last_active DESC
        "#,
    )
    .bind(stream_id)
    .bind(prefix)
    .fetch_all(pool)
    .await
}

pub async fn update_last_active(
    pool: &PgPool,
    topic_id: Uuid,
    timestamp: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE topics
        SET last_active = $1
        WHERE id = $2
        "#,
    )
    .bind(timestamp)
    .bind(topic_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_status(
    pool: &PgPool,
    topic_id: Uuid,
    status: TopicStatus,
) -> Result<(), sqlx::Error> {
    let status_str = match status {
        TopicStatus::Open => "open",
        TopicStatus::Closed => "closed",
        TopicStatus::Archived => "archived",
    };
    sqlx::query(
        r#"
        UPDATE topics
        SET status = $1
        WHERE id = $2
        "#,
    )
    .bind(status_str)
    .bind(topic_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn rename(pool: &PgPool, topic_id: Uuid, new_name: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE topics
        SET name = $1
        WHERE id = $2
        "#,
    )
    .bind(new_name)
    .bind(topic_id)
    .execute(pool)
    .await?;
    Ok(())
}
