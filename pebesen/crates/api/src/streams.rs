use crate::middleware::{AuthUser, OptionalAuthUser};
use axum::{Json, extract::Path, extract::State, http::StatusCode};
use pebesen_core::{AppError, StreamVisibility};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Deserialize)]
pub struct CreateStreamRequest {
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateStreamRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StreamDTO {
    pub id: uuid::Uuid,
    pub space_id: uuid::Uuid,
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn create_stream(
    State(pool): State<PgPool>,
    Path(slug): Path<String>,
    auth_user: AuthUser,
    Json(payload): Json<CreateStreamRequest>,
) -> Result<(StatusCode, Json<StreamDTO>), AppError> {
    // Look up space by slug
    let space = pebesen_db::spaces::find_by_slug(&pool, &slug)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Require auth + role admin or owner
    let membership = pebesen_db::memberships::find(&pool, auth_user.id, space.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::Forbidden)?;

    if membership.role != pebesen_core::Role::Owner && membership.role != pebesen_core::Role::Admin
    {
        return Err(AppError::Forbidden);
    }

    // Validate name: 1–64 chars, no leading/trailing whitespace
    let name = payload.name.trim();
    if name.is_empty() || name.len() > 64 {
        return Err(AppError::BadRequest(
            "Name must be 1-64 characters, no leading/trailing whitespace".to_string(),
        ));
    }
    if name != payload.name {
        return Err(AppError::BadRequest(
            "Name cannot have leading or trailing whitespace".to_string(),
        ));
    }

    // Check name uniqueness within space (case-insensitive)
    let existing_streams = pebesen_db::streams::list_by_space(&pool, space.id, Some(auth_user.id))
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;
    if existing_streams
        .iter()
        .any(|s| s.name.to_lowercase() == name.to_lowercase())
    {
        return Err(AppError::Conflict);
    }

    // Parse visibility
    let visibility = match payload.visibility.to_lowercase().as_str() {
        "public" => StreamVisibility::Public,
        "private" => StreamVisibility::Private,
        _ => {
            return Err(AppError::BadRequest(
                "Invalid visibility. Must be public or private".to_string(),
            ));
        }
    };

    // Insert stream
    let stream = pebesen_db::streams::insert(
        &pool,
        space.id,
        name,
        payload.description.as_deref(),
        visibility,
    )
    .await
    .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    let visibility_str = match stream.visibility {
        StreamVisibility::Public => "public",
        StreamVisibility::Private => "private",
    };

    Ok((
        StatusCode::CREATED,
        Json(StreamDTO {
            id: stream.id,
            space_id: stream.space_id,
            name: stream.name,
            description: stream.description,
            visibility: visibility_str.to_string(),
            created_at: stream.created_at,
        }),
    ))
}

pub async fn list_streams(
    State(pool): State<PgPool>,
    Path(slug): Path<String>,
    OptionalAuthUser(auth_user): OptionalAuthUser,
) -> Result<Json<Vec<StreamDTO>>, AppError> {
    // Look up space by slug
    let space = pebesen_db::spaces::find_by_slug(&pool, &slug)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Public spaces: return public streams without auth
    // Private/secret spaces: require auth + membership
    let user_id = match space.visibility {
        pebesen_core::Visibility::Public => auth_user.map(|u| u.id),
        pebesen_core::Visibility::Private | pebesen_core::Visibility::Secret => {
            let user = auth_user.ok_or(AppError::Forbidden)?;
            pebesen_db::memberships::find(&pool, user.id, space.id)
                .await
                .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
                .ok_or(AppError::Forbidden)?;
            Some(user.id)
        }
    };

    let mut streams = pebesen_db::streams::list_by_space(&pool, space.id, user_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Return ordered by created_at ASC
    streams.sort_by_key(|a| a.created_at);

    let stream_dtos: Vec<StreamDTO> = streams
        .into_iter()
        .map(|stream| {
            let visibility_str = match stream.visibility {
                StreamVisibility::Public => "public",
                StreamVisibility::Private => "private",
            };
            StreamDTO {
                id: stream.id,
                space_id: stream.space_id,
                name: stream.name,
                description: stream.description,
                visibility: visibility_str.to_string(),
                created_at: stream.created_at,
            }
        })
        .collect();

    Ok(Json(stream_dtos))
}

pub async fn update_stream(
    State(pool): State<PgPool>,
    Path((slug, stream_id)): Path<(String, uuid::Uuid)>,
    auth_user: AuthUser,
    Json(payload): Json<UpdateStreamRequest>,
) -> Result<Json<StreamDTO>, AppError> {
    // Look up space by slug
    let space = pebesen_db::spaces::find_by_slug(&pool, &slug)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Require auth + role admin or owner
    let membership = pebesen_db::memberships::find(&pool, auth_user.id, space.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::Forbidden)?;

    if membership.role != pebesen_core::Role::Owner && membership.role != pebesen_core::Role::Admin
    {
        return Err(AppError::Forbidden);
    }

    // Look up stream
    let stream = pebesen_db::streams::find_by_id(&pool, stream_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Validate and update name if provided
    let new_name = if let Some(ref name) = payload.name {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.len() > 64 {
            return Err(AppError::BadRequest(
                "Name must be 1-64 characters, no leading/trailing whitespace".to_string(),
            ));
        }
        if trimmed != *name {
            return Err(AppError::BadRequest(
                "Name cannot have leading or trailing whitespace".to_string(),
            ));
        }
        // Re-validate name uniqueness if changed
        if trimmed.to_lowercase() != stream.name.to_lowercase() {
            let existing_streams =
                pebesen_db::streams::list_by_space(&pool, space.id, Some(auth_user.id))
                    .await
                    .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;
            if existing_streams
                .iter()
                .any(|s| s.id != stream_id && s.name.to_lowercase() == trimmed.to_lowercase())
            {
                return Err(AppError::Conflict);
            }
        }
        Some(trimmed.to_string())
    } else {
        None
    };

    // Parse visibility if provided
    let new_visibility = if let Some(ref vis) = payload.visibility {
        Some(match vis.to_lowercase().as_str() {
            "public" => StreamVisibility::Public,
            "private" => StreamVisibility::Private,
            _ => {
                return Err(AppError::BadRequest(
                    "Invalid visibility. Must be public or private".to_string(),
                ));
            }
        })
    } else {
        None
    };

    // Update stream in database
    let updated_stream = pebesen_db::streams::update(
        &pool,
        stream_id,
        new_name.as_deref(),
        payload.description.as_deref().map(Some),
        new_visibility,
    )
    .await
    .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    let visibility_str = match updated_stream.visibility {
        StreamVisibility::Public => "public",
        StreamVisibility::Private => "private",
    };

    Ok(Json(StreamDTO {
        id: updated_stream.id,
        space_id: updated_stream.space_id,
        name: updated_stream.name,
        description: updated_stream.description,
        visibility: visibility_str.to_string(),
        created_at: updated_stream.created_at,
    }))
}
