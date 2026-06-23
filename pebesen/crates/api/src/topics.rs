use crate::middleware::{AuthUser, OptionalAuthUser};
use axum::{Json, extract::Path, extract::Query, extract::State, http::StatusCode};
use pebesen_core::{AppError, TopicStatus};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Deserialize)]
pub struct CreateTopicRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct RenameTopicRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTopicStatusRequest {
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct TopicDTO {
    pub id: uuid::Uuid,
    pub stream_id: uuid::Uuid,
    pub name: String,
    pub status: String,
    pub created_by: Option<uuid::Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_active: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct TopicsQuery {
    pub status: Option<String>,
    pub cursor: Option<String>,
}

pub async fn create_topic(
    State(pool): State<PgPool>,
    Path(stream_id): Path<uuid::Uuid>,
    auth_user: AuthUser,
    Json(payload): Json<CreateTopicRequest>,
) -> Result<(StatusCode, Json<TopicDTO>), AppError> {
    // Look up stream
    let stream = pebesen_db::streams::find_by_id(&pool, stream_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Require auth + space membership
    pebesen_db::memberships::find(&pool, auth_user.id, stream.space_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::Forbidden)?;

    // Validate name: 1–128 chars
    let name = payload.name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(AppError::BadRequest(
            "Name must be 1-128 characters".to_string(),
        ));
    }

    // Check name uniqueness within stream (case-insensitive)
    let existing_topics = pebesen_db::topics::list_by_stream(&pool, stream_id, None)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;
    if existing_topics
        .iter()
        .any(|t| t.name.to_lowercase() == name.to_lowercase())
    {
        return Err(AppError::Conflict);
    }

    // Insert topic (last_active is set to NOW() by default)
    let topic = pebesen_db::topics::insert(&pool, stream_id, name, auth_user.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    let status_str = match topic.status {
        TopicStatus::Open => "open",
        TopicStatus::Closed => "closed",
        TopicStatus::Archived => "archived",
    };

    Ok((
        StatusCode::CREATED,
        Json(TopicDTO {
            id: topic.id,
            stream_id: topic.stream_id,
            name: topic.name,
            status: status_str.to_string(),
            created_by: topic.created_by,
            created_at: topic.created_at,
            last_active: topic.last_active,
        }),
    ))
}

pub async fn list_topics(
    State(pool): State<PgPool>,
    Path(stream_id): Path<uuid::Uuid>,
    OptionalAuthUser(auth_user): OptionalAuthUser,
    Query(query): Query<TopicsQuery>,
) -> Result<Json<Vec<TopicDTO>>, AppError> {
    // Look up stream
    let stream = pebesen_db::streams::find_by_id(&pool, stream_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Look up space to check visibility
    let space = pebesen_db::spaces::find_by_id(&pool, stream.space_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Public streams in public spaces: no auth required
    // Otherwise require auth + space membership
    if !(stream.visibility == pebesen_core::StreamVisibility::Public
        && space.visibility == pebesen_core::Visibility::Public)
    {
        let user = auth_user.ok_or(AppError::Forbidden)?;
        pebesen_db::memberships::find(&pool, user.id, space.id)
            .await
            .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
            .ok_or(AppError::Forbidden)?;
    }

    // Parse status filter (default: open)
    let status_filter = if let Some(ref status) = query.status {
        if status == "all" {
            None
        } else {
            Some(match status.to_lowercase().as_str() {
                "open" => TopicStatus::Open,
                "closed" => TopicStatus::Closed,
                "archived" => TopicStatus::Archived,
                _ => {
                    return Err(AppError::BadRequest(
                        "Invalid status. Must be open, closed, archived, or all".to_string(),
                    ));
                }
            })
        }
    } else {
        Some(TopicStatus::Open)
    };

    let mut topics = pebesen_db::topics::list_by_stream(&pool, stream_id, status_filter)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Apply cursor pagination
    let page_size = 50;
    if let Some(cursor) = query.cursor
        && let Ok(timestamp) = cursor.parse::<i64>()
    {
        let cursor_time =
            chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_else(chrono::Utc::now);
        topics.retain(|t| t.last_active < cursor_time);
    }
    topics.truncate(page_size);

    let topic_dtos: Vec<TopicDTO> = topics
        .into_iter()
        .map(|topic| {
            let status_str = match topic.status {
                TopicStatus::Open => "open",
                TopicStatus::Closed => "closed",
                TopicStatus::Archived => "archived",
            };
            TopicDTO {
                id: topic.id,
                stream_id: topic.stream_id,
                name: topic.name,
                status: status_str.to_string(),
                created_by: topic.created_by,
                created_at: topic.created_at,
                last_active: topic.last_active,
            }
        })
        .collect();

    Ok(Json(topic_dtos))
}

pub async fn rename_topic(
    State(pool): State<PgPool>,
    Path(topic_id): Path<uuid::Uuid>,
    auth_user: AuthUser,
    Json(payload): Json<RenameTopicRequest>,
) -> Result<Json<TopicDTO>, AppError> {
    // Look up topic
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

    // Require auth + space membership (any member)
    pebesen_db::memberships::find(&pool, auth_user.id, space.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::Forbidden)?;

    // Validate new name: 1–128 chars
    let name = payload.name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(AppError::BadRequest(
            "Name must be 1-128 characters".to_string(),
        ));
    }

    // Check uniqueness if name changed
    if name.to_lowercase() != topic.name.to_lowercase() {
        let existing_topics = pebesen_db::topics::list_by_stream(&pool, topic.stream_id, None)
            .await
            .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;
        if existing_topics
            .iter()
            .any(|t| t.id != topic_id && t.name.to_lowercase() == name.to_lowercase())
        {
            return Err(AppError::Conflict);
        }
    }

    // Rename topic
    pebesen_db::topics::rename(&pool, topic_id, name)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Fetch updated topic
    let updated_topic = pebesen_db::topics::find_by_id(&pool, topic_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    let status_str = match updated_topic.status {
        TopicStatus::Open => "open",
        TopicStatus::Closed => "closed",
        TopicStatus::Archived => "archived",
    };

    Ok(Json(TopicDTO {
        id: updated_topic.id,
        stream_id: updated_topic.stream_id,
        name: updated_topic.name,
        status: status_str.to_string(),
        created_by: updated_topic.created_by,
        created_at: updated_topic.created_at,
        last_active: updated_topic.last_active,
    }))
}

pub async fn update_topic_status(
    State(pool): State<PgPool>,
    Path(topic_id): Path<uuid::Uuid>,
    auth_user: AuthUser,
    Json(payload): Json<UpdateTopicStatusRequest>,
) -> Result<Json<TopicDTO>, AppError> {
    // Look up topic
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

    // Validate status value
    let new_status = match payload.status.to_lowercase().as_str() {
        "open" => TopicStatus::Open,
        "closed" => TopicStatus::Closed,
        "archived" => TopicStatus::Archived,
        _ => {
            return Err(AppError::BadRequest(
                "Invalid status. Must be open, closed, or archived".to_string(),
            ));
        }
    };

    // Update status
    pebesen_db::topics::set_status(&pool, topic_id, new_status)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Fetch updated topic
    let updated_topic = pebesen_db::topics::find_by_id(&pool, topic_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    let status_str = match updated_topic.status {
        TopicStatus::Open => "open",
        TopicStatus::Closed => "closed",
        TopicStatus::Archived => "archived",
    };

    Ok(Json(TopicDTO {
        id: updated_topic.id,
        stream_id: updated_topic.stream_id,
        name: updated_topic.name,
        status: status_str.to_string(),
        created_by: updated_topic.created_by,
        created_at: updated_topic.created_at,
        last_active: updated_topic.last_active,
    }))
}

pub async fn search_topics(
    State(pool): State<PgPool>,
    Path(stream_id): Path<uuid::Uuid>,
    Query(query): Query<TopicsQuery>,
) -> Result<Json<Vec<TopicDTO>>, AppError> {
    let prefix = query.cursor.clone().unwrap_or_default();

    let topics = pebesen_db::topics::search_by_name_prefix(&pool, stream_id, &prefix)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Exclude archived from suggestions
    let topics: Vec<_> = topics
        .into_iter()
        .filter(|t| !matches!(t.status, TopicStatus::Archived))
        .take(10)
        .collect();

    let topic_dtos: Vec<TopicDTO> = topics
        .into_iter()
        .map(|topic| {
            let status_str = match topic.status {
                TopicStatus::Open => "open",
                TopicStatus::Closed => "closed",
                TopicStatus::Archived => "archived",
            };
            TopicDTO {
                id: topic.id,
                stream_id: topic.stream_id,
                name: topic.name,
                status: status_str.to_string(),
                created_by: topic.created_by,
                created_at: topic.created_at,
                last_active: topic.last_active,
            }
        })
        .collect();

    Ok(Json(topic_dtos))
}
