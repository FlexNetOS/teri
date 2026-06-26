use axum::{async_trait, extract::FromRequestParts, http::request::Parts};
use pebesen_core::AppError;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: Uuid,
    pub username: String,
    pub email: String,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Extract Authorization: Bearer <token> header
        let auth_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(AppError::Unauthorized)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AppError::Unauthorized)?;

        // Decode and validate JWT signature + expiry
        let jwt_secret = std::env::var("JWT_SECRET")
            .map_err(|_| AppError::Internal("JWT_SECRET not configured".to_string()))?;
        let token_data = jsonwebtoken::decode::<pebesen_core::AuthClaims>(
            token,
            &jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_ref()),
            &jsonwebtoken::Validation::default(),
        )
        .map_err(|_| AppError::Unauthorized)?;

        let claims = token_data.claims;

        // Load user from DB (or short-lived cache)
        let pool = parts.extensions.get::<PgPool>().ok_or(AppError::Internal(
            "Database pool not available".to_string(),
        ))?;
        let user = pebesen_db::users::find_by_id(pool, claims.user_id)
            .await
            .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
            .ok_or(AppError::Unauthorized)?;

        // Return AuthUser { id, username, email } or 401
        Ok(AuthUser {
            id: user.id,
            username: user.username,
            email: user.email,
        })
    }
}

#[derive(Debug, Clone)]
pub struct OptionalAuthUser(pub Option<AuthUser>);

#[async_trait]
impl<S> FromRequestParts<S> for OptionalAuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(OptionalAuthUser(Some(user))),
            Err(AppError::Unauthorized) => Ok(OptionalAuthUser(None)),
            Err(e) => Err(e),
        }
    }
}

// Rate limiting removed for now - will be re-added with compatible tower_governor API
