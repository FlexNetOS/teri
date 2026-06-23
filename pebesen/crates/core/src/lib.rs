#![deny(clippy::all)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Domain Types

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
    pub settings: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Space {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub visibility: Visibility,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "TEXT")]
pub enum Visibility {
    Public,
    Private,
    Secret,
}

impl std::str::FromStr for Visibility {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "public" => Ok(Visibility::Public),
            "private" => Ok(Visibility::Private),
            "secret" => Ok(Visibility::Secret),
            _ => Err(format!("Invalid visibility: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Membership {
    pub user_id: Uuid,
    pub space_id: Uuid,
    pub role: Role,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Stream {
    pub id: Uuid,
    pub space_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub visibility: StreamVisibility,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Topic {
    pub id: Uuid,
    pub stream_id: Uuid,
    pub name: String,
    pub status: TopicStatus,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Message {
    pub id: Uuid,
    pub topic_id: Uuid,
    pub author_id: Uuid,
    pub content: String,
    pub rendered: Option<String>,
    pub edited_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
pub enum TopicStatus {
    Open,
    Closed,
    Archived,
}

impl std::str::FromStr for TopicStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(TopicStatus::Open),
            "closed" => Ok(TopicStatus::Closed),
            "archived" => Ok(TopicStatus::Archived),
            _ => Err(format!("Invalid topic status: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "TEXT")]
pub enum StreamVisibility {
    Public,
    Private,
}

impl std::str::FromStr for StreamVisibility {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "public" => Ok(StreamVisibility::Public),
            "private" => Ok(StreamVisibility::Private),
            _ => Err(format!("Invalid stream visibility: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
pub enum Role {
    Owner,
    Admin,
    Editor,
    Viewer,
}

impl std::str::FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "owner" => Ok(Role::Owner),
            "admin" => Ok(Role::Admin),
            "editor" => Ok(Role::Editor),
            "viewer" => Ok(Role::Viewer),
            _ => Err(format!("Invalid role: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaims {
    pub user_id: Uuid,
    pub email: String,
    pub exp: i64,
}

// Error Types

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Unauthorized")]
    Unauthorized,

    #[error("Forbidden")]
    Forbidden,

    #[error("Not found")]
    NotFound,

    #[error("Conflict")]
    Conflict,

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            AppError::Unauthorized => (axum::http::StatusCode::UNAUTHORIZED, self.to_string()),
            AppError::Forbidden => (axum::http::StatusCode::FORBIDDEN, self.to_string()),
            AppError::NotFound => (axum::http::StatusCode::NOT_FOUND, self.to_string()),
            AppError::Conflict => (axum::http::StatusCode::CONFLICT, self.to_string()),
            AppError::BadRequest(_) => (axum::http::StatusCode::BAD_REQUEST, self.to_string()),
            AppError::Internal(_) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                self.to_string(),
            ),
        };

        (status, axum::Json(serde_json::json!({ "error": message }))).into_response()
    }
}

/// Renders markdown input to sanitized HTML
pub fn render_markdown(input: &str) -> String {
    use ammonia::{Builder, UrlRelative};
    use pulldown_cmark::{Parser, html};

    // Configure markdown parser with enabled extensions
    let parser = Parser::new_ext(
        input,
        pulldown_cmark::Options::ENABLE_TABLES
            | pulldown_cmark::Options::ENABLE_FOOTNOTES
            | pulldown_cmark::Options::ENABLE_STRIKETHROUGH
            | pulldown_cmark::Options::ENABLE_TASKLISTS,
    );

    // Convert markdown to HTML
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    // Sanitize HTML to prevent XSS
    let mut builder = Builder::new();
    builder
        .add_tags(&[
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "p",
            "br",
            "hr",
            "strong",
            "em",
            "u",
            "s",
            "del",
            "ul",
            "ol",
            "li",
            "code",
            "pre",
            "blockquote",
            "a",
            "img",
            "table",
            "thead",
            "tbody",
            "tr",
            "th",
            "td",
            "input",
        ])
        .add_generic_attributes(&["id", "class"])
        .add_tag_attributes("a", &["href", "title", "rel"])
        .add_tag_attributes("img", &["src", "alt", "title", "width", "height"])
        .add_tag_attributes("input", &["type", "checked", "disabled"])
        .add_tag_attributes("code", &["class"])
        .add_tag_attributes("pre", &["class"])
        .url_relative(UrlRelative::Deny);

    let sanitized = builder.clean(&html_output).to_string();

    // Add target="_blank" to external links
    sanitized.replace("<a href=", "<a target=\"_blank\" href=")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_role_from_str() {
        assert_eq!(Role::from_str("owner").unwrap(), Role::Owner);
        assert_eq!(Role::from_str("ADMIN").unwrap(), Role::Admin);
        assert!(Role::from_str("invalid").is_err());
    }

    #[test]
    fn test_visibility_from_str() {
        assert_eq!(Visibility::from_str("public").unwrap(), Visibility::Public);
        assert_eq!(
            Visibility::from_str("PRIVATE").unwrap(),
            Visibility::Private
        );
        assert!(Visibility::from_str("invalid").is_err());
    }
}
