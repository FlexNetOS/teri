use meilisearch_sdk::Client;
use pebesen_core::AppError;
use serde_json::json;
use uuid::Uuid;

/// Index a message in Meilisearch
pub async fn index_message(
    meilisearch_url: &str,
    api_key: Option<&str>,
    message_id: Uuid,
    topic_id: Uuid,
    space_id: Uuid,
    author_id: Uuid,
    content: &str,
) -> Result<(), AppError> {
    let client = Client::new(meilisearch_url.to_string(), api_key.map(|k| k.to_string()));

    let index = client.index("messages");

    let document = json!({
        "id": message_id,
        "topic_id": topic_id,
        "space_id": space_id,
        "author_id": author_id,
        "content": content,
    });

    index
        .add_documents(&[document], Some("id"))
        .await
        .map_err(|e| AppError::Internal(format!("Meilisearch error: {}", e)))?;

    Ok(())
}

/// Remove a message from Meilisearch index
pub async fn remove_message(
    meilisearch_url: &str,
    api_key: Option<&str>,
    message_id: Uuid,
) -> Result<(), AppError> {
    let client = Client::new(meilisearch_url.to_string(), api_key.map(|k| k.to_string()));

    let index = client.index("messages");

    index
        .delete_document(message_id.to_string())
        .await
        .map_err(|e| AppError::Internal(format!("Meilisearch error: {}", e)))?;

    Ok(())
}
