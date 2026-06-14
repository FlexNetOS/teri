# Reuse-Y Differential Verification — U-006 (retry) & U-008 (LLMClient.chat/chat_json)

Date: 2026-06-14
Verifier: rust-port-parity-verifier (reuse-Y mode, default-skeptical)
DEST: `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri/src/llm.rs`
SOURCE (read-only): `MiroFish/backend/app/utils/{retry.py,llm_client.py}`

Method: differential contract comparison — read BOTH sides, compare shapes/behaviors. Reuse never
trusted: any divergence from the MiroFish contract reclassifies the unit `reuse-Y → extend-Y` with the
exact missing behavior. teri build GREEN (220 tests). No ledger edits (orchestrator applies).

---

## U-006 — retry  →  VERDICT: extend-Y (PARTIAL reuse; behavioral parity present, but UNPROVEN + jitter/cap gaps)

teri has no standalone `retry` module/symbol. Retry lives **inline** inside each adapter's `call_api`
(OpenAiAdapter `llm.rs:51-89`, Anthropic `llm.rs:260-299`, Gemini `llm.rs:450-490`). That is the symbol
the orchestrator flagged as the reuse target. Differential against MiroFish `retry_with_backoff`
(`retry.py:15-77`) + `RetryableAPIClient.call_with_retry` (`retry.py:149-193`):

| Contract behavior | MiroFish (retry.py) | teri (llm.rs call_api) | Match? |
|---|---|---|---|
| Exponential backoff | `delay *= backoff_factor` (×2.0) `retry.py:72` | `sleep(2^retries)` `llm.rs:72,83` | YES (≈2^n) |
| Max-retries cap | `range(max_retries+1)` `retry.py:47` | `retries < self.max_retries` `llm.rs:70,81` | YES |
| Re-raise on final attempt | `raise` at `attempt==max_retries` `retry.py:54-56` | returns `Err(TeriError::Http(...))` after loop exhausts `llm.rs:78,86` | YES (Err propagated) |
| Retry only right error classes | caller passes `exceptions=(...)`; teri's real call sites use API/timeout exceptions | retries ONLY 5xx (`is_server_error` `llm.rs:70`) + timeout (`e.is_timeout()` `llm.rs:81`); 4xx/parse NOT retried | YES (correct class restriction; arguably **stricter/safer** than MiroFish default `(Exception,)`) |
| `initial_delay` (first wait) | `1.0s` `retry.py:18` | first retry waits `2^1=2s` `llm.rs:72` | DIVERGE (minor, timing only) |
| `max_delay` cap | `min(delay,30.0)` `retry.py:59` | **no cap** — `2^retries` unbounded `llm.rs:72` | **GAP (minor)** — at max_retries=3 → 8s, harmless; would matter at higher caps |
| jitter | `delay*(0.5+random())` `retry.py:61` | **none** | GAP (minor; jitter is anti-thundering-herd, not a correctness behavior) |
| **Retry TESTS exist** | n/a | **NONE** — grep `src/`,`tests/` for 5xx-mock/max_retries-cap/re-raise: zero. Only `seed/mod.rs:477` 500-mock (unrelated unit). `llm.rs` tests cover complete/stream happy-path only | **GAP (BLOCKING for symbol [x])** |

`RetryableAPIClient.call_batch_with_retry` (`retry.py:195-237`, per-item retry + collect failures) and the
standalone `retry_with_backoff_async` decorator have **no teri equivalent** — but teri's architecture
inlines retry per-call rather than as a reusable decorator/batch helper, so these are not required for the
adapter contract; flag as not-needed-for-U008-consumers (no current teri caller needs batch-collect).

**Why extend-Y not reuse-confirmed:** core retry behavior matches, BUT (a) NO differential retry test
proves the cap/re-raise/backoff actually fire — per the symbol-rollup rule an unexercised branch stays
unproven (`- [~]`), and (b) `max_delay` cap + jitter are absent. The blocking item is the missing test.

**Exact extend-Y work:**
1. **Add retry differential tests** (REQUIRED to flip S-043…S-048 region): in `src/llm.rs` tests add a
   `MockServer` that returns 503 N times then 200 — assert success after exactly `max_retries` retries;
   and a 503-always mock with `max_retries=2` — assert `Err(TeriError::Http)` after the cap (re-raise).
   Target: `src/llm.rs` tests mod (~`llm.rs:601`). Source contract: `retry.py:47-74`.
