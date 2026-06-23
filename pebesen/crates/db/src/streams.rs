use pebesen_core::{Stream, StreamVisibility};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn update(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    description: Option<Option<&str>>,
    visibility: Option<StreamVisibility>,
) -> Result<Stream, sqlx::Error> {
    // Build dynamic query based on provided fields
    let mut query = String::from("UPDATE streams SET ");
    let mut updates = Vec::new();
    let mut index = 1;

    if name.is_some() {
        updates.push(format!("name = ${}", index));
        index += 1;
    }
    if description.is_some() {
        updates.push(format!("description = ${}", index));
        index += 1;
    }
    if visibility.is_some() {
        updates.push(format!("visibility = ${}", index));
        index += 1;
    }

    if updates.is_empty() {
        // No updates, just return the existing stream
        return find_by_id(pool, id).await?.ok_or(sqlx::Error::RowNotFound);
    }

    query.push_str(&updates.join(", "));
    query.push_str(&format!(" WHERE id = ${} RETURNING id, space_id, name, description, visibility as \"visibility: StreamVisibility\", created_at", index));

    let mut query_builder = sqlx::query_as::<_, Stream>(&query);

    if let Some(n) = name {
        query_builder = query_builder.bind(n);
    }
    if let Some(d) = description {
        query_builder = query_builder.bind(d);
    }
    if let Some(v) = visibility {
        let visibility_str = match v {
            StreamVisibility::Public => "public",
            StreamVisibility::Private => "private",
        };
        query_builder = query_builder.bind(visibility_str);
    }
    query_builder = query_builder.bind(id);

    query_builder.fetch_one(pool).await
}

pub async fn insert(
    pool: &PgPool,
    space_id: Uuid,
    name: &str,
    description: Option<&str>,
    visibility: StreamVisibility,
) -> Result<Stream, sqlx::Error> {
    let visibility_str = match visibility {
        StreamVisibility::Public => "public",
        StreamVisibility::Private => "private",
    };

    sqlx::query_as::<_, Stream>(
        r#"
        INSERT INTO streams (space_id, name, description, visibility)
        VALUES ($1, $2, $3, $4)
        RETURNING id, space_id, name, description, visibility as "visibility: StreamVisibility", created_at
        "#,
    )
    .bind(space_id)
    .bind(name)
    .bind(description)
    .bind(visibility_str)
    .fetch_one(pool)
    .await
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Stream>, sqlx::Error> {
    sqlx::query_as::<_, Stream>(
        r#"
        SELECT id, space_id, name, description, visibility as "visibility: StreamVisibility", created_at
        FROM streams
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn list_by_space(
    pool: &PgPool,
    space_id: Uuid,
    user_id: Option<Uuid>,
) -> Result<Vec<Stream>, sqlx::Error> {
    if let Some(uid) = user_id {
        // Filter: include public streams + private streams where user has membership
        sqlx::query_as::<_, Stream>(
            r#"
            SELECT s.id, s.space_id, s.name, s.description, s.visibility as "visibility: StreamVisibility", s.created_at
            FROM streams s
            WHERE s.space_id = $1
            AND (
                s.visibility = 'public'
                OR EXISTS (
                    SELECT 1 FROM memberships m
                    WHERE m.space_id = s.space_id AND m.user_id = $2
                )
            )
            "#,
        )
        .bind(space_id)
        .bind(uid)
        .fetch_all(pool)
        .await
    } else {
        // Only public streams for unauthenticated users
        sqlx::query_as::<_, Stream>(
            r#"
            SELECT id, space_id, name, description, visibility as "visibility: StreamVisibility", created_at
            FROM streams
            WHERE space_id = $1 AND visibility = 'public'
            "#,
        )
        .bind(space_id)
        .fetch_all(pool)
        .await
    }
}
