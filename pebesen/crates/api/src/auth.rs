use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use pebesen_core::AppError;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub username: String,
    pub display_name: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub id: uuid::Uuid,
    pub email: String,
    pub username: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub access_token: String,
}

pub async fn register(
    State(pool): State<PgPool>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, AppError> {
    // Validate email format
    let email_regex = regex::Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$")
        .map_err(|e| AppError::Internal(format!("Invalid email regex: {}", e)))?;
    if !email_regex.is_match(&payload.email) {
        return Err(AppError::BadRequest("Invalid email format".to_string()));
    }

    // Validate username: 3–32 chars, alphanumeric + underscore + hyphen
    let username_regex = regex::Regex::new(r"^[a-zA-Z0-9_-]{3,32}$")
        .map_err(|e| AppError::Internal(format!("Invalid username regex: {}", e)))?;
    if !username_regex.is_match(&payload.username) {
        return Err(AppError::BadRequest("Username must be 3-32 characters, containing only letters, numbers, underscores, and hyphens".to_string()));
    }

    // Validate password: minimum 8 chars, at least one non-alpha char
    if payload.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }
    let has_non_alpha = payload.password.chars().any(|c| !c.is_alphabetic());
    if !has_non_alpha {
        return Err(AppError::BadRequest(
            "Password must contain at least one non-alphabetic character".to_string(),
        ));
    }

    // Check email uniqueness
    if pebesen_db::users::find_by_email(&pool, &payload.email)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .is_some()
    {
        return Err(AppError::Conflict);
    }

    // Check username uniqueness
    if pebesen_db::users::find_by_username(&pool, &payload.username)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .is_some()
    {
        return Err(AppError::Conflict);
    }

    // Hash password with Argon2id (memory: 64MB, iterations: 3, parallelism: 4)
    let password_hash =
        argon2::password_hash::SaltString::generate(argon2::password_hash::rand_core::OsRng);
    let argon2_config = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(65536 * 1024, 3, 4, None) // 64MB = 65536KB
            .map_err(|e| AppError::Internal(format!("Argon2 config error: {}", e)))?,
    );
    let password_hash = argon2::password_hash::PasswordHash::generate(
        argon2_config,
        payload.password.as_bytes(),
        &password_hash,
    )
    .map_err(|e| AppError::Internal(format!("Password hashing error: {}", e)))?
    .to_string();

    // Insert user row
    let user = pebesen_db::users::insert(
        &pool,
        &payload.email,
        &payload.username,
        &payload.display_name,
        &password_hash,
    )
    .await
    .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Return 201 with UserDTO (no password hash)
    Ok(Json(RegisterResponse {
        id: user.id,
        email: user.email,
        username: user.username,
        display_name: user.display_name,
    }))
}

pub async fn login(
    State(pool): State<PgPool>,
    Json(payload): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    // Look up user by email
    let user = pebesen_db::users::find_by_email(&pool, &payload.email)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::Unauthorized)?;

    // Verify password with Argon2id — constant-time comparison
    let parsed_hash = argon2::password_hash::PasswordHash::new(&user.password_hash)
        .map_err(|e| AppError::Internal(format!("Invalid password hash: {}", e)))?;
    let argon2_config = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(65536, 3, 4, None)
            .map_err(|e| AppError::Internal(format!("Argon2 config error: {}", e)))?,
    );
    argon2::PasswordVerifier::verify_password(
        &argon2_config,
        payload.password.as_bytes(),
        &parsed_hash,
    )
    .map_err(|_| AppError::Unauthorized)?;

    // On success: generate access JWT (15 min, signed with JWT_SECRET)
    let jwt_secret = std::env::var("JWT_SECRET")
        .map_err(|_| AppError::Internal("JWT_SECRET not configured".to_string()))?;
    let now = chrono::Utc::now();
    let exp = now + chrono::Duration::seconds(900); // 15 minutes
    let claims = pebesen_core::AuthClaims {
        user_id: user.id,
        email: user.email.clone(),
        exp: exp.timestamp(),
    };
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_ref()),
    )
    .map_err(|e| AppError::Internal(format!("JWT encoding error: {}", e)))?;

    // Generate refresh token (UUID v4, stored in Redis with TTL 30 days)
    let refresh_token = Uuid::new_v4();
    let redis_url = std::env::var("REDIS_URL")
        .map_err(|_| AppError::Internal("REDIS_URL not configured".to_string()))?;
    let mut redis_conn = redis::Client::open(redis_url)
        .map_err(|e| AppError::Internal(format!("Redis connection error: {}", e)))?
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| AppError::Internal(format!("Redis connection error: {}", e)))?;
    let key = format!("refresh_token:{}", refresh_token);
    redis_conn
        .set_ex::<_, _, ()>(&key, user.id.to_string(), 2592000u64)
        .await
        .map_err(|e| AppError::Internal(format!("Redis error: {}", e)))?;

    // Set refresh token as httpOnly; Secure; SameSite=Strict cookie
    let cookie = format!(
        "refresh_token={}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=2592000",
        refresh_token
    );

    // Return 200 with access token in response body
    let mut response = Json(LoginResponse {
        access_token: token,
    })
    .into_response();
    response.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        cookie
            .parse()
            .map_err(|e| AppError::Internal(format!("Cookie error: {}", e)))?,
    );
    Ok(response)
}

