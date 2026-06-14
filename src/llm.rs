use crate::config::LlmConfig;
use crate::error::{Result, TeriError};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde::de::DeserializeOwned;
use std::pin::Pin;

// ============================================================================
// Post-processing helpers (MiroFish parity — llm_client.py:67,94-97)
// ============================================================================

/// Remove every `<think>…</think>` block from LLM output, then trim.
///
/// Matches MiroFish llm_client.py:67:
///   `re.sub(r'<think>[\s\S]*?</think>', '', content).strip()`
///
/// Implementation: manual scan (no `regex` dep) — finds the leftmost
/// `<think>` / `</think>` pair non-greedily and loops until none remain,
/// preserving all content outside the think blocks.
pub fn strip_think(content: &str) -> String {
    let open = "<think>";
    let close = "</think>";
    let mut result = content.to_string();
    while let Some(start) = result.find(open) {
        // Search for the FIRST closing tag after the opening tag.
        if let Some(end_rel) = result[start + open.len()..].find(close) {
            let end = start + open.len() + end_rel + close.len();
            result.replace_range(start..end, "");
        } else {
            // Unclosed <think> — leave intact (mirrors Python's non-greedy
            // match which requires a paired close tag).
            break;
        }
    }
    result.trim().to_string()
}

/// Strip markdown JSON code-fence wrappers before parsing.
///
/// Matches MiroFish llm_client.py:94-97:
///   strip leading ```json\n? or ```\n?, strip trailing \n?```
///   (case-insensitive on the "json" label)
///
/// Handles:
///   ```json\n{…}\n```
///   ```\n{…}\n```
///   {…}   (no fence — returned unchanged)
pub fn strip_json_fence(content: &str) -> String {
    let s = content.trim();
    // Strip leading fence: ```json or ``` (case-insensitive json label)
    let after_open = if let Some(rest) = s.strip_prefix("```") {
        // consume optional "json" (case-insensitive) then optional newline
        let rest = if rest.to_ascii_lowercase().starts_with("json") {
            &rest["json".len()..]
        } else {
            rest
        };
        // consume optional leading newline
        rest.strip_prefix('\n').unwrap_or(rest)
    } else {
        return s.to_string();
    };
    // Strip trailing fence: optional newline then ```
    let after_close = if let Some(before) = after_open.strip_suffix("```") {
        before.strip_suffix('\n').unwrap_or(before)
    } else {
        after_open
    };
    after_close.trim().to_string()
}

// Maximum backoff delay (seconds) — matches MiroFish retry.py:59 `min(delay, max_delay)`
const MAX_BACKOFF_SECS: u64 = 30;

/// Core LLM client trait - completely provider-agnostic.
/// This trait makes NO assumptions about the underlying provider.
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;
    async fn complete_json<T: DeserializeOwned>(&self, prompt: &str) -> Result<T>;
    async fn stream(
        &self,
        prompt: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>>;
}

// ============================================================================
// Provider Adapters
// ============================================================================
// Each adapter implements LlmClient for a specific provider's API.
// Users can choose which adapter to use, or implement their own.

/// Adapter for providers using OpenAI's chat completions API format.
/// Examples: OpenAI, Ollama, LM Studio, vLLM, Together AI, Groq
pub struct OpenAiAdapter {
    base_url: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
    timeout_secs: u64,
    max_retries: u32,
}

