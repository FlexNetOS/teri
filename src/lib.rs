pub mod agent;
pub mod api;
pub mod config;
pub mod embedding;
pub mod error;
pub mod graph;
pub mod i18n;
pub mod llm;
pub mod logging;
pub mod memory;
pub mod models;
pub mod preflight;
pub mod report;
pub mod seed;
pub mod sim;
pub mod task;

pub use config::Config;
pub use error::{Result, TeriError};
pub use llm::{AnthropicAdapter, GeminiAdapter, LlmClient, OpenAiAdapter};
pub use logging::init_logging;

/// Preflight check: probes the configured LLM endpoint and returns true if it appears to be
/// a stub/canned-text backend (e.g., shimmy SafeTensors mode).
///
/// FIX-1.3: Before any simulation run, teri MUST verify the backend is not stubby. Stub backends
/// produce deterministic, cached responses — running simulations on them yields meaningless results
/// indistinguishable from garbage output. The guard refuses and exits with a clear message.
///
/// Detection heuristic: issue a 1-token completion request and check if the response matches known
/// canned-text patterns (e.g., "Full transformer inference", placeholder text, etc.).
pub async fn preflight_check_backend(llm: &config::LlmConfig) -> std::result::Result<bool, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(llm.timeout_secs.min(10)))
        .build()
        .map_err(|e| format!("Failed to create HTTP client for backend probe: {e}"))?;

    // Check if there's a /health endpoint on the configured base_url.
    let health_url = format!("{}/health", llm.base_url.trim_end_matches('/'));
    match client.get(&health_url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let is_stub = detect_stub_response(&body);
            if status.is_success() {
                tracing::info!("Backend probe: health endpoint reachable, stub={is_stub}");
            } else {
                tracing::warn!("Health check returned status {}", status);
            }
            Ok(is_stub)
        }
        Err(_) => {
            // Health probe failed — fall through to sentinel.
            sentinel_completion(&client, llm).await
        }
    }
}

/// Detect canned-text patterns in an HTTP response body.
fn detect_stub_response(body: &str) -> bool {
    let lower = body.to_lowercase();
    // Known stub/canned-text signatures from shimmy and other local inference servers.
    const STUB_PATTERNS: &[&str] = &[
        "stub mode",
        "stubbed",
        "safe tensors",
        "full transformer",
        "coming soon",
        "not implemented",
        "placeholder",
        "no backend",
        "canned text",
    ];

    for pattern in STUB_PATTERNS {
        if lower.contains(pattern) {
            return true;
        }
    }
    false
}

/// Sentinel completion: a minimal token request to detect stub behavior when /health is unavailable.
async fn sentinel_completion(
    client: &reqwest::Client,
    llm: &config::LlmConfig,
) -> std::result::Result<bool, String> {
    // Send a single-token completion with a generic prompt. Stub backends often respond
    // with predictable patterns or very short canned text.
    let body = serde_json::json!({
        "model": &llm.model,
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 1,
        "temperature": 0.0,
    });

    match client
        .post(format!("{}/completions", llm.base_url.trim_end_matches('/')))
        .header("Authorization", format!("Bearer {}", llm.api_key))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            let is_stub = detect_stub_response(&body_text);
            if !is_stub && status.is_success() {
                tracing::info!("Backend probe: sentinel completion responded (non-stub)");
            }
            Ok(is_stub)
        }
        Err(e) => {
            // If we can't reach the endpoint at all, return Ok(false) to allow the caller
            // to decide. The sim will fail naturally if the backend is unreachable.
            tracing::warn!("Sentinel completion probe failed: {e}");
            Ok(false)
        }
    }
}