pub async fn refresh(
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    // Read refresh token from cookie
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .ok_or(AppError::Unauthorized)?;
    let refresh_token = cookie_header
        .split(';')
        .find_map(|c| {
            let parts = c.trim().split_once('=');
            if let Some((key, value)) = parts {
                if key == "refresh_token" {
                    Some(value.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .ok_or(AppError::Unauthorized)?;

    // Look up token in Redis — return 401 if missing/expired
    let redis_url = std::env::var("REDIS_URL")
        .map_err(|_| AppError::Internal("REDIS_URL not configured".to_string()))?;
    let mut redis_conn = redis::Client::open(redis_url)
        .map_err(|e| AppError::Internal(format!("Redis connection error: {}", e)))?
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| AppError::Internal(format!("Redis connection error: {}", e)))?;
    let key = format!("refresh_token:{}", refresh_token);
    let user_id_str: String = redis_conn
        .get(&key)
        .await
        .map_err(|e| AppError::Internal(format!("Redis error: {}", e)))?;
    let user_id = Uuid::parse_str(&user_id_str)
        .map_err(|_| AppError::Internal("Invalid user ID in Redis".to_string()))?;

    // Look up user to get email
    let user = pebesen_db::users::find_by_id(&pool, user_id)
        .await
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?
        .ok_or(AppError::Unauthorized)?;

    // Generate new access JWT
    let jwt_secret = std::env::var("JWT_SECRET")
        .map_err(|_| AppError::Internal("JWT_SECRET not configured".to_string()))?;
    let now = chrono::Utc::now();
    let exp = now + chrono::Duration::seconds(900); // 15 minutes
    let claims = pebesen_core::AuthClaims {
        user_id: user.id,
        email: user.email.clone(),
        exp: exp.timestamp(),
    };
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_ref()),
    )
    .map_err(|e| AppError::Internal(format!("JWT encoding error: {}", e)))?;

    // Rotate refresh token (delete old, insert new) — prevent replay
    redis_conn
        .del::<_, ()>(&key)
        .await
        .map_err(|e| AppError::Internal(format!("Redis error: {}", e)))?;
    let new_refresh_token = Uuid::new_v4();
    let new_key = format!("refresh_token:{}", new_refresh_token);
    redis_conn
        .set_ex::<_, _, ()>(&new_key, user_id.to_string(), 2592000u64)
        .await
        .map_err(|e| AppError::Internal(format!("Redis error: {}", e)))?;

    // Set new refresh token cookie
    let cookie = format!(
        "refresh_token={}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=2592000",
        new_refresh_token
    );

    // Return 200 with new access token
    let mut response = Json(LoginResponse {
        access_token: token,
    })
    .into_response();
    response.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        cookie
            .parse()
            .map_err(|e| AppError::Internal(format!("Cookie error: {}", e)))?,
    );
    Ok(response)
}

pub async fn logout(headers: axum::http::HeaderMap) -> Result<impl IntoResponse, AppError> {
    // Read refresh token from cookie
    let cookie_header = headers
        .get(axum::http::header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .ok_or(AppError::Unauthorized)?;
    let refresh_token = cookie_header
        .split(';')
        .find_map(|c| {
            let parts = c.trim().split_once('=');
            if let Some((key, value)) = parts {
                if key == "refresh_token" {
                    Some(value.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .ok_or(AppError::Unauthorized)?;

    // Delete from Redis
    let redis_url = std::env::var("REDIS_URL")
        .map_err(|_| AppError::Internal("REDIS_URL not configured".to_string()))?;
    let mut redis_conn = redis::Client::open(redis_url)
        .map_err(|e| AppError::Internal(format!("Redis connection error: {}", e)))?
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| AppError::Internal(format!("Redis connection error: {}", e)))?;
    let key = format!("refresh_token:{}", refresh_token);
    redis_conn
        .del::<_, ()>(&key)
        .await
        .map_err(|e| AppError::Internal(format!("Redis error: {}", e)))?;

    // Clear cookie (set Max-Age=0)
    let cookie = "refresh_token=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0";

    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        axum::http::header::SET_COOKIE,
        cookie
            .parse()
            .map_err(|e| AppError::Internal(format!("Cookie error: {}", e)))?,
    );
    Ok(response)
}