impl OpenAiAdapter {
    pub fn new(config: &LlmConfig) -> Self {
        let client = reqwest::Client::new();
        Self {
            base_url: config.base_url.clone(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            client,
            timeout_secs: config.timeout_secs,
            max_retries: config.max_retries,
        }
    }

    async fn call_api(&self, payload: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut retries = 0;

        loop {
            let response = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(self.timeout_secs))
                .json(&payload)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return resp.json().await.map_err(|e| TeriError::Http(e.to_string()));
                    } else if resp.status().is_server_error() && retries < self.max_retries {
                        retries += 1;
                        let delay =
                            (2_u64.pow(retries)).min(MAX_BACKOFF_SECS);
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        continue;
                    } else {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        return Err(TeriError::Http(format!("HTTP {status}: {body}")));
                    }
                }
                Err(e) if retries < self.max_retries && e.is_timeout() => {
                    retries += 1;
                    let delay = (2_u64.pow(retries)).min(MAX_BACKOFF_SECS);
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    continue;
                }
                Err(e) => return Err(TeriError::Http(e.to_string())),
            }
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiAdapter {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.7,
        });

        let response = self.call_api(payload).await?;

        let raw = response
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| TeriError::Llm("Invalid response format".to_string()))?;

        // Strip <think>…</think> blocks (MiroFish llm_client.py:67)
        Ok(strip_think(raw))
    }

    async fn complete_json<T: DeserializeOwned>(&self, prompt: &str) -> Result<T> {
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.0,
            "response_format": {
                "type": "json_object"
            }
        });

        let response = self.call_api(payload).await?;

        let raw = response
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| TeriError::Llm("Invalid response format".to_string()))?;

        // Strip <think> blocks, then strip markdown fences (MiroFish llm_client.py:67,94-97)
        let content = strip_json_fence(&strip_think(raw));

        serde_json::from_str(&content)
            .map_err(|e| TeriError::Llm(format!("Failed to parse JSON response: {e}")))
    }

    async fn stream(
        &self,
        prompt: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.7,
            "stream": true,
        });

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .json(&payload)
            .send()
            .await
            .map_err(|e| TeriError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(TeriError::Http(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }

        let mut byte_stream = resp.bytes_stream();
        let sse_stream = try_stream! {
            let mut buffer = String::new();
            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk.map_err(|e| TeriError::Http(e.to_string()))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(idx) = buffer.find('\n') {
                    let line = buffer[..idx].trim_end_matches('\r').to_string();
                    buffer = buffer[idx + 1..].to_string();

                    if line.is_empty() || !line.starts_with("data: ") {
                        continue;
                    }

                    let data = line.trim_start_matches("data: ").trim();
                    if data == "[DONE]" {
                        return;
                    }

                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(data)
                        && let Some(content) = json
                            .get("choices")
                            .and_then(|c| c.get(0))
                            .and_then(|c| c.get("delta"))
                            .and_then(|d| d.get("content"))
                            .and_then(|c| c.as_str()) {
                                yield content.to_string();
                    }
                }
            }
        };

        Ok(Box::pin(sse_stream))
    }
}

// ============================================================================
// Anthropic Claude Adapter
// ============================================================================

/// Adapter for Anthropic Claude API (non-OpenAI-compatible).
/// Uses Anthropic's Messages API format.
pub struct AnthropicAdapter {
    base_url: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
    timeout_secs: u64,
    max_retries: u32,
}

impl AnthropicAdapter {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            base_url: "https://api.anthropic.com".to_string(),
            api_key,
            model,
            client: reqwest::Client::new(),
            timeout_secs: 30,
            max_retries: 3,
        }
    }

    #[cfg(test)]
    pub fn new_with_base(api_key: String, model: String, base_url: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            client: reqwest::Client::new(),
            timeout_secs: 30,
            max_retries: 0,
        }
    }

    async fn call_api(&self, payload: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/v1/messages", self.base_url);
        let mut retries = 0;

        loop {
            let response = self
                .client
                .post(&url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(self.timeout_secs))
                .json(&payload)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return resp.json().await.map_err(|e| TeriError::Http(e.to_string()));
                    } else if resp.status().is_server_error() && retries < self.max_retries {
                        retries += 1;
                        let delay = (2_u64.pow(retries)).min(MAX_BACKOFF_SECS);
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        continue;
                    } else {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        return Err(TeriError::Http(format!("HTTP {status}: {body}")));
                    }
                }
                Err(e) if retries < self.max_retries && e.is_timeout() => {
                    retries += 1;
                    let delay = (2_u64.pow(retries)).min(MAX_BACKOFF_SECS);
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    continue;
                }
                Err(e) => return Err(TeriError::Http(e.to_string())),
            }
        }
    }
}

