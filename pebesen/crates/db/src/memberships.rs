use pebesen_core::{Membership, Role};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn insert(
    pool: &PgPool,
    user_id: Uuid,
    space_id: Uuid,
    role: Role,
) -> Result<Membership, sqlx::Error> {
    let role_str = match role {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Editor => "editor",
        Role::Viewer => "viewer",
    };

    sqlx::query_as::<_, Membership>(
        r#"
        INSERT INTO memberships (user_id, space_id, role)
        VALUES ($1, $2, $3)
        RETURNING user_id, space_id, role as "role: Role", joined_at
        "#,
    )
    .bind(user_id)
    .bind(space_id)
    .bind(role_str)
    .fetch_one(pool)
    .await
}

pub async fn find(
    pool: &PgPool,
    user_id: Uuid,
    space_id: Uuid,
) -> Result<Option<Membership>, sqlx::Error> {
    sqlx::query_as::<_, Membership>(
        r#"
        SELECT user_id, space_id, role as "role: Role", joined_at
        FROM memberships
        WHERE user_id = $1 AND space_id = $2
        "#,
    )
    .bind(user_id)
    .bind(space_id)
    .fetch_optional(pool)
    .await
}

pub async fn list_by_space(pool: &PgPool, space_id: Uuid) -> Result<Vec<Membership>, sqlx::Error> {
    sqlx::query_as::<_, Membership>(
        r#"
        SELECT user_id, space_id, role as "role: Role", joined_at
        FROM memberships
        WHERE space_id = $1
        "#,
    )
    .bind(space_id)
    .fetch_all(pool)
    .await
}
