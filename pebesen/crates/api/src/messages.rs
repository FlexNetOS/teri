use crate::middleware::AuthUser;
use axum::{Json, extract::Path, extract::Query, extract::State, http::StatusCode};
use pebesen_core::AppError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Deserialize)]
pub struct CreateMessageRequest {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMessageRequest {
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct MessageDTO {
    pub id: uuid::Uuid,
    pub topic_id: uuid::Uuid,
    pub author_id: uuid::Uuid,
    pub content: String,
    pub rendered: Option<String>,
    pub edited_at: Option<chrono::DateTime<chrono::Utc>>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct MessageWithAuthorDTO {
    pub message: MessageDTO,
    pub author: crate::spaces::UserDTO,
}

#[derive(Debug, Deserialize)]
pub struct MessagesQuery {
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

pub async fn create_message(
    State(pool): State<PgPool>,
    Path(topic_id): Path<uuid::Uuid>,
    auth_user: AuthUser,
    Json(payload): Json<CreateMessageRequest>,
) -> Result<(StatusCode, Json<MessageDTO>), AppError> {
    // Load topic — return 404 if not found
    let topic = pebesen_db::topics::find_by_id(&pool, topic_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Look up stream and space
    let stream = pebesen_db::streams::find_by_id(&pool, topic.stream_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;
    let space = pebesen_db::spaces::find_by_id(&pool, stream.space_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Require auth + space membership
    pebesen_db::memberships::find(&pool, auth_user.id, space.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::Forbidden)?;

    // Return 403 if topic status is archived
    if matches!(topic.status, pebesen_core::TopicStatus::Archived) {
        return Err(AppError::Forbidden);
    }

    // Validate content: not empty, max 10,000 chars
    let content = payload.content.trim();
    if content.is_empty() || content.len() > 10_000 {
        return Err(AppError::BadRequest(
            "Content must be 1-10,000 characters".to_string(),
        ));
    }

    // Render Markdown
    let rendered = pebesen_core::render_markdown(content);

    // Insert message
    let message = pebesen_db::messages::insert(&pool, topic_id, auth_user.id, content, &rendered)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // After insert: call topics::update_last_active
    pebesen_db::topics::update_last_active(&pool, topic_id, chrono::Utc::now())
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    Ok((
        StatusCode::CREATED,
        Json(MessageDTO {
            id: message.id,
            topic_id: message.topic_id,
            author_id: message.author_id,
            content: message.content,
            rendered: message.rendered,
            edited_at: message.edited_at,
            deleted_at: message.deleted_at,
            created_at: message.created_at,
        }),
    ))
}

pub async fn get_messages(
    State(pool): State<PgPool>,
    Path(topic_id): Path<uuid::Uuid>,
    auth_user: AuthUser,
    Query(query): Query<MessagesQuery>,
) -> Result<Json<Vec<MessageWithAuthorDTO>>, AppError> {
    // Load topic — return 404 if not found
    let topic = pebesen_db::topics::find_by_id(&pool, topic_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Look up stream and space
    let stream = pebesen_db::streams::find_by_id(&pool, topic.stream_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;
    let space = pebesen_db::spaces::find_by_id(&pool, stream.space_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Require auth + space membership
    pebesen_db::memberships::find(&pool, auth_user.id, space.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::Forbidden)?;

    // Parse cursor and limit
    let cursor = query.cursor.as_ref().map(|c| {
        chrono::DateTime::from_timestamp(c.parse::<i64>().unwrap_or(0), 0)
            .unwrap_or_else(chrono::Utc::now)
    });
    let limit = query.limit.unwrap_or(50).min(100);

    // Get messages page
    let messages = pebesen_db::messages::get_page(&pool, topic_id, cursor, limit)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Batch fetch all users in one query to avoid N+1
    let author_ids: Vec<_> = messages.iter().map(|m| m.author_id).collect();
    let users = pebesen_db::users::find_by_ids(&pool, &author_ids)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Create user lookup map
    let user_map: std::collections::HashMap<_, _> = users.into_iter().map(|u| (u.id, u)).collect();

    // Build message DTOs with authors
    let mut message_dtos: Vec<MessageWithAuthorDTO> = Vec::new();
    for message in messages {
        if let Some(user) = user_map.get(&message.author_id) {
            message_dtos.push(MessageWithAuthorDTO {
                message: MessageDTO {
                    id: message.id,
                    topic_id: message.topic_id,
                    author_id: message.author_id,
                    content: message.content,
                    rendered: message.rendered,
                    edited_at: message.edited_at,
                    deleted_at: message.deleted_at,
                    created_at: message.created_at,
                },
                author: crate::spaces::UserDTO {
                    id: user.id,
                    username: user.username.clone(),
                    display_name: user.display_name.clone(),
                },
            });
        }
    }

    Ok(Json(message_dtos))
}

pub async fn update_message(
    State(pool): State<PgPool>,
    Path(message_id): Path<uuid::Uuid>,
    auth_user: AuthUser,
    Json(payload): Json<UpdateMessageRequest>,
) -> Result<Json<MessageDTO>, AppError> {
    // Load message — return 404 if not found
    let message = pebesen_db::messages::find_by_id(&pool, message_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Load topic and space for Redis/Meilisearch
    let topic = pebesen_db::topics::find_by_id(&pool, message.topic_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;
    let stream = pebesen_db::streams::find_by_id(&pool, topic.stream_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;
    let space = pebesen_db::spaces::find_by_id(&pool, stream.space_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Require auth — must be message author
    if message.author_id != auth_user.id {
        return Err(AppError::Forbidden);
    }

    // Validate new content
    let content = payload.content.trim();
    if content.is_empty() || content.len() > 10_000 {
        return Err(AppError::BadRequest(
            "Content must be 1-10,000 characters".to_string(),
        ));
    }

    // Re-render Markdown
    let rendered = pebesen_core::render_markdown(content);

    // Update message
    let updated_message =
        pebesen_db::messages::update_content(&pool, message_id, auth_user.id, content, &rendered)
            .await
            .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Update Meilisearch index entry
    let meilisearch_url =
        std::env::var("MEILISEARCH_URL").unwrap_or_else(|_| "http://localhost:7700".to_string());
    let meilisearch_key = std::env::var("MEILISEARCH_KEY").ok();
    crate::search::index_message(
        &meilisearch_url,
        meilisearch_key.as_deref(),
        updated_message.id,
        updated_message.topic_id,
        space.id,
        updated_message.author_id,
        &updated_message.content,
    )
    .await
    .map_err(|e| AppError::Internal(format!("Meilisearch error: {}", e)))?;

    // Publish {type: "message_updated"} to Redis
    let redis_url = std::env::var("REDIS_URL")
        .map_err(|_| AppError::Internal("REDIS_URL not configured".to_string()))?;
    crate::redis_pubsub::publish_message_updated(&redis_url, space.id, updated_message.id)
        .await
        .map_err(|e| AppError::Internal(format!("Redis publish error: {}", e)))?;

    Ok(Json(MessageDTO {
        id: updated_message.id,
        topic_id: updated_message.topic_id,
        author_id: updated_message.author_id,
        content: updated_message.content,
        rendered: updated_message.rendered,
        edited_at: updated_message.edited_at,
        deleted_at: updated_message.deleted_at,
        created_at: updated_message.created_at,
    }))
}

pub async fn delete_message(
    State(pool): State<PgPool>,
    Path(message_id): Path<uuid::Uuid>,
    auth_user: AuthUser,
) -> Result<StatusCode, AppError> {
    // Load message — return 404 if not found
    let message = pebesen_db::messages::find_by_id(&pool, message_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Load topic, stream, space
    let topic = pebesen_db::topics::find_by_id(&pool, message.topic_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;
    let stream = pebesen_db::streams::find_by_id(&pool, topic.stream_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;
    let space = pebesen_db::spaces::find_by_id(&pool, stream.space_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Get user's role in space
    let membership = pebesen_db::memberships::find(&pool, auth_user.id, space.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::Forbidden)?;

    let is_admin_or_owner = membership.role == pebesen_core::Role::Admin
        || membership.role == pebesen_core::Role::Owner;

    // Require auth — author OR space admin/owner
    if message.author_id != auth_user.id && !is_admin_or_owner {
        return Err(AppError::Forbidden);
    }

    // Soft delete only
    pebesen_db::messages::soft_delete_with_auth(&pool, message_id, auth_user.id, is_admin_or_owner)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Remove from Meilisearch index
    let meilisearch_url =
        std::env::var("MEILISEARCH_URL").unwrap_or_else(|_| "http://localhost:7700".to_string());
    let meilisearch_key = std::env::var("MEILISEARCH_KEY").ok();
    crate::search::remove_message(&meilisearch_url, meilisearch_key.as_deref(), message_id)
        .await
        .map_err(|e| AppError::Internal(format!("Meilisearch error: {}", e)))?;

    // Publish {type: "message_deleted", id} to Redis
    let redis_url = std::env::var("REDIS_URL")
        .map_err(|_| AppError::Internal("REDIS_URL not configured".to_string()))?;
    crate::redis_pubsub::publish_message_deleted(&redis_url, space.id, message_id)
        .await
        .map_err(|e| AppError::Internal(format!("Redis publish error: {}", e)))?;

    Ok(StatusCode::NO_CONTENT)
}