#[async_trait]
impl LlmClient for AnthropicAdapter {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": 4096,
        });

        let response = self.call_api(payload).await?;

        let raw = response
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| TeriError::Llm("Invalid response format".to_string()))?;

        // Strip <think>…</think> blocks (MiroFish llm_client.py:67)
        Ok(strip_think(raw))
    }

    async fn complete_json<T: DeserializeOwned>(&self, prompt: &str) -> Result<T> {
        let json_prompt = format!("{prompt}\n\nRespond with valid JSON only.");
        let response = self.complete(&json_prompt).await?;

        // complete() already applied strip_think; apply fence-strip before parse
        // (MiroFish llm_client.py:94-97)
        let content = strip_json_fence(&response);

        serde_json::from_str(&content)
            .map_err(|e| TeriError::Llm(format!("Failed to parse JSON response: {e}")))
    }

    async fn stream(
        &self,
        prompt: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        // Simplified streaming - for now just return complete response as single chunk
        // TODO: Implement proper SSE streaming with Anthropic's streaming API
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": 4096,
            "stream": true,
        });

        let url = format!("{}/v1/messages", self.base_url);

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .json(&payload)
            .send()
            .await
            .map_err(|e| TeriError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(TeriError::Http(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }

        let mut byte_stream = resp.bytes_stream();
        let sse_stream = try_stream! {
            let mut buffer = String::new();
            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk.map_err(|e| TeriError::Http(e.to_string()))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(idx) = buffer.find('\n') {
                    let line = buffer[..idx].trim_end_matches('\r').to_string();
                    buffer = buffer[idx + 1..].to_string();

                    if line.is_empty() || !line.starts_with("data: ") {
                        continue;
                    }

                    let data = line.trim_start_matches("data: ").trim();
                    if data == "[DONE]" {
                        return;
                    }

                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(data)
                        && let Some(content) = json
                            .get("delta")
                            .and_then(|d| d.get("text"))
                            .and_then(|t| t.as_str()) {
                                yield content.to_string();
                    }
                }
            }
        };

        Ok(Box::pin(sse_stream))
    }
}

// ============================================================================
// Google Gemini Adapter
// ============================================================================

/// Adapter for Google Gemini API (non-OpenAI-compatible).
/// Uses Google's generateContent API format.
pub struct GeminiAdapter {
    base_url: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
    timeout_secs: u64,
    max_retries: u32,
}

impl GeminiAdapter {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            api_key,
            model,
            client: reqwest::Client::new(),
            timeout_secs: 30,
            max_retries: 3,
        }
    }

    #[cfg(test)]
    pub fn new_with_base(api_key: String, model: String, base_url: String) -> Self {
        Self {
            base_url,
            api_key,
            model,
            client: reqwest::Client::new(),
            timeout_secs: 30,
            max_retries: 0,
        }
    }

    async fn call_api(&self, payload: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url, self.model, self.api_key
        );
        let mut retries = 0;

        loop {
            let response = self
                .client
                .post(&url)
                .header("Content-Type", "application/json")
                .timeout(std::time::Duration::from_secs(self.timeout_secs))
                .json(&payload)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return resp.json().await.map_err(|e| TeriError::Http(e.to_string()));
                    } else if resp.status().is_server_error() && retries < self.max_retries {
                        retries += 1;
                        let delay = (2_u64.pow(retries)).min(MAX_BACKOFF_SECS);
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        continue;
                    } else {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        return Err(TeriError::Http(format!("HTTP {status}: {body}")));
                    }
                }
                Err(e) if retries < self.max_retries && e.is_timeout() => {
                    retries += 1;
                    let delay = (2_u64.pow(retries)).min(MAX_BACKOFF_SECS);
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    continue;
                }
                Err(e) => return Err(TeriError::Http(e.to_string())),
            }
        }
    }
}

