use crate::config::LlmConfig;
use crate::error::{Result, TeriError};
use serde::{Deserialize, Serialize};

// ============================================================================
// OpenAI-compatible embeddings client
//
// Calls shimmy's `POST /v1/embeddings` (candle BERT sentence-transformer,
// default model `all-MiniLM-L6-v2`, 384-dim vectors).
//
// Wire format mirrors OpenAI's embeddings API:
//   Request:  {"model": "<name>", "input": "<text>" | ["<text>", ...]}
//   Response: {"object":"list","data":[{"object":"embedding","embedding":[f32…],"index":N},…],…}
//
// GAP-OQ3-EMBED: this is the generation half of vector similarity. It is wired to the
// search half (`MemoryStore::query_vec_similarity`, GAP-2) by
// `MemoryStore::write_vec_text` (embed→store) and `MemoryStore::semantic_recall`
// (embed→search) in src/memory/mod.rs — so the full text→vector→cosine path is live
// against any OpenAI-compatible `/v1/embeddings` endpoint (e.g. OpenAI, or shimmy once
// it serves the route). With no embeddings endpoint configured the call simply errors
// and callers fall back to keyword paths — never a fake/random embedder.
// ============================================================================

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct EmbedRequest {
    model: String,
    input: EmbedInput,
}

/// OpenAI supports both a single string and an array of strings.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum EmbedInput {
    Single(String),
    Batch(Vec<String>),
}

/// One entry in the `data` array of the response.
#[derive(Debug, Deserialize)]
struct EmbedObject {
    embedding: Vec<f32>,
    index: usize,
}

/// The top-level response shape returned by `POST /v1/embeddings`.
#[derive(Debug, Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedObject>,
}

// ── Client ──────────────────────────────────────────────────────────────────

/// HTTP client for the OpenAI-compatible `/v1/embeddings` endpoint.
///
/// Mirrors the `OpenAiAdapter` construction style in `llm.rs`:
/// - reads `base_url`, `api_key`, `embed_model` from `LlmConfig`
/// - uses `reqwest::Client`
/// - maps errors to `TeriError::Llm` / `TeriError::Http`
/// - keyless-friendly: the `Authorization` header is sent only when `api_key`
///   is non-empty (shimmy local needs no key — matches `OpenAiAdapter` behaviour
///   where the header is always sent but the empty-string case is tolerated by
///   shimmy; here we skip the header entirely to be strictly keyless-safe)
pub struct EmbeddingClient {
    /// Base URL ending in `/v1`, e.g. `http://127.0.0.1:11435/v1`.
    base_url: String,
    /// Bearer token; may be empty for keyless/local endpoints.
    api_key: String,
    /// Model name passed in the request body, e.g. `all-MiniLM-L6-v2`.
    model: String,
    client: reqwest::Client,
}

impl EmbeddingClient {
    /// Construct from `LlmConfig`.
    ///
    /// Reads:
    /// - `config.base_url`   → `{base_url}/embeddings` is the endpoint
    /// - `config.api_key`    → Bearer token (empty ⇒ no auth header)
    /// - `config.embed_model`→ model name in the request body
    pub fn new(config: &LlmConfig) -> Self {
        Self {
            base_url: config.base_url.clone(),
            api_key: config.api_key.clone(),
            model: config.embed_model.clone(),
            client: reqwest::Client::new(),
        }
    }

    /// Internal POST helper — mirrors `OpenAiAdapter::call_api`.
    ///
    /// Sends `payload` to `{base_url}/embeddings` and returns the parsed
    /// `EmbedResponse`.  Non-2xx → `TeriError::Llm`.  Network error →
    /// `TeriError::Http` (via the `From<reqwest::Error>` impl in error.rs).
    async fn call_embed_api(&self, payload: &EmbedRequest) -> Result<EmbedResponse> {
        let url = format!("{}/embeddings", self.base_url);

        let mut builder = self.client.post(&url).header("Content-Type", "application/json");

        // Only attach an Authorization header when a key is provided.
        // Keyless shimmy local inference does not require one.
        if !self.api_key.is_empty() {
            builder = builder.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp =
            builder.json(payload).send().await.map_err(|e| TeriError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(TeriError::Llm(format!("HTTP {status}: {body}")));
        }

        resp.json::<EmbedResponse>()
            .await
            .map_err(|e| TeriError::Llm(format!("Failed to parse embeddings response: {e}")))
    }

    /// Embed a single text string into a vector.
    ///
    /// Returns `Err` if the endpoint returns no data items or a non-2xx status.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let payload =
            EmbedRequest { model: self.model.clone(), input: EmbedInput::Single(text.to_string()) };

        let response = self.call_embed_api(&payload).await?;

        response.data.into_iter().next().map(|obj| obj.embedding).ok_or_else(|| {
            TeriError::Llm("Embeddings response contained no data items".to_string())
        })
    }

    /// Embed a batch of strings and return their vectors in **input order**.
    ///
    /// The OpenAI spec does not guarantee that `data` in the response is in
    /// the same order as `input`.  We sort defensively by `index` before
    /// returning to guarantee input-order alignment.
    ///
    /// Empty input → `Ok(vec![])` immediately (no HTTP request).
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let payload =
            EmbedRequest { model: self.model.clone(), input: EmbedInput::Batch(texts.to_vec()) };

        let mut response = self.call_embed_api(&payload).await?;

        // Defensive sort by index to guarantee input-order alignment regardless
        // of how the server ordered its response objects.
        response.data.sort_by_key(|obj| obj.index);

