use crate::middleware::{AuthUser, OptionalAuthUser};
use axum::{Json, extract::Path, extract::Query, extract::State, http::StatusCode};
use pebesen_core::{AppError, Visibility};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Deserialize)]
pub struct CreateSpaceRequest {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
}

#[derive(Debug, Serialize)]
pub struct SpaceDTO {
    pub id: uuid::Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub visibility: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub member_count: i64,
}

#[derive(Debug, Serialize)]
pub struct MembershipDTO {
    pub user_id: uuid::Uuid,
    pub space_id: uuid::Uuid,
    pub role: String,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct UserDTO {
    pub id: uuid::Uuid,
    pub username: String,
    pub display_name: String,
}

#[derive(Debug, Serialize)]
pub struct MemberDTO {
    pub user: UserDTO,
    pub role: String,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct MembersQuery {
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedMembers {
    pub members: Vec<MemberDTO>,
    pub next_cursor: Option<String>,
}

pub async fn create_space(
    State(pool): State<PgPool>,
    auth_user: AuthUser,
    Json(payload): Json<CreateSpaceRequest>,
) -> Result<(StatusCode, Json<SpaceDTO>), AppError> {
    // Require auth (handled by AuthUser extractor)

    // Validate slug: 3–48 chars, lowercase alphanumeric + hyphen, no leading/trailing hyphen
    let slug_regex = regex::Regex::new(r"^[a-z0-9][a-z0-9-]{1,46}[a-z0-9]$")
        .map_err(|e| AppError::Internal(format!("Invalid slug regex: {}", e)))?;
    if !slug_regex.is_match(&payload.slug) {
        return Err(AppError::BadRequest("Slug must be 3-48 characters, lowercase alphanumeric with hyphens, no leading/trailing hyphens".to_string()));
    }

    // Check slug uniqueness — return 409 if taken
    if pebesen_db::spaces::find_by_slug(&pool, &payload.slug)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .is_some()
    {
        return Err(AppError::Conflict);
    }

    // Parse visibility
    let visibility = match payload.visibility.to_lowercase().as_str() {
        "public" => Visibility::Public,
        "private" => Visibility::Private,
        "secret" => Visibility::Secret,
        _ => {
            return Err(AppError::BadRequest(
                "Invalid visibility. Must be public, private, or secret".to_string(),
            ));
        }
    };

    // Insert space row
    let space = pebesen_db::spaces::insert(
        &pool,
        &payload.slug,
        &payload.name,
        visibility,
        payload.description.as_deref(),
    )
    .await
    .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Create membership with role=Owner for creator
    pebesen_db::memberships::insert(&pool, auth_user.id, space.id, pebesen_core::Role::Owner)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Return 201 with SpaceDTO
    let visibility_str = match space.visibility {
        Visibility::Public => "public",
        Visibility::Private => "private",
        Visibility::Secret => "secret",
    };

    Ok((
        StatusCode::CREATED,
        Json(SpaceDTO {
            id: space.id,
            slug: space.slug,
            name: space.name,
            description: space.description,
            visibility: visibility_str.to_string(),
            created_at: space.created_at,
            member_count: 0, // Will be updated after membership insertion
        }),
    ))
}

pub async fn get_space(
    State(pool): State<PgPool>,
    Path(slug): Path<String>,
    OptionalAuthUser(auth_user): OptionalAuthUser,
) -> Result<Json<SpaceDTO>, AppError> {
    // Look up space by slug
    let space = pebesen_db::spaces::find_by_slug(&pool, &slug)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Check visibility and membership
    match space.visibility {
        Visibility::Public => {
            // Public spaces: no auth required
        }
        Visibility::Private | Visibility::Secret => {
            // Private/secret spaces: require auth + membership
            let user = auth_user.ok_or(AppError::Forbidden)?;
            pebesen_db::memberships::find(&pool, user.id, space.id)
                .await
                .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
                .ok_or(AppError::Forbidden)?;
        }
    }

    // Get member count
    let members = pebesen_db::memberships::list_by_space(&pool, space.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;
    let member_count = members.len() as i64;

    let visibility_str = match space.visibility {
        Visibility::Public => "public",
        Visibility::Private => "private",
        Visibility::Secret => "secret",
    };

    Ok(Json(SpaceDTO {
        id: space.id,
        slug: space.slug,
        name: space.name,
        description: space.description,
        visibility: visibility_str.to_string(),
        created_at: space.created_at,
        member_count,
    }))
}

pub async fn join_space(
    State(pool): State<PgPool>,
    Path(slug): Path<String>,
    auth_user: AuthUser,
) -> Result<(StatusCode, Json<MembershipDTO>), AppError> {
    // Require auth (handled by AuthUser extractor)

    // Look up space by slug
    let space = pebesen_db::spaces::find_by_slug(&pool, &slug)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    // Return 403 if space is private
    if space.visibility == Visibility::Private || space.visibility == Visibility::Secret {
        return Err(AppError::Forbidden);
    }

    // Return 409 if already a member
    if pebesen_db::memberships::find(&pool, auth_user.id, space.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .is_some()
    {
        return Err(AppError::Conflict);
    }

    // Insert membership with role member
    let membership =
        pebesen_db::memberships::insert(&pool, auth_user.id, space.id, pebesen_core::Role::Editor)
            .await
            .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    let role_str = match membership.role {
        pebesen_core::Role::Owner => "owner",
        pebesen_core::Role::Admin => "admin",
        pebesen_core::Role::Editor => "editor",
        pebesen_core::Role::Viewer => "viewer",
    };

    Ok((
        StatusCode::CREATED,
        Json(MembershipDTO {
            user_id: membership.user_id,
            space_id: membership.space_id,
            role: role_str.to_string(),
            joined_at: membership.joined_at,
        }),
    ))
}

pub async fn list_members(
    State(pool): State<PgPool>,
    Path(slug): Path<String>,
    auth_user: AuthUser,
    Query(query): Query<MembersQuery>,
) -> Result<Json<PaginatedMembers>, AppError> {
    // Require auth + membership
    let space = pebesen_db::spaces::find_by_slug(&pool, &slug)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::NotFound)?;

    pebesen_db::memberships::find(&pool, auth_user.id, space.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::Forbidden)?;

    // Get all members (cursor-based pagination)
    let memberships = pebesen_db::memberships::list_by_space(&pool, space.id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Apply cursor filtering
    let page_size = 50;
    let start_index = if let Some(cursor) = query.cursor {
        cursor.parse::<usize>().unwrap_or(0)
    } else {
        0
    };

    let paginated_memberships: Vec<_> = memberships
        .into_iter()
        .skip(start_index)
        .take(page_size)
        .collect();

    // Batch fetch all users in one query to avoid N+1
    let user_ids: Vec<_> = paginated_memberships.iter().map(|m| m.user_id).collect();
    let users = pebesen_db::users::find_by_ids(&pool, &user_ids)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Create user lookup map
    let user_map: std::collections::HashMap<_, _> = users.into_iter().map(|u| (u.id, u)).collect();

    // Build member DTOs
    let mut paginated_members: Vec<MemberDTO> = Vec::new();
    for membership in paginated_memberships {
        if let Some(user) = user_map.get(&membership.user_id) {
            let role_str = match membership.role {
                pebesen_core::Role::Owner => "owner",
                pebesen_core::Role::Admin => "admin",
                pebesen_core::Role::Editor => "editor",
                pebesen_core::Role::Viewer => "viewer",
            };
            paginated_members.push(MemberDTO {
                user: UserDTO {
                    id: user.id,
                    username: user.username.clone(),
                    display_name: user.display_name.clone(),
                },
                role: role_str.to_string(),
                joined_at: membership.joined_at,
            });
        }
    }

    // Calculate next cursor
    let next_cursor = if start_index + page_size < paginated_members.len() + start_index {
        Some((start_index + page_size).to_string())
    } else {
        None
    };

    Ok(Json(PaginatedMembers {
        members: paginated_members,
        next_cursor,
    }))
}
