use chrono::{DateTime, Utc};
use pebesen_core::Message;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn insert(
    pool: &PgPool,
    topic_id: Uuid,
    author_id: Uuid,
    content: &str,
    rendered: &str,
) -> Result<Message, sqlx::Error> {
    sqlx::query_as::<_, Message>(
        r#"
        INSERT INTO messages (topic_id, author_id, content, rendered)
        VALUES ($1, $2, $3, $4)
        RETURNING id, topic_id, author_id, content, rendered, edited_at, deleted_at, created_at
        "#,
    )
    .bind(topic_id)
    .bind(author_id)
    .bind(content)
    .bind(rendered)
    .fetch_one(pool)
    .await
}

pub async fn get_page(
    pool: &PgPool,
    topic_id: Uuid,
    cursor: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<Message>, sqlx::Error> {
    if let Some(cursor_time) = cursor {
        sqlx::query_as::<_, Message>(
            r#"
            SELECT id, topic_id, author_id, content, rendered, edited_at, deleted_at, created_at
            FROM messages
            WHERE topic_id = $1
            AND created_at < $2
            AND deleted_at IS NULL
            ORDER BY created_at ASC
            LIMIT $3
            "#,
        )
        .bind(topic_id)
        .bind(cursor_time)
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, Message>(
            r#"
            SELECT id, topic_id, author_id, content, rendered, edited_at, deleted_at, created_at
            FROM messages
            WHERE topic_id = $1
            AND deleted_at IS NULL
            ORDER BY created_at ASC
            LIMIT $2
            "#,
        )
        .bind(topic_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    }
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Message>, sqlx::Error> {
    sqlx::query_as::<_, Message>(
        r#"
        SELECT id, topic_id, author_id, content, rendered, edited_at, deleted_at, created_at
        FROM messages
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn update(
    pool: &PgPool,
    id: Uuid,
    content: &str,
    rendered: &str,
) -> Result<Message, sqlx::Error> {
    sqlx::query_as::<_, Message>(
        r#"
        UPDATE messages
        SET content = $1, rendered = $2, edited_at = NOW()
        WHERE id = $3
        RETURNING id, topic_id, author_id, content, rendered, edited_at, deleted_at, created_at
        "#,
    )
    .bind(content)
    .bind(rendered)
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn update_content(
    pool: &PgPool,
    id: Uuid,
    author_id: Uuid,
    content: &str,
    rendered: &str,
) -> Result<Message, sqlx::Error> {
    sqlx::query_as::<_, Message>(
        r#"
        UPDATE messages
        SET content = $1, rendered = $2, edited_at = NOW()
        WHERE id = $3 AND author_id = $4
        RETURNING id, topic_id, author_id, content, rendered, edited_at, deleted_at, created_at
        "#,
    )
    .bind(content)
    .bind(rendered)
    .bind(id)
    .bind(author_id)
    .fetch_one(pool)
    .await
}

pub async fn soft_delete(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE messages
        SET deleted_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn soft_delete_with_auth(
    pool: &PgPool,
    id: Uuid,
    requester_id: Uuid,
    is_admin_or_owner: bool,
) -> Result<(), sqlx::Error> {
    if is_admin_or_owner {
        sqlx::query(
            r#"
            UPDATE messages
            SET deleted_at = NOW(), content = '[deleted]', rendered = '<p>[deleted]</p>'
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            r#"
            UPDATE messages
            SET deleted_at = NOW(), content = '[deleted]', rendered = '<p>[deleted]</p>'
            WHERE id = $1 AND author_id = $2
            "#,
        )
        .bind(id)
        .bind(requester_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}
