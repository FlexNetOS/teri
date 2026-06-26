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
///
/// These match only stub engines that ignore `max_tokens` and return a fixed
/// placeholder string — a real backend honoring the 1-token probe returns a
/// single token that cannot contain a multi-word marker, so a false positive is
/// structurally impossible here. The list is the union of every stub phrase teri
/// has seen (formerly split across two guards); fold new stub engines in rather
/// than weakening it.
const STUB_MARKERS: &[&str] = &[
    // shimmy SafeTensors placeholder engine (src/engine/safetensors_native.rs)
    "full transformer inference coming soon",
    "full transformer",
    "coming soon",
    // NOTE: shimmy spells it "SafeTensors" (no space); the lowercased marker must
    // be "safetensors" — the old "safe tensors" with a space never matched.
    "safetensors",
    // generic stub / placeholder engine signatures
    "stub mode",
    "stubbed",
    "not implemented",
    "placeholder",
    "no backend",
    "canned text",
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

/// Pull `choices[0].delta.content` out of a single streaming chunk object, if present.
/// Streaming chunks carry the text under `delta` (not `message`); the role-only first
/// chunk has `content: null`, which `as_str()` skips.
fn chunk_delta_content(chunk: &serde_json::Value) -> Option<&str> {
    chunk
        .get("choices")?
        .as_array()?
        .first()?
        .get("delta")?
        .get("content")?
        .as_str()
}

/// Extract the probe completion text from a raw chat-completions response body,
/// tolerating BOTH framings teri's backends use:
///
/// - **Single JSON object** (`choices[0].message.content`) — OpenAI/ollama/vLLM
///   when `stream` is unset, the classic non-streaming response.
/// - **SSE stream** (`data: {…delta.content…}` lines ending in `data: [DONE]`) —
///   shimmy's OpenAI-compatible endpoint ALWAYS streams, even when `stream` is
///   not requested, so the probe receives `text/event-stream` rather than a JSON
///   object. Without this branch `resp.json()` fails with "non-JSON" and the
///   guard wrongly rejects a perfectly real shimmy backend.
///
/// Mirrors the SSE parsing of the main streaming client (`llm.rs`): split on
/// lines, `strip_prefix("data:")` (the space is optional per the SSE spec),
/// skip blanks and the `[DONE]` sentinel, concatenate every `delta.content`.
/// Returns the empty string when nothing parses (the caller treats empty as
/// "no usable completion", which is itself a refusal upstream).
pub fn extract_probe_text(raw: &str) -> String {
    if raw.trim_start().starts_with("data:") {
        let mut out = String::new();
        for line in raw.lines() {
            let Some(data) = line.strip_prefix("data:").map(str::trim) else { continue };
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let Ok(chunk) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            if let Some(text) = chunk_delta_content(&chunk) {
                out.push_str(text);
            }
        }
        return out;
    }

    // Non-streaming JSON object: prefer message.content, fall back to a lone
    // delta.content (some backends emit a single streaming-shaped object).
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(body) => parse_probe_content(&body)
            .or_else(|| chunk_delta_content(&body).map(str::to_string))
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
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
    // shimmy's OpenAI-compatible endpoint ALWAYS streams (SSE) even when `stream`
    // is unset, so we read the body as text and parse both framings (SSE deltas or
    // a single JSON object) rather than assuming `resp.json()` — which would reject
    // a real shimmy backend with "non-JSON".
    let raw = resp
        .text()
        .await
        .map_err(|e| TeriError::Config(format!("probe response unreadable at {chat_url}: {e}")))?;
    let probe_text = extract_probe_text(&raw);

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
    use httpmock::prelude::*;

    fn probe_config(server: &MockServer) -> LlmConfig {
        LlmConfig {
            base_url: server.base_url(),
            api_key: "test-key".to_string(),
            model: "m".to_string(),
            embed_model: "all-MiniLM-L6-v2".to_string(),
            timeout_secs: 5,
            max_retries: 0,
            max_tokens: 2048,
            provider: crate::config::LlmProvider::Openai,
        }
    }

    // ── verify_backend (end-to-end over a mock OpenAI surface) ──

    /// A backend that serves the configured model and answers the probe with
    /// real text passes, returning its served model list.
    #[tokio::test]
    async fn verify_backend_passes_real_backend() {
        let server = MockServer::start();
        let models = server.mock(|when, then| {
            when.method(GET).path("/models");
            then.status(200)
                .body(r#"{"object":"list","data":[{"id":"m","object":"model"}]}"#);
        });
        let chat = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200)
                .body(r#"{"choices":[{"message":{"role":"assistant","content":"Pong!"}}]}"#);
        });

        let identity = verify_backend(&probe_config(&server)).await.expect("real backend passes");
        assert_eq!(identity.models, vec!["m".to_string()]);
        assert_eq!(identity.probe_text, "Pong!");
        models.assert();
        chat.assert();
    }

    /// A backend listing no models is refused before any probe.
    #[tokio::test]
    async fn verify_backend_refuses_empty_model_list() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/models");
            then.status(200).body(r#"{"object":"list","data":[]}"#);
        });

        let err = verify_backend(&probe_config(&server)).await.unwrap_err();
        assert!(matches!(err, TeriError::Config(_)));
        assert!(err.to_string().contains("lists no models"), "got: {err}");
    }

    /// A backend whose probe returns canned stub text is refused.
    #[tokio::test]
    async fn verify_backend_refuses_canned_probe() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/models");
            then.status(200)
                .body(r#"{"object":"list","data":[{"id":"m","object":"model"}]}"#);
        });
        server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).body(
                r#"{"choices":[{"message":{"role":"assistant","content":"SafeTensors model loaded successfully! Full transformer inference coming soon!"}}]}"#,
            );
        });

        let err = verify_backend(&probe_config(&server)).await.unwrap_err();
        assert!(matches!(err, TeriError::Config(_)));
        assert!(err.to_string().contains("REFUSING stub"), "got: {err}");
    }

    /// A non-2xx from `/models` is refused (not silently treated as healthy).
    #[tokio::test]
    async fn verify_backend_refuses_non_success_models() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/models");
            then.status(503).body("service unavailable");
        });

        let err = verify_backend(&probe_config(&server)).await.unwrap_err();
        assert!(matches!(err, TeriError::Config(_)));
    }

    /// An unreachable backend is refused — the guard catches absence, not just
    /// canned text. (This is the regression the weak `/health` guard allowed.)
    #[tokio::test]
    async fn verify_backend_refuses_unreachable() {
        // Port 1 is privileged and never listening in CI → connection refused.
        let cfg = LlmConfig {
            base_url: "http://127.0.0.1:1/v1".to_string(),
            api_key: String::new(),
            model: "m".to_string(),
            embed_model: "all-MiniLM-L6-v2".to_string(),
            timeout_secs: 2,
            max_retries: 0,
            max_tokens: 2048,
            provider: crate::config::LlmProvider::Openai,
        };
        let err = verify_backend(&cfg).await.unwrap_err();
        assert!(matches!(err, TeriError::Config(_)));
        assert!(err.to_string().contains("unreachable"), "got: {err}");
    }

    /// shimmy's OpenAI endpoint ALWAYS streams SSE (even with `stream` unset),
    /// so the probe receives `data: {…delta…}` chunks, not a JSON object. The
    /// guard must parse them and pass a real backend — the whole point of using
    /// shimmy instead of ollama.
    #[tokio::test]
    async fn verify_backend_passes_sse_streaming_backend() {
        let server = MockServer::start();
        let models = server.mock(|when, then| {
            when.method(GET).path("/models");
            then.status(200)
                .body(r#"{"object":"list","data":[{"id":"m","object":"model"}]}"#);
        });
        // First chunk is role-only (content:null), then the content token, then [DONE].
        let chat = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("content-type", "text/event-stream").body(
                "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":null}}]}\n\n\
                 data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"pong\"}}]}\n\n\
                 data: [DONE]\n\n",
            );
        });

        let identity = verify_backend(&probe_config(&server)).await.expect("sse backend passes");
        assert_eq!(identity.models, vec!["m".to_string()]);
        assert_eq!(identity.probe_text, "pong");
        models.assert();
        chat.assert();
    }

    /// A streaming backend whose concatenated deltas spell canned stub text is
    /// still refused — the SSE path must not become a hole in the honesty guard.
    #[tokio::test]
    async fn verify_backend_refuses_canned_sse_probe() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/models");
            then.status(200)
                .body(r#"{"object":"list","data":[{"id":"m","object":"model"}]}"#);
        });
        server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("content-type", "text/event-stream").body(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Full transformer \"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\"inference coming soon!\"}}]}\n\n\
                 data: [DONE]\n\n",
            );
        });

        let err = verify_backend(&probe_config(&server)).await.unwrap_err();
        assert!(matches!(err, TeriError::Config(_)));
        assert!(err.to_string().contains("REFUSING stub"), "got: {err}");
    }

    // ── extract_probe_text (both framings) ──────────────

    #[test]
    fn extract_probe_text_parses_sse_deltas() {
        let raw = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":null}}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
                   data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n\
                   data: [DONE]\n\n";
        assert_eq!(extract_probe_text(raw), "Hello world");
    }

    #[test]
    fn extract_probe_text_parses_data_with_no_space() {
        // The SSE space after `data:` is optional; shimmy/compliant servers may omit it.
        let raw = "data:{\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n";
        assert_eq!(extract_probe_text(raw), "hi");
    }

    #[test]
    fn extract_probe_text_parses_json_object() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"Pong!"}}]}"#;
        assert_eq!(extract_probe_text(raw), "Pong!");
    }

    #[test]
    fn extract_probe_text_empty_on_garbage() {
        assert_eq!(extract_probe_text("not json, not sse"), "");
        assert_eq!(extract_probe_text(""), "");
    }

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

    #[test]
    fn detects_safetensors_no_space() {
        // shimmy writes "SafeTensors" (no space); the lowercased marker is
        // "safetensors". The old "safe tensors" with a space would have missed this.
        assert!(detect_stub_text("SafeTensors model loaded").is_some());
    }

    #[test]
    fn detects_generic_stub_signatures() {
        for canned in [
            "stub mode active",
            "this endpoint is stubbed",
            "not implemented yet",
            "placeholder response",
            "no backend configured",
            "canned text reply",
        ] {
            assert!(detect_stub_text(canned).is_some(), "should flag stub: {canned:?}");
        }
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
