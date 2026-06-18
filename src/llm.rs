use crate::config::LlmConfig;
use crate::error::{Result, TeriError};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde::Serialize;
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

// ============================================================================
// Batch retry helper (MiroFish parity — retry.py:195 `call_batch_with_retry`)
// ============================================================================

/// A single batch-item failure: the index in the original ops slice, and the
/// error returned after all per-item retries were exhausted.
///
/// Matches MiroFish retry.py:228 `{"index": idx, "error": str(e)}`.
/// The Python version also stores `"item"` (the input), but closures in Rust
/// are consumed on call — the index is sufficient for callers to recover the
/// input from their own slice, which preserves all information.
#[derive(Debug)]
pub struct BatchFailure {
    /// Zero-based index of the failed operation in the input slice.
    pub index: usize,
    /// The error returned after all retries for this item were exhausted.
    pub error: TeriError,
}

/// Outcome of [`call_batch_with_retry`].
///
/// Mirrors MiroFish `(results, failures)` return tuple (retry.py:195):
/// - `results`: one `Ok(T)` per item that eventually succeeded.
/// - `failures`: one `BatchFailure` per item that was exhausted after all retries.
///
/// When `continue_on_failure` is `true` (the default), `results` + `failures`
/// together account for every input operation.  When `false`, the function
/// aborts at the first permanent failure and returns `Err(TeriError)` instead
/// of a `BatchResult`.
#[derive(Debug)]
pub struct BatchResult<T> {
    /// Successful results, in input order.
    pub results: Vec<T>,
    /// Per-item failures for items that exhausted all retries.
    pub failures: Vec<BatchFailure>,
}

/// Run `ops` as a batch, retrying each failing operation individually with
/// exponential back-off.
///
/// # Contract (MiroFish retry.py:195 `call_batch_with_retry`)
///
/// For each operation `ops[i]`:
/// 1. Attempt the operation; on `Err` retry with backoff up to `max_retries`
///    times (identical to the per-adapter `call_api` retry loop).
/// 2. If it succeeds, push the result to `BatchResult::results`.
/// 3. If it exhausts all retries:
///    - When `continue_on_failure == true` (default in MiroFish): record a
///      `BatchFailure { index, error }` in `BatchResult::failures` and
///      continue to the next operation.
///    - When `continue_on_failure == false`: abort immediately and return
///      `Err(error)` (mirrors `raise` at retry.py:234).
///
/// # Closure contract
/// Each element of `ops` is a `Fn() -> Fut` factory so the helper can
/// re-invoke it on every retry attempt.  Pass `|| async { your_call() }`.
///
/// # Back-off
/// Delay = `2^attempt` seconds, clamped to `MAX_BACKOFF_SECS` (30 s).
/// No jitter — intentional divergence (`[≠]`) matching the rest of teri's
/// retry contract.
///
/// # Returns
/// `Ok(BatchResult<T>)` — always, even when some items failed, as long as
/// `continue_on_failure` is `true`.
/// `Err(TeriError)` — only when `continue_on_failure` is `false` AND an
/// operation exhausts its retries.
pub async fn call_batch_with_retry<T, F, Fut>(
    ops: Vec<F>,
    max_retries: u32,
    continue_on_failure: bool,
) -> Result<BatchResult<T>>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut results: Vec<T> = Vec::new();
    let mut failures: Vec<BatchFailure> = Vec::new();

    for (idx, op) in ops.iter().enumerate() {
        // Per-item retry loop — mirrors RetryableAPIClient.call_with_retry
        // (retry.py:149): attempt max_retries+1 times, exponential backoff
        // between attempts, raise on final exhaustion.
        let mut retries: u32 = 0;
        let item_result = loop {
            match op().await {
                Ok(val) => break Ok(val),
                Err(e) => {
                    if retries >= max_retries {
                        // All retries exhausted for this item (retry.py:178)
                        break Err(e);
                    }
                    retries += 1;
                    let delay = (2_u64.pow(retries)).min(MAX_BACKOFF_SECS);
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    // Loop continues: op() is called again on next iteration
                }
            }
        };

        match item_result {
            Ok(val) => results.push(val),
            Err(e) => {
                if continue_on_failure {
                    // Record failure but continue processing remaining items
                    // (retry.py:227-232: append to failures, do NOT raise)
                    failures.push(BatchFailure { index: idx, error: e });
                } else {
                    // Abort entire batch — retry.py:234 `raise`
                    return Err(e);
                }
            }
        }
    }

    Ok(BatchResult { results, failures })
}

