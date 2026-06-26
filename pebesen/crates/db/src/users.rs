use pebesen_core::User;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn insert(
    pool: &PgPool,
    email: &str,
    username: &str,
    display_name: &str,
    password_hash: &str,
) -> Result<User, sqlx::Error> {
    sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (email, username, display_name, password_hash, settings)
        VALUES ($1, $2, $3, $4, '{}')
        RETURNING id, username, display_name, email, password_hash, created_at, settings
        "#,
    )
    .bind(email)
    .bind(username)
    .bind(display_name)
    .bind(password_hash)
    .fetch_one(pool)
    .await
}

pub async fn find_by_email(pool: &PgPool, email: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT id, username, display_name, email, password_hash, created_at, settings
        FROM users
        WHERE lower(email) = lower($1)
        "#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT id, username, display_name, email, password_hash, created_at, settings
        FROM users
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_username(pool: &PgPool, username: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT id, username, display_name, email, password_hash, created_at, settings
        FROM users
        WHERE lower(username) = lower($1)
        "#,
    )
    .bind(username)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_ids(pool: &PgPool, ids: &[Uuid]) -> Result<Vec<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        r#"
        SELECT id, username, display_name, email, password_hash, created_at, settings
        FROM users
        WHERE id = ANY($1)
        "#,
    )
    .bind(ids)
    .fetch_all(pool)
    .await
}