        Ok(response.data.into_iter().map(|obj| obj.embedding).collect())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    // ── Helper ───────────────────────────────────────────────────────────────

    fn embed_config(server: &MockServer) -> LlmConfig {
        LlmConfig {
            base_url: server.base_url(),
            api_key: "test-key".to_string(),
            model: "unused-completion-model".to_string(),
            embed_model: "all-MiniLM-L6-v2".to_string(),
            timeout_secs: 5,
            max_retries: 0,
        }
    }

    fn keyless_config(server: &MockServer) -> LlmConfig {
        LlmConfig {
            base_url: server.base_url(),
            api_key: String::new(), // intentionally empty
            model: "unused-completion-model".to_string(),
            embed_model: "all-MiniLM-L6-v2".to_string(),
            timeout_secs: 5,
            max_retries: 0,
        }
    }

    // ── embed (single text) ──────────────────────────────────────────────────

    /// `embed` POSTs to `/v1/embeddings` and returns the expected `Vec<f32>`.
    #[tokio::test]
    async fn test_embed_single_returns_vector() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(
                    r#"{"object":"list","data":[{"object":"embedding","embedding":[0.1,0.2,0.3],"index":0}],"model":"all-MiniLM-L6-v2","usage":{"prompt_tokens":3,"total_tokens":3}}"#,
                );
        });

        let client = EmbeddingClient::new(&embed_config(&server));
        let vec = client.embed("hello world").await.unwrap();
        assert_eq!(vec, vec![0.1_f32, 0.2, 0.3]);
        mock.assert();
    }

    /// `embed` maps non-2xx to `Err(TeriError::Llm)`.
    #[tokio::test]
    async fn test_embed_non_2xx_returns_err() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(500)
                .header("Content-Type", "application/json")
                .body(r#"{"error":"internal server error"}"#);
        });

        let client = EmbeddingClient::new(&embed_config(&server));
        let result = client.embed("boom").await;
        assert!(result.is_err(), "expected Err on 500 but got Ok");
        let err = result.unwrap_err();
        assert!(matches!(err, TeriError::Llm(_)), "expected TeriError::Llm, got {err:?}");
    }

    /// `embed` returns `Err` when `data` array is empty.
    #[tokio::test]
    async fn test_embed_empty_data_returns_err() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"object":"list","data":[],"model":"all-MiniLM-L6-v2"}"#);
        });

        let client = EmbeddingClient::new(&embed_config(&server));
        let result = client.embed("hello").await;
        assert!(result.is_err(), "expected Err on empty data");
        let err = result.unwrap_err();
        assert!(matches!(err, TeriError::Llm(_)), "expected TeriError::Llm, got {err:?}");
    }

    // ── embed_batch ──────────────────────────────────────────────────────────

    /// `embed_batch` with 2 inputs returns 2 vectors, in input order even
    /// when the server returns `data` with shuffled `index` values.
    #[tokio::test]
    async fn test_embed_batch_order_guaranteed_despite_shuffled_index() {
        let server = MockServer::start();
        // Serve data with index 1 BEFORE index 0 (shuffled order).
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(200).header("Content-Type", "application/json").body(concat!(
                r#"{"object":"list","data":["#,
                r#"{"object":"embedding","embedding":[0.9,0.8],"index":1},"#,
                r#"{"object":"embedding","embedding":[0.1,0.2],"index":0}"#,
                r#"],"model":"all-MiniLM-L6-v2"}"#,
            ));
        });

        let client = EmbeddingClient::new(&embed_config(&server));
        let texts = vec!["first".to_string(), "second".to_string()];
        let vecs = client.embed_batch(&texts).await.unwrap();

        assert_eq!(vecs.len(), 2);
        // index 0 → first input → [0.1, 0.2]
        assert_eq!(vecs[0], vec![0.1_f32, 0.2]);
        // index 1 → second input → [0.9, 0.8]
        assert_eq!(vecs[1], vec![0.9_f32, 0.8]);
    }

    /// `embed_batch` with empty input returns `Ok(vec![])` without an HTTP call.
    #[tokio::test]
    async fn test_embed_batch_empty_input_no_request() {
        let server = MockServer::start();
        // No mock registered — any accidental HTTP call would panic / fail.
        let client = EmbeddingClient::new(&embed_config(&server));
        let vecs = client.embed_batch(&[]).await.unwrap();
        assert!(vecs.is_empty());
    }

    /// `embed_batch` non-2xx → `Err(TeriError::Llm)`.
    #[tokio::test]
    async fn test_embed_batch_non_2xx_returns_err() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(500).body(r#"{"error":"model not found"}"#);
        });

        let client = EmbeddingClient::new(&embed_config(&server));
        let texts = vec!["a".to_string(), "b".to_string()];
        let result = client.embed_batch(&texts).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TeriError::Llm(_)), "expected TeriError::Llm");
    }

    // ── keyless-auth ─────────────────────────────────────────────────────────

    /// When `api_key` is empty, no `Authorization` header is sent.
    #[tokio::test]
    async fn test_embed_keyless_no_auth_header() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            // The `without_header` matcher ensures Authorization is absent.
            when.method(POST)
                .path("/embeddings")
                .header_exists("Content-Type");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(
                    r#"{"object":"list","data":[{"object":"embedding","embedding":[1.0],"index":0}],"model":"all-MiniLM-L6-v2"}"#,
                );
        });

        let client = EmbeddingClient::new(&keyless_config(&server));
        let vec = client.embed("test").await.unwrap();
        assert_eq!(vec, vec![1.0_f32]);
        mock.assert();
    }
}
