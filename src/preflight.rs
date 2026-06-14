//! Inference-backend preflight: identify the backend and refuse stub/canned
//! engines before any simulation work.
//!
//! Rationale: shimmy's SafeTensors engine returns a fixed placeholder string
//! ("Full transformer inference coming soon!") instead of real completions
//! (shimmy: `src/engine/safetensors_native.rs`). A persona swarm run against
//! such a backend fabricates an entire simulation from canned text with no
//! error anywhere. GGUF-served models (or any real OpenAI-compatible backend)
//! pass; stubs are refused loudly, before any pipeline work starts.

use crate::config::LlmConfig;
use crate::{Result, TeriError};

/// Known canned-output markers emitted by stub engines (matched
/// case-insensitively against the probe completion).
const STUB_MARKERS: &[&str] = &[
    // shimmy SafeTensors placeholder engine
    "full transformer inference coming soon",
];

/// What the preflight learned about the backend.
#[derive(Debug, Clone)]
pub struct BackendIdentity {
    /// Model ids the backend serves (`GET /models`).
    pub models: Vec<String>,
    /// Text returned by the 1-token probe completion.
    pub probe_text: String,
}

/// Returns the matching stub marker when `text` looks like canned stub output.
pub fn detect_stub_text(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    STUB_MARKERS.iter().copied().find(|m| lower.contains(m))
}

/// Extract model ids from an OpenAI-compatible `GET /models` response body.
pub fn parse_model_ids(body: &serde_json::Value) -> Vec<String> {
    body.get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract the first choice's message content from a chat-completions response.
pub fn parse_probe_content(body: &serde_json::Value) -> Option<String> {
    body.get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?
        .as_str()
        .map(str::to_string)
}

/// Pick the model to probe with: the configured one when the backend serves
/// it, otherwise the first served model (with a warning) — a probe against a
/// model the backend doesn't have would fail for the wrong reason.
pub fn choose_probe_model<'a>(configured: &'a str, served: &'a [String]) -> &'a str {
    if served.iter().any(|m| m == configured) {
        configured
    } else {
        served.first().map(String::as_str).unwrap_or(configured)
    }
}

/// Drive the backend at its OpenAI-compatible surface: list models, then run
/// a 1-token completion probe. Refuses unreachable backends, empty model
/// lists, and canned stub output.
pub async fn verify_backend(llm: &LlmConfig) -> Result<BackendIdentity> {
    let base = llm.base_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(llm.timeout_secs))
        .build()
        .map_err(|e| TeriError::Unknown(format!("preflight: building http client: {e}")))?;

    // 1. Identity: which models does the backend actually serve?
    let models_url = format!("{base}/models");
    let resp = client.get(&models_url).bearer_auth(&llm.api_key).send().await.map_err(|e| {
        TeriError::Config(format!(
            "inference backend unreachable at {models_url}: {e}. Start the backend \
                 (e.g. `shimmy serve` with a GGUF model registered) or fix LLM_BASE_URL."
        ))
    })?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(TeriError::Config(format!("backend {models_url} answered {status}: {text}")));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| TeriError::Config(format!("backend {models_url} returned non-JSON: {e}")))?;
    let models = parse_model_ids(&body);
    if models.is_empty() {
        return Err(TeriError::Config(format!(
            "backend at {base} lists no models — load a real GGUF model before \
             running a simulation (stub/canned engines are refused)."
        )));
    }

    let probe_model = choose_probe_model(&llm.model, &models);
    if probe_model != llm.model {
        tracing::warn!(
            "configured LLM_MODEL '{}' is not served by {base}; probing '{}' instead \
             (served: {:?})",
            llm.model,
            probe_model,
            models
        );
    }

    // 2. Honesty: a 1-token probe. Canned stub text here means the backend
    //    would silently fabricate the whole simulation.
    let chat_url = format!("{base}/chat/completions");
    let probe = serde_json::json!({
        "model": probe_model,
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1,
    });
    let resp = client
        .post(&chat_url)
        .bearer_auth(&llm.api_key)
        .json(&probe)
        .send()
        .await
        .map_err(|e| TeriError::Config(format!("probe completion failed at {chat_url}: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(TeriError::Config(format!(
            "probe completion at {chat_url} answered {status}: {text} \
             (model '{probe_model}'; served models: {models:?})"
        )));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| TeriError::Config(format!("probe returned non-JSON: {e}")))?;
    let probe_text = parse_probe_content(&body).unwrap_or_default();

    if let Some(marker) = detect_stub_text(&probe_text) {
        return Err(TeriError::Config(format!(
            "REFUSING stub inference backend at {base}: the probe returned canned \
             text (matched \"{marker}\"). Serve a real GGUF model — shimmy's \
             SafeTensors engine is a placeholder, and a swarm on canned text \
             fabricates predictions."
        )));
    }

    Ok(BackendIdentity { models, probe_text })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_stub_text ───────────────────────────────

    #[test]
    fn detects_shimmy_safetensors_canned_text() {
        // Exact string from shimmy src/engine/safetensors_native.rs:580
        let canned = "SafeTensors model loaded successfully! \
                      Full transformer inference coming soon!";
        assert!(detect_stub_text(canned).is_some());
    }

    #[test]
    fn detection_is_case_insensitive() {
        assert!(detect_stub_text("FULL TRANSFORMER INFERENCE COMING SOON!").is_some());
    }

    #[test]
    fn real_completions_pass() {
        assert!(detect_stub_text("Pong! How can I help?").is_none());
        assert!(detect_stub_text("").is_none());
    }

    // ── parse_model_ids ────────────────────────────────

    #[test]
    fn parses_openai_compatible_model_list() {
        let body = serde_json::json!({
            "object": "list",
            "data": [
                {"id": "llama-3.2-1b-instruct-gguf", "object": "model"},
                {"id": "qwen2.5-coder", "object": "model"},
            ]
        });
        assert_eq!(parse_model_ids(&body), vec!["llama-3.2-1b-instruct-gguf", "qwen2.5-coder"]);
    }

    #[test]
    fn empty_or_malformed_model_list_yields_empty() {
        assert!(parse_model_ids(&serde_json::json!({"data": []})).is_empty());
        assert!(parse_model_ids(&serde_json::json!({})).is_empty());
        assert!(parse_model_ids(&serde_json::json!({"data": "nope"})).is_empty());
    }

    // ── parse_probe_content ────────────────────────────

    #[test]
    fn parses_chat_completion_content() {
        let body = serde_json::json!({
            "choices": [{"message": {"role": "assistant", "content": "hi"}}]
        });
        assert_eq!(parse_probe_content(&body).as_deref(), Some("hi"));
    }

    #[test]
    fn missing_content_yields_none() {
        assert!(parse_probe_content(&serde_json::json!({})).is_none());
        assert!(parse_probe_content(&serde_json::json!({"choices": []})).is_none());
    }

    // ── choose_probe_model ─────────────────────────────

    #[test]
    fn probes_configured_model_when_served() {
        let served = vec!["a".to_string(), "b".to_string()];
        assert_eq!(choose_probe_model("b", &served), "b");
    }

    #[test]
    fn falls_back_to_first_served_model() {
        let served = vec!["a".to_string()];
        assert_eq!(choose_probe_model("gpt-4o", &served), "a");
    }

    #[test]
    fn keeps_configured_when_nothing_served() {
        assert_eq!(choose_probe_model("gpt-4o", &[]), "gpt-4o");
    }
}
