use pebesen_core::{Space, Visibility};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn insert(
    pool: &PgPool,
    slug: &str,
    name: &str,
    visibility: Visibility,
    description: Option<&str>,
) -> Result<Space, sqlx::Error> {
    let visibility_str = match visibility {
        Visibility::Public => "public",
        Visibility::Private => "private",
        Visibility::Secret => "secret",
    };

    sqlx::query_as::<_, Space>(
        r#"
        INSERT INTO spaces (slug, name, visibility, description)
        VALUES ($1, $2, $3, $4)
        RETURNING id, slug, name, description, visibility as "visibility: Visibility", created_at
        "#,
    )
    .bind(slug)
    .bind(name)
    .bind(visibility_str)
    .bind(description)
    .fetch_one(pool)
    .await
}

pub async fn find_by_slug(pool: &PgPool, slug: &str) -> Result<Option<Space>, sqlx::Error> {
    sqlx::query_as::<_, Space>(
        r#"
        SELECT id, slug, name, description, visibility as "visibility: Visibility", created_at
        FROM spaces
        WHERE lower(slug) = lower($1)
        "#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Space>, sqlx::Error> {
    sqlx::query_as::<_, Space>(
        r#"
        SELECT id, slug, name, description, visibility as "visibility: Visibility", created_at
        FROM spaces
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}
