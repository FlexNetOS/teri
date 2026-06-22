# Implementation log: MiroFish→teri spine + LLM-layer fixes (FIX-1..FIX-5)

Branch: `fix/mirofish-spine-and-llm` (off `main`). Not pushed; no PR.

## Changes
- `src/config.rs`: added `LlmConfig.max_tokens: u32` (env `LLM_MAX_TOKENS`, default 2048) and `LlmConfig.provider: LlmProvider`; new `LlmProvider` enum (openai/anthropic/gemini, `from_env_str`, env `LLM_PROVIDER`); wired both defaults in `Config::build`.
- `src/llm.rs`: (FIX-4) `OpenAiAdapter` gained a `max_tokens` field, emitted in `complete()`/`complete_json()` payloads (`0` opts out); Anthropic `complete`/`stream` now use the configured cap; added `AnthropicAdapter::from_config`/`with_max_tokens` and `GeminiAdapter::from_config`. (FIX-5) rewrote Anthropic `stream()` to parse REAL Messages SSE (`event:`-typed `content_block_delta`, terminate on `message_stop`, surface `error` events) and Gemini `stream()` to request `?alt=sse` and parse `data:` JSON with no `[DONE]`. (FIX-3) added `ProviderAdapter` enum (Clone + `LlmClient`, static dispatch) + `from_config`/`provider`; derived `Clone` on Anthropic/Gemini adapters. Updated the two stream tests to real framing; added 6 unit tests. Patched all `#[cfg(test)]` `LlmConfig {…}` literals.
- `src/api/mod.rs`: (FIX-3) new `pub fn build_provider_llm(config) -> ProviderAdapter` (the run-pipeline selection seam); `build_llm` unchanged (serve/ApiState stay `OpenAiAdapter` per DECISION-U025-1).
- `src/pipeline.rs` (NEW): the in-process `run_pipeline<L: LlmClient + Clone + …>` composing seed→ontology→graph extraction→prepare(personas+config)→start(sim, write-back)→report; `PipelineOutcome` + `to_verdict_json`. In-crate so it reuses `pub(crate)` managers.
- `src/main.rs`: (FIX-1) `run_cmd` now selects the provider (`build_provider_llm`) and drives `run_pipeline`; removed the `Err("Pipeline not yet implemented")` bail. (FIX-2) added `-o/--out <path>` to the `run` subcommand; writes `verdict.json` on completion.
- `src/lib.rs`: `pub mod pipeline`; export `LlmProvider`, `ProviderAdapter`.
- `src/{memory/mod,preflight,embedding,services/graph_backend,services/graph_memory}.rs`: extended the test-only `LlmConfig` literals with the two new fields.
- `tests/pipeline_run.rs` (NEW): the injected-mock-LLM integration test.

## Engine API (the parity contract)
- `teri::api::build_provider_llm(&Config) -> ProviderAdapter` — provider selection (FIX-3).
- `teri::llm::ProviderAdapter` — concrete enum-dispatch `LlmClient` (no `dyn`; satisfies the `L: LlmClient + Clone` pipeline bound).
- `teri::config::{LlmProvider, LlmConfig.provider, LlmConfig.max_tokens}` — new config surface.
- `teri::pipeline::{run_pipeline, PipelineOutcome, PipelineOutcome::to_verdict_json}` — the run spine + verdict.
- LLM wire behavior: `complete`/`complete_json` now send `max_tokens`; Anthropic/Gemini `stream()` use their real SSE framing.

## Tests added
- `tests/pipeline_run.rs::run_pipeline_with_mock_llm_produces_report_and_verdict` — drives the FULL `run_pipeline` composition with a content-aware injected MockLlm (no live GGUF), asserts: LLM was actually called, ≥2 graph nodes extracted, ≥1 persona generated, sim reached a terminal status, a `report_` report with key-findings sections was produced, and `verdict.json` was written with the expected shape.
- `src/llm.rs` unit tests: `test_openai_complete_sends_max_tokens`, `test_openai_complete_json_sends_max_tokens`, `test_openai_complete_max_tokens_zero_omits_key` (FIX-4); `test_provider_adapter_selection`, `test_llm_provider_from_env_str` (FIX-3); rewrote `test_anthropic_adapter_stream` + `test_gemini_adapter_stream` to assert REAL provider SSE framing (FIX-5).

## Build/test status
- `cargo build -p teri` → PASS.
- `cargo test --lib` → **1619 passed / 0 failed** (baseline 1614; net +5). Invariant (1) ≥1614 satisfied.
- `cargo test --test pipeline_run` → **1 passed**.
- `cargo fmt --all --check` → clean (EXIT 0).
- `cargo clippy --all-targets -- -D warnings` → **No issues found** (teri's CI gate).
- `cargo run -p teri -- run --help` → shows the new `-o/--out` flag.

## Deviations
- **FIX-3 seam split.** Plan said make `api/mod.rs:248 build_llm()` select by provider. `build_llm` is hard-monomorphized into `ApiState`/`SimulationRunner<OpenAiAdapter>`/serve (DECISION-U025-1: `LlmClient` not dyn-compatible, axum state can't be generic) — changing its return type cascades across the server. I added `build_provider_llm -> ProviderAdapter` (the genuine selector) and used it in `run_cmd`'s pipeline exactly as the plan requires ("Use this same selection in run_cmd's pipeline"). For openai/ollama/lmstudio/vllm the two are identical (base_url-distinguished); only anthropic/gemini diverge. No capability lost.
- **FIX-5 chose implement-real, not gate** — both Anthropic and Gemini now parse genuine SSE framing (no-downgrade).
- **`--agents` is advisory.** The prepared pipeline derives one persona per surviving graph entity (MiroFish has no hard count knob on this path). `--agents` is recorded in `verdict.json` as `agents.requested` next to the actual `agents.generated` — matches source behavior (no fabricated count).

## Handoff notes
- **No stubs:** `grep -rn "not yet implemented\|todo!()\|unimplemented!()" src/` → none. The bail is gone.
- **No new deps:** uses existing serde_json/reqwest/async-trait/tokio — no-C / single-rustls invariants untouched.
- **Mock-LLM test bypasses the honesty guard correctly:** it calls `run_pipeline` directly with an injected adapter; `preflight_backend` still guards `main.rs::run_cmd`/`serve` and is unchanged. Live e2e still needs a real shimmy+GGUF backend.
- **Verify targets:** FIX-4 → the 3 mock-server unit tests prove `max_tokens` on the wire (and `0` omits it). FIX-3 → `test_provider_adapter_selection` + serve still compiles against `OpenAiAdapter`. FIX-5 → rewritten stream tests include ignored `message_start`/`ping` events and the `alt=sse` query matcher. FIX-1/FIX-2 → integration test asserts report + verdict.json from the real composition.
- **Pre-existing repo pollution (NOT mine to commit):** the initial `git status` already showed many untracked `uploads/simulations/sim_*` dirs; lib tests using the repo-default `uploads/` path add more, and `uploads/` is NOT in `.gitignore`. I did not stage them. Recommend adding `uploads/` to `.gitignore` (or `git add` only source files) before committing — do not commit the sim dirs.