2. (minor, owner-optional) add `max_delay` clamp (`retry.py:59`) + jitter (`retry.py:61`) to the
   `2^retries` sleep at `src/llm.rs:72,83` (and Anthropic/Gemini twins). Not correctness-blocking.

---

## U-008 — LLMClient.chat / chat_json  →  VERDICT: extend-Y (GAP-6 `<think>` strip CONFIRMED ABSENT + json-fence-strip gap)

Differential `OpenAiAdapter::complete` (`llm.rs:94-116`) / `complete_json` (`llm.rs:118-145`) vs MiroFish
`chat` (`llm_client.py:35-68`) / `chat_json` (`llm_client.py:70-102`):

| Contract behavior | MiroFish | teri | Match? |
|---|---|---|---|
| chat: call OpenAI chat.completions | `client.chat.completions.create` `llm_client.py:64` | POST `/chat/completions` `llm.rs:52` | YES |
| **chat: strip `<think>...</think>`** | `re.sub(r'<think>[\s\S]*?</think>','',content).strip()` `llm_client.py:67` | **ABSENT** — `complete` returns raw `content` `llm.rs:114`; grep `src/` for `think`/`strip_think`/`<think>` → zero hits in llm code | **GAP-6 CONFIRMED — extend-Y** |
| chat_json: JSON mode | `response_format={"type":"json_object"}` `llm_client.py:91` | `"response_format":{"type":"json_object"}` `llm.rs:128-130` | YES |
| **chat_json: strip markdown ```json fences** | strips `^```(?:json)?` + trailing ``` `llm_client.py:95-97` | **ABSENT** — `serde_json::from_str(content)` directly `llm.rs:143` | **GAP — extend-Y** |
| chat_json: parse JSON | `json.loads(cleaned)` `llm_client.py:100` | `serde_json::from_str` `llm.rs:143` | YES |
| chat_json: error on parse fail | `raise ValueError(f"...无效: {cleaned}")` `llm_client.py:101-102` | `Err(TeriError::Llm("Failed to parse JSON response: {e}"))` `llm.rs:144` | YES (equivalent error path; teri omits raw body in msg — cosmetic) |
| missing-key error | `__init__` `raise ValueError("LLM_API_KEY 未配置")` `llm_client.py:27-28` | config layer: `ConfigMissing` if unset (`config.rs:73`) + validate `Config` err if empty (`config.rs:137-139`) | YES (equivalent — moved from constructor to config/preflight gate) |
| chat_json: chat-then-strip order | `chat_json` calls `chat` (so `<think>` already stripped) THEN strips fences `llm_client.py:87-97` | teri `complete_json` is independent of `complete` — does NEITHER strip | compound gap (both strips missing on json path) |

**Note on `<think>` scope:** MiroFish strips `<think>` only in `chat` (`llm_client.py:67`), the common path
all reasoning-model (DeepSeek-R1, MiniMax-M2.5) responses flow through. To preserve parity for reasoning
models the strip must apply to **all three teri adapters' `complete`** (OpenAi `llm.rs:114`, Anthropic
`llm.rs:323`, Gemini `llm.rs:514`) AND be reachable on the json path. Recommend a shared
`strip_think(&str) -> String` helper applied in each `complete` return, with `complete_json` first
fence-stripping then parsing (mirroring MiroFish's chat→chat_json layering).

**Exact extend-Y work (port next cycle):**
1. **GAP-6:** add `<think>...</think>` strip. Add helper `fn strip_think(s:&str)->String` (regex
   `(?s)<think>.*?</think>` + `.trim()`). Apply at OpenAi `complete` return `src/llm.rs:114`, Anthropic
   `src/llm.rs:323`, Gemini `src/llm.rs:514`. Source: `llm_client.py:67`.
2. **JSON-fence strip:** before `serde_json::from_str` in `complete_json`, strip leading
   ` ```json `/` ``` ` and trailing ` ``` ` (mirror `re.sub` `llm_client.py:95-96`). Target
   `src/llm.rs:143` (OpenAi); same for Anthropic `llm.rs:331` / Gemini `llm.rs:522` complete_json.
   Source: `llm_client.py:94-97`.
3. **Tests:** golden cases — `complete` input `"a<think>x</think>b"` → `"ab"`; `complete_json` input
   ` ```json\n{"k":1}\n``` ` → parses to `{k:1}`. Add to `src/llm.rs` tests mod.

Missing-key behavior: **no work needed** (config gate equivalent, confirmed).