#[async_trait]
impl LlmClient for GeminiAdapter {
    async fn complete(&self, prompt: &str) -> Result<String> {
        let payload = serde_json::json!({
            "contents": [{
                "parts": [{
                    "text": prompt
                }]
            }]
        });

        let response = self.call_api(payload).await?;

        let raw = response
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| TeriError::Llm("Invalid response format".to_string()))?;

        // Strip <think>…</think> blocks (MiroFish llm_client.py:67)
        Ok(strip_think(raw))
    }

    async fn complete_json<T: DeserializeOwned>(&self, prompt: &str) -> Result<T> {
        let json_prompt = format!("{prompt}\n\nRespond with valid JSON only.");
        let response = self.complete(&json_prompt).await?;

        // complete() already applied strip_think; apply fence-strip before parse
        // (MiroFish llm_client.py:94-97)
        let content = strip_json_fence(&response);

        serde_json::from_str(&content)
            .map_err(|e| TeriError::Llm(format!("Failed to parse JSON response: {e}")))
    }

    async fn stream(
        &self,
        prompt: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
        let payload = serde_json::json!({
            "contents": [{
                "parts": [{
                    "text": prompt
                }]
            }]
        });

        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?key={}",
            self.base_url, self.model, self.api_key
        );

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .json(&payload)
            .send()
            .await
            .map_err(|e| TeriError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(TeriError::Http(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }

        let mut byte_stream = resp.bytes_stream();
        let sse_stream = try_stream! {
            let mut buffer = String::new();
            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk.map_err(|e| TeriError::Http(e.to_string()))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(idx) = buffer.find('\n') {
                    let line = buffer[..idx].trim_end_matches('\r').to_string();
                    buffer = buffer[idx + 1..].to_string();

                    if line.is_empty() || !line.starts_with("data: ") {
                        continue;
                    }

                    let data = line.trim_start_matches("data: ").trim();
                    if data == "[DONE]" {
                        return;
                    }

                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(data)
                        && let Some(content) = json
                            .get("candidates")
                            .and_then(|c| c.get(0))
                            .and_then(|c| c.get("content"))
                            .and_then(|c| c.get("parts"))
                            .and_then(|p| p.get(0))
                            .and_then(|p| p.get("text"))
                            .and_then(|t| t.as_str()) {
                                yield content.to_string();
                    }
                }
            }
        };

        Ok(Box::pin(sse_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    // -------------------------------------------------------------------------
    // Helper: build an OpenAiAdapter pointing at a mock server
    // -------------------------------------------------------------------------
    fn openai_config(server: &MockServer, max_retries: u32) -> LlmConfig {
        LlmConfig {
            base_url: server.base_url(),
            api_key: "test-key".to_string(),
            model: "gpt-4o".to_string(),
            embed_model: "text-embedding-3-small".to_string(),
            timeout_secs: 5,
            max_retries,
        }
    }

    // =========================================================================
    // U-008 helpers — strip_think (unit tests, no HTTP required)
    // =========================================================================

    #[test]
    fn test_strip_think_single_block() {
        let input = "<think>internal thoughts</think>The actual answer.";
        assert_eq!(strip_think(input), "The actual answer.");
    }

    #[test]
    fn test_strip_think_multiple_blocks() {
        let input = "<think>step 1</think>Result<think>step 2</think> done";
        assert_eq!(strip_think(input), "Result done");
    }

    #[test]
    fn test_strip_think_no_block_unchanged() {
        let input = "Plain response with no think tags.";
        assert_eq!(strip_think(input), input);
    }

    #[test]
    fn test_strip_think_multiline_block() {
        let input = "<think>\nline 1\nline 2\n</think>Answer";
        assert_eq!(strip_think(input), "Answer");
    }

    #[test]
    fn test_strip_think_trims_whitespace() {
        let input = "<think>x</think>  \n  Hello  \n";
        assert_eq!(strip_think(input), "Hello");
    }

    // =========================================================================
    // U-008 helpers — strip_json_fence (unit tests)
    // =========================================================================

    #[test]
    fn test_strip_json_fence_json_labeled() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_json_fence(input), r#"{"key": "value"}"#);
    }

    #[test]
    fn test_strip_json_fence_bare_backticks() {
        let input = "```\n{\"key\": \"value\"}\n```";
        assert_eq!(strip_json_fence(input), r#"{"key": "value"}"#);
    }

    #[test]
    fn test_strip_json_fence_unfenced_unchanged() {
        let input = r#"{"key": "value"}"#;
        assert_eq!(strip_json_fence(input), input);
    }

    #[test]
    fn test_strip_json_fence_json_label_case_insensitive() {
        let input = "```JSON\n{\"k\":1}\n```";
        assert_eq!(strip_json_fence(input), r#"{"k":1}"#);
    }

    // =========================================================================
    // U-008 — OpenAiAdapter::complete with <think> stripping
    // =========================================================================

    #[test]
    fn test_openai_adapter_creation() {
        let config = LlmConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "test-key".to_string(),
            model: "gpt-4o".to_string(),
            embed_model: "text-embedding-3-small".to_string(),
            timeout_secs: 30,
            max_retries: 3,
        };
        let _client = OpenAiAdapter::new(&config);
    }

    #[tokio::test]
    async fn test_openai_adapter_complete() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"choices":[{"message":{"content":"Hello from mock"}}]}"#,
            );
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp = client.complete("hi").await.unwrap();
        assert_eq!(resp, "Hello from mock");
        mock.assert();
    }

    #[tokio::test]
    async fn test_openai_complete_strips_think() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"choices":[{"message":{"content":"<think>inner</think>Result"}}]}"#,
            );
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp = client.complete("hi").await.unwrap();
        assert_eq!(resp, "Result");
    }

    #[tokio::test]
    async fn test_openai_complete_strips_multiple_think_blocks() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"choices":[{"message":{"content":"<think>a</think>Mid<think>b</think>End"}}]}"#,
            );
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp = client.complete("hi").await.unwrap();
        assert_eq!(resp, "MidEnd");
    }

    // =========================================================================
    // U-008 — OpenAiAdapter::complete_json with fence + think stripping
    // =========================================================================

    #[tokio::test]
    async fn test_openai_complete_json_plain() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"choices":[{"message":{"content":"{\"v\":42}"}}]}"#,
            );
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp: serde_json::Value = client.complete_json("q").await.unwrap();
        assert_eq!(resp["v"], 42);
    }

    #[tokio::test]
    async fn test_openai_complete_json_fenced() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            // Return content with ```json fence
            then.status(200).header("Content-Type", "application/json").body(
                "{\
                    \"choices\":[{\"message\":{\"content\":\"```json\\n{\\\"v\\\":7}\\n```\"}}]\
                }",
            );
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp: serde_json::Value = client.complete_json("q").await.unwrap();
        assert_eq!(resp["v"], 7);
    }

    #[tokio::test]
    async fn test_openai_complete_json_think_and_fence() {
        // A reasoning model emits <think>…</think> then ```json…```
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("Content-Type", "application/json").body(
                "{\
                    \"choices\":[{\"message\":{\"content\":\"<think>reasoning</think>```json\\n{\\\"v\\\":99}\\n```\"}}]\
                }",
            );
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp: serde_json::Value = client.complete_json("q").await.unwrap();
        assert_eq!(resp["v"], 99);
    }

    // =========================================================================
    // U-008 — AnthropicAdapter::complete with think stripping
    // =========================================================================

    #[tokio::test]
    async fn test_anthropic_complete_strips_think() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"content":[{"text":"<think>internal</think>Answer"}]}"#,
            );
        });

        let client = AnthropicAdapter::new_with_base(
            "sk-ant-test".to_string(),
            "claude-3.5-sonnet".to_string(),
            server.base_url(),
        );
        let resp = client.complete("hi").await.unwrap();
        assert_eq!(resp, "Answer");
    }

    #[tokio::test]
    async fn test_anthropic_complete_json_fenced() {
        // complete_json calls complete() (which strips think), then strip_json_fence
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200).header("Content-Type", "application/json").body(
                "{\
                    \"content\":[{\"text\":\"```json\\n{\\\"x\\\":5}\\n```\"}]\
                }",
            );
        });

        let client = AnthropicAdapter::new_with_base(
            "sk-ant-test".to_string(),
            "claude-3.5-sonnet".to_string(),
            server.base_url(),
        );
        let resp: serde_json::Value = client.complete_json("q").await.unwrap();
        assert_eq!(resp["x"], 5);
    }

    // =========================================================================
    // U-008 — GeminiAdapter::complete with think stripping
    // =========================================================================

    #[tokio::test]
    async fn test_gemini_complete_strips_think() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/v1beta/models/gemini-1.5-pro:generateContent");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"candidates":[{"content":{"parts":[{"text":"<think>t</think>Gemini answer"}]}}]}"#,
            );
        });

        let client = GeminiAdapter::new_with_base(
            "AIza-test".to_string(),
            "gemini-1.5-pro".to_string(),
            server.base_url(),
        );
        let resp = client.complete("hi").await.unwrap();
        assert_eq!(resp, "Gemini answer");
    }

    #[tokio::test]
    async fn test_gemini_complete_json_fenced() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/v1beta/models/gemini-1.5-pro:generateContent");
            then.status(200).header("Content-Type", "application/json").body(
                "{\
                    \"candidates\":[{\"content\":{\"parts\":[{\"text\":\"```\\n{\\\"g\\\":3}\\n```\"}]}}]\
                }",
            );
        });

        let client = GeminiAdapter::new_with_base(
            "AIza-test".to_string(),
            "gemini-1.5-pro".to_string(),
            server.base_url(),
        );
        let resp: serde_json::Value = client.complete_json("q").await.unwrap();
        assert_eq!(resp["g"], 3);
    }

    // =========================================================================
    // U-006 — retry tests: OpenAiAdapter (retry logic is identical across all
    //          three adapters; OpenAi has the test-accessible constructor)
    // =========================================================================

    /// 503 always with max_retries=1 → attempts exactly 2 times then returns Err.
    ///
    /// This proves: (a) retries happen on 5xx, (b) the cap is respected,
    /// (c) the final error propagates.
    #[tokio::test]
    async fn test_openai_retry_exhausted_returns_err() {
        let server = MockServer::start();
        let mock_503 = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(503).body("always down");
        });

        let config = LlmConfig {
            base_url: server.base_url(),
            api_key: "k".to_string(),
            model: "m".to_string(),
            embed_model: "e".to_string(),
            timeout_secs: 5,
            max_retries: 1, // 1 retry → 2 total attempts; both 503 → Err
        };
        let client = OpenAiAdapter::new(&config);
        let result = client.complete("q").await;

        assert!(result.is_err(), "should fail after retries exhausted");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("503") || err_msg.contains("HTTP"),
            "error should reference HTTP status, got: {err_msg}"
        );
        // max_retries=1 → initial attempt + 1 retry = 2 total hits
        mock_503.assert_hits(2);
    }

    /// 503 with max_retries=2 → 3 total attempts before giving up.
    #[tokio::test]
    async fn test_openai_retry_hits_cap() {
        let server = MockServer::start();
        let mock_503 = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(503).body("down");
        });

        let config = LlmConfig {
            base_url: server.base_url(),
            api_key: "k".to_string(),
            model: "m".to_string(),
            embed_model: "e".to_string(),
            timeout_secs: 5,
            max_retries: 2, // 2 retries → 3 total attempts
        };
        let client = OpenAiAdapter::new(&config);
        let result = client.complete("q").await;

        assert!(result.is_err());
        mock_503.assert_hits(3);
    }

    /// 200 with max_retries=0 → exactly 1 attempt, succeeds immediately.
    /// This is the success side of the retry gate: no unnecessary retries.
    #[tokio::test]
    async fn test_openai_retry_no_retry_on_success() {
        let server = MockServer::start();
        let mock_200 = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"choices":[{"message":{"content":"immediate-ok"}}]}"#,
            );
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 3));
        let resp = client.complete("q").await.unwrap();

        assert_eq!(resp, "immediate-ok");
        // Despite max_retries=3, should have been called exactly once
        mock_200.assert_hits(1);
    }

    #[tokio::test]
    async fn test_openai_retry_recovers_after_503() {
        // U-006: the RECOVERY path — a 503 then a 200 must succeed-after-retry,
        // not just hit the cap. Technique (per the parity gate): a request-counting
        // static lets the 503 mock match ONLY the first request; the second falls
        // through to the 200 mock.
        use std::sync::atomic::{AtomicUsize, Ordering};
        static ATTEMPT: AtomicUsize = AtomicUsize::new(0);
        ATTEMPT.store(0, Ordering::SeqCst);

        let server = MockServer::start();
        let mock_503 = server.mock(|when, then| {
            when.method(POST)
                .path("/chat/completions")
                .matches(|_req| ATTEMPT.fetch_add(1, Ordering::SeqCst) == 0);
            then.status(503).body("temporarily down");
        });
        let mock_200 = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"choices":[{"message":{"content":"recovered-ok"}}]}"#,
            );
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 3));
        let resp = client.complete("q").await.unwrap();

        assert_eq!(resp, "recovered-ok", "must recover on retry after a 503");
        mock_503.assert_hits(1); // exactly one 503 was served (request #1)
        mock_200.assert_hits(1); // recovered on the retried request
    }

    // =========================================================================
    // Existing streaming tests (unchanged, kept for regression)
    // =========================================================================

    #[tokio::test]
    async fn test_openai_adapter_stream() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("Content-Type", "text/event-stream").body(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\
data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\
data: [DONE]\n",
            );
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let mut stream = client.stream("hi").await.unwrap();
        let mut output = String::new();
        while let Some(chunk) = stream.next().await {
            output.push_str(&chunk.unwrap());
        }
        assert_eq!(output, "Hello world");
        mock.assert();
    }

    #[tokio::test]
    async fn test_anthropic_adapter_stream() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200).header("Content-Type", "text/event-stream").body(
                "data: {\"delta\":{\"text\":\"Hello\"}}\n\
data: {\"delta\":{\"text\":\" Claude\"}}\n\
data: [DONE]\n",
            );
        });

        let client = AnthropicAdapter::new_with_base(
            "sk-ant-test".to_string(),
            "claude-3.5-sonnet".to_string(),
            server.base_url(),
        );

        let mut stream = client.stream("hi").await.unwrap();
        let mut output = String::new();
        while let Some(chunk) = stream.next().await {
            output.push_str(&chunk.unwrap());
        }
        assert_eq!(output, "Hello Claude");
        mock.assert();
    }

    #[tokio::test]
    async fn test_gemini_adapter_stream() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1beta/models/gemini-1.5-pro:streamGenerateContent");
            then.status(200).header("Content-Type", "text/event-stream").body(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hello\"}]}}]}\n\
data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" Gemini\"}]}}]}\n\
data: [DONE]\n",
            );
        });

        let client = GeminiAdapter::new_with_base(
            "AIza-test".to_string(),
            "gemini-1.5-pro".to_string(),
            server.base_url(),
        );

        let mut stream = client.stream("hi").await.unwrap();
        let mut output = String::new();
        while let Some(chunk) = stream.next().await {
            output.push_str(&chunk.unwrap());
        }
        assert_eq!(output, "Hello Gemini");
        mock.assert();
    }
}
