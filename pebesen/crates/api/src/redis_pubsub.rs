use pebesen_core::AppError;
use redis::AsyncCommands;
use serde_json::json;
use uuid::Uuid;

/// Publish a message event to Redis pub/sub
pub async fn publish_message_updated(
    redis_url: &str,
    space_id: Uuid,
    message_id: Uuid,
) -> Result<(), AppError> {
    let mut redis_conn = redis::Client::open(redis_url)
        .map_err(|e| AppError::Internal(format!("Redis connection error: {}", e)))?
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| AppError::Internal(format!("Redis connection error: {}", e)))?;

    let channel = format!("space:{}", space_id);
    let payload = json!({
        "type": "message_updated",
        "space_id": space_id,
        "message_id": message_id
    });

    redis_conn
        .publish::<_, _, ()>(&channel, payload.to_string())
        .await
        .map_err(|e| AppError::Internal(format!("Redis publish error: {}", e)))?;

    Ok(())
}

/// Publish a message deleted event to Redis pub/sub
pub async fn publish_message_deleted(
    redis_url: &str,
    space_id: Uuid,
    message_id: Uuid,
) -> Result<(), AppError> {
    let mut redis_conn = redis::Client::open(redis_url)
        .map_err(|e| AppError::Internal(format!("Redis connection error: {}", e)))?
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| AppError::Internal(format!("Redis connection error: {}", e)))?;

    let channel = format!("space:{}", space_id);
    let payload = json!({
        "type": "message_deleted",
        "space_id": space_id,
        "message_id": message_id
    });

    redis_conn
        .publish::<_, _, ()>(&channel, payload.to_string())
        .await
        .map_err(|e| AppError::Internal(format!("Redis publish error: {}", e)))?;

    Ok(())
}