// ============================================================================
// DECISION-7 — parameterized chat types (additive; U-008 bodies are UNCHANGED)
// ============================================================================

/// The role of a [`ChatMessage`].
///
/// Serializes to the lowercase wire string each provider expects
/// (`"system"`, `"user"`, `"assistant"`).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

/// One chat message.  Role is a closed enum so a typo can't silently produce
/// an unknown role.
///
/// # DECISION-7 — MiroFish parity
/// Mirrors the `{"role": …, "content": …}` dicts in MiroFish
/// `llm_client.py:35-102`.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    /// Construct a system-role message.
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: ChatRole::System, content: content.into() }
    }

    /// Construct a user-role message.
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: ChatRole::User, content: content.into() }
    }
}

/// Optional per-call tuning knobs.
///
/// `None` on either field means "let the adapter apply its own default" —
/// callers that don't care about temperature or max_tokens use
/// `ChatOptions::default()` and nothing changes from today's behaviour.
///
/// # DECISION-7 — MiroFish parity
/// Maps directly to the `temperature` / `max_tokens` kwargs of
/// `LLMClient.chat()` / `LLMClient.chat_json()` in `llm_client.py`.
#[derive(Debug, Clone, Default)]
pub struct ChatOptions {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

/// Core LLM client trait - completely provider-agnostic.
/// This trait makes NO assumptions about the underlying provider.
#[async_trait]
pub trait LlmClient: Send + Sync {
    // --- UNCHANGED (U-008 verified) ---
    async fn complete(&self, prompt: &str) -> Result<String>;
    async fn complete_json<T: DeserializeOwned>(&self, prompt: &str) -> Result<T>;
    async fn stream(
        &self,
        prompt: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>>;

    // --- NEW (DECISION-7): parameterized chat — system+user vector, optional temp/max_tokens ---
    //
    // NOT a default impl: a no-op/single-prompt default would silently drop the system role and
    // temperature/max_tokens — an observable downgrade.  Each adapter MUST implement these properly.

    /// Send a multi-role message vector to the LLM and return the text response.
    ///
    /// `messages` may contain system, user, and/or assistant turns.
    /// `opts` controls temperature and max_tokens (both optional).
    ///
    /// Post-processing: `strip_think` is applied to the raw response (matches
    /// MiroFish `llm_client.py:67`).
    async fn chat(&self, messages: &[ChatMessage], opts: &ChatOptions) -> Result<String>;

    /// Send a multi-role message vector and parse the response as JSON.
    ///
    /// Post-processing: `strip_json_fence(&strip_think(raw))` then
    /// `serde_json::from_str` (matches MiroFish `llm_client.py:94-100`).
    async fn chat_json<T: DeserializeOwned>(
        &self,
        messages: &[ChatMessage],
        opts: &ChatOptions,
    ) -> Result<T>;
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

    // --- DECISION-7: parameterized chat (additive; complete/complete_json/stream UNCHANGED) ---

    async fn chat(&self, messages: &[ChatMessage], opts: &ChatOptions) -> Result<String> {
        // Serialize messages verbatim — roles are lowercased by serde (ChatRole's rename_all).
        let mut payload = serde_json::json!({
            "model": self.model,
            "messages": messages,
        });

        // Add temperature only when explicitly set (DECISION-7: omit key → server default).
        if let Some(temp) = opts.temperature {
            payload["temperature"] = serde_json::json!(temp);
        }
        // Add max_tokens only when explicitly set.
        if let Some(max) = opts.max_tokens {
            payload["max_tokens"] = serde_json::json!(max);
        }

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

    async fn chat_json<T: DeserializeOwned>(
        &self,
        messages: &[ChatMessage],
        opts: &ChatOptions,
    ) -> Result<T> {
        let mut payload = serde_json::json!({
            "model": self.model,
            "messages": messages,
            // JSON mode — matches existing complete_json and MiroFish chat_json
            "response_format": { "type": "json_object" },
        });

        if let Some(temp) = opts.temperature {
            payload["temperature"] = serde_json::json!(temp);
        }
        if let Some(max) = opts.max_tokens {
            payload["max_tokens"] = serde_json::json!(max);
        }

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

    // --- DECISION-7: parameterized chat for Anthropic (additive; existing methods UNCHANGED) ---

    async fn chat(&self, messages: &[ChatMessage], opts: &ChatOptions) -> Result<String> {
        // Anthropic API: system messages are a TOP-LEVEL "system" string, NOT a role in messages[].
        // Partition: join all System contents into the top-level system param;
        // User/Assistant messages go in "messages".
        let system_text: String = messages
            .iter()
            .filter(|m| matches!(m.role, ChatRole::System))
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let user_msgs: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| !matches!(m.role, ChatRole::System))
            .map(|m| {
                let role = match m.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::System => unreachable!("system filtered above"),
                };
                serde_json::json!({ "role": role, "content": m.content })
            })
            .collect();

        // max_tokens is REQUIRED by the Anthropic API; default to 4096 when not specified
        // (matches the hardcoded value at llm.rs:505,547 and MiroFish's default).
        let max_tokens = opts.max_tokens.unwrap_or(4096);

        let mut payload = serde_json::json!({
            "model": self.model,
            "messages": user_msgs,
            "max_tokens": max_tokens,
        });

        if !system_text.is_empty() {
            payload["system"] = serde_json::json!(system_text);
        }

        if let Some(temp) = opts.temperature {
            payload["temperature"] = serde_json::json!(temp);
        }

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

    async fn chat_json<T: DeserializeOwned>(
        &self,
        messages: &[ChatMessage],
        opts: &ChatOptions,
    ) -> Result<T> {
        // Anthropic has no native JSON mode; append the JSON sentinel (matches complete_json).
        // We add it to the last user message or as an extra user message.
        let mut extended: Vec<ChatMessage> = messages.to_vec();
        // Append to user side: tack onto a fresh user turn (matches existing complete_json approach).
        extended.push(ChatMessage::user("\n\nRespond with valid JSON only."));

        let response = self.chat(&extended, opts).await?;

        // chat() already applied strip_think; apply fence-strip before parse
        let content = strip_json_fence(&response);

        serde_json::from_str(&content)
            .map_err(|e| TeriError::Llm(format!("Failed to parse JSON response: {e}")))
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

    // --- DECISION-7: parameterized chat for Gemini (additive; existing methods UNCHANGED) ---

    async fn chat(&self, messages: &[ChatMessage], opts: &ChatOptions) -> Result<String> {
        // Gemini API:
        //   - system messages → top-level "systemInstruction": {"parts":[{"text": …}]}
        //   - user/assistant messages → "contents": [{"role": "user"/"model", "parts":[{"text": …}]}]
        //   - role mapping: User→"user", Assistant→"model"
        let system_text: String = messages
            .iter()
            .filter(|m| matches!(m.role, ChatRole::System))
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let contents: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| !matches!(m.role, ChatRole::System))
            .map(|m| {
                let role = match m.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "model",
                    ChatRole::System => unreachable!("system filtered above"),
                };
                serde_json::json!({
                    "role": role,
                    "parts": [{ "text": m.content }]
                })
            })
            .collect();

        let mut payload = serde_json::json!({
            "contents": contents,
        });

        if !system_text.is_empty() {
            payload["systemInstruction"] = serde_json::json!({
                "parts": [{ "text": system_text }]
            });
        }

        // generationConfig: temperature and/or maxOutputTokens when set
        let mut gen_config = serde_json::Map::new();
        if let Some(temp) = opts.temperature {
            gen_config.insert("temperature".to_string(), serde_json::json!(temp));
        }
        if let Some(max) = opts.max_tokens {
            gen_config.insert("maxOutputTokens".to_string(), serde_json::json!(max));
        }
        if !gen_config.is_empty() {
            payload["generationConfig"] = serde_json::Value::Object(gen_config);
        }

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

    async fn chat_json<T: DeserializeOwned>(
        &self,
        messages: &[ChatMessage],
        opts: &ChatOptions,
    ) -> Result<T> {
        // Gemini JSON mode: set responseMimeType in generationConfig.
        // Build payload directly (can't delegate to chat() because we need to inject into
        // generationConfig before the call_api call).
        let system_text: String = messages
            .iter()
            .filter(|m| matches!(m.role, ChatRole::System))
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let contents: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| !matches!(m.role, ChatRole::System))
            .map(|m| {
                let role = match m.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "model",
                    ChatRole::System => unreachable!("system filtered above"),
                };
                serde_json::json!({
                    "role": role,
                    "parts": [{ "text": m.content }]
                })
            })
            .collect();

        let mut payload = serde_json::json!({
            "contents": contents,
        });

        if !system_text.is_empty() {
            payload["systemInstruction"] = serde_json::json!({
                "parts": [{ "text": system_text }]
            });
        }

        // generationConfig with responseMimeType for JSON mode
        let mut gen_config = serde_json::Map::new();
        if let Some(temp) = opts.temperature {
            gen_config.insert("temperature".to_string(), serde_json::json!(temp));
        }
        if let Some(max) = opts.max_tokens {
            gen_config.insert("maxOutputTokens".to_string(), serde_json::json!(max));
        }
        // Belt-and-suspenders JSON sentinel (also set responseMimeType)
        gen_config.insert("responseMimeType".to_string(), serde_json::json!("application/json"));
        payload["generationConfig"] = serde_json::Value::Object(gen_config);

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

        // Strip <think> blocks, then strip markdown fences, then parse
        let content = strip_json_fence(&strip_think(raw));

        serde_json::from_str(&content)
            .map_err(|e| TeriError::Llm(format!("Failed to parse JSON response: {e}")))
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
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"choices":[{"message":{"content":"Hello from mock"}}]}"#);
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
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"choices":[{"message":{"content":"<think>inner</think>Result"}}]}"#);
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
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"choices":[{"message":{"content":"{\"v\":42}"}}]}"#);
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
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"content":[{"text":"<think>internal</think>Answer"}]}"#);
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
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"choices":[{"message":{"content":"immediate-ok"}}]}"#);
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
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"choices":[{"message":{"content":"recovered-ok"}}]}"#);
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

    // =========================================================================
    // S-048 — call_batch_with_retry (MiroFish retry.py:195)
    // =========================================================================

    // Type alias to avoid repetitive complex trait-object types in test bodies.
    type BoxOp = Box<
        dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send>>
            + Send
            + Sync,
    >;

    fn ok_op(v: u32) -> BoxOp {
        Box::new(move || Box::pin(async move { Ok(v) }))
    }

    fn err_op(msg: &'static str) -> BoxOp {
        Box::new(move || Box::pin(async move { Err(TeriError::Llm(msg.to_string())) }))
    }

    /// All ops succeed → results contains every value in input order; no failures.
    #[tokio::test]
    async fn test_batch_all_succeed() {
        let ops: Vec<BoxOp> = vec![ok_op(1), ok_op(2), ok_op(3)];
        let batch = call_batch_with_retry(ops, 0, true).await.unwrap();
        assert_eq!(batch.results, vec![1, 2, 3]);
        assert!(batch.failures.is_empty());
    }

    /// Empty ops slice → empty BatchResult; no panic, no error.
    #[tokio::test]
    async fn test_batch_empty() {
        let ops: Vec<BoxOp> = vec![];
        let batch = call_batch_with_retry(ops, 3, true).await.unwrap();
        assert!(batch.results.is_empty());
        assert!(batch.failures.is_empty());
    }

    /// One op always fails; continue_on_failure=true → other ops succeed,
    /// failure is recorded with the correct index.
    /// Matches retry.py:226-232: failures.append({index, item, error}).
    #[tokio::test]
    async fn test_batch_one_fails_continue_true() {
        let ops: Vec<BoxOp> = vec![
            ok_op(10),
            err_op("permanent failure"), // index 1 — always fails after retries
            ok_op(30),
        ];
        // max_retries=0: no retries, immediate failure recording
        let batch = call_batch_with_retry(ops, 0, true).await.unwrap();
        assert_eq!(batch.results, vec![10, 30], "succeeded items must be present");
        assert_eq!(batch.failures.len(), 1, "exactly one failure");
        assert_eq!(batch.failures[0].index, 1, "failure index must be 1");
        assert!(
            batch.failures[0].error.to_string().contains("permanent failure"),
            "failure must carry the error"
        );
    }

    /// One op always fails; continue_on_failure=false → batch aborts with Err,
    /// no BatchResult returned.
    /// Matches retry.py:233-234: `if not continue_on_failure: raise`.
    #[tokio::test]
    async fn test_batch_one_fails_continue_false() {
        let ops: Vec<BoxOp> = vec![
            ok_op(10),
            err_op("abort trigger"), // index 1 — permanent failure
            ok_op(30),               // should never run
        ];
        let result = call_batch_with_retry(ops, 0, false).await;
        assert!(result.is_err(), "batch must abort with Err when continue_on_failure=false");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("abort trigger"), "error must propagate: {err_msg}");
    }

    /// An op that fails-then-succeeds recovers via retry and lands in results.
    /// Uses AtomicUsize to make the closure stateful without capturing a
    /// non-Send local (same technique as test_openai_retry_recovers_after_503).
    #[tokio::test]
    async fn test_batch_fail_then_succeed_via_retry() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);
        CALL_COUNT.store(0, Ordering::SeqCst);

        // Op: fails on attempt 0 (call #0), succeeds on attempt 1 (call #1)
        let ops: Vec<BoxOp> = vec![Box::new(|| {
            Box::pin(async {
                let attempt = CALL_COUNT.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 { Err(TeriError::Llm("transient".to_string())) } else { Ok(42u32) }
            })
        })];
        // max_retries=1 → up to 2 total attempts; op recovers on attempt 1
        let batch = call_batch_with_retry(ops, 1, true).await.unwrap();
        assert_eq!(batch.results, vec![42], "must recover on retry");
        assert!(batch.failures.is_empty(), "no permanent failures");
        assert_eq!(CALL_COUNT.load(Ordering::SeqCst), 2, "must have been called exactly twice");
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

    // =========================================================================
    // DECISION-7 — OpenAiAdapter::chat / chat_json (parameterized multi-role)
    // =========================================================================

    /// Unit test: verify the payload shape built for chat() contains the right
    /// message roles and optional fields without making HTTP calls.
    #[test]
    fn test_openai_chat_payload_shape() {
        // Build the payload that chat() would send (mirrors the implementation)
        let messages = &[ChatMessage::system("You are helpful"), ChatMessage::user("Hello")];
        let opts = ChatOptions { temperature: Some(0.3), max_tokens: Some(4096) };

        let mut payload = serde_json::json!({
            "model": "gpt-4o",
            "messages": messages,
        });
        if let Some(temp) = opts.temperature {
            payload["temperature"] = serde_json::json!(temp);
        }
        if let Some(max) = opts.max_tokens {
            payload["max_tokens"] = serde_json::json!(max);
        }

        // Verify message roles
        let msgs = payload["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are helpful");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[1]["content"], "Hello");
        // temperature and max_tokens present when set
        assert!((payload["temperature"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(payload["max_tokens"], 4096);
    }

    /// Unit test: when opts fields are None, temperature and max_tokens are absent.
    #[test]
    fn test_openai_chat_payload_opts_absent_when_none() {
        let messages = &[ChatMessage::user("hi")];
        let opts = ChatOptions::default();

        let mut payload = serde_json::json!({
            "model": "gpt-4o",
            "messages": messages,
        });
        if let Some(temp) = opts.temperature {
            payload["temperature"] = serde_json::json!(temp);
        }
        if let Some(max) = opts.max_tokens {
            payload["max_tokens"] = serde_json::json!(max);
        }

        assert!(
            payload.get("temperature").map(|v| v.is_null()).unwrap_or(true),
            "temperature must be absent when None"
        );
        assert!(
            payload.get("max_tokens").map(|v| v.is_null()).unwrap_or(true),
            "max_tokens must be absent when None"
        );
    }

    /// Unit test: chat_json payload includes response_format.
    #[test]
    fn test_openai_chat_json_payload_has_response_format() {
        let messages = &[ChatMessage::system("sys"), ChatMessage::user("user")];
        let opts = ChatOptions { temperature: Some(0.3), max_tokens: Some(4096) };

        let mut payload = serde_json::json!({
            "model": "gpt-4o",
            "messages": messages,
            "response_format": { "type": "json_object" },
        });
        if let Some(temp) = opts.temperature {
            payload["temperature"] = serde_json::json!(temp);
        }
        if let Some(max) = opts.max_tokens {
            payload["max_tokens"] = serde_json::json!(max);
        }

        assert_eq!(payload["response_format"]["type"], "json_object");
        assert!((payload["temperature"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(payload["max_tokens"], 4096);
    }

    /// chat: system+user messages reach the server and the response is returned.
    #[tokio::test]
    async fn test_openai_chat_with_system_and_user() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"choices":[{"message":{"content":"Hi there"}}]}"#);
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp = client
            .chat(
                &[ChatMessage::system("You are helpful"), ChatMessage::user("Hello")],
                &ChatOptions { temperature: Some(0.3), max_tokens: Some(4096) },
            )
            .await
            .unwrap();
        assert_eq!(resp, "Hi there");
        mock.assert();
    }

    /// chat: temperature and max_tokens are absent when None — response still received.
    #[tokio::test]
    async fn test_openai_chat_opts_absent_when_none() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"choices":[{"message":{"content":"default response"}}]}"#);
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp = client.chat(&[ChatMessage::user("hi")], &ChatOptions::default()).await.unwrap();
        assert_eq!(resp, "default response");
        mock.assert();
    }

    /// chat: think-blocks are stripped from the response.
    #[tokio::test]
    async fn test_openai_chat_strips_think() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"choices":[{"message":{"content":"<think>reasoning</think>Answer"}}]}"#);
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp = client.chat(&[ChatMessage::user("q")], &ChatOptions::default()).await.unwrap();
        assert_eq!(resp, "Answer");
    }

    /// chat_json: response is parsed as JSON.
    #[tokio::test]
    async fn test_openai_chat_json_parses_object() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"choices":[{"message":{"content":"{\"entity_types\":[]}"}}]}"#);
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp: serde_json::Value = client
            .chat_json(
                &[ChatMessage::system("sys"), ChatMessage::user("user")],
                &ChatOptions { temperature: Some(0.3), max_tokens: Some(4096) },
            )
            .await
            .unwrap();
        assert_eq!(resp["entity_types"], serde_json::json!([]));
        mock.assert();
    }

    /// chat_json: fenced JSON in the response is stripped before parsing.
    #[tokio::test]
    async fn test_openai_chat_json_strips_fence_and_think() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200).header("Content-Type", "application/json").body(
                "{\
                    \"choices\":[{\"message\":{\"content\":\"<think>r</think>```json\\n{\\\"v\\\":1}\\n```\"}}]\
                }",
            );
        });

        let client = OpenAiAdapter::new(&openai_config(&server, 0));
        let resp: serde_json::Value = client
            .chat_json(&[ChatMessage::user("q")], &ChatOptions::default())
            .await
            .unwrap();
        assert_eq!(resp["v"], 1);
    }

    // =========================================================================
    // DECISION-7 — AnthropicAdapter::chat / chat_json
    // =========================================================================

    /// Unit test: Anthropic chat payload puts system messages in top-level "system",
    /// user messages in "messages", and max_tokens defaults to 4096.
    #[test]
    fn test_anthropic_chat_payload_system_partition() {
        let messages =
            &[ChatMessage::system("You are a helpful assistant"), ChatMessage::user("Hello")];
        let opts = ChatOptions::default();

        let system_text: String = messages
            .iter()
            .filter(|m| matches!(m.role, ChatRole::System))
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let user_msgs: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| !matches!(m.role, ChatRole::System))
            .map(|m| {
                let role = match m.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::System => unreachable!(),
                };
                serde_json::json!({ "role": role, "content": m.content })
            })
            .collect();

        let max_tokens = opts.max_tokens.unwrap_or(4096);
        let mut payload = serde_json::json!({
            "model": "claude-3.5-sonnet",
            "messages": user_msgs,
            "max_tokens": max_tokens,
        });
        if !system_text.is_empty() {
            payload["system"] = serde_json::json!(system_text);
        }
        if let Some(temp) = opts.temperature {
            payload["temperature"] = serde_json::json!(temp);
        }

        // system messages partitioned to top-level "system"
        assert_eq!(payload["system"], "You are a helpful assistant");
        // user messages in "messages"
        let msgs = payload["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "Hello");
        // max_tokens defaults to 4096
        assert_eq!(payload["max_tokens"], 4096);
        // temperature absent when None
        assert!(payload.get("temperature").is_none() || payload["temperature"].is_null());
    }

    /// chat: system+user messages are delivered; response is returned.
    #[tokio::test]
    async fn test_anthropic_chat_system_partition() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"content":[{"text":"Hi from Claude"}]}"#);
        });

        let client = AnthropicAdapter::new_with_base(
            "sk-ant-test".to_string(),
            "claude-3.5-sonnet".to_string(),
            server.base_url(),
        );
        let resp = client
            .chat(
                &[ChatMessage::system("You are a helpful assistant"), ChatMessage::user("Hello")],
                &ChatOptions::default(),
            )
            .await
            .unwrap();
        assert_eq!(resp, "Hi from Claude");
        mock.assert();
    }

    /// chat: temperature is included when set; max_tokens uses explicit value.
    #[tokio::test]
    async fn test_anthropic_chat_with_temperature_and_max_tokens() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"content":[{"text":"response"}]}"#);
        });

        let client = AnthropicAdapter::new_with_base(
            "sk-ant-test".to_string(),
            "claude-3.5-sonnet".to_string(),
            server.base_url(),
        );
        let resp = client
            .chat(
                &[ChatMessage::user("q")],
                &ChatOptions { temperature: Some(0.5), max_tokens: Some(2048) },
            )
            .await
            .unwrap();
        assert_eq!(resp, "response");
        mock.assert();
    }

    /// chat_json: JSON sentinel is appended and fence is stripped.
    #[tokio::test]
    async fn test_anthropic_chat_json_parses_json() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"content":[{"text":"{\"k\":42}"}]}"#);
        });

        let client = AnthropicAdapter::new_with_base(
            "sk-ant-test".to_string(),
            "claude-3.5-sonnet".to_string(),
            server.base_url(),
        );
        let resp: serde_json::Value = client
            .chat_json(
                &[ChatMessage::system("sys"), ChatMessage::user("produce JSON")],
                &ChatOptions { temperature: Some(0.3), max_tokens: Some(4096) },
            )
            .await
            .unwrap();
        assert_eq!(resp["k"], 42);
        mock.assert();
    }

    // =========================================================================
    // DECISION-7 — GeminiAdapter::chat / chat_json
    // =========================================================================

    /// Unit test: Gemini chat payload puts system messages in systemInstruction,
    /// user/assistant in contents, and generationConfig when opts are set.
    #[test]
    fn test_gemini_chat_payload_shape() {
        let messages = &[ChatMessage::system("Be brief"), ChatMessage::user("Q?")];
        let opts = ChatOptions { temperature: Some(0.3), max_tokens: Some(4096) };

        let system_text: String = messages
            .iter()
            .filter(|m| matches!(m.role, ChatRole::System))
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");

        let contents: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| !matches!(m.role, ChatRole::System))
            .map(|m| {
                let role = match m.role {
                    ChatRole::User => "user",
                    ChatRole::Assistant => "model",
                    ChatRole::System => unreachable!(),
                };
                serde_json::json!({ "role": role, "parts": [{ "text": m.content }] })
            })
            .collect();

        let mut payload = serde_json::json!({ "contents": contents });
        if !system_text.is_empty() {
            payload["systemInstruction"] = serde_json::json!({
                "parts": [{ "text": system_text }]
            });
        }
        let mut gen_config = serde_json::Map::new();
        if let Some(temp) = opts.temperature {
            gen_config.insert("temperature".to_string(), serde_json::json!(temp));
        }
        if let Some(max) = opts.max_tokens {
            gen_config.insert("maxOutputTokens".to_string(), serde_json::json!(max));
        }
        if !gen_config.is_empty() {
            payload["generationConfig"] = serde_json::Value::Object(gen_config);
        }

        // systemInstruction present
        assert_eq!(payload["systemInstruction"]["parts"][0]["text"], "Be brief");
        // contents has user message
        let contents = payload["contents"].as_array().unwrap();
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Q?");
        // generationConfig carries temperature and maxOutputTokens
        let gc = &payload["generationConfig"];
        assert!((gc["temperature"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        assert_eq!(gc["maxOutputTokens"], 4096);
    }

    /// chat: system+user reach server; response is returned.
    #[tokio::test]
    async fn test_gemini_chat_system_and_user() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1beta/models/gemini-1.5-pro:generateContent");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"candidates":[{"content":{"parts":[{"text":"Gemini answer"}]}}]}"#);
        });

        let client = GeminiAdapter::new_with_base(
            "AIza-test".to_string(),
            "gemini-1.5-pro".to_string(),
            server.base_url(),
        );
        let resp = client
            .chat(
                &[ChatMessage::system("Be brief"), ChatMessage::user("Q?")],
                &ChatOptions { temperature: Some(0.3), max_tokens: Some(4096) },
            )
            .await
            .unwrap();
        assert_eq!(resp, "Gemini answer");
        mock.assert();
    }

    /// chat_json: responseMimeType is in payload (unit test); response is parsed.
    #[test]
    fn test_gemini_chat_json_mime_type_in_payload() {
        let opts = ChatOptions::default();
        let mut gen_config = serde_json::Map::new();
        if let Some(temp) = opts.temperature {
            gen_config.insert("temperature".to_string(), serde_json::json!(temp));
        }
        if let Some(max) = opts.max_tokens {
            gen_config.insert("maxOutputTokens".to_string(), serde_json::json!(max));
        }
        gen_config.insert("responseMimeType".to_string(), serde_json::json!("application/json"));
        let payload = serde_json::json!({
            "contents": [],
            "generationConfig": serde_json::Value::Object(gen_config),
        });
        assert_eq!(payload["generationConfig"]["responseMimeType"], "application/json");
    }

    /// chat_json: response is parsed from JSON.
    #[tokio::test]
    async fn test_gemini_chat_json_sets_mime_type() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1beta/models/gemini-1.5-pro:generateContent");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"candidates":[{"content":{"parts":[{"text":"{\"x\":7}"}]}}]}"#);
        });

        let client = GeminiAdapter::new_with_base(
            "AIza-test".to_string(),
            "gemini-1.5-pro".to_string(),
            server.base_url(),
        );
        let resp: serde_json::Value = client
            .chat_json(&[ChatMessage::user("give me JSON")], &ChatOptions::default())
            .await
            .unwrap();
        assert_eq!(resp["x"], 7);
        mock.assert();
    }

    /// chat_json: no system messages → no systemInstruction key in payload.
    #[tokio::test]
    async fn test_gemini_chat_json_no_system_no_instruction() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/v1beta/models/gemini-1.5-pro:generateContent");
            then.status(200)
                .header("Content-Type", "application/json")
                .body(r#"{"candidates":[{"content":{"parts":[{"text":"{\"r\":1}"}]}}]}"#);
        });

        let client = GeminiAdapter::new_with_base(
            "AIza-test".to_string(),
            "gemini-1.5-pro".to_string(),
            server.base_url(),
        );
        let resp: serde_json::Value = client
            .chat_json(&[ChatMessage::user("q")], &ChatOptions::default())
            .await
            .unwrap();
        assert_eq!(resp["r"], 1);
        mock.assert();
    }
}
