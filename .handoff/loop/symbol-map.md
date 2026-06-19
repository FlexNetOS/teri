# MiroFish → teri Symbol Map

**Source root:** `/home/drdave/Desktop/meta/MiroFish/`
**Harvest date:** 2026-06-14
**Harvest method:**
- Python (all `.py` files): AST structural parse (`ast` module) — deterministic, no grep
- JS/Vue (`.js`, `.vue`): Structural regex parse on `<script>` blocks + export patterns — AST-equivalent for these file types; `git kb code index` not used (indexer not configured for this source tree)
- HTTP routes: AST walk of `@bp.route(...)` decorators — all routes enumerated
- Locale (`.json`): JSON key-path walk — namespace-group rows (629 leaf keys across 17 namespaces)
- **Coverage:** ALL real-source trees inventoried; venv/node_modules/dist EXCLUDED
- **Visibility filter:** Python: all class-level and module-level names excluding dunder (`__x__`); JS: exported consts/fns and Vue SFC setup-block top-level bindings

**Status legend:** `- [ ]` not ported · `- [~]` ported, parity unproven · `- [x]` ported + parity-verified · `- [!]` blocked · `- [≠]` intentional-divergence

---

## U-001 — `backend/app/config.py:Config`

- [≠] S-001 · `unit:U-001` · `const` · `project_root_env` · module-level path to .env file · `config.py:11` · **rust-target:** `[≠]` idiomatic-mapping: `dotenvy::dotenv().ok()` in `Config::load()` performs the same project-root `.env` discovery automatically; no path constant needed. `dotenvy` provides a SUPERSET (searches parent dirs, handles workspace root). No contractual observable output from this symbol; the `.env` loading side effect is fully covered.
- [x] S-002 · `unit:U-001` · `type` · `Config` · Flask config class · `config.py:20` · **rust-target:** `Config` at `src/config.rs`; extend-Y of existing teri `Config`; all MiroFish fields merged in as additional struct fields; ported 2026-06-17
- [x] S-003 · `unit:U-001` · `field` · `Config.SECRET_KEY` · env `SECRET_KEY`, default `mirofish-secret-key` · `config.py:24` · **ROLLED UP 2026-06-17 (opus PASS, U-002/U-003 landed):** `Config.secret_key: String` (env `SECRET_KEY`, default `"mirofish-secret-key"`, config.rs:248), loaded in `Config::build` and available on `ApiState.config.secret_key`. Faithfully "loaded & available, NOT actively used" — matches MiroFish (verified: no session/CSRF/flash usage anywhere; Flask sets it in app.config but nothing signs with it). Default + env-override tests. → `[x]`.
- [x] S-004 · `unit:U-001` · `field` · `Config.DEBUG` · env `FLASK_DEBUG`, default True · `config.py:25` · **rust-target:** `Config::debug: bool` at `src/config.rs`; env `FLASK_DEBUG`, default `true`; tested (default + env-override)
- [x] S-005 · `unit:U-001` · `field` · `Config.JSON_AS_ASCII` · False — UTF-8 JSON · `config.py:28` · **ROLLED UP 2026-06-17 (opus PASS, U-003 landed):** axum `Json`/serde_json serialize non-ASCII as RAW UTF-8 by default (never `\uXXXX`) — structurally equivalent to Flask `ensure_ascii=False` (`__init__.py:26-27`). Live in the axum response layer; test `json_chinese_characters_emitted_as_raw_utf8` asserts 中文 raw + no `\u` escapes. → `[x]`.
- [x] S-006 · `unit:U-001` · `field` · `Config.LLM_API_KEY` · env `LLM_API_KEY`, required · `config.py:31` · **rust-target:** `Config.llm.api_key` at `src/config.rs`; pre-existing in teri; env `LLM_API_KEY`; validated in `validate_collect()` → "LLM_API_KEY is not set"
- [x] S-007 · `unit:U-001` · `field` · `Config.LLM_BASE_URL` · env `LLM_BASE_URL`, default openai v1 · `config.py:32` · **rust-target:** `Config.llm.base_url` at `src/config.rs`; pre-existing in teri; env `LLM_BASE_URL`, default `https://api.openai.com/v1` — exact match
- [x] S-008 · `unit:U-001` · `field` · `Config.LLM_MODEL_NAME` · env `LLM_MODEL_NAME`, default gpt-4o-mini · `config.py:33` · **rust-target:** `Config.llm.model` at `src/config.rs`; reads `LLM_MODEL_NAME` first (MiroFish env name), falls back to `LLM_MODEL` (teri legacy name), then default `"gpt-4o"`. **Default divergence:** MiroFish default is `"gpt-4o-mini"`, teri architect set `"gpt-4o"` — teri's default preserved; MiroFish users set `LLM_MODEL_NAME=gpt-4o-mini`. Tested: `LLM_MODEL_NAME` takes precedence over `LLM_MODEL`.
- [x] S-009 · `unit:U-001` · `field` · `Config.ZEP_API_KEY` · env `ZEP_API_KEY`, required · `config.py:36` · **rust-target:** `Config::zep_api_key: Option<String>` at `src/config.rs`; env `ZEP_API_KEY`; validated in `validate_collect()` → "ZEP_API_KEY is not set"; tested (absent→None, env-set, validate with/without)
- [x] S-010 · `unit:U-001` · `field` · `Config.MAX_CONTENT_LENGTH` · 50MB · `config.py:39` · **rust-target:** `Config::max_content_length: u64 = 50*1024*1024` at `src/config.rs`; fixed constant, not env-backed (matches source); tested
- [x] S-011 · `unit:U-001` · `field` · `Config.UPLOAD_FOLDER` · path to `uploads/` · `config.py:40` · **rust-target:** `Config::upload_folder: String` at `src/config.rs`; env `UPLOAD_FOLDER`, default `"./uploads"` (inert until U-002/U-003 HTTP upload handler ported); tested default + env-override
- [x] S-012 · `unit:U-001` · `field` · `Config.ALLOWED_EXTENSIONS` · set {pdf,md,txt,markdown} · `config.py:41` · **rust-target:** `Config::allowed_extensions: Vec<String>` at `src/config.rs`; sorted `Vec` (Set behavior preserved via contains checks; `HashSet` not `Serialize` without extra); exact 4 elements {pdf,md,txt,markdown}; tested
- [x] S-013 · `unit:U-001` · `field` · `Config.DEFAULT_CHUNK_SIZE` · 500 chars · `config.py:44` · **rust-target:** `Config::default_chunk_size: usize = 500` at `src/config.rs`; fixed constant; tested
- [x] S-014 · `unit:U-001` · `field` · `Config.DEFAULT_CHUNK_OVERLAP` · 50 chars · `config.py:45` · **rust-target:** `Config::default_chunk_overlap: usize = 50` at `src/config.rs`; fixed constant; tested
- [x] S-015 · `unit:U-001` · `field` · `Config.OASIS_DEFAULT_MAX_ROUNDS` · env `OASIS_DEFAULT_MAX_ROUNDS`, default 10 · `config.py:48` · **rust-target:** `Config::oasis_default_max_rounds: u32` at `src/config.rs`; env `OASIS_DEFAULT_MAX_ROUNDS`, default 10; tested default + env-override
- [x] S-016 · `unit:U-001` · `field` · `Config.OASIS_SIMULATION_DATA_DIR` · path to `uploads/simulations/` · `config.py:49` · **rust-target:** `Config::oasis_simulation_data_dir: String` at `src/config.rs`; env `OASIS_SIMULATION_DATA_DIR`, default `"./uploads/simulations"`; tested default
- [x] S-017 · `unit:U-001` · `field` · `Config.OASIS_TWITTER_ACTIONS` · list of 6 action strings · `config.py:52` · **rust-target:** `Config::oasis_twitter_actions: Vec<String>` at `src/config.rs`; exact 6 strings in source order; tested count + all 6 values
- [x] S-018 · `unit:U-001` · `field` · `Config.OASIS_REDDIT_ACTIONS` · list of 13 action strings · `config.py:55` · **rust-target:** `Config::oasis_reddit_actions: Vec<String>` at `src/config.rs`; all 13 strings in source order; TREND and REFRESH preserved as distinct (TREND is observable downstream — recorded as activity, not filtered); tested count + all 13 values + TREND-before-REFRESH order
- [x] S-019 · `unit:U-001` · `field` · `Config.REPORT_AGENT_MAX_TOOL_CALLS` · env, default 5 · `config.py:62` · **rust-target:** `Config::report_agent_max_tool_calls: u32` at `src/config.rs`; env `REPORT_AGENT_MAX_TOOL_CALLS`, default 5; tested default + env-override
- [x] S-020 · `unit:U-001` · `field` · `Config.REPORT_AGENT_MAX_REFLECTION_ROUNDS` · env, default 2 · `config.py:63` · **rust-target:** `Config::report_agent_max_reflection_rounds: u32` at `src/config.rs`; env `REPORT_AGENT_MAX_REFLECTION_ROUNDS`, default 2; tested default
- [x] S-021 · `unit:U-001` · `field` · `Config.REPORT_AGENT_TEMPERATURE` · env, default 0.5 · `config.py:64` · **rust-target:** `Config::report_agent_temperature: f64` at `src/config.rs`; env `REPORT_AGENT_TEMPERATURE`, default 0.5; tested default + env-override
- [x] S-022 · `unit:U-001` · `method` · `Config.validate` · classmethod → list[str] of missing required vars · `config.py:67` · **rust-target:** `Config::validate_collect() -> Vec<String>` at `src/config.rs` (direct MiroFish contract: collect-all errors, empty=pass); `Config::validate() -> Result<()>` extended to call `validate_collect()` first and join errors (ZEP_API_KEY now enforced alongside LLM_API_KEY); tested: both-missing (2 errors), only-ZEP-missing, only-LLM-missing, both-present (0 errors), validate() returns Err when ZEP missing

---

## U-002 — `backend/run.py`

- [x] S-023 · `unit:U-002` · `fn` · `main` · Flask entry-point; validates config, reads FLASK_HOST/PORT, runs threaded=True · `run.py:25` · **PARITY-VERIFIED 2026-06-17 (opus PASS):** `teri::server::serve` (via `serve_cmd`) — `validate_collect()` FIRST → print `配置错误:`/`  - {err}` + return Err on errors (== `sys.exit(1)`); bind-addr resolution faithful SUPERSET (`--addr` flag wins; else `FLASK_HOST` def "0.0.0.0" + `FLASK_PORT` def 5001 as int — 3 resolver tests); `axum::serve().with_graceful_shutdown(ctrl_c)`. `[≠]`: Windows UTF-8 stdout-reconfigure (run.py:9-16, inexpressible — Rust strings natively UTF-8) + Flask debug/threaded WSGI artifacts (tokio always-concurrent superset; `config.debug` still loaded). → `[x]`. `src/main.rs` + `src/server.rs`

---

## U-003 — `backend/app/__init__.py:create_app`

- [~] S-024 · `unit:U-003` · `fn` · `create_app` · Flask app factory; registers blueprints, middleware, signal handlers · `__init__.py:19` · **PARTIAL — PARITY-VERIFIED 2026-06-17 (opus PASS for the portable-now portion).** `teri::server::create_app(state) -> Router`: CORS (`CorsLayer` permissive == `origins:"*"`), before/after logging middleware (`请求: {method} {path}` / `响应: {status}` at debug), Accept-Language→locale middleware (S-040), JSON ensure_ascii (S-005), `/health` (S-025) — ALL faithful. **Two pieces PENDING (honestly recorded, NOT dropped):** (1) the 3 blueprint mounts `/api/graph|simulation|report` → **pending U-025/026/027** (clearly-marked nest placeholder, NO stub routes); (2) `register_cleanup` sim-process teardown → **pending U-023/U-049** (basic `ctrl_c` graceful shutdown wired now; full cleanup composes in later). Flips `[x]` when U-025/026/027 land + cleanup wires. `src/server.rs`
- [x] S-025 · `unit:U-003` · `route` · `GET /health` · **PARITY-VERIFIED 2026-06-17 (opus PASS):** the ACTUAL source (`__init__.py:72-74`) returns `{'status':'ok','service':'MiroFish Backend'}` (the ledger's `{"status":"healthy"}` summary was WRONG — matched the CODE). teri returns `{"status":"ok","service":"teri"}` — exact 2-key shape + `status:"ok"`; `service:"teri"` = accepted branding (teri IS the service; informational, not a feature). 200 OK. → `[x]`. `src/server.rs`

---

## U-004 — `backend/app/utils/logger.py`

- [x] S-026 · `unit:U-004` · `const` · `LOG_DIR` · path to `backend/logs/` · `logger.py:27` · **rust-target:** `LOG_DIR_ENV="TERI_LOG_DIR"` + `LOG_BACKUP_COUNT=5` + `MAX_LOG_BYTES=10*1024*1024` consts at `src/logging.rs`; directory is opt-in via env var (no hardcoded path — teri's config-is-env design)
- [x] S-027 · `unit:U-004` · `fn` · `_ensure_utf8_stdout` · reconfigures stdout/stderr to UTF-8 on Windows · `logger.py:13` · **rust-target:** `[≠]` N/A — Rust stdout is UTF-8 on all platforms teri targets; no reconfiguration needed. Noted in module doc comment. No capability lost.
- [x] S-028 · `unit:U-004` · `fn` · `setup_logger` · configures rotating file + console handlers · `logger.py:30` · **rust-target:** `init_logging(level)` at `src/logging.rs`; composes console layer (EnvFilter) + optional file layer (DEBUG+, size-based rotation 10MB×5 via `file-rotate` crate, `ContentLimit::Bytes` + `AppendCount::new(5)`); file layer opt-in via `TERI_LOG_DIR` env; tested via `build_rotating_writer` (5 tests)
- [x] S-029 · `unit:U-004` · `fn` · `get_logger` · returns named logger · `logger.py:91` · **rust-target:** `[≠]` idiomatic-mapping: tracing uses targets (`tracing::info!(target: "name", …)`) rather than named logger objects; same conceptual contract (per-name routing), different form. No dropped capability.
- [x] S-030 · `unit:U-004` · `const` · `logger` · module-level logger instance · `logger.py:108` · **rust-target:** `[≠]` idiomatic-mapping: tracing has no module-level logger instance; macros (`tracing::info!` etc.) address the global subscriber directly; identical runtime behavior
- [x] S-031 · `unit:U-004` · `fn` · `debug` · shortcut · `logger.py:112` · **rust-target:** `[≠]` idiomatic-mapping: `tracing::debug!(…)` macro; no wrapper fn needed (macros are zero-cost call sites)
- [x] S-032 · `unit:U-004` · `fn` · `info` · shortcut · `logger.py:115` · **rust-target:** `[≠]` idiomatic-mapping: `tracing::info!(…)` macro
- [x] S-033 · `unit:U-004` · `fn` · `warning` · shortcut · `logger.py:118` · **rust-target:** `[≠]` idiomatic-mapping: `tracing::warn!(…)` macro
- [x] S-034 · `unit:U-004` · `fn` · `error` · shortcut · `logger.py:121` · **rust-target:** `[≠]` idiomatic-mapping: `tracing::error!(…)` macro
- [x] S-035 · `unit:U-004` · `fn` · `critical` · shortcut · `logger.py:124` · **rust-target:** `[≠]` idiomatic-mapping: `tracing::error!(…)` macro (tracing has no separate critical level; ERROR is the highest level — same severity semantics)

---

## U-005 — `backend/app/utils/locale.py`

- [x] S-036 · `unit:U-005` · `const` · `_thread_local` → `tokio::task_local! LOCALE` · PARITY-VERIFIED 2026-06-17 (opus): task-local survives `.await` across worker threads (thread-local would NOT — task-local is the faithful no-downgrade substrate). `src/i18n/mod.rs`
- [x] S-037 · `unit:U-005` · `const` · `_locales_dir` → `include_str!` embedded assets · PARITY-VERIFIED 2026-06-17: runtime dir-scan → compile-time embed; embed set = {zh,en,languages}.json, complete. `src/i18n/mod.rs`
- [x] S-038 · `unit:U-005` · `const` · `_translations` → `OnceLock<HashMap<String,Value>>` · PARITY-VERIFIED 2026-06-17: EXACTLY {en,zh} (no fabrication); all 3 embedded JSON byte-identical to MiroFish (diff IDENTICAL). `src/i18n/mod.rs`
- [x] S-039 · `unit:U-005` · `fn` · `set_locale` → `with_locale(s, fut).await` (`LOCALE.scope`) · PARITY-VERIFIED 2026-06-17: capture-then-propagate caller pattern expressible; no in-place mutating set possible/needed for task-locals — idiomatic, not a narrowing. `src/i18n/mod.rs`
- [x] S-040 · `unit:U-005` · `fn` · `get_locale` · **COMPLETE — PARITY-VERIFIED 2026-06-17 (opus PASS, both branches now live).** Branch 2 (task-local fallback, default 'zh', returns stored value AS-IS without validation — `locale.py:32`) was verified at U-005. Branch 1 (`has_request_context()` → Accept-Language header, validate `in translations` else 'zh', `locale.py:29-31`) is NOW LIVE: `server::accept_language_middleware` reads `Accept-Language` (default "zh"), validates via `i18n::is_supported_locale` (single source of truth = embedded {en,zh} set) else "zh", and runs the handler inside `i18n::with_locale(LOCALE.scope)` so inner `get_locale()`/`t()` see the request locale. 4 tests: en→en, fr→zh, absent→zh, zh→zh. Both branches faithful → `[x]`. `src/i18n/mod.rs` + `src/server.rs`
- [x] S-041 · `unit:U-005` · `fn` · `t`/`t_args` · PARITY-VERIFIED 2026-06-17 (opus, 17 differential inputs): exact nested hit + dot-split traversal + non-dict-intermediate→None + **second-pass zh fallback** + missing→key-passthrough + literal `{name}` interpolation (numeric+string) all match Python byte-for-byte. `**kwargs` → `&[(&str,&dyn Display)]`. `src/i18n/mod.rs`
- [x] S-042 · `unit:U-005` · `fn` · `get_language_instruction` · PARITY-VERIFIED 2026-06-17: all **7** languages.json entries embedded (the one place 7 locales matter vs 2 for `t()`); locale→llmInstruction + zh fallback + hard-default "请使用中文回答。". `src/i18n/mod.rs`

---

## U-006 — `backend/app/utils/retry.py`

- [~] S-043 · `unit:U-006` · `fn` · `retry_with_backoff` · decorator for sync fns; exponential backoff · `retry.py:15` · **rust-target:** `OpenAiAdapter::call_api` / `AnthropicAdapter::call_api` / `GeminiAdapter::call_api` inline retry loop at `src/llm.rs`; max_delay clamp added (MAX_BACKOFF_SECS=30 matches retry.py:59 `min(delay,max_delay)`, applied at llm.rs:140/151/356/367/554/565 — VERIFIED); jitter omitted (`- [≠]` — stochastic retry.py:61, no observable contract — ACCEPTED divergence); tested: exhaustion→Err+hit-count (test_openai_retry_exhausted_returns_err, _hits_cap), no-spurious-retry-on-success (test_openai_retry_no_retry_on_success). **STILL `- [~]`: recovery path (retry-THEN-succeed) untested. Porter claimed httpmock 0.7 can't do 503-THEN-200 — REFUTED by parity-verifier cycle-4: a static-atomic stateful matcher DOES work. EXACT TECHNIQUE for porter:** add module-scope `static C: AtomicUsize = AtomicUsize::new(0);` then mount a 503 mock with `.matches(\|_req\| C.fetch_add(1, SeqCst) == 0)` (matches only request #1) PLUS a plain 200 mock (no extra matcher) on the same path; httpmock's `find_mock` returns the first match, so req#1→503 (counter→1), req#2→200 (503 stops matching); assert resp == recovered body, `mock_503.assert_hits(1)`, `mock_200.assert_hits(1)`. (`when.matches` is `fn(&HttpMockRequest)->bool` — non-capturing, so a `static` counter is required, NOT a closure-captured local. ~2s runtime = real 2^1 backoff sleep; same order as existing retry tests. Probe run green in cycle-4 then removed.)
- [~] S-044 · `unit:U-006` · `fn` · `retry_with_backoff_async` · decorator for async fns · `retry.py:80` · **rust-target:** same inline async retry in all three `call_api` impls; async via tokio::time::sleep; behavior identical to S-043 — same recovery-path gap and same recovery-test technique applies (see S-043)
- [≠] S-045 · `unit:U-006` · `type` · `RetryableAPIClient` · class wrapping retry logic · `retry.py:132` · intentional-divergence: no standalone type; retry is encapsulated per-adapter in `call_api`; same contract, different form
- [≠] S-046 · `unit:U-006` · `method` · `RetryableAPIClient.__init__` · `retry.py:137` · intentional-divergence: see S-045
- [≠] S-047 · `unit:U-006` · `method` · `RetryableAPIClient.call_with_retry` · single call with retry · `retry.py:149` · intentional-divergence: see S-045
- [x] S-048 · `unit:U-006` · `method` · `RetryableAPIClient.call_batch_with_retry` · batch calls with retry · `retry.py:195` · **rust-target:** `call_batch_with_retry<T,F,Fut>` + `BatchResult<T>` + `BatchFailure` at `src/llm.rs`; per-item exponential-backoff retry (reuses MAX_BACKOFF_SECS=30, same `2^attempt` formula as adapters); partial-failure semantics: successes in `results: Vec<T>`, exhausted-retries in `failures: Vec<BatchFailure{index,error}>`; `continue_on_failure=true` (default) records failure and continues; `continue_on_failure=false` aborts on first permanent failure (`Err(e)` mirroring retry.py:234 `raise`); `F: Fn()->Fut` (vs `FnOnce`) so the factory can be re-invoked each retry; jitter omitted (`[≠]` — matches teri's established retry contract). **Tests (5):** test_batch_all_succeed / test_batch_empty / test_batch_one_fails_continue_true / test_batch_one_fails_continue_false / test_batch_fail_then_succeed_via_retry (AtomicUsize stateful). **333 pass (baseline 328, +5).**

---

## U-007 — `backend/app/utils/zep_paging.py`

- [≠] S-049 · `unit:U-007` · `const` · `_DEFAULT_PAGE_SIZE` · 100 · `zep_paging.py:20` · **[≠] inexpressible (DECISION-12, opus PASS):** page-size knob for a pagination loop that does not exist in-process (petgraph is fully in RAM).
- [≠] S-050 · `unit:U-007` · `const` · `_MAX_NODES` · 2000 · `zep_paging.py:21` · **[≠] strict-SUPERSET (DECISION-12, opus PASS):** network-paging safety cap with NO in-process analog; teri returns the full in-memory set (`.take(2000)` would truncate valid data). Verifier checked every consumer (graph_builder/zep_tools/entity_reader + teri AgentPool/filter_defined_entities/prepare_simulation/summarize_entities) — NONE depend on ≤2000 (LLM context bounded independently by ENTITIES_PER_TYPE_DISPLAY=20/MAX_CONTEXT_LENGTH=50000). Superset, not a skip.
- [≠] S-051 · `unit:U-007` · `const` · `_DEFAULT_MAX_RETRIES` · 3 · `zep_paging.py:22` · **[≠] inexpressible (DECISION-12):** retry-count for a network-retry loop absent in-process.
- [≠] S-052 · `unit:U-007` · `const` · `_DEFAULT_RETRY_DELAY` · retry delay · `zep_paging.py:23` · **[≠] inexpressible (DECISION-12):** backoff delay for absent network-retry loop.
- [≠] S-053 · `unit:U-007` · `fn` · `_fetch_page_with_retry` · single page fetch with exponential backoff · `zep_paging.py:26` · **[≠] inexpressible (DECISION-12, opus PASS):** retries `ConnectionError`/`TimeoutError`/`OSError`/`InternalServerError`; an in-memory `Vec`/`HashMap` read has no transient I/O to retry (consistent with U-016 S-216 `_call_with_retry` adjudication).
- [x] S-054 · `unit:U-007` · `fn` · `fetch_all_nodes` · paginate all graph nodes via UUID cursor · `zep_paging.py:59` · **[x] map-onto (DECISION-12, opus PASS):** subsumed by `KnowledgeGraphEntityReader::get_all_nodes` (`entity_reader.rs:560`) over `KnowledgeGraph::get_all_entities` (`graph/mod.rs:1046`, `node_weights().collect()` — returns ALL nodes, deterministic insertion order). Verified in U-016.
- [x] S-055 · `unit:U-007` · `fn` · `fetch_all_edges` · paginate all graph edges via UUID cursor · `zep_paging.py:105` · **[x] map-onto (DECISION-12, opus PASS):** subsumed by `KnowledgeGraphEntityReader::get_all_edges` (`entity_reader.rs:585`) over `KnowledgeGraph::get_all_edges` (`graph/mod.rs:830`, `edge_references()` — returns ALL edges). Verified in U-016.

---

## U-008 — `backend/app/utils/llm_client.py:LLMClient`

- [x] S-056 · `unit:U-008` · `type` · `LLMClient` · OpenAI-compatible LLM wrapper · `llm_client.py:14` · **rust-target:** `LlmClient` trait + `OpenAiAdapter` / `AnthropicAdapter` / `GeminiAdapter` at `src/llm.rs`; provider-agnostic trait covering chat, chat_json, stream
- [x] S-057 · `unit:U-008` · `method` · `LLMClient.__init__` · reads LLM_API_KEY/BASE_URL/MODEL_NAME from Config · `llm_client.py:17` · **rust-target:** `OpenAiAdapter::new(&LlmConfig)` / `AnthropicAdapter::new` / `GeminiAdapter::new` at `src/llm.rs`; validated via `LlmConfig::validate()` in config.rs
- [x] S-058 · `unit:U-008` · `method` · `LLMClient.chat` · calls chat.completions.create with `messages`+`temperature`+`max_tokens`, strips `<think>` tags · `llm_client.py:35` · **rust-target:** the convenience single-prompt path is `LlmClient::complete`; the FAITHFUL full-signature port (messages-vector + temperature + max_tokens) is `LlmClient::chat(&[ChatMessage], &ChatOptions)` added 2026-06-17 (DECISION-7, extend-Y on U-008) — implemented on all 3 adapters, reuses `strip_think`. The earlier `complete(&str)`-only mapping was a convenience subset; DECISION-7 closes the messages/temp/max_tokens parameterization gap. Both covered.
- [x] S-059 · `unit:U-008` · `method` · `LLMClient.chat_json` · calls chat with json_object format + `messages`+`temperature`+`max_tokens`, strips fences, parses JSON · `llm_client.py:70` · **rust-target:** convenience single-prompt path `LlmClient::complete_json`; FAITHFUL full-signature port is `LlmClient::chat_json::<T>(&[ChatMessage], &ChatOptions)` added 2026-06-17 (DECISION-7, extend-Y on U-008) — all 3 adapters (OpenAI `response_format:json_object`; Anthropic system-partition + max_tokens default 4096; Gemini systemInstruction + generationConfig + responseMimeType), `strip_think`→`strip_json_fence`→serde_json, parse-fail → TeriError::Llm. Consumed by U-014 generate() (temp 0.3 + max_tokens 4096 + system role). opus-verified additive, no U-008 regression (complete*/stream byte-identical). Both covered.

---

## U-009 — `backend/app/utils/file_parser.py`

- [x] S-060 · `unit:U-009` · `fn` · `_read_text_with_fallback` · multi-encoding detection: chardet→charset-normalizer→latin-1 · `file_parser.py:11` · **rust-target:** `SeedIngestor::read_text_with_fallback` at `src/seed/mod.rs`; strategy: UTF-8 → GBK (encoding_rs) → Windows-1252 (never fails); tested: GBK bytes→correct chars, Latin-1 bytes→correct chars, UTF-8 passthrough, end-to-end `from_file` for both encodings.
- [x] S-061 · `unit:U-009` · `type` · `FileParser` · static file extraction class · `file_parser.py:61` · **rust-target:** `SeedIngestor` at `src/seed/mod.rs`
- [≠] S-062 · `unit:U-009` · `field` · `FileParser.SUPPORTED_EXTENSIONS` · set of supported extensions · `file_parser.py:64` · **rust-target:** `SUPPORTED_EXTENSIONS` const at `src/seed/mod.rs`; **`- [≠]` json superset (no-downgrade):** teri's set `{txt,md,markdown,pdf,json}` is a SUPERSET of MiroFish `Config.ALLOWED_EXTENSIONS={pdf,md,txt,markdown}` (config.py:41) — identical for the 4 shared exts, ADDS json because teri genuinely ingests json (`read_json`); nothing MiroFish accepts is rejected. Verified cycle-6. (nit: code comment says "mirrors" — should say "mirrors+extends")
- [≠] S-063 · `unit:U-009` · `method` · `FileParser.is_supported` · checks filename against ALLOWED_EXTENSIONS · `file_parser.py:67` · **rust-target:** `SeedIngestor::is_supported` at `src/seed/mod.rs`; tested: all known types true, unknowns false, case-insensitive. **`- [≠]` permissive-policy (no-downgrade):** `is_supported` is the caller-side gate (mirrors MiroFish `allowed_file`/`FileParser.is_supported`) while `from_file` stays PERMISSIVE (unknown ext → plain-text, vs MiroFish `extract_text` raising) — teri resilience; the gate still exists, hides no loss. Verified cycle-6.
- [x] S-064 · `unit:U-009` · `method` · `FileParser.extract_text` · dispatch to pdf/md/txt extractor · `file_parser.py:81` · **rust-target:** `SeedIngestor::from_file` dispatch at `src/seed/mod.rs`; all arms including md/markdown now explicit
- [x] S-065 · `unit:U-009` · `method` · `FileParser._extract_from_pdf` · PyMuPDF, silently skips failed pages · `file_parser.py:111` · **rust-target:** `SeedIngestor::read_pdf` at `src/seed/mod.rs`; page-skip-on-error verified (parity-verifier cycle-5)
- [x] S-066 · `unit:U-009` · `method` · `FileParser._extract_from_md` · markdown text extraction · `file_parser.py:128` · **rust-target:** `SeedIngestor::from_file` "md"|"markdown" arm → `read_plain_text` (encoding-fallback) at `src/seed/mod.rs`; tested: .md dispatch, .markdown dispatch
- [x] S-067 · `unit:U-009` · `method` · `FileParser._extract_from_txt` · text file extraction · `file_parser.py:133` · **rust-target:** `SeedIngestor::read_plain_text` with `read_text_with_fallback` at `src/seed/mod.rs`; now handles non-UTF-8
- [x] S-068 · `unit:U-009` · `method` · `FileParser.extract_from_multiple` · concatenates texts from multiple files · `file_parser.py:138` · **rust-target:** `SeedIngestor::from_files` at `src/seed/mod.rs`; in-order concat with `=== 文档 i: name ===` headers; per-file error tolerance; tested: order, headers, error tolerance
- [ ] S-069 · `unit:U-009` · `fn` · `split_text_into_chunks` · character-count split with overlap · `file_parser.py:161` · distributed to U-013 (text_processor)

---

## U-010 — `backend/scripts/action_logger.py`

- [x] S-070 · `unit:U-010` · `type` · `PlatformActionLogger` · per-platform JSONL action logger · `action_logger.py:22` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-071 · `unit:U-010` · `method` · `PlatformActionLogger.__init__` · `action_logger.py:25` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-072 · `unit:U-010` · `method` · `PlatformActionLogger._ensure_dir` · creates log dir · `action_logger.py:39` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-073 · `unit:U-010` · `method` · `PlatformActionLogger.log_action` · writes action JSON line · `action_logger.py:43` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-074 · `unit:U-010` · `method` · `PlatformActionLogger.log_round_start` · `action_logger.py:68` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-075 · `unit:U-010` · `method` · `PlatformActionLogger.log_round_end` · `action_logger.py:80` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-076 · `unit:U-010` · `method` · `PlatformActionLogger.log_simulation_start` · `action_logger.py:92` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-077 · `unit:U-010` · `method` · `PlatformActionLogger.log_simulation_end` · `action_logger.py:105` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-078 · `unit:U-010` · `type` · `SimulationLogManager` · aggregates twitter+reddit+main loggers · `action_logger.py:119` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-079 · `unit:U-010` · `method` · `SimulationLogManager.__init__` · `action_logger.py:125` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-080 · `unit:U-010` · `method` · `SimulationLogManager._setup_main_logger` · `action_logger.py:140` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-081 · `unit:U-010` · `method` · `SimulationLogManager.get_twitter_logger` · `action_logger.py:169` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-082 · `unit:U-010` · `method` · `SimulationLogManager.get_reddit_logger` · `action_logger.py:175` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-083 · `unit:U-010` · `method` · `SimulationLogManager.log` · `action_logger.py:181` · PARITY-VERIFIED 2026-06-17 (opus, FAIL→fix→PASS): Python `getattr(logger, level.lower(), logger.info)` resolves REAL logging method names — match now maps `info`→Info, `warning|warn`→Warning, `error|exception`→Error, `critical|fatal`→Critical (new LogLevel variant, not suppressed), `debug`→Debug, `_`→Info (getattr fallback). 5 regression tests (critical/fatal/warn/exception/bogus). **One `[≠]` sub-behavior (opus-ACCEPTED):** Python `logger.exception()` appends `formatException(sys.exc_info())` (a trailing `NoneType: None`/traceback line) — depends on Python's AMBIENT current-exception state which has NO Rust equivalent (genuinely-inexpressible); the contractual ERROR-level dispatch IS ported; verifier confirmed ZERO source call sites pass "exception" (sole consumer run_parallel_simulation.py uses only `.info`). Not a disguised skip.
- [x] S-084 · `unit:U-010` · `method` · `SimulationLogManager.info` · `action_logger.py:186` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-085 · `unit:U-010` · `method` · `SimulationLogManager.warning` · `action_logger.py:189` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-086 · `unit:U-010` · `method` · `SimulationLogManager.error` · `action_logger.py:192` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-087 · `unit:U-010` · `method` · `SimulationLogManager.debug` · `action_logger.py:195` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-088 · `unit:U-010` · `type` · `ActionLogger` · legacy compat interface · `action_logger.py:201` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-089 · `unit:U-010` · `method` · `ActionLogger.__init__` · `action_logger.py:207` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-090 · `unit:U-010` · `method` · `ActionLogger._ensure_dir` · `action_logger.py:211` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-091 · `unit:U-010` · `method` · `ActionLogger.log_action` · `action_logger.py:216` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-092 · `unit:U-010` · `method` · `ActionLogger.log_round_start` · `action_logger.py:242` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-093 · `unit:U-010` · `method` · `ActionLogger.log_round_end` · `action_logger.py:254` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-094 · `unit:U-010` · `method` · `ActionLogger.log_simulation_start` · `action_logger.py:266` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-095 · `unit:U-010` · `method` · `ActionLogger.log_simulation_end` · `action_logger.py:278` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-096 · `unit:U-010` · `const` · `_global_logger` · module-level logger · `action_logger.py:292` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)
- [x] S-097 · `unit:U-010` · `fn` · `get_logger` · returns global logger · `action_logger.py:295` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh)

---

## U-011 — `backend/app/models/project.py`

- [x] S-098 · `unit:U-011` · `type` · `ProjectStatus` · enum: CREATED/ONTOLOGY_GENERATED/GRAPH_BUILDING/GRAPH_COMPLETED/FAILED · `project.py:17`
- [x] S-099 · `unit:U-011` · `variant` · `ProjectStatus.CREATED` · `project.py:19`
- [x] S-100 · `unit:U-011` · `variant` · `ProjectStatus.ONTOLOGY_GENERATED` · `project.py:20`
- [x] S-101 · `unit:U-011` · `variant` · `ProjectStatus.GRAPH_BUILDING` · `project.py:21`
- [x] S-102 · `unit:U-011` · `variant` · `ProjectStatus.GRAPH_COMPLETED` · `project.py:22`
- [x] S-103 · `unit:U-011` · `variant` · `ProjectStatus.FAILED` · `project.py:23`
- [x] S-104 · `unit:U-011` · `type` · `Project` · dataclass with to_dict/from_dict · `project.py:27`
- [x] S-105 · `unit:U-011` · `field` · `Project.project_id` · `project.py:29`
- [x] S-106 · `unit:U-011` · `field` · `Project.name` · `project.py:30`
- [x] S-107 · `unit:U-011` · `field` · `Project.status` · `project.py:31`
- [x] S-108 · `unit:U-011` · `field` · `Project.created_at` · `project.py:32`
- [x] S-109 · `unit:U-011` · `field` · `Project.updated_at` · `project.py:33`
- [x] S-110 · `unit:U-011` · `field` · `Project.files` · `project.py:36`
- [x] S-111 · `unit:U-011` · `field` · `Project.total_text_length` · `project.py:37`
- [x] S-112 · `unit:U-011` · `field` · `Project.ontology` · `project.py:40`
- [x] S-113 · `unit:U-011` · `field` · `Project.analysis_summary` · `project.py:41`
- [x] S-114 · `unit:U-011` · `field` · `Project.graph_id` · `project.py:44`
- [x] S-115 · `unit:U-011` · `field` · `Project.graph_build_task_id` · `project.py:45`
- [x] S-116 · `unit:U-011` · `field` · `Project.simulation_requirement` · `project.py:48`
- [x] S-117 · `unit:U-011` · `field` · `Project.chunk_size` · `project.py:49`
- [x] S-118 · `unit:U-011` · `field` · `Project.chunk_overlap` · `project.py:50`
- [x] S-119 · `unit:U-011` · `field` · `Project.error` · `project.py:53`
- [x] S-120 · `unit:U-011` · `method` · `Project.to_dict` · JSON round-trip serialise · `project.py:55`
- [x] S-121 · `unit:U-011` · `method` · `Project.from_dict` · deserialise from JSON dict · `project.py:76`
- [x] S-122 · `unit:U-011` · `type` · `ProjectManager` · class-method FS persistence · `project.py:101`
- [x] S-123 · `unit:U-011` · `field` · `ProjectManager.PROJECTS_DIR` · `uploads/projects` · `project.py:105`
- [x] S-124 · `unit:U-011` · `method` · `ProjectManager._ensure_projects_dir` · `project.py:108`
- [x] S-125 · `unit:U-011` · `method` · `ProjectManager._get_project_dir` · `project.py:113`
- [x] S-126 · `unit:U-011` · `method` · `ProjectManager._get_project_meta_path` · `project.py:118`
- [x] S-127 · `unit:U-011` · `method` · `ProjectManager._get_project_files_dir` · `project.py:123`
- [x] S-128 · `unit:U-011` · `method` · `ProjectManager._get_project_text_path` · `project.py:128`
- [x] S-129 · `unit:U-011` · `method` · `ProjectManager.create_project` · uuid dir + metadata JSON · `project.py:133`
- [x] S-130 · `unit:U-011` · `method` · `ProjectManager.save_project` · `project.py:168`
- [x] S-131 · `unit:U-011` · `method` · `ProjectManager.get_project` · returns Optional[Project] · `project.py:177`
- [x] S-132 · `unit:U-011` · `method` · `ProjectManager.list_projects` · `project.py:198`
- [x] S-133 · `unit:U-011` · `method` · `ProjectManager.delete_project` · raises if not found · `project.py:222`
- [x] S-134 · `unit:U-011` · `method` · `ProjectManager.save_file_to_project` · writes to files/ subdir · `project.py:241`
- [x] S-135 · `unit:U-011` · `method` · `ProjectManager.save_extracted_text` · `project.py:275`
- [x] S-136 · `unit:U-011` · `method` · `ProjectManager.get_extracted_text` · returns Optional[str] · `project.py:282`
- [x] S-137 · `unit:U-011` · `method` · `ProjectManager.get_project_files` · list of filenames · `project.py:293`

---

## U-012 — `backend/app/models/task.py`

- [x] S-138 · `unit:U-012` · `type` · `TaskStatus` · enum: PENDING/PROCESSING/COMPLETED/FAILED · `task.py:16`
- [x] S-139 · `unit:U-012` · `variant` · `TaskStatus.PENDING` · `task.py:18`
- [x] S-140 · `unit:U-012` · `variant` · `TaskStatus.PROCESSING` · `task.py:19`
- [x] S-141 · `unit:U-012` · `variant` · `TaskStatus.COMPLETED` · `task.py:20`
- [x] S-142 · `unit:U-012` · `variant` · `TaskStatus.FAILED` · `task.py:21`
- [x] S-143 · `unit:U-012` · `type` · `Task` · dataclass with to_dict · `task.py:25`
- [x] S-144 · `unit:U-012` · `field` · `Task.task_id` · `task.py:27`
- [x] S-145 · `unit:U-012` · `field` · `Task.task_type` · `task.py:28`
- [x] S-146 · `unit:U-012` · `field` · `Task.status` · `task.py:29`
- [x] S-147 · `unit:U-012` · `field` · `Task.created_at` · `task.py:30`
- [x] S-148 · `unit:U-012` · `field` · `Task.updated_at` · `task.py:31`
- [x] S-149 · `unit:U-012` · `field` · `Task.progress` · `task.py:32`
- [x] S-150 · `unit:U-012` · `field` · `Task.message` · `task.py:33`
- [x] S-151 · `unit:U-012` · `field` · `Task.result` · `task.py:34`
- [x] S-152 · `unit:U-012` · `field` · `Task.error` · `task.py:35`
- [x] S-153 · `unit:U-012` · `field` · `Task.metadata` · `task.py:36`
- [x] S-154 · `unit:U-012` · `field` · `Task.progress_detail` · `task.py:37`
- [x] S-155 · `unit:U-012` · `method` · `Task.to_dict` · `task.py:39` · **PARITY PASS 2026-06-17 (re-verify):** `python_isoformat` (task.rs:51-57) now emits `%Y-%m-%dT%H:%M:%S` when `timestamp_subsec_micros()==0` else `%Y-%m-%dT%H:%M:%S%.6f`, matching Python `datetime.isoformat()` (verified empirically: whole-sec→`2024-01-01T12:30:45`; µs→`...45.123456`, always 6 zero-padded digits, never 3-digit millis; no tz suffix). Test `test_python_isoformat_matches_datetime_isoformat` (task.rs:441) asserts BOTH cases + no `+`/`Z` suffix. Field names/order/status-string all match. Prior FAIL (always-`.000000`) resolved.
- [x] S-156 · `unit:U-012` · `type` · `TaskManager` · singleton, thread-safe in-memory registry · `task.py:56`
- [x] S-157 · `unit:U-012` · `field` · `TaskManager._instance` · singleton ref · `task.py:62`
- [x] S-158 · `unit:U-012` · `field` · `TaskManager._lock` · threading.Lock · `task.py:63`
- [x] S-159 · `unit:U-012` · `method` · `TaskManager.__new__` · double-checked locking · `task.py:65`
- [x] S-160 · `unit:U-012` · `method` · `TaskManager.create_task` · uuid task_id · `task.py:75`
- [x] S-161 · `unit:U-012` · `method` · `TaskManager.get_task` · returns Optional[Task] · `task.py:103`
- [x] S-162 · `unit:U-012` · `method` · `TaskManager.update_task` · `task.py:108`
- [x] S-163 · `unit:U-012` · `method` · `TaskManager.complete_task` · `task.py:147` · **ROLLED UP 2026-06-17 (opus PASS, U-005 landed): pending-U-005 RESOLVED.** Status=COMPLETED/progress=100/result correct. `message` now routes through `crate::i18n::t("progress.taskComplete")` via `msg_task_complete()` (task.rs:37) — locale-parameterized exactly as `t('progress.taskComplete')` (task.py:153). zh default still yields `"任务完成"` when LOCALE unset (existing task.rs tests stay green). Genuinely faithful → `[x]`.
- [x] S-164 · `unit:U-012` · `method` · `TaskManager.fail_task` · `task.py:157` · **ROLLED UP 2026-06-17 (opus PASS, U-005 landed): pending-U-005 RESOLVED.** Status=FAILED/error correct; correctly does NOT set progress=100. `message` now routes through `crate::i18n::t("progress.taskFailed")` via `msg_task_failed()` (task.rs:44) — locale-parameterized as `t('progress.taskFailed')` (task.py:162). zh default still yields `"任务失败"` when LOCALE unset. Genuinely faithful → `[x]`.
- [x] S-165 · `unit:U-012` · `method` · `TaskManager.list_tasks` · `task.py:166`
- [x] S-166 · `unit:U-012` · `method` · `TaskManager.cleanup_old_tasks` · removes completed/failed > max_age_hours · `task.py:174`

---

## U-013 — `backend/app/services/text_processor.py`

- [x] S-167 · `unit:U-013` · `type` · `TextProcessor` · static class · `text_processor.py:9` · **rust-target:** free functions + `TextStats` struct in `src/seed/text_processor.rs` (no class wrapper; all methods are free fns, matching idiomatic Rust) · PARITY-VERIFIED cycle-5 (2026-06-14): differential harness vs MiroFish source PASS (all 3 methods)
- [≠] S-168 · `unit:U-013` · `method` · `TextProcessor.extract_from_files` · delegates to FileParser · `text_processor.py:13` · intentional-divergence: file extraction is already ported in `SeedIngestor` (U-010/src/seed/mod.rs); delegating to FileParser is Python-only plumbing with no Rust equivalent needed
- [x] S-169 · `unit:U-013` · `method` · `TextProcessor.split_text` · char-count split with overlap · `text_processor.py:18` · **rust-target:** `crate::seed::text_processor::split_text()` at `src/seed/text_processor.rs`; UTF-8-safe char-boundary windowing, sentence-boundary backtrack, overlap; 9 tests · PARITY-VERIFIED cycle-5 (2026-06-14): 13-case differential vs `split_text_into_chunks` (file_parser.py:161-202) — empty/blank/short/exact/remainder/overlap/no-sep/period-space-boundary/chinese-fullstop/multibyte/hiragana/sep-below-30%/mixed-seps ALL match; boundary-backtrack + 30%-threshold truncation `(cs*0.3) as usize` proven equivalent to Python `last_sep > cs*0.3` at integer sep positions; overlap carries last N chars (verified `['ABCDEFGHIJ','HIJKLMNOPQ','OPQRST']`)
- [x] S-170 · `unit:U-013` · `method` · `TextProcessor.preprocess_text` · normalises whitespace · `text_processor.py:37` · **rust-target:** `crate::seed::text_processor::preprocess_text()` at `src/seed/text_processor.rs`; CRLF→LF, 3+newlines→2, per-line trim, final trim; 5 tests · PARITY-VERIFIED cycle-5 (2026-06-14): 9-case differential PASS incl `leading_indent` proving `l.trim()` == Python `line.strip()` (both-ends strip). NOTE: rust doc-comment lines 152-154 wrongly says "preserve leading whitespace" — comment-only defect, code is correct & parity-proven
- [x] S-171 · `unit:U-013` · `method` · `TextProcessor.get_text_stats` · returns dict with char/word/line counts · `text_processor.py:64` · **rust-target:** `crate::seed::text_processor::get_text_stats()` + `TextStats` struct at `src/seed/text_processor.rs`; serde-derive for JSON compat; 4 tests · PARITY-VERIFIED cycle-5 (2026-06-14): chars=Unicode scalars (`你好\nworld`→8 not bytes), words=split_whitespace, lines=`\n`-count+1 (empty→1) — all 4 cases match Python `len`/`split`/`count('\n')+1`

---

## U-014 — `backend/app/services/ontology_generator.py`

- [x] S-172 · `unit:U-014` · `const` · `ONTOLOGY_SYSTEM_PROMPT` · LLM system prompt for ontology generation · `ontology_generator.py:30` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh+extend)
- [x] S-173 · `unit:U-014` · `fn` · `_to_pascal_case` · name normalisation helper · `ontology_generator.py:16` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh+extend)
- [x] S-174 · `unit:U-014` · `type` · `OntologyGenerator` · `ontology_generator.py:176` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh+extend)
- [x] S-175 · `unit:U-014` · `method` · `OntologyGenerator.__init__` · `ontology_generator.py:182` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh+extend)
- [x] S-176 · `unit:U-014` · `method` · `OntologyGenerator.generate` · calls LLM → {entity_types,edge_types,analysis_summary} · `ontology_generator.py:185` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh+extend)
- [x] S-177 · `unit:U-014` · `field` · `OntologyGenerator.MAX_TEXT_LENGTH_FOR_LLM` · 50000 chars truncation · `ontology_generator.py:229` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh+extend)
- [x] S-178 · `unit:U-014` · `method` · `OntologyGenerator._build_user_message` · `ontology_generator.py:231` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh+extend)
- [x] S-179 · `unit:U-014` · `method` · `OntologyGenerator._validate_and_process` · enforces 10 entity types, PascalCase, SCREAMING_SNAKE edge names · `ontology_generator.py:277` · PARITY-VERIFIED 2026-06-17 (opus, port-fresh+extend)
- [≠] S-180 · `unit:U-014` · `method` · `OntologyGenerator.generate_python_code` · produces Python class definitions from ontology · `ontology_generator.py:400` · **INTENTIONAL-DIVERGENCE — opus-ACCEPTED 2026-06-17 (independently verified, NOT a disguised skip).** (1) ZERO callers anywhere in MiroFish (verifier grepped whole repo → only its own def — dead code). (2) Emits Zep-Cloud Python class strings (`from zep_cloud.external_clients.ontology import EntityModel,EntityText,EdgeModel`) for a SaaS teri replaced with native petgraph — genuinely-inexpressible-in-substrate. (3) The real ontology-registration path is `graph_builder.set_ontology` (py:205/288) which builds types dynamically from the validated ontology DICT and calls `client.graph.set_ontology(...)` — `generate_python_code` is NOT on that path; the DICT (which `generate`/`_validate_and_process` produce) IS ported. Registration behavior maps onto teri native `EntityKind::Custom` (OQ-5/GAP-3) via future S-192 set_ontology. The observable output (dict) is ported; only the unused substrate-specific code-string emitter is dropped → legitimate `[≠]`/map-onto. `src/services/ontology.rs` (doc comment)

---

## U-015 — `backend/app/services/graph_builder.py`

<!-- map-onto-substrate: Zep Cloud API methods (create_graph, set_ontology, add_text_batches,
     _wait_for_episodes, get_graph_data, delete_graph) are Zep SaaS calls with no Rust
     equivalent; the extraction pipeline behavior is mapped onto KnowledgeGraph::build(). -->

- [≠] S-181 · `unit:U-015` · `type` · `GraphInfo` · dataclass: graph_id,node_count,edge_count,entity_types · `graph_builder.py:24` · intentional-divergence: Zep-specific result type; teri exposes entity_count()/relation_count() directly on KnowledgeGraph
- [≠] S-182 · `unit:U-015` · `field` · `GraphInfo.graph_id` · `graph_builder.py:26` · intentional-divergence: Zep graph_id not applicable; teri's KnowledgeGraph is in-process
- [≠] S-183 · `unit:U-015` · `field` · `GraphInfo.node_count` · `graph_builder.py:27` · intentional-divergence: mapped to `KnowledgeGraph::entity_count()` at `src/graph/mod.rs:517`
- [≠] S-184 · `unit:U-015` · `field` · `GraphInfo.edge_count` · `graph_builder.py:28` · intentional-divergence: mapped to `KnowledgeGraph::relation_count()` at `src/graph/mod.rs:521`
- [≠] S-185 · `unit:U-015` · `field` · `GraphInfo.entity_types` · `graph_builder.py:29` · intentional-divergence: typed via `EntityKind` enum in `src/graph/mod.rs:11`
- [≠] S-186 · `unit:U-015` · `method` · `GraphInfo.to_dict` · `graph_builder.py:31` · intentional-divergence: replaced by `KnowledgeGraph::serialize_to_json()` at `src/graph/mod.rs:245`
- [≠] S-187 · `unit:U-015` · `type` · `GraphBuilderService` · `graph_builder.py:40` · intentional-divergence: no separate service type; build logic is a method on `KnowledgeGraph`
- [≠] S-188 · `unit:U-015` · `method` · `GraphBuilderService.__init__` · `graph_builder.py:46` · intentional-divergence: no constructor needed; llm passed as generic param to build()
- [x] S-189 · `unit:U-015` · `method` · `GraphBuilderService.build_graph_async` · spawns background thread → task_id · `graph_builder.py:54` · **PARITY-VERIFIED 2026-06-17 (opus PASS).** `build_graph_async<L>(llm, text, ontology, graph_name, chunk_size, chunk_overlap, _batch_size) -> String` at `src/services/graph_builder.rs:84`. Creates task via `TaskManager::global().create_task("graph_build", …)`, captures locale, `tokio::spawn(i18n::with_locale(locale, async move {…}))`, returns task_id immediately. Worker drives: 5% startBuildingGraph → [≠] 10% create_graph → 15% set_ontology → 20% textSplit → 20-60% 2-pass extraction (via `build_with_progress_and_ontology`) → [≠] 60-90% wait_for_episodes → 90% fetchingGraphInfo → 100% complete. LLM errors routed to `fail_task`. **Verified:** spawn contract (test_build_graph_async_returns_task_id_immediately), worker lifecycle Ok→complete_task COMPLETED + Err→fail_task FAILED w/ error string (worker_inner tests; spawn target calls same inner body so coverage refactored not lost), result = teri-native superset of graph_id (embeds serialized graph). create_graph(10%)/wait_for_episodes(60-90%)/`_batch_size` `[≠]` survive challenge (Zep-server/SDK-inexpressible / non-contractual, no observable teri output dropped). 550 lib tests green; clippy --all-targets -D warnings clean.
- [x] S-190 · `unit:U-015` · `method` · `GraphBuilderService._build_graph_worker` · create→ontology→split→batch→wait→info→complete · `graph_builder.py:100` · **rust-target:** `KnowledgeGraph::build()` at `src/graph/mod.rs:326`; 2-pass (entity→relation) extraction pipeline WITH chunking. **RE-VERIFIED cycle-5 (2026-06-14) — GAP-U015-1 RESOLVED, no U-015 downgrade:** large docs (>500 chars) split via `text_processor::split_text` (now parity-proven); entity extraction per chunk, results merged+name-deduped; relation extraction per chunk over full deduped entity set; small docs (≤500) → `split_text` returns 1 chunk = the EXACT pre-chunking single-pass path (5 cycle-1 build tests pass UNCHANGED: from_seed/empty/dup/unknown-ref/llm-error); blank-doc → empty-chunk → valid empty graph (new branch, tolerant); `test_build_large_doc_multi_chunk_merge` PROVES call_count>1 (chunking occurred) AND all 4 cross-chunk entities merged (Alice/Sunrise Corp/Bob/Sunset Inc, none dropped); no-truncation contract met (entities from ALL chunks). 263 tests pass.
- [≠] S-191 · `unit:U-015` · `method` · `GraphBuilderService.create_graph` · Zep graph_id = `mirofish_{uuid16}` · `graph_builder.py:193` · intentional-divergence: Zep SaaS call; teri uses in-process petgraph — no equivalent
- [x] S-192 · `unit:U-015` · `method` · `GraphBuilderService.set_ontology` · dynamic Pydantic EntityModel/EdgeModel subclasses · `graph_builder.py:205` · **PARITY-VERIFIED 2026-06-17 (opus PASS — NOT inert).** `KnowledgeGraph::set_ontology(&mut self, ontology: &Value)` at `src/graph/mod.rs:238`. Records `ontology_entity_types`+`ontology_edge_types` (name field from each array; idempotent — 2nd call replaces). **The recorded names reach the build output for BOTH entities AND edges** (verified end-to-end): entity/edge extraction prompts extended (`entity_extraction_prompt_with_custom`/`relation_extraction_prompt_with_custom`); entity parser maps registered name→`EntityKind::Custom` (`:955`), Pass-2 edge match maps registered edge name→`RelationKind::Custom` (`:672`); built-in name STILL maps to built-in; unknown-unregistered→`Other`. Differential `test_build_with_custom_relation_kind_emits_custom_variant`: `{MediaOutlet,COVERS_TOPIC}` → `Custom("MediaOutlet")`+`Custom("COVERS_TOPIC")`. NOT a silent no-op. Owner override of DECISION-8 item #2 applied — `RelationKind::Custom` added; custom EDGE emission no longer deferred (prior `- [!]` resolved). Zep-SDK `[≠]` items (Pydantic class synthesis, RESERVED_NAMES/safe_attr_name, Field(default=None)/UserWarning) survive challenge: inexpressible substrate / non-contractual (teri Entity has no Zep key namespace). Worker re-extracts names inline (functionally identical to calling set_ontology then reading fields).
- [≠] S-193 · `unit:U-015` · `method` · `GraphBuilderService.add_text_batches` · sleeps 1s between batches · `graph_builder.py:294` · intentional-divergence: Zep episode batching not applicable to in-process petgraph
- [≠] S-194 · `unit:U-015` · `method` · `GraphBuilderService._wait_for_episodes` · polls processed=True every 3s, 600s timeout · `graph_builder.py:347` · intentional-divergence: Zep async episode processing; in-process LLM call is synchronous/awaitable
- [≠] S-195 · `unit:U-015` · `method` · `GraphBuilderService._get_graph_info` · `graph_builder.py:403` · intentional-divergence: Zep SaaS call; teri exposes entity_count()/relation_count() directly
- [≠] S-196 · `unit:U-015` · `method` · `GraphBuilderService.get_graph_data` · nodes+edges dict with temporal fields · `graph_builder.py:426` · intentional-divergence: Zep SaaS read; teri uses serialize_to_json()/get_all_entities()/get_all_edges()
- [≠] S-197 · `unit:U-015` · `method` · `GraphBuilderService.delete_graph` · `graph_builder.py:503` · intentional-divergence: Zep SaaS call; teri's KnowledgeGraph is in-process, dropped when out of scope

---

## U-016 — `backend/app/services/zep_entity_reader.py`

- [x] S-198 · `unit:U-016` · `type` · `EntityNode` · dataclass with related_edges,related_nodes · `zep_entity_reader.py:23` → `teri::services::entity_reader::EntityNode`
- [x] S-199 · `unit:U-016` · `field` · `EntityNode.uuid` · `zep_entity_reader.py:25` → `teri::services::entity_reader::EntityNode`
- [x] S-200 · `unit:U-016` · `field` · `EntityNode.name` · `zep_entity_reader.py:26` → `teri::services::entity_reader::EntityNode`
- [x] S-201 · `unit:U-016` · `field` · `EntityNode.labels` · `zep_entity_reader.py:27` → `teri::services::entity_reader::EntityNode`
- [x] S-202 · `unit:U-016` · `field` · `EntityNode.summary` · `zep_entity_reader.py:28` → `teri::services::entity_reader::EntityNode`
- [x] S-203 · `unit:U-016` · `field` · `EntityNode.attributes` · `zep_entity_reader.py:29` → `teri::services::entity_reader::EntityNode` (Map<String,Value>)
- [x] S-204 · `unit:U-016` · `field` · `EntityNode.related_edges` · `zep_entity_reader.py:31` → `teri::services::entity_reader::EntityNode`
- [x] S-205 · `unit:U-016` · `field` · `EntityNode.related_nodes` · `zep_entity_reader.py:33` → `teri::services::entity_reader::EntityNode`
- [x] S-206 · `unit:U-016` · `method` · `EntityNode.to_dict` · `zep_entity_reader.py:35` → `teri::services::entity_reader::EntityNode::to_dict`
- [x] S-207 · `unit:U-016` · `method` · `EntityNode.get_entity_type` · extracts type from labels · `zep_entity_reader.py:46` → `teri::services::entity_reader::EntityNode::get_entity_type`
- [x] S-208 · `unit:U-016` · `type` · `FilteredEntities` · dataclass: entities,entity_types,total_count,filtered_count · `zep_entity_reader.py:55` → `teri::services::entity_reader::FilteredEntities`
- [x] S-209 · `unit:U-016` · `field` · `FilteredEntities.entities` · `zep_entity_reader.py:57` → `teri::services::entity_reader::FilteredEntities`
- [x] S-210 · `unit:U-016` · `field` · `FilteredEntities.entity_types` · `zep_entity_reader.py:58` → `teri::services::entity_reader::FilteredEntities` (HashSet<String>)
- [x] S-211 · `unit:U-016` · `field` · `FilteredEntities.total_count` · `zep_entity_reader.py:59` → `teri::services::entity_reader::FilteredEntities`
- [x] S-212 · `unit:U-016` · `field` · `FilteredEntities.filtered_count` · `zep_entity_reader.py:60` → `teri::services::entity_reader::FilteredEntities`
- [x] S-213 · `unit:U-016` · `method` · `FilteredEntities.to_dict` · `zep_entity_reader.py:62` → `teri::services::entity_reader::FilteredEntities::to_dict`
- [x] S-214 · `unit:U-016` · `type` · `ZepEntityReader` · `zep_entity_reader.py:71` → `teri::services::entity_reader::KnowledgeGraphEntityReader<'a>` (DECISION-9 Q1: borrowed struct over &KnowledgeGraph; no api_key/client/graph_id — Zep-auth [≠] inexpressible) — **PARITY-VERIFIED 2026-06-17 (opus PASS, self-loop fix re-verified)**
- [≠] S-215 · `unit:U-016` · `method` · `ZepEntityReader.__init__` · `zep_entity_reader.py:81` → `KnowledgeGraphEntityReader::new(&KnowledgeGraph)` (constructor ported; api_key + ZEP_API_KEY + Zep(api_key=…) client construction are [≠] inexpressible — in-process petgraph read has no auth, no client — DECISION-9 Q1)
- [≠] S-216 · `unit:U-016` · `method` · `ZepEntityReader._call_with_retry` · `zep_entity_reader.py:88` → NOT ported as retry ([≠] non-contractual — in-process petgraph read has no I/O, no transient failure; DECISION-9 Q9). The except→None/except→[] fallback contracts ARE ported (contractual) in get_entity_with_context/get_node_edges.
- [x] S-217 · `unit:U-016` · `method` · `ZepEntityReader.get_all_nodes` · paged via fetch_all_nodes · `zep_entity_reader.py:127` → `teri::services::entity_reader::KnowledgeGraphEntityReader::get_all_nodes(&self) -> Vec<Value>` (paging [≠] Zep-artifact; native get_all_entities() iteration. summary="" attributes={} [≠] DECISION-9 Q2) — **PARITY-VERIFIED 2026-06-17 (opus PASS)**
- [x] S-218 · `unit:U-016` · `method` · `ZepEntityReader.get_all_edges` · paged via fetch_all_edges · `zep_entity_reader.py:154` → `teri::services::entity_reader::KnowledgeGraphEntityReader::get_all_edges(&self) -> Vec<Value>` (uuid="" fact="" attributes={} [≠] DECISION-9 Q4; name←kind.to_string() PORT) — **PARITY-VERIFIED 2026-06-17 (opus PASS; get_all_edges iterates edge_references once, no self-loop double-count)**
- [x] S-219 · `unit:U-016` · `method` · `ZepEntityReader.get_node_edges` · uses client.graph.node.get_entity_edges with retry · `zep_entity_reader.py:182` → `teri::services::entity_reader::KnowledgeGraphEntityReader::get_node_edges(&self, node_uuid: &str) -> Vec<Value>` (except→[] ported; retry [≠] non-contractual) — **FAIL→FIX→PASS 2026-06-17 (opus re-verify)**: self-loop double-count FIXED. Guard `edge.source()==*idx` in the Incoming pass of `get_neighbor_relations` (graph/mod.rs:348) skips a self-loop in the incoming pass so it is emitted ONCE as outgoing, matching MiroFish's exclusive if/elif (L288-303). Re-verified: get_node_edges len=1 for a self-loop (was 2). 4-case trace proved the guard fires ONLY for self-loops (normal outgoing/incoming/bidirectional unchanged). except→[] + 6-key shape OK.
- [x] S-220 · `unit:U-016` · `method` · `ZepEntityReader.filter_defined_entities` · filters nodes beyond Entity/Node base labels · `zep_entity_reader.py:215` → `teri::services::entity_reader::KnowledgeGraphEntityReader::filter_defined_entities(&self, defined_entity_types: Option<&[String]>, enrich_with_edges: bool) -> FilteredEntities` (Entity/Node skip ported verbatim — always-pass in teri; counts/entity_types/dedup/direction/bidirectional all verified MATCH) — **FAIL→FIX→PASS 2026-06-17 (opus re-verify)**: enrich self-loop double-count FIXED via the same `get_neighbor_relations` guard. Re-verified: enriched related_edges len=1 for a self-loop, direction="outgoing", target_node_uuid==self (matching MiroFish if/elif L288-303). Test `self_loop_edge_emitted_once_as_outgoing` asserts all three.
- [x] S-221 · `unit:U-016` · `method` · `ZepEntityReader.get_entity_with_context` · returns Optional[EntityNode] · `zep_entity_reader.py:333` → `teri::services::entity_reader::KnowledgeGraphEntityReader::get_entity_with_context(&self, entity_uuid: &str) -> Option<EntityNode>` (except→None ported for bad/missing uuid — VERIFIED; retry [≠]; get_all_nodes re-fetch [≠] replaced by bound graph) — **FAIL→FIX→PASS 2026-06-17 (opus re-verify)**: related_edges self-loop double-count FIXED via the shared guard. Re-verified: ctx.related_edges len=1, direction="outgoing" for a self-loop. except→None contract intact; enrich shape otherwise unchanged.
- [x] S-222 · `unit:U-016` · `method` · `ZepEntityReader.get_entities_by_type` · `zep_entity_reader.py:413` → `teri::services::entity_reader::KnowledgeGraphEntityReader::get_entities_by_type(&self, entity_type: &str, enrich_with_edges: bool) -> Vec<EntityNode>` (1:1 delegation to filter_defined_entities — delegation VERIFIED correct) — **PASS 2026-06-17 (opus re-verify, inherited)**: delegate S-220's self-loop divergence FIXED; no own defect. Delegation unchanged.

---

## U-017 — `backend/app/services/zep_tools.py`

- [ ] S-223 · `unit:U-017` · `type` · `SearchResult` · dataclass: facts,edges,nodes,query,total_count · `zep_tools.py:28`
- [ ] S-224 · `unit:U-017` · `field` · `SearchResult.facts` · `zep_tools.py:30`
- [ ] S-225 · `unit:U-017` · `field` · `SearchResult.edges` · `zep_tools.py:31`
- [ ] S-226 · `unit:U-017` · `field` · `SearchResult.nodes` · `zep_tools.py:32`
- [ ] S-227 · `unit:U-017` · `field` · `SearchResult.query` · `zep_tools.py:33`
- [ ] S-228 · `unit:U-017` · `field` · `SearchResult.total_count` · `zep_tools.py:34`
- [ ] S-229 · `unit:U-017` · `method` · `SearchResult.to_dict` · `zep_tools.py:36`
- [ ] S-230 · `unit:U-017` · `method` · `SearchResult.to_text` · text render for LLM · `zep_tools.py:45`
- [ ] S-231 · `unit:U-017` · `type` · `NodeInfo` · `zep_tools.py:58`
- [ ] S-232 · `unit:U-017` · `field` · `NodeInfo.uuid` · `zep_tools.py:60`
- [ ] S-233 · `unit:U-017` · `field` · `NodeInfo.name` · `zep_tools.py:61`
- [ ] S-234 · `unit:U-017` · `field` · `NodeInfo.labels` · `zep_tools.py:62`
- [ ] S-235 · `unit:U-017` · `field` · `NodeInfo.summary` · `zep_tools.py:63`
- [ ] S-236 · `unit:U-017` · `field` · `NodeInfo.attributes` · `zep_tools.py:64`
- [ ] S-237 · `unit:U-017` · `method` · `NodeInfo.to_dict` · `zep_tools.py:66`
- [ ] S-238 · `unit:U-017` · `method` · `NodeInfo.to_text` · `zep_tools.py:75`
- [ ] S-239 · `unit:U-017` · `type` · `EdgeInfo` · `zep_tools.py:82`
- [ ] S-240 · `unit:U-017` · `field` · `EdgeInfo.uuid` · `zep_tools.py:84`
- [ ] S-241 · `unit:U-017` · `field` · `EdgeInfo.name` · `zep_tools.py:85`
- [ ] S-242 · `unit:U-017` · `field` · `EdgeInfo.fact` · `zep_tools.py:86`
- [ ] S-243 · `unit:U-017` · `field` · `EdgeInfo.source_node_uuid` · `zep_tools.py:87`
- [ ] S-244 · `unit:U-017` · `field` · `EdgeInfo.target_node_uuid` · `zep_tools.py:88`
- [ ] S-245 · `unit:U-017` · `field` · `EdgeInfo.source_node_name` · `zep_tools.py:89`
- [ ] S-246 · `unit:U-017` · `field` · `EdgeInfo.target_node_name` · `zep_tools.py:90`
- [ ] S-247 · `unit:U-017` · `field` · `EdgeInfo.created_at` · `zep_tools.py:92`
- [ ] S-248 · `unit:U-017` · `field` · `EdgeInfo.valid_at` · `zep_tools.py:93`
- [ ] S-249 · `unit:U-017` · `field` · `EdgeInfo.invalid_at` · `zep_tools.py:94`
- [ ] S-250 · `unit:U-017` · `field` · `EdgeInfo.expired_at` · `zep_tools.py:95`
- [ ] S-251 · `unit:U-017` · `method` · `EdgeInfo.to_dict` · `zep_tools.py:97`
- [ ] S-252 · `unit:U-017` · `method` · `EdgeInfo.to_text` · `zep_tools.py:112`
- [ ] S-253 · `unit:U-017` · `method` · `EdgeInfo.is_expired` · `zep_tools.py:128`
- [ ] S-254 · `unit:U-017` · `method` · `EdgeInfo.is_invalid` · `zep_tools.py:133`
- [ ] S-255 · `unit:U-017` · `type` · `InsightForgeResult` · `zep_tools.py:139`
- [ ] S-256 · `unit:U-017` · `field` · `InsightForgeResult.query` · `zep_tools.py:144`
- [ ] S-257 · `unit:U-017` · `field` · `InsightForgeResult.simulation_requirement` · `zep_tools.py:145`
- [ ] S-258 · `unit:U-017` · `field` · `InsightForgeResult.sub_queries` · `zep_tools.py:146`
- [ ] S-259 · `unit:U-017` · `field` · `InsightForgeResult.semantic_facts` · `zep_tools.py:149`
- [ ] S-260 · `unit:U-017` · `field` · `InsightForgeResult.entity_insights` · `zep_tools.py:150`
- [ ] S-261 · `unit:U-017` · `field` · `InsightForgeResult.relationship_chains` · `zep_tools.py:151`
- [ ] S-262 · `unit:U-017` · `field` · `InsightForgeResult.total_facts` · `zep_tools.py:154`
- [ ] S-263 · `unit:U-017` · `field` · `InsightForgeResult.total_entities` · `zep_tools.py:155`
- [ ] S-264 · `unit:U-017` · `field` · `InsightForgeResult.total_relationships` · `zep_tools.py:156`
- [ ] S-265 · `unit:U-017` · `method` · `InsightForgeResult.to_dict` · `zep_tools.py:158`
- [ ] S-266 · `unit:U-017` · `method` · `InsightForgeResult.to_text` · `zep_tools.py:171`
- [ ] S-267 · `unit:U-017` · `type` · `PanoramaResult` · `zep_tools.py:215`
- [ ] S-268 · `unit:U-017` · `field` · `PanoramaResult.query` · `zep_tools.py:220`
- [ ] S-269 · `unit:U-017` · `field` · `PanoramaResult.all_nodes` · `zep_tools.py:223`
- [ ] S-270 · `unit:U-017` · `field` · `PanoramaResult.all_edges` · `zep_tools.py:225`
- [ ] S-271 · `unit:U-017` · `field` · `PanoramaResult.active_facts` · `zep_tools.py:227`
- [ ] S-272 · `unit:U-017` · `field` · `PanoramaResult.historical_facts` · `zep_tools.py:229`
- [ ] S-273 · `unit:U-017` · `field` · `PanoramaResult.total_nodes` · `zep_tools.py:232`
- [ ] S-274 · `unit:U-017` · `field` · `PanoramaResult.total_edges` · `zep_tools.py:233`
- [ ] S-275 · `unit:U-017` · `field` · `PanoramaResult.active_count` · `zep_tools.py:234`
- [ ] S-276 · `unit:U-017` · `field` · `PanoramaResult.historical_count` · `zep_tools.py:235`
- [ ] S-277 · `unit:U-017` · `method` · `PanoramaResult.to_dict` · `zep_tools.py:237`
- [ ] S-278 · `unit:U-017` · `method` · `PanoramaResult.to_text` · `zep_tools.py:250`
- [ ] S-279 · `unit:U-017` · `type` · `AgentInterview` · `zep_tools.py:285`
- [ ] S-280 · `unit:U-017` · `field` · `AgentInterview.agent_name` · `zep_tools.py:287`
- [ ] S-281 · `unit:U-017` · `field` · `AgentInterview.agent_role` · `zep_tools.py:288`
- [ ] S-282 · `unit:U-017` · `field` · `AgentInterview.agent_bio` · `zep_tools.py:289`
- [ ] S-283 · `unit:U-017` · `field` · `AgentInterview.question` · `zep_tools.py:290`
- [ ] S-284 · `unit:U-017` · `field` · `AgentInterview.response` · `zep_tools.py:291`
- [ ] S-285 · `unit:U-017` · `field` · `AgentInterview.key_quotes` · `zep_tools.py:292`
- [ ] S-286 · `unit:U-017` · `method` · `AgentInterview.to_dict` · `zep_tools.py:294`
- [ ] S-287 · `unit:U-017` · `method` · `AgentInterview.to_text` · `zep_tools.py:304`
- [ ] S-288 · `unit:U-017` · `type` · `InterviewResult` · `zep_tools.py:341`
- [ ] S-289 · `unit:U-017` · `field` · `InterviewResult.interview_topic` · `zep_tools.py:346`
- [ ] S-290 · `unit:U-017` · `field` · `InterviewResult.interview_questions` · `zep_tools.py:347`
- [ ] S-291 · `unit:U-017` · `field` · `InterviewResult.selected_agents` · `zep_tools.py:350`
- [ ] S-292 · `unit:U-017` · `field` · `InterviewResult.interviews` · `zep_tools.py:352`
- [ ] S-293 · `unit:U-017` · `field` · `InterviewResult.selection_reasoning` · `zep_tools.py:355`
- [ ] S-294 · `unit:U-017` · `field` · `InterviewResult.summary` · `zep_tools.py:357`
- [ ] S-295 · `unit:U-017` · `field` · `InterviewResult.total_agents` · `zep_tools.py:360`
- [ ] S-296 · `unit:U-017` · `field` · `InterviewResult.interviewed_count` · `zep_tools.py:361`
- [ ] S-297 · `unit:U-017` · `method` · `InterviewResult.to_dict` · `zep_tools.py:363`
- [ ] S-298 · `unit:U-017` · `method` · `InterviewResult.to_text` · `zep_tools.py:375`
- [ ] S-299 · `unit:U-017` · `type` · `ZepToolsService` · high-level Zep retrieval tools for ReportAgent · `zep_tools.py:401`
- [ ] S-300 · `unit:U-017` · `field` · `ZepToolsService.MAX_RETRIES` · 3 · `zep_tools.py:422`
- [ ] S-301 · `unit:U-017` · `field` · `ZepToolsService.RETRY_DELAY` · 2.0s · `zep_tools.py:423`
- [ ] S-302 · `unit:U-017` · `method` · `ZepToolsService.__init__` · `zep_tools.py:425`
- [ ] S-303 · `unit:U-017` · `method` · `ZepToolsService.llm` · lazy property → LLMClient · `zep_tools.py:436`
- [ ] S-304 · `unit:U-017` · `method` · `ZepToolsService._call_with_retry` · `zep_tools.py:442`
- [ ] S-305 · `unit:U-017` · `method` · `ZepToolsService.search_graph` · Zep hybrid search + cross-encoder, falls back to _local_search · `zep_tools.py:464`
- [ ] S-306 · `unit:U-017` · `method` · `ZepToolsService._local_search` · keyword-scored fallback · `zep_tools.py:546`
- [ ] S-307 · `unit:U-017` · `method` · `ZepToolsService.get_all_nodes` · `zep_tools.py:650`
- [ ] S-308 · `unit:U-017` · `method` · `ZepToolsService.get_all_edges` · `zep_tools.py:678`
- [ ] S-309 · `unit:U-017` · `method` · `ZepToolsService.get_node_detail` · `zep_tools.py:716`
- [ ] S-310 · `unit:U-017` · `method` · `ZepToolsService.get_node_edges` · `zep_tools.py:748`
- [ ] S-311 · `unit:U-017` · `method` · `ZepToolsService.get_entities_by_type` · `zep_tools.py:780`
- [ ] S-312 · `unit:U-017` · `method` · `ZepToolsService.get_entity_summary` · `zep_tools.py:808`
- [ ] S-313 · `unit:U-017` · `method` · `ZepToolsService.get_graph_statistics` · `zep_tools.py:855`
- [ ] S-314 · `unit:U-017` · `method` · `ZepToolsService.get_simulation_context` · `zep_tools.py:890`
- [ ] S-315 · `unit:U-017` · `method` · `ZepToolsService.insight_forge` · LLM sub-queries → semantic search → InsightForgeResult · `zep_tools.py:945`
- [ ] S-316 · `unit:U-017` · `method` · `ZepToolsService._generate_sub_queries` · `zep_tools.py:1092`
- [ ] S-317 · `unit:U-017` · `method` · `ZepToolsService.panorama_search` · all nodes+edges classified active vs historical · `zep_tools.py:1145`
- [ ] S-318 · `unit:U-017` · `method` · `ZepToolsService.quick_search` · `zep_tools.py:1237`
- [ ] S-319 · `unit:U-017` · `method` · `ZepToolsService.interview_agents` · loads profiles, LLM selects agents, generates questions, calls SimulationRunner.interview_agents_batch · `zep_tools.py:1272`
- [ ] S-320 · `unit:U-017` · `method` · `ZepToolsService._clean_tool_call_response` · `zep_tools.py:1485`
- [ ] S-321 · `unit:U-017` · `method` · `ZepToolsService._load_agent_profiles` · reads reddit JSON or twitter CSV · `zep_tools.py:1505`
- [ ] S-322 · `unit:U-017` · `method` · `ZepToolsService._select_agents_for_interview` · LLM selects agents · `zep_tools.py:1551`
- [ ] S-323 · `unit:U-017` · `method` · `ZepToolsService._generate_interview_questions` · `zep_tools.py:1634`
- [ ] S-324 · `unit:U-017` · `method` · `ZepToolsService._generate_interview_summary` · `zep_tools.py:1683`

---

## U-018 — `backend/app/services/oasis_profile_generator.py`

<!-- cycle-8 FIX: to_reddit_format / to_twitter_format / to_dict ported on Persona; bio+persona de-narrowed as distinct SocialProfile fields; user_id added; 7 sim/mod.rs cascade fixed; 310 tests pass -->

- [x] S-325 · `unit:U-018` · `type` · `OasisAgentProfile` · → `SocialProfile` (src/agent/mod.rs) + `Platform` enum nested in `Persona.social: Option<SocialProfile>` — now includes `user_id`, `bio`, `persona` as distinct fields (de-narrowed from the prior collapse into `Persona.background`)
- [x] S-326 · `unit:U-018` · `field` · `OasisAgentProfile.user_id` · → `SocialProfile.user_id: u64` (src/agent/mod.rs) — used by to_reddit/twitter/to_dict serializers; defaults to 0, callers set to OASIS numeric id at export time
- [x] S-327 · `unit:U-018` · `field` · `OasisAgentProfile.user_name` · → `SocialProfile.user_name: String` (src/agent/mod.rs)
- [≠] S-328 · `unit:U-018` · `field` · `OasisAgentProfile.name` · REUSE: `Persona.name` (already exists, not duplicated in SocialProfile)
- [x] S-329 · `unit:U-018` · `field` · `OasisAgentProfile.bio` · → `SocialProfile.bio: String` (src/agent/mod.rs) — distinct from `persona`; short public bio serialized as `"bio"` key
- [x] S-330 · `unit:U-018` · `field` · `OasisAgentProfile.persona` · → `SocialProfile.persona: String` (src/agent/mod.rs) — distinct from `bio`; detailed LLM-context description serialized as `"persona"` key
- [x] S-331 · `unit:U-018` · `field` · `OasisAgentProfile.karma` · → `SocialProfile.karma: i64` default=1000 (src/agent/mod.rs)
- [x] S-332 · `unit:U-018` · `field` · `OasisAgentProfile.friend_count` · → `SocialProfile.friend_count: i64` default=100 (src/agent/mod.rs)
- [x] S-333 · `unit:U-018` · `field` · `OasisAgentProfile.follower_count` · → `SocialProfile.follower_count: i64` default=150 (src/agent/mod.rs)
- [x] S-334 · `unit:U-018` · `field` · `OasisAgentProfile.statuses_count` · → `SocialProfile.statuses_count: i64` default=500 (src/agent/mod.rs)
- [x] S-335 · `unit:U-018` · `field` · `OasisAgentProfile.age` · → `SocialProfile.age: Option<u32>` (src/agent/mod.rs)
- [x] S-336 · `unit:U-018` · `field` · `OasisAgentProfile.gender` · → `SocialProfile.gender: Option<String>` (src/agent/mod.rs)
- [x] S-337 · `unit:U-018` · `field` · `OasisAgentProfile.mbti` · → `SocialProfile.mbti: Option<String>` (src/agent/mod.rs)
- [x] S-338 · `unit:U-018` · `field` · `OasisAgentProfile.country` · → `SocialProfile.country: Option<String>` (src/agent/mod.rs)
- [x] S-339 · `unit:U-018` · `field` · `OasisAgentProfile.profession` · → `SocialProfile.profession: Option<String>` (src/agent/mod.rs)
- [x] S-340 · `unit:U-018` · `field` · `OasisAgentProfile.interested_topics` · → `SocialProfile.interested_topics: Vec<String>` (src/agent/mod.rs)
- [x] S-341 · `unit:U-018` · `field` · `OasisAgentProfile.source_entity_uuid` · → `SocialProfile.source_entity_uuid: Option<String>` (src/agent/mod.rs)
- [x] S-342 · `unit:U-018` · `field` · `OasisAgentProfile.source_entity_type` · → `SocialProfile.source_entity_type: Option<String>` (src/agent/mod.rs)
- [x] S-343 · `unit:U-018` · `field` · `OasisAgentProfile.created_at` · → `SocialProfile.created_at: String` (src/agent/mod.rs)
- [x] S-344 · `unit:U-018` · `method` · `OasisAgentProfile.to_reddit_format` · → `Persona::to_reddit_format(&self) -> Option<serde_json::Value>` (src/agent/mod.rs) — exact key map: `user_id`, `username` (no underscore — OASIS lib requirement), `name`, `bio`, `persona`, `karma`, `created_at`; conditional demographics mirror Python falsy guards (`age>0`, non-empty string, non-empty vec); returns None when social=None
- [x] S-345 · `unit:U-018` · `method` · `OasisAgentProfile.to_twitter_format` · → `Persona::to_twitter_format(&self) -> Option<serde_json::Value>` (src/agent/mod.rs) — exact key map: `user_id`, `username` (no underscore), `name`, `bio`, `persona`, `friend_count`, `follower_count`, `statuses_count`, `created_at`; NO `karma`; same conditional demographics; returns None when social=None
- [x] S-346 · `unit:U-018` · `method` · `OasisAgentProfile.to_dict` · → `Persona::to_dict(&self) -> Option<serde_json::Value>` (src/agent/mod.rs) — all fields unconditionally; uses `user_name` (with underscore) unlike platform formats; null for None optionals; returns None when social=None
- [x] S-347 · `unit:U-018` · `type` · `OasisProfileGenerator` · → `PersonaGenerator` extended with `generate_social<L>()` + `generate_social_rule_based()` + `generate_username()` (src/agent/mod.rs:~750-990)
- [≠] S-348 · `unit:U-018` · `field` · `OasisProfileGenerator.MBTI_TYPES` · INTENTIONAL DIVERGENCE: MBTI type list embedded in rule-based prompt strings; no global const array needed in native Rust (LLM picks MBTI freely; rule-based uses named values inline)
- [≠] S-349 · `unit:U-018` · `field` · `OasisProfileGenerator.COUNTRIES` · INTENTIONAL DIVERGENCE: same — embedded inline in rule-based fallback; no global const needed
- [≠] S-350 · `unit:U-018` · `field` · `OasisProfileGenerator.INDIVIDUAL_ENTITY_TYPES` · INTENTIONAL DIVERGENCE: embedded as match arms in `generate_social_rule_based` (src/agent/mod.rs:~920-960)
- [≠] S-351 · `unit:U-018` · `field` · `OasisProfileGenerator.GROUP_ENTITY_TYPES` · INTENTIONAL DIVERGENCE: same — embedded as match arms
- [≠] S-352 · `unit:U-018` · `method` · `OasisProfileGenerator.__init__` · REUSE: `PersonaGenerator::new()` already handles init; no separate LLM client field needed (LLM passed per-call)
- [x] S-353 · `unit:U-018` · `method` · `OasisProfileGenerator.generate_profile_from_entity` · → `PersonaGenerator::generate_social<L>()` (src/agent/mod.rs) — LLM path + rule-based fallback; entity context via entity_name/entity_type/entity_summary args
- [x] S-354 · `unit:U-018` · `method` · `OasisProfileGenerator._generate_username` · → `PersonaGenerator::generate_username()` (src/agent/mod.rs) — deterministic hash-derived suffix instead of random (parity: both produce alphanumeric handle with numeric suffix)
- [≠] S-355 · `unit:U-018` · `method` · `OasisProfileGenerator._search_zep_for_entity` · **RE-AUDIT 2026-06-14: KEEP-[≠] sub-rule (b).** Pure Zep-SaaS hybrid search (`zep_client.graph.search` scope=edges/nodes, rrf reranker, parallel ThreadPool, 30s timeout) — server-side machinery with no in-process analogue. NOTE: this is the Zep-SEARCH half only; the IN-PROCESS enrichment (related_edges/related_nodes neighbor context) is S-356, which IS portable and is now port-now.
- [x] S-356 · `unit:U-018` · `method` · `OasisProfileGenerator._build_entity_context` · **PARITY-VERIFIED 2026-06-17.** Part 2 (related_edges → `### Related Facts and Relationships`) now ported: `KnowledgeGraph::get_neighbor_relations` + directional `_relation_line` (outgoing `{e} --[Kind]--> ({n})` / incoming `({n}) --[Kind]--> {e}`, matches MiroFish :443-451 exactly; teri substitutes real neighbor name = strict superset over Python's `(相关实体)` placeholder). Fact-branch (`- {fact}`) adjudicated **(a) faithful**: MiroFish edge `fact` is Zep-server-derived (`zep_entity_reader.get_node_edges` → `client.graph.node.get_entity_edges` → `edge.fact`), no in-process analogue; teri `Relation{kind,weight,valid_at}`/`Entity{id,name,kind}` carry no fact text → nothing to drop. Inert commented fact-branch is a documented S-355-class `[≠]` boundary, NOT a skip. Empty→no-section fallback preserved. 2 direction tests pass (verifier-rerun). **PORTED 2026-06-14 (ITERATE cycle).** → `PersonaGenerator::build_entity_context(graph: &KnowledgeGraph, entity: &Entity) -> String` (src/agent/mod.rs) + `generate_social` extended with `graph_ctx: Option<(&KnowledgeGraph, &Entity)>` param. In-process parts 1+2+3 ported. Zep-search half (part 4) stays `[≠]` (S-355).
- [≠] S-357 · `unit:U-018` · `method` · `OasisProfileGenerator._is_individual_entity` · INTENTIONAL DIVERGENCE: entity-type classification embedded as match arms in rule-based fallback
- [≠] S-358 · `unit:U-018` · `method` · `OasisProfileGenerator._is_group_entity` · INTENTIONAL DIVERGENCE: same
- [x] S-359 · `unit:U-018` · `method` · `OasisProfileGenerator._generate_profile_with_llm` · → LLM path in `generate_social` (src/agent/mod.rs) — retry not ported (teri LLM trait handles retries at adapter level)
- [x] S-360 · `unit:U-018` · `method` · `OasisProfileGenerator._fix_truncated_json` · **PORTED 2026-06-14 (ITERATE cycle).** → `PersonaGenerator::fix_truncated_json(content: &str) -> String` (src/agent/mod.rs). Exact strategy: strip, count unbalanced `{`/`[`, append `"` if last char is not `",}]`, close brackets then braces. Wired into `generate_social` parse path (first repair step inside salvage chain). Tests: closes-open-brace, closes-dangling-string+brace, closes-array+brace, valid-input-unchanged. **PARITY-VERIFIED 2026-06-17** (opus gate, resume): exact char-set match on dangling-string close (`'",}]'`), brace/bracket counting, close-order, `.max(0)` over-close guards.
- [x] S-361 · `unit:U-018` · `method` · `OasisProfileGenerator._try_fix_json` · **PORTED 2026-06-14 (ITERATE cycle).** → `PersonaGenerator::try_fix_json(content: &str, entity_name, entity_type, entity_summary) -> Option<Value>` (src/agent/mod.rs). Full 7-step salvage chain: (1) fix_truncated_json, (2) extract `{…}` via brace-depth scan, (3) normalize newlines inside JSON strings, (4) parse, (5) strip control chars + collapse whitespace + retry, (6) field-level bio/persona regex extraction (mirrors Python `bio_match`/`persona_match` guards; returns None if neither matches), (7) complete-failure→None. Wired into `generate_social` as the second salvage step after direct parse fails. Also provides helper methods: `extract_json_object`, `normalize_json_string_newlines`, `strip_control_chars`, `extract_json_string_field`, `extract_json_string_field_partial`. Tests: salvage-truncated-bio-persona, string-truncation-with-all-fields, garbage→None, field-extraction-from-broken-JSON, generate_social-salvage-path-taken (UNIQUE_LLM_SIGNATURE in bio/persona proves salvage vs rule-based), genuine-garbage→rule-based. **PARITY-VERIFIED 2026-06-17** (opus gate, resume): all 7 steps faithful incl. bio (closed-quote) vs persona (open/partial) regex asymmetry + `bio_match or persona_match` guard; two non-downgrading divergences (stricter `{…}` first-balanced-`}` stop; None→rule-based reproduces Python step-7 base dict).
- [≠] S-362 · `unit:U-018` · `method` · `OasisProfileGenerator._get_system_prompt` · INTENTIONAL DIVERGENCE: system-prompt logic folded into `generate_social` prompt string; no separate method needed
- [≠] S-363 · `unit:U-018` · `method` · `OasisProfileGenerator._build_individual_persona_prompt` · INTENTIONAL DIVERGENCE: prompt template is unified in `generate_social`; individual vs group distinction handled by entity_type arg to rule-based fallback
- [≠] S-364 · `unit:U-018` · `method` · `OasisProfileGenerator._build_group_persona_prompt` · INTENTIONAL DIVERGENCE: same as S-363
- [x] S-365 · `unit:U-018` · `method` · `OasisProfileGenerator._generate_profile_rule_based` · → `PersonaGenerator::generate_social_rule_based()` (src/agent/mod.rs) — all entity-type branches ported (student/alumni, expert/faculty, university/org/ngo/media, default)
- [≠] S-366 · `unit:U-018` · `method` · `OasisProfileGenerator.set_graph_id` · INTENTIONAL DIVERGENCE: Zep graph_id not applicable; teri uses KnowledgeGraph passed by reference per call
- [x] S-367 · `unit:U-018` · `method` · `OasisProfileGenerator.generate_profiles_from_entities` · **RE-VERIFIED [x] by opus gate 2026-06-17 (live concurrency, determinism proven byte-for-byte across parallel_count ∈ {1,3,10}).** RE-OPENED by DECISION-11 (2026-06-17): previously PARITY-VERIFIED sequential; upgraded to live bounded concurrency via `futures::stream::iter(...).buffer_unordered(parallel_count.max(1))` per DECISION-11 §2 (Python `ThreadPoolExecutor(max_workers=parallel_count)` + `as_completed`). `_parallel_count: usize` renamed to `parallel_count: usize` (live knob). Consumer loop: pre-allocated indexed Vec (order-preserving), realtime-save per completion, monotonic 1-based progress callback. Final Vec + files deterministic regardless of parallel_count (only wall-clock/intermediate-realtime ordering differ — non-contractual). 3 new determinism tests added (`generate_profiles_determinism_across_parallel_counts`, `generate_profiles_final_file_bytes_deterministic`). Re-parity-verify required (opus gate).
- [≠] S-368 · `unit:U-018` · `method` · `OasisProfileGenerator._print_generated_profile` · **LEGITIMATELY STAYS [≠] per DECISION-10 §2.** Console pretty-print (`【简介】`/`【详细人设】`/`【基本属性】` blocks) is a non-contractual stdout debug aid; no API/get_profiles/SimEngine consumer depends on it. Progress is carried by `progress_callback` + the already-ported `progress.profileGenerated` i18n key. Genuinely non-contractual → legal `[≠]`.
- [x] S-369 · `unit:U-018` · `method` · `OasisProfileGenerator.save_profiles` · **PARITY-VERIFIED 2026-06-17 (opus gate).** dispatch Twitter→csv/Reddit→json confirmed vs py:1065-1068. **UN-[≠]'d DECISION-10 (contractual file output).** → `save_profiles(profiles: &[(SocialProfile,String)], file_path: &Path, platform: OutputPlatform) -> io::Result<()>` (src/services/oasis_profile_export.rs) — platform-dispatch: Twitter→save_twitter_csv, Reddit→save_reddit_json. Parity: dispatch tests pass (2026-06-17).
- [x] S-370 · `unit:U-018` · `method` · `OasisProfileGenerator._save_twitter_csv` · **PARITY-VERIFIED 2026-06-17 (opus gate).** header/row-index/user_char/description/.csv-swap confirmed vs py:1070-1119; CSV quoting differential = field-level quoting/escaping BYTE-IDENTICAL (comma-quoted, `"`→`""`, tab/semicolon unquoted). Line terminator CRLF(py)/LF(rust) divergence found → adjudicated NON-CONTRACTUAL (read path csv.DictReader text-mode universal-newlines normalizes; API parsed rows identical). Doc-comment "matches exactly" over-stated → porter cleanup (non-blocking). **UN-[≠]'d DECISION-10 (OASIS CSV column contract).** → `save_twitter_csv(profiles, file_path) -> io::Result<()>` (src/services/oasis_profile_export.rs). EXACT header: `['user_id','name','username','user_char','description']`; user_id = ROW INDEX (0-based, not profile.user_id); user_char = bio when persona==bio else "{bio} {persona}" with \n/\r→space; description = bio with \n/\r→space. .csv extension enforced (replaces .json). csv crate QUOTE_MINIMAL matches Python csv.writer default. Parity: 7 dedicated CSV tests pass (2026-06-17).
- [x] S-371 · `unit:U-018` · `method` · `OasisProfileGenerator._normalize_gender` · **PARITY-VERIFIED 2026-06-17 (opus gate).** exact map verified vs py:1121-1144 (男→male,女→female,机构/其他→other, en passthrough, lower().strip() mirrored, default→other); 11 tests. **RE-FLAG FIRES → PORTED (dependency on _save_reddit_json which is now ported per DECISION-10).** → `normalize_gender(gender: Option<&str>) -> &'static str` (src/services/oasis_profile_export.rs). EXACT map: 男→male, 女→female, 机构→other, 其他→other; English male/female/other passthrough; None/empty/whitespace-only/anything-else→other; Python `gender.lower().strip()` mirrored. Parity: 11 dedicated normalize_gender tests pass (2026-06-17).
- [x] S-372 · `unit:U-018` · `method` · `OasisProfileGenerator._save_reddit_json` · **PARITY-VERIFIED 2026-06-17 (opus gate).** ALL forced defaults confirmed vs py:1146-1193 (age=30 UNCONDITIONAL, gender ALWAYS normalized, mbti=ISTJ, country=中国, karma=1000, bio[:150] CHAR-truncate-or-name, persona fallback); does NOT route through to_reddit_format (no downgrade); key order BYTE-IDENTICAL (preserve_order feature, LOAD-BEARING); UTF-8 raw + indent=2 byte-identical to Python json.dump. **UN-[≠]'d DECISION-10 (OASIS JSON contract with forced defaults — NOT to_reddit_format).** → `save_reddit_json(profiles: &[(SocialProfile,String)], file_path: &Path) -> io::Result<()>` (src/services/oasis_profile_export.rs). FORCED OASIS defaults: age=profile.age.filter(>0).unwrap_or(30); gender=normalize_gender (ALWAYS present); mbti=profile.mbti.unwrap_or("ISTJ"); country=profile.country.unwrap_or("中国"); bio=bio.chars().take(150) or name; persona=persona or "{name} is a participant in social discussions."; karma=karma or 1000; user_id=profile.user_id (u64, always set). Optional profession/interested_topics only when truthy. JSON: serde_json_pretty=ensure_ascii=False UTF-8. Parity: 7 reddit tests pass (2026-06-17).
- [x] S-373 · `unit:U-018` · `method` · `OasisProfileGenerator.save_profiles_to_json` · **PARITY-VERIFIED 2026-06-17 (opus gate).** thin alias: warn!(deprecated) then delegate to save_profiles, confirmed vs py:1196-1205. **UN-[≠]'d DECISION-10 (deprecated alias — ported as thin delegating fn).** → `save_profiles_to_json(profiles, file_path, platform) -> io::Result<()>` (src/services/oasis_profile_export.rs) — emits tracing::warn!("save_profiles_to_json is deprecated; use save_profiles instead") then delegates to save_profiles. Parity: 1 alias test passes (2026-06-17).

---

## U-019 — `backend/app/services/simulation_config_generator.py`

- [x] S-374 · `unit:U-019` · `const` · `CHINA_TIMEZONE_CONFIG` · timezone constant dict · `simulation_config_generator.py:29` → `teri::services::simulation_config::china_timezone_config` (fn)
- [x] S-375 · `unit:U-019` · `type` · `AgentActivityConfig` · dataclass · `simulation_config_generator.py:52` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-376 · `unit:U-019` · `field` · `AgentActivityConfig.agent_id` · `simulation_config_generator.py:54` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-377 · `unit:U-019` · `field` · `AgentActivityConfig.entity_uuid` · `simulation_config_generator.py:55` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-378 · `unit:U-019` · `field` · `AgentActivityConfig.entity_name` · `simulation_config_generator.py:56` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-379 · `unit:U-019` · `field` · `AgentActivityConfig.entity_type` · `simulation_config_generator.py:57` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-380 · `unit:U-019` · `field` · `AgentActivityConfig.activity_level` · `simulation_config_generator.py:60` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-381 · `unit:U-019` · `field` · `AgentActivityConfig.posts_per_hour` · `simulation_config_generator.py:63` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-382 · `unit:U-019` · `field` · `AgentActivityConfig.comments_per_hour` · `simulation_config_generator.py:64` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-383 · `unit:U-019` · `field` · `AgentActivityConfig.active_hours` · `simulation_config_generator.py:67` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-384 · `unit:U-019` · `field` · `AgentActivityConfig.response_delay_min` · `simulation_config_generator.py:70` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-385 · `unit:U-019` · `field` · `AgentActivityConfig.response_delay_max` · `simulation_config_generator.py:71` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-386 · `unit:U-019` · `field` · `AgentActivityConfig.sentiment_bias` · `simulation_config_generator.py:74` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-387 · `unit:U-019` · `field` · `AgentActivityConfig.stance` · `simulation_config_generator.py:77` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-388 · `unit:U-019` · `field` · `AgentActivityConfig.influence_weight` · `simulation_config_generator.py:80` → `teri::services::simulation_config::AgentActivityConfig`
- [x] S-389 · `unit:U-019` · `type` · `TimeSimulationConfig` · dataclass · `simulation_config_generator.py:84` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-390 · `unit:U-019` · `field` · `TimeSimulationConfig.total_simulation_hours` · `simulation_config_generator.py:87` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-391 · `unit:U-019` · `field` · `TimeSimulationConfig.minutes_per_round` · `simulation_config_generator.py:90` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-392 · `unit:U-019` · `field` · `TimeSimulationConfig.agents_per_hour_min` · `simulation_config_generator.py:93` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-393 · `unit:U-019` · `field` · `TimeSimulationConfig.agents_per_hour_max` · `simulation_config_generator.py:94` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-394 · `unit:U-019` · `field` · `TimeSimulationConfig.peak_hours` · `simulation_config_generator.py:97` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-395 · `unit:U-019` · `field` · `TimeSimulationConfig.peak_activity_multiplier` · `simulation_config_generator.py:98` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-396 · `unit:U-019` · `field` · `TimeSimulationConfig.off_peak_hours` · `simulation_config_generator.py:101` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-397 · `unit:U-019` · `field` · `TimeSimulationConfig.off_peak_activity_multiplier` · `simulation_config_generator.py:102` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-398 · `unit:U-019` · `field` · `TimeSimulationConfig.morning_hours` · `simulation_config_generator.py:105` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-399 · `unit:U-019` · `field` · `TimeSimulationConfig.morning_activity_multiplier` · `simulation_config_generator.py:106` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-400 · `unit:U-019` · `field` · `TimeSimulationConfig.work_hours` · `simulation_config_generator.py:109` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-401 · `unit:U-019` · `field` · `TimeSimulationConfig.work_activity_multiplier` · `simulation_config_generator.py:110` → `teri::services::simulation_config::TimeSimulationConfig`
- [x] S-402 · `unit:U-019` · `type` · `EventConfig` · dataclass · `simulation_config_generator.py:114` → `teri::services::simulation_config::EventConfig`
- [x] S-403 · `unit:U-019` · `field` · `EventConfig.initial_posts` · `simulation_config_generator.py:117` → `teri::services::simulation_config::EventConfig`
- [x] S-404 · `unit:U-019` · `field` · `EventConfig.scheduled_events` · `simulation_config_generator.py:120` → `teri::services::simulation_config::EventConfig`
- [x] S-405 · `unit:U-019` · `field` · `EventConfig.hot_topics` · `simulation_config_generator.py:123` → `teri::services::simulation_config::EventConfig`
- [x] S-406 · `unit:U-019` · `field` · `EventConfig.narrative_direction` · `simulation_config_generator.py:126` → `teri::services::simulation_config::EventConfig`
- [x] S-407 · `unit:U-019` · `type` · `PlatformConfig` · dataclass · `simulation_config_generator.py:130` → `teri::services::simulation_config::PlatformConfig`
- [x] S-408 · `unit:U-019` · `field` · `PlatformConfig.platform` · `simulation_config_generator.py:132` → `teri::services::simulation_config::PlatformConfig`
- [x] S-409 · `unit:U-019` · `field` · `PlatformConfig.recency_weight` · `simulation_config_generator.py:135` → `teri::services::simulation_config::PlatformConfig`
- [x] S-410 · `unit:U-019` · `field` · `PlatformConfig.popularity_weight` · `simulation_config_generator.py:136` → `teri::services::simulation_config::PlatformConfig`
- [x] S-411 · `unit:U-019` · `field` · `PlatformConfig.relevance_weight` · `simulation_config_generator.py:137` → `teri::services::simulation_config::PlatformConfig`
- [x] S-412 · `unit:U-019` · `field` · `PlatformConfig.viral_threshold` · `simulation_config_generator.py:140` → `teri::services::simulation_config::PlatformConfig`
- [x] S-413 · `unit:U-019` · `field` · `PlatformConfig.echo_chamber_strength` · `simulation_config_generator.py:143` → `teri::services::simulation_config::PlatformConfig`
- [x] S-414 · `unit:U-019` · `type` · `SimulationParameters` · master config dataclass · `simulation_config_generator.py:147` → `teri::services::simulation_config::SimulationParameters`
- [x] S-415 · `unit:U-019` · `field` · `SimulationParameters.simulation_id` · `simulation_config_generator.py:150` → `teri::services::simulation_config::SimulationParameters`
- [x] S-416 · `unit:U-019` · `field` · `SimulationParameters.project_id` · `simulation_config_generator.py:151` → `teri::services::simulation_config::SimulationParameters`
- [x] S-417 · `unit:U-019` · `field` · `SimulationParameters.graph_id` · `simulation_config_generator.py:152` → `teri::services::simulation_config::SimulationParameters`
- [x] S-418 · `unit:U-019` · `field` · `SimulationParameters.simulation_requirement` · `simulation_config_generator.py:153` → `teri::services::simulation_config::SimulationParameters`
- [x] S-419 · `unit:U-019` · `field` · `SimulationParameters.time_config` · `simulation_config_generator.py:156` → `teri::services::simulation_config::SimulationParameters`
- [x] S-420 · `unit:U-019` · `field` · `SimulationParameters.agent_configs` · `simulation_config_generator.py:159` → `teri::services::simulation_config::SimulationParameters`
- [x] S-421 · `unit:U-019` · `field` · `SimulationParameters.event_config` · `simulation_config_generator.py:162` → `teri::services::simulation_config::SimulationParameters`
- [x] S-422 · `unit:U-019` · `field` · `SimulationParameters.twitter_config` · `simulation_config_generator.py:165` → `teri::services::simulation_config::SimulationParameters`
- [x] S-423 · `unit:U-019` · `field` · `SimulationParameters.reddit_config` · `simulation_config_generator.py:166` → `teri::services::simulation_config::SimulationParameters`
- [x] S-424 · `unit:U-019` · `field` · `SimulationParameters.llm_model` · `simulation_config_generator.py:169` → `teri::services::simulation_config::SimulationParameters`
- [x] S-425 · `unit:U-019` · `field` · `SimulationParameters.llm_base_url` · `simulation_config_generator.py:170` → `teri::services::simulation_config::SimulationParameters`
- [x] S-426 · `unit:U-019` · `field` · `SimulationParameters.generated_at` · `simulation_config_generator.py:173` → `teri::services::simulation_config::SimulationParameters`
- [x] S-427 · `unit:U-019` · `field` · `SimulationParameters.generation_reasoning` · `simulation_config_generator.py:174` → `teri::services::simulation_config::SimulationParameters`
- [x] S-428 · `unit:U-019` · `method` · `SimulationParameters.to_dict` · `simulation_config_generator.py:176` → `teri::services::simulation_config::SimulationParameters::to_dict`
- [x] S-429 · `unit:U-019` · `method` · `SimulationParameters.to_json` · `simulation_config_generator.py:195` → `teri::services::simulation_config::SimulationParameters::to_json`
- [x] S-430 · `unit:U-019` · `type` · `SimulationConfigGenerator` · `simulation_config_generator.py:200` → `teri::services::simulation_config::SimulationConfigGenerator`
- [x] S-431 · `unit:U-019` · `field` · `SimulationConfigGenerator.MAX_CONTEXT_LENGTH` · `simulation_config_generator.py:214` → `SimulationConfigGenerator::MAX_CONTEXT_LENGTH` (50_000)
- [x] S-432 · `unit:U-019` · `field` · `SimulationConfigGenerator.AGENTS_PER_BATCH` · `simulation_config_generator.py:216` → `SimulationConfigGenerator::AGENTS_PER_BATCH` (15)
- [x] S-433 · `unit:U-019` · `field` · `SimulationConfigGenerator.TIME_CONFIG_CONTEXT_LENGTH` · `simulation_config_generator.py:219` → `SimulationConfigGenerator::TIME_CONFIG_CONTEXT_LENGTH` (10_000)
- [x] S-434 · `unit:U-019` · `field` · `SimulationConfigGenerator.EVENT_CONFIG_CONTEXT_LENGTH` · `simulation_config_generator.py:220` → `SimulationConfigGenerator::EVENT_CONFIG_CONTEXT_LENGTH` (8_000)
- [x] S-435 · `unit:U-019` · `field` · `SimulationConfigGenerator.ENTITY_SUMMARY_LENGTH` · `simulation_config_generator.py:221` → `SimulationConfigGenerator::ENTITY_SUMMARY_LENGTH` (300)
- [x] S-436 · `unit:U-019` · `field` · `SimulationConfigGenerator.AGENT_SUMMARY_LENGTH` · `simulation_config_generator.py:222` → `SimulationConfigGenerator::AGENT_SUMMARY_LENGTH` (300)
- [x] S-437 · `unit:U-019` · `field` · `SimulationConfigGenerator.ENTITIES_PER_TYPE_DISPLAY` · `simulation_config_generator.py:223` → `SimulationConfigGenerator::ENTITIES_PER_TYPE_DISPLAY` (20)
- [x] S-438 · `unit:U-019` · `method` · `SimulationConfigGenerator.__init__` · `simulation_config_generator.py:225` → `SimulationConfigGenerator::new` (LlmClient injected; model_name + base_url explicit)
- [x] S-439 · `unit:U-019` · `method` · `SimulationConfigGenerator.generate_config` · 4-stage: context→time→event→agent_configs · `simulation_config_generator.py:243` → `teri::services::simulation_config::SimulationConfigGenerator::generate_config` · `src/services/simulation_config.rs` · coverage: total_steps=3+ceil(N/15); step sequence 1/2/3..N/total_steps; progress_callback (step,total,msg) at each stage; time-config reasoning-or-success fallback; event-config reasoning-or-success fallback; agent-config batch loop (all N entities); assign_initial_post_agents + assigned_count; twitter literals (recency=0.4,popularity=0.3,viral=10,echo=0.5); reddit NON-default literals (recency=0.3,popularity=0.4,viral=15,echo=0.6); enable_twitter=false/enable_reddit=false; 0-entities edge (total_steps=3,no batches); llm_model/llm_base_url passthrough; generated_at=python_isoformat_local; generation_reasoning joins 4 parts with " | "; common.success fallback when no reasoning key; 14 new tests, 662 lib green · parity PASS 2026-06-17 (differential vs py:243-379; step math div_ceil≡ceil all cases incl 0-edge; i18n keys+placeholders exact; reasoning-or-success both sites; reddit non-default literals; assigned_count filter equiv; field-for-field params)
- [x] S-440 · `unit:U-019` · `method` · `SimulationConfigGenerator._build_context` · `simulation_config_generator.py:381` → `SimulationConfigGenerator::build_context`
- [x] S-441 · `unit:U-019` · `method` · `SimulationConfigGenerator._summarize_entities` · `simulation_config_generator.py:409` → `SimulationConfigGenerator::summarize_entities`
- [x] S-442 · `unit:U-019` · `method` · `SimulationConfigGenerator._call_llm_with_retry` · up to 3 retries · `simulation_config_generator.py:434` → `SimulationConfigGenerator::call_llm_with_retry`
- [x] S-443 · `unit:U-019` · `method` · `SimulationConfigGenerator._fix_truncated_json` · `simulation_config_generator.py:483` → `SimulationConfigGenerator::fix_truncated_json`
- [x] S-444 · `unit:U-019` · `method` · `SimulationConfigGenerator._try_fix_config_json` · `simulation_config_generator.py:501` → `SimulationConfigGenerator::try_fix_config_json`
- [x] S-445 · `unit:U-019` · `method` · `SimulationConfigGenerator._generate_time_config` · LLM → parse · `simulation_config_generator.py:535` → `SimulationConfigGenerator::generate_time_config`
- [x] S-446 · `unit:U-019` · `method` · `SimulationConfigGenerator._get_default_time_config` · `simulation_config_generator.py:597` → `SimulationConfigGenerator::get_default_time_config`
- [x] S-447 · `unit:U-019` · `method` · `SimulationConfigGenerator._parse_time_config` · `simulation_config_generator.py:611` → `SimulationConfigGenerator::parse_time_config`
- [x] S-448 · `unit:U-019` · `method` · `SimulationConfigGenerator._generate_event_config` · LLM → parse · `simulation_config_generator.py:646` → `SimulationConfigGenerator::generate_event_config`
- [x] S-449 · `unit:U-019` · `method` · `SimulationConfigGenerator._parse_event_config` · `simulation_config_generator.py:719` → `SimulationConfigGenerator::parse_event_config`
- [x] S-450 · `unit:U-019` · `method` · `SimulationConfigGenerator._assign_initial_post_agents` · LLM assigns initial-post agents · `simulation_config_generator.py:728` → `SimulationConfigGenerator::assign_initial_post_agents` · `src/services/simulation_config.rs:1764` · coverage: direct/alias/influence-fallback(strict->,first-on-tie)/empty-posts/round-robin/original-casing/default-Unknown/no-agents-0/alias-scan-continues-past-empty-group · parity re-verified 2026-06-17 (FAIL→fix→PASS): unconditional inner-alias `break` removed; outer alias scan now continues past an empty-member group exactly as Python L780-790 (conditional `if matched_agent_id is not None: break`); divergent case poster=person agents=[alumni 1.0, official 9.0] now → 100 (was 200); 12 S-450 tests + 648 lib tests green
- [x] S-451 · `unit:U-019` · `method` · `SimulationConfigGenerator._generate_agent_configs_batch` · LLM per-agent config in batches · `simulation_config_generator.py:813` → `SimulationConfigGenerator::generate_agent_configs_batch` · `src/services/simulation_config.rs` · coverage: happy-path/llm-failure-rule-fallback/missing-agent_id-rule-fallback/batch-defaults-differ/start_idx/entity-fields/summary-truncation · **PARITY-VERIFIED 2026-06-17 (opus): prompt byte-IDENTICAL (1657B incl embedded json.dumps ensure_ascii=False indent=2), system_prompt byte-IDENTICAL (400B incl get_language_instruction placement + English stance note); batch defaults differ-from-dataclass confirmed (posts 0.5/comments 1.0/active_hours [9..=22]=14); cfg precedence (empty-object→rule) + .get(field,default) on both LLM & rule paths; LLM-failure→rule fallback (no error propagation).**
- [x] S-452 · `unit:U-019` · `method` · `SimulationConfigGenerator._generate_agent_config_by_rule` · fallback rule-based · `simulation_config_generator.py:908` → `SimulationConfigGenerator::generate_agent_config_by_rule` · `src/services/simulation_config.rs` · coverage: all 6 branches with exact numeric values and active_hours lists · **PARITY-VERIFIED 2026-06-17 (opus): all 6 branches, every numeric value + active_hours list byte-exact vs L912-989 (range(9,18)/range(7,24)/range(8,22) expanded correctly; student/alumni/else explicit lists exact); branch match on get_entity_type()||"Unknown".lower().**

---

## U-020 — `backend/app/services/simulation_ipc.py`

- [x] S-453 · `unit:U-020` · `type` · `CommandType` · enum: INTERVIEW/BATCH_INTERVIEW/CLOSE_ENV · `simulation_ipc.py:25` → `teri::services::simulation_ipc::CommandType`
- [x] S-454 · `unit:U-020` · `variant` · `CommandType.INTERVIEW` · `simulation_ipc.py:27` → `CommandType::Interview` (serde→"interview")
- [x] S-455 · `unit:U-020` · `variant` · `CommandType.BATCH_INTERVIEW` · `simulation_ipc.py:28` → `CommandType::BatchInterview` (serde→"batch_interview")
- [x] S-456 · `unit:U-020` · `variant` · `CommandType.CLOSE_ENV` · `simulation_ipc.py:29` → `CommandType::CloseEnv` (serde→"close_env")
- [x] S-457 · `unit:U-020` · `type` · `CommandStatus` · enum: PENDING/PROCESSING/COMPLETED/FAILED · `simulation_ipc.py:32` → `teri::services::simulation_ipc::CommandStatus`
- [x] S-458 · `unit:U-020` · `variant` · `CommandStatus.PENDING` · `simulation_ipc.py:34` → `CommandStatus::Pending` (serde→"pending")
- [x] S-459 · `unit:U-020` · `variant` · `CommandStatus.PROCESSING` · `simulation_ipc.py:35` → `CommandStatus::Processing` (serde→"processing")
- [x] S-460 · `unit:U-020` · `variant` · `CommandStatus.COMPLETED` · `simulation_ipc.py:36` → `CommandStatus::Completed` (serde→"completed")
- [x] S-461 · `unit:U-020` · `variant` · `CommandStatus.FAILED` · `simulation_ipc.py:37` → `CommandStatus::Failed` (serde→"failed")
- [x] S-462 · `unit:U-020` · `type` · `IPCCommand` · dataclass with to_dict/from_dict · `simulation_ipc.py:41` → `teri::services::simulation_ipc::IPCCommand`
- [x] S-463 · `unit:U-020` · `field` · `IPCCommand.command_id` · `simulation_ipc.py:43` → `IPCCommand::command_id: String`
- [x] S-464 · `unit:U-020` · `field` · `IPCCommand.command_type` · `simulation_ipc.py:44` → `IPCCommand::command_type: CommandType`
- [x] S-465 · `unit:U-020` · `field` · `IPCCommand.args` · `simulation_ipc.py:45` → `IPCCommand::args: serde_json::Map<String, Value>`
- [x] S-466 · `unit:U-020` · `field` · `IPCCommand.timestamp` · `simulation_ipc.py:46` → `IPCCommand::timestamp: String` (default=python_isoformat_local())
- [x] S-467 · `unit:U-020` · `method` · `IPCCommand.to_dict` · `simulation_ipc.py:48` → `IPCCommand::to_dict(&self) -> Value` (4-key ordered map; command_type=.value string)
- [x] S-468 · `unit:U-020` · `method` · `IPCCommand.from_dict` · `simulation_ipc.py:57` → `IPCCommand::from_dict(data: &Value) -> Result<Self>` (command_id/command_type required; args/timestamp tolerant)
- [x] S-469 · `unit:U-020` · `type` · `IPCResponse` · dataclass with to_dict/from_dict · `simulation_ipc.py:67` → `teri::services::simulation_ipc::IPCResponse`
- [x] S-470 · `unit:U-020` · `field` · `IPCResponse.command_id` · `simulation_ipc.py:69` → `IPCResponse::command_id: String`
- [x] S-471 · `unit:U-020` · `field` · `IPCResponse.status` · `simulation_ipc.py:70` → `IPCResponse::status: CommandStatus`
- [x] S-472 · `unit:U-020` · `field` · `IPCResponse.result` · `simulation_ipc.py:71` → `IPCResponse::result: Option<Map<String, Value>>`
- [x] S-473 · `unit:U-020` · `field` · `IPCResponse.error` · `simulation_ipc.py:72` → `IPCResponse::error: Option<String>`
- [x] S-474 · `unit:U-020` · `field` · `IPCResponse.timestamp` · `simulation_ipc.py:73` → `IPCResponse::timestamp: String` (default=python_isoformat_local())
- [x] S-475 · `unit:U-020` · `method` · `IPCResponse.to_dict` · `simulation_ipc.py:75` → `IPCResponse::to_dict(&self) -> Value` (5-key ordered; status=.value; result/error=null not omitted)
- [x] S-476 · `unit:U-020` · `method` · `IPCResponse.from_dict` · `simulation_ipc.py:85` → `IPCResponse::from_dict(data: &Value) -> Result<Self>` (command_id/status required; result/error/timestamp tolerant)
- [x] S-477 · `unit:U-020` · `type` · `SimulationIPCClient` · file-based IPC client → `pub struct SimulationIPCClient { tx: mpsc::Sender<IpcEnvelope>, alive: Arc<AtomicBool> }` (Clone) · `src/services/simulation_ipc.rs` · `simulation_ipc.py:95`
- [x] S-478 · `unit:U-020` · `method` · `SimulationIPCClient.__init__` → `pub fn channel(buffer: usize) -> (SimulationIPCClient, SimulationIPCServer)` factory; dirs/makedirs `[≠]` (no FS boundary in-process; replaced by shared mpsc+AtomicBool) · `src/services/simulation_ipc.rs` · `simulation_ipc.py:102`
- [x] S-479 · `unit:U-020` · `method` · `SimulationIPCClient.send_command` → `async fn send_command(&self, command_type, args, timeout: Duration) -> Result<IPCResponse>`; uuid v4 command_id; oneshot; tx.send(env).await; tokio::time::timeout(timeout, reply_rx); elapsed → TeriError::Sim("等待命令响应超时 (N秒)"); log lines preserved; poll_interval `[≠]` (channel wakes immediately); file-write/poll-loop/os.remove `[≠]` · `src/services/simulation_ipc.rs` · `simulation_ipc.py:117`
- [x] S-480 · `unit:U-020` · `method` · `SimulationIPCClient.send_interview` → `async fn send_interview(&self, agent_id: i64, prompt: &str, platform: Option<&str>, timeout: Duration) -> Result<IPCResponse>`; args={agent_id,prompt,[platform]}; platform key inserted only when Some; delegates to send_command(Interview, ...) · `src/services/simulation_ipc.rs` · `simulation_ipc.py:189`
- [x] S-481 · `unit:U-020` · `method` · `SimulationIPCClient.send_batch_interview` → `async fn send_batch_interview(&self, interviews: Vec<Value>, platform: Option<&str>, timeout: Duration) -> Result<IPCResponse>`; args={interviews,[platform]}; delegates to send_command(BatchInterview, ...) · `src/services/simulation_ipc.rs` · `simulation_ipc.py:224`
- [x] S-482 · `unit:U-020` · `method` · `SimulationIPCClient.send_close_env` → `async fn send_close_env(&self, timeout: Duration) -> Result<IPCResponse>`; args={}; delegates to send_command(CloseEnv, ...) · `src/services/simulation_ipc.rs` · `simulation_ipc.py:254`
- [x] S-483 · `unit:U-020` · `method` · `SimulationIPCClient.check_env_alive` → `fn check_env_alive(&self) -> bool`; reads shared Arc<AtomicBool>; env_status.json file read `[≠]` (in-process bool; no FS round-trip) · `src/services/simulation_ipc.rs` · `simulation_ipc.py:270`
- [x] S-484 · `unit:U-020` · `type` · `SimulationIPCServer` · file-based IPC server → `pub struct SimulationIPCServer { rx: mpsc::Receiver<IpcEnvelope>, running: Arc<AtomicBool> }` · `src/services/simulation_ipc.rs` · `simulation_ipc.py:288`
- [x] S-485 · `unit:U-020` · `method` · `SimulationIPCServer.__init__` → via `channel()` factory; dirs `[≠]`; running starts false · `src/services/simulation_ipc.rs` · `simulation_ipc.py:295`
- [x] S-486 · `unit:U-020` · `method` · `SimulationIPCServer.start` → `fn start(&self)`; running.store(true, SeqCst); _update_env_status("alive") `[≠]` (AtomicBool store) · `src/services/simulation_ipc.rs` · `simulation_ipc.py:313`
- [x] S-487 · `unit:U-020` · `method` · `SimulationIPCServer.stop` → `fn stop(&self)`; running.store(false, SeqCst) · `src/services/simulation_ipc.rs` · `simulation_ipc.py:318`
- [≠] S-488 · `unit:U-020` · `method` · `SimulationIPCServer._update_env_status` · `[≠]` fully absorbed into start/stop AtomicBool stores; env_status.json file write + timestamp are cross-process artifacts; nothing in-process consumes the timestamp · `src/services/simulation_ipc.rs` · `simulation_ipc.py:323`
- [x] S-489 · `unit:U-020` · `method` · `SimulationIPCServer.poll_commands` → `fn poll_commands(&mut self) -> Option<IpcEnvelope>`; rx.try_recv().ok(); FIFO preserves mtime oldest-first ordering; returns IpcEnvelope (command+reply sink) not bare IPCCommand; mtime dir scan + JSONDecodeError-retry `[≠]` · `src/services/simulation_ipc.rs` · `simulation_ipc.py:332`
- [x] S-490 · `unit:U-020` · `method` · `SimulationIPCServer.send_response` → `fn send_response(envelope: IpcEnvelope, response: IPCResponse)`; fires envelope.reply oneshot; os.remove cleanup `[≠]` (oneshot consumes itself) · `src/services/simulation_ipc.rs` · `simulation_ipc.py:362`
- [x] S-491 · `unit:U-020` · `method` · `SimulationIPCServer.send_success` → `fn send_success(envelope: IpcEnvelope, result: Map<String,Value>)`; builds IPCResponse{command_id=envelope.command.command_id, status=Completed, result=Some(result), error=None}; command_id preserved for protocol/log fidelity · `src/services/simulation_ipc.rs` · `simulation_ipc.py:380`
- [x] S-492 · `unit:U-020` · `method` · `SimulationIPCServer.send_error` → `fn send_error(envelope: IpcEnvelope, error: String)`; builds IPCResponse{status=Failed, error=Some(error), result=None} · `src/services/simulation_ipc.rs` · `simulation_ipc.py:388`

---

## U-021 — `backend/app/services/zep_graph_memory_updater.py`

- [x] S-493 · `unit:U-021` · `type` · `AgentActivity` · dataclass with to_episode_text + 12 action describers · `zep_graph_memory_updater.py:25`
- [x] S-494 · `unit:U-021` · `field` · `AgentActivity.platform` · `zep_graph_memory_updater.py:27`
- [x] S-495 · `unit:U-021` · `field` · `AgentActivity.agent_id` · `zep_graph_memory_updater.py:28`
- [x] S-496 · `unit:U-021` · `field` · `AgentActivity.agent_name` · `zep_graph_memory_updater.py:29`
- [x] S-497 · `unit:U-021` · `field` · `AgentActivity.action_type` · `zep_graph_memory_updater.py:30`
- [x] S-498 · `unit:U-021` · `field` · `AgentActivity.action_args` · `zep_graph_memory_updater.py:31`
- [x] S-499 · `unit:U-021` · `field` · `AgentActivity.round_num` · `zep_graph_memory_updater.py:32`
- [x] S-500 · `unit:U-021` · `field` · `AgentActivity.timestamp` · `zep_graph_memory_updater.py:33`
- [x] S-501 · `unit:U-021` · `method` · `AgentActivity.to_episode_text` · dispatches to 12 action describers · `zep_graph_memory_updater.py:35`
- [x] S-502 · `unit:U-021` · `method` · `AgentActivity._describe_create_post` · `zep_graph_memory_updater.py:64`
- [x] S-503 · `unit:U-021` · `method` · `AgentActivity._describe_like_post` · `zep_graph_memory_updater.py:70`
- [x] S-504 · `unit:U-021` · `method` · `AgentActivity._describe_dislike_post` · `zep_graph_memory_updater.py:83`
- [x] S-505 · `unit:U-021` · `method` · `AgentActivity._describe_repost` · `zep_graph_memory_updater.py:96`
- [x] S-506 · `unit:U-021` · `method` · `AgentActivity._describe_quote_post` · `zep_graph_memory_updater.py:109`
- [x] S-507 · `unit:U-021` · `method` · `AgentActivity._describe_follow` · `zep_graph_memory_updater.py:129`
- [x] S-508 · `unit:U-021` · `method` · `AgentActivity._describe_create_comment` · `zep_graph_memory_updater.py:137`
- [x] S-509 · `unit:U-021` · `method` · `AgentActivity._describe_like_comment` · `zep_graph_memory_updater.py:153`
- [x] S-510 · `unit:U-021` · `method` · `AgentActivity._describe_dislike_comment` · `zep_graph_memory_updater.py:166`
- [x] S-511 · `unit:U-021` · `method` · `AgentActivity._describe_search` · `zep_graph_memory_updater.py:179`
- [x] S-512 · `unit:U-021` · `method` · `AgentActivity._describe_search_user` · `zep_graph_memory_updater.py:184`
- [x] S-513 · `unit:U-021` · `method` · `AgentActivity._describe_mute` · `zep_graph_memory_updater.py:189`
- [x] S-514 · `unit:U-021` · `method` · `AgentActivity._describe_generic` · `zep_graph_memory_updater.py:197`
- [x] S-515 · `unit:U-021` · `type` · `ZepGraphMemoryUpdater` · daemon-threaded queue flusher · `zep_graph_memory_updater.py:202` → `teri::services::graph_memory::GraphMemoryUpdater<L>` (`src/services/graph_memory.rs`)
- [x] S-516 · `unit:U-021` · `field` · `ZepGraphMemoryUpdater.BATCH_SIZE` · 5 · `zep_graph_memory_updater.py:217` → `const BATCH_SIZE: usize = 5` (`src/services/graph_memory.rs`)
- [x] S-517 · `unit:U-021` · `field` · `ZepGraphMemoryUpdater.PLATFORM_DISPLAY_NAMES` · `zep_graph_memory_updater.py:220` → `fn platform_display_name(platform: &str) -> &str` (twitter→"世界1", reddit→"世界2") (`src/services/graph_memory.rs`)
- [≠] S-518 · `unit:U-021` · `field` · `ZepGraphMemoryUpdater.SEND_INTERVAL` · 0.5s · `zep_graph_memory_updater.py:226` — Zep network rate-limit; non-contractual in-process (DECISION-14 Decision 4)
- [≠] S-519 · `unit:U-021` · `field` · `ZepGraphMemoryUpdater.MAX_RETRIES` · 3 · `zep_graph_memory_updater.py:229` — Zep-network transient-retry; failed_count+continue-on-error IS ported; literal retry cadence is non-contractual (DECISION-14 Decision 4)
- [≠] S-520 · `unit:U-021` · `field` · `ZepGraphMemoryUpdater.RETRY_DELAY` · 2s · `zep_graph_memory_updater.py:230` — Zep-network backoff delay; non-contractual (DECISION-14 Decision 4)
- [x] S-521 · `unit:U-021` · `method` · `ZepGraphMemoryUpdater.__init__` · `zep_graph_memory_updater.py:232` → `GraphMemoryUpdater::new` (`src/services/graph_memory.rs`)
- [x] S-522 · `unit:U-021` · `method` · `ZepGraphMemoryUpdater._get_platform_display_name` · `zep_graph_memory_updater.py:271` → `fn platform_display_name` (merged with S-517) (`src/services/graph_memory.rs`)
- [x] S-523 · `unit:U-021` · `method` · `ZepGraphMemoryUpdater.start` · spawns daemon thread, locale-captured before spawn · `zep_graph_memory_updater.py:275` → `GraphMemoryUpdater::start` with U-050 `with_locale` locale-capture (`src/services/graph_memory.rs`)
- [x] S-524 · `unit:U-021` · `method` · `ZepGraphMemoryUpdater.stop` · signals worker, flushes remaining · `zep_graph_memory_updater.py:293` → `GraphMemoryUpdater::stop` (drop tx + join) (`src/services/graph_memory.rs`)
- [x] S-525 · `unit:U-021` · `method` · `ZepGraphMemoryUpdater.add_activity` · skips DO_NOTHING, enqueues · `zep_graph_memory_updater.py:310` → `GraphMemoryUpdater::add_activity` (DO_NOTHING skip before enqueue) (`src/services/graph_memory.rs`)
- [x] S-526 · `unit:U-021` · `method` · `ZepGraphMemoryUpdater.add_activity_from_dict` · skips event_type entries · `zep_graph_memory_updater.py:340` → `GraphMemoryUpdater::add_activity_from_dict` (event_type skip) (`src/services/graph_memory.rs`)
- [x] S-527 · `unit:U-021` · `method` · `ZepGraphMemoryUpdater._worker_loop` · queue→per-platform buffers→flush at BATCH_SIZE · `zep_graph_memory_updater.py:364` → `async fn worker_loop<L>` (`src/services/graph_memory.rs`)
- [x] S-528 · `unit:U-021` · `method` · `ZepGraphMemoryUpdater._send_batch_activities` · merges to text episode, client.graph.add · `zep_graph_memory_updater.py:396` → `async fn flush_batch<L>` (combined_text "\n".join → `extend_from_text`) (`src/services/graph_memory.rs`)
- [x] S-529 · `unit:U-021` · `method` · `ZepGraphMemoryUpdater._flush_remaining` · `zep_graph_memory_updater.py:435` → drain section at end of `worker_loop` (channel-closed branch) (`src/services/graph_memory.rs`)
- [x] S-530 · `unit:U-021` · `method` · `ZepGraphMemoryUpdater.get_stats` · `zep_graph_memory_updater.py:460` → `GraphMemoryUpdater::get_stats` → `UpdaterStats` (10 serde fields, byte-identical JSON keys) (`src/services/graph_memory.rs`)
- [x] S-531 · `unit:U-021` · `type` · `ZepGraphMemoryManager` · class-level dict of updaters · `zep_graph_memory_updater.py:479` → `GraphMemoryManager<L>` instance struct (`src/services/graph_memory.rs`); class→instance map-onto: observable contract (ONE registry per process, keyed by simulation_id, idempotent stop_all) fully preserved
- [x] S-532 · `unit:U-021` · `field` · `ZepGraphMemoryManager._updaters` · `zep_graph_memory_updater.py:486` → `updaters: tokio::sync::Mutex<HashMap<String, GraphMemoryUpdater<L>>>` (`src/services/graph_memory.rs`)
- [x] S-533 · `unit:U-021` · `field` · `ZepGraphMemoryManager._lock` · `zep_graph_memory_updater.py:487` → folded into `tokio::sync::Mutex` wrapping `updaters` (no separate observable; mutual exclusion is provided by the Mutex) (`src/services/graph_memory.rs`)
- [x] S-534 · `unit:U-021` · `method` · `ZepGraphMemoryManager.create_updater` · `zep_graph_memory_updater.py:490` → `GraphMemoryManager::create_updater` async; stops old if present, constructs+starts new, inserts; return type `Result<(),TeriError>` (Python returned the updater; Rust cannot — JoinHandle not Clone — registry is the access path) (`src/services/graph_memory.rs`)
- [x] S-535 · `unit:U-021` · `method` · `ZepGraphMemoryManager.get_updater` · `zep_graph_memory_updater.py:514` → `GraphMemoryManager::get_updater` async → `Option<UpdaterStats>` (Python returned &updater; returning &mut through Mutex to caller is a deadlock footgun; stats snapshot is the faithful composable return; primary read path is get_all_stats) (`src/services/graph_memory.rs`)
- [x] S-536 · `unit:U-021` · `method` · `ZepGraphMemoryManager.stop_updater` · `zep_graph_memory_updater.py:519` → `GraphMemoryManager::stop_updater` async; remove from map, stop if Some, no-op if None (`src/services/graph_memory.rs`)
- [x] S-537 · `unit:U-021` · `field` · `ZepGraphMemoryManager._stop_all_done` · idempotency flag · `zep_graph_memory_updater.py:528` → `stop_all_done: AtomicBool`; compare_exchange(AcqRel/Acquire) matches Python's check-before-lock semantics (`src/services/graph_memory.rs`)
- [x] S-538 · `unit:U-021` · `method` · `ZepGraphMemoryManager.stop_all` · idempotent via _stop_all_done · `zep_graph_memory_updater.py:531` → `GraphMemoryManager::stop_all` async; idempotent via AtomicBool compare_exchange; drain+stop all (catch-log-continue); clears map; U-049 shutdown entry point (`src/services/graph_memory.rs`)
- [x] S-539 · `unit:U-021` · `method` · `ZepGraphMemoryManager.get_all_stats` · `zep_graph_memory_updater.py:549` → `GraphMemoryManager::get_all_stats` async → `HashMap<String, UpdaterStats>` (`src/services/graph_memory.rs`)

---

## U-022 — `backend/app/services/simulation_runner.py`

- [≠] S-540 · `unit:U-022` · `const` · `IS_WINDOWS` · platform check · `simulation_runner.py:33` · **[≠] non-contractual:** used only to select `taskkill` vs `killpg` in subprocess-terminate path; teri's stop is OS-agnostic (cooperative `shutdown` flag + `task.abort()`), so no branch exists. No observable output. (DECISION-17 §17.4)
- [x] S-541 · `unit:U-022` · `type` · `RunnerStatus` · enum: IDLE/STARTING/RUNNING/PAUSED/STOPPING/STOPPED/COMPLETED/FAILED · `simulation_runner.py:36` · **rust-target:** `RunnerStatus` at `src/services/simulation_runner.rs`; `#[serde(rename_all="lowercase")]`; 8 variants; `as_str()` + `Display`; ported sub-cycle (a) 2026-06-17
- [x] S-542 · `unit:U-022` · `variant` · `RunnerStatus.IDLE` · `simulation_runner.py:38` · **rust-target:** `RunnerStatus::Idle` → `"idle"`
- [x] S-543 · `unit:U-022` · `variant` · `RunnerStatus.STARTING` · `simulation_runner.py:39` · **rust-target:** `RunnerStatus::Starting` → `"starting"`
- [x] S-544 · `unit:U-022` · `variant` · `RunnerStatus.RUNNING` · `simulation_runner.py:40` · **rust-target:** `RunnerStatus::Running` → `"running"`
- [x] S-545 · `unit:U-022` · `variant` · `RunnerStatus.PAUSED` · `simulation_runner.py:41` · **rust-target:** `RunnerStatus::Paused` → `"paused"`
- [x] S-546 · `unit:U-022` · `variant` · `RunnerStatus.STOPPING` · `simulation_runner.py:42` · **rust-target:** `RunnerStatus::Stopping` → `"stopping"`
- [x] S-547 · `unit:U-022` · `variant` · `RunnerStatus.STOPPED` · `simulation_runner.py:43` · **rust-target:** `RunnerStatus::Stopped` → `"stopped"`
- [x] S-548 · `unit:U-022` · `variant` · `RunnerStatus.COMPLETED` · `simulation_runner.py:44` · **rust-target:** `RunnerStatus::Completed` → `"completed"`
- [x] S-549 · `unit:U-022` · `variant` · `RunnerStatus.FAILED` · `simulation_runner.py:45` · **rust-target:** `RunnerStatus::Failed` → `"failed"`
- [x] S-550 · `unit:U-022` · `type` · `AgentAction` · dataclass with to_dict · `simulation_runner.py:49` · **rust-target:** `AgentAction` struct at `src/services/simulation_runner.rs`; 9 fields + `to_dict()` 9-key ordered map; ported sub-cycle (a) 2026-06-17
- [x] S-551 · `unit:U-022` · `field` · `AgentAction.round_num` · `simulation_runner.py:51` · **rust-target:** `AgentAction::round_num: i64`
- [x] S-552 · `unit:U-022` · `field` · `AgentAction.timestamp` · `simulation_runner.py:52` · **rust-target:** `AgentAction::timestamp: String`
- [x] S-553 · `unit:U-022` · `field` · `AgentAction.platform` · `simulation_runner.py:53` · **rust-target:** `AgentAction::platform: String`
- [x] S-554 · `unit:U-022` · `field` · `AgentAction.agent_id` · `simulation_runner.py:54` · **rust-target:** `AgentAction::agent_id: i64`
- [x] S-555 · `unit:U-022` · `field` · `AgentAction.agent_name` · `simulation_runner.py:55` · **rust-target:** `AgentAction::agent_name: String`
- [x] S-556 · `unit:U-022` · `field` · `AgentAction.action_type` · `simulation_runner.py:56` · **rust-target:** `AgentAction::action_type: String`
- [x] S-557 · `unit:U-022` · `field` · `AgentAction.action_args` · `simulation_runner.py:57` · **rust-target:** `AgentAction::action_args: Map<String, Value>`; default empty map
- [x] S-558 · `unit:U-022` · `field` · `AgentAction.result` · `simulation_runner.py:58` · **rust-target:** `AgentAction::result: Option<String>`; emits `null` when None
- [x] S-559 · `unit:U-022` · `field` · `AgentAction.success` · `simulation_runner.py:59` · **rust-target:** `AgentAction::success: bool`; default `true`
- [x] S-560 · `unit:U-022` · `method` · `AgentAction.to_dict` · `simulation_runner.py:61` · **rust-target:** `AgentAction::to_dict() -> Map<String, Value>`; 9-key ordered map; byte-exact key order; `result: None` → `Value::Null`
- [x] S-561 · `unit:U-022` · `type` · `RoundSummary` · dataclass with to_dict · `simulation_runner.py:76` · **rust-target:** `RoundSummary` struct at `src/services/simulation_runner.rs`; 8 fields + `to_dict()` 9-key map (computed `actions_count`); ported sub-cycle (a) 2026-06-17
- [x] S-562 · `unit:U-022` · `field` · `RoundSummary.round_num` · `simulation_runner.py:78` · **rust-target:** `RoundSummary::round_num: i64`
- [x] S-563 · `unit:U-022` · `field` · `RoundSummary.start_time` · `simulation_runner.py:79` · **rust-target:** `RoundSummary::start_time: String`
- [x] S-564 · `unit:U-022` · `field` · `RoundSummary.end_time` · `simulation_runner.py:80` · **rust-target:** `RoundSummary::end_time: Option<String>`; emits `null` when None
- [x] S-565 · `unit:U-022` · `field` · `RoundSummary.simulated_hour` · `simulation_runner.py:81` · **rust-target:** `RoundSummary::simulated_hour: i64`; default 0
- [x] S-566 · `unit:U-022` · `field` · `RoundSummary.twitter_actions` · `simulation_runner.py:82` · **rust-target:** `RoundSummary::twitter_actions: i64`; default 0
- [x] S-567 · `unit:U-022` · `field` · `RoundSummary.reddit_actions` · `simulation_runner.py:83` · **rust-target:** `RoundSummary::reddit_actions: i64`; default 0
- [x] S-568 · `unit:U-022` · `field` · `RoundSummary.active_agents` · `simulation_runner.py:84` · **rust-target:** `RoundSummary::active_agents: Vec<i64>`; default empty
- [x] S-569 · `unit:U-022` · `field` · `RoundSummary.actions` · `simulation_runner.py:85` · **rust-target:** `RoundSummary::actions: Vec<AgentAction>`; default empty
- [x] S-570 · `unit:U-022` · `method` · `RoundSummary.to_dict` · `simulation_runner.py:87` · **rust-target:** `RoundSummary::to_dict() -> Map<String, Value>`; 9-key map; `actions_count = len(actions)` computed; nested actions via `a.to_dict()`; `end_time: None` → `Value::Null`
- [x] S-571 · `unit:U-022` · `type` · `SimulationRunState` · full run state dataclass · `simulation_runner.py:102` · **rust-target:** `SimulationRunState` struct at `src/services/simulation_runner.rs`; all 24 fields + 3 methods; ported sub-cycle (a) 2026-06-17
- [x] S-572 · `unit:U-022` · `field` · `SimulationRunState.simulation_id` · `simulation_runner.py:104` · **rust-target:** `SimulationRunState::simulation_id: String`
- [x] S-573 · `unit:U-022` · `field` · `SimulationRunState.runner_status` · `simulation_runner.py:105` · **rust-target:** `SimulationRunState::runner_status: RunnerStatus`; default `Idle`
- [x] S-574 · `unit:U-022` · `field` · `SimulationRunState.current_round` · `simulation_runner.py:108` · **rust-target:** `SimulationRunState::current_round: i64`; default 0
- [x] S-575 · `unit:U-022` · `field` · `SimulationRunState.total_rounds` · `simulation_runner.py:109` · **rust-target:** `SimulationRunState::total_rounds: i64`; default 0
- [x] S-576 · `unit:U-022` · `field` · `SimulationRunState.simulated_hours` · `simulation_runner.py:110` · **rust-target:** `SimulationRunState::simulated_hours: i64`; default 0
- [x] S-577 · `unit:U-022` · `field` · `SimulationRunState.total_simulation_hours` · `simulation_runner.py:111` · **rust-target:** `SimulationRunState::total_simulation_hours: i64`; default 0
- [x] S-578 · `unit:U-022` · `field` · `SimulationRunState.twitter_current_round` · `simulation_runner.py:114` · **rust-target:** `SimulationRunState::twitter_current_round: i64`; default 0
- [x] S-579 · `unit:U-022` · `field` · `SimulationRunState.reddit_current_round` · `simulation_runner.py:115` · **rust-target:** `SimulationRunState::reddit_current_round: i64`; default 0
- [x] S-580 · `unit:U-022` · `field` · `SimulationRunState.twitter_simulated_hours` · `simulation_runner.py:116` · **rust-target:** `SimulationRunState::twitter_simulated_hours: i64`; default 0
- [x] S-581 · `unit:U-022` · `field` · `SimulationRunState.reddit_simulated_hours` · `simulation_runner.py:117` · **rust-target:** `SimulationRunState::reddit_simulated_hours: i64`; default 0
- [x] S-582 · `unit:U-022` · `field` · `SimulationRunState.twitter_running` · `simulation_runner.py:120` · **rust-target:** `SimulationRunState::twitter_running: bool`; default false
- [x] S-583 · `unit:U-022` · `field` · `SimulationRunState.reddit_running` · `simulation_runner.py:121` · **rust-target:** `SimulationRunState::reddit_running: bool`; default false
- [x] S-584 · `unit:U-022` · `field` · `SimulationRunState.twitter_actions_count` · `simulation_runner.py:122` · **rust-target:** `SimulationRunState::twitter_actions_count: i64`; default 0; incremented by `add_action` when platform="twitter"
- [x] S-585 · `unit:U-022` · `field` · `SimulationRunState.reddit_actions_count` · `simulation_runner.py:123` · **rust-target:** `SimulationRunState::reddit_actions_count: i64`; default 0; incremented by `add_action` when platform!="twitter"
- [x] S-586 · `unit:U-022` · `field` · `SimulationRunState.twitter_completed` · `simulation_runner.py:126` · **rust-target:** `SimulationRunState::twitter_completed: bool`; default false
- [x] S-587 · `unit:U-022` · `field` · `SimulationRunState.reddit_completed` · `simulation_runner.py:127` · **rust-target:** `SimulationRunState::reddit_completed: bool`; default false
- [x] S-588 · `unit:U-022` · `field` · `SimulationRunState.rounds` · `simulation_runner.py:130` · **rust-target:** `SimulationRunState::rounds: Vec<RoundSummary>`; default empty
- [x] S-589 · `unit:U-022` · `field` · `SimulationRunState.recent_actions` · `simulation_runner.py:133` · **rust-target:** `SimulationRunState::recent_actions: Vec<AgentAction>`; newest-first; default empty
- [x] S-590 · `unit:U-022` · `field` · `SimulationRunState.max_recent_actions` · `simulation_runner.py:134` · **rust-target:** `SimulationRunState::max_recent_actions: usize`; default 50
- [x] S-591 · `unit:U-022` · `field` · `SimulationRunState.started_at` · `simulation_runner.py:137` · **rust-target:** `SimulationRunState::started_at: Option<String>`; default None → `null`
- [x] S-592 · `unit:U-022` · `field` · `SimulationRunState.updated_at` · `simulation_runner.py:138` · **rust-target:** `SimulationRunState::updated_at: String`; default `python_isoformat_local()`; refreshed by `add_action`
- [x] S-593 · `unit:U-022` · `field` · `SimulationRunState.completed_at` · `simulation_runner.py:139` · **rust-target:** `SimulationRunState::completed_at: Option<String>`; default None → `null`
- [x] S-594 · `unit:U-022` · `field` · `SimulationRunState.error` · `simulation_runner.py:142` · **rust-target:** `SimulationRunState::error: Option<String>`; default None → `null`
- [x] S-595 · `unit:U-022` · `field` · `SimulationRunState.process_pid` · `simulation_runner.py:145` · **rust-target:** `SimulationRunState::process_pid: Option<i64>`; field + `to_dict` key PORTED for shape parity; value always `null` in teri (no OS subprocess). `[≠]` value-only per DECISION-17 §17.4.
- [x] S-596 · `unit:U-022` · `method` · `SimulationRunState.add_action` · `simulation_runner.py:147` · **rust-target:** `SimulationRunState::add_action(&mut self, action: AgentAction)`; insert-at-front, truncate to cap, per-platform counter bump, refresh `updated_at`
- [x] S-597 · `unit:U-022` · `method` · `SimulationRunState.to_dict` · `simulation_runner.py:160` · **rust-target:** `SimulationRunState::to_dict() -> Map<String, Value>`; 23-key ordered map; `progress_percent = round(current_round / max(total_rounds,1) * 100, 1)` computed; `total_actions_count = twitter+reddit` computed; all Option fields emit `null` when None
- [x] S-598 · `unit:U-022` · `method` · `SimulationRunState.to_detail_dict` · `simulation_runner.py:188` · **rust-target:** `SimulationRunState::to_detail_dict() -> Map<String, Value>`; superset of `to_dict` + `recent_actions: [..to_dict()]` + `rounds_count: len(rounds)` (25 keys total); used by `save_run_state`
- [x] S-599 · `unit:U-022` · `type` · `SimulationRunner` · subprocess orchestrator · `simulation_runner.py:196` · **rust-target:** `SimulationRunner<L: LlmClient>` at `src/services/simulation_runner.rs`; owned struct (NOT process-global statics) holding `sim_data_dir: PathBuf` + `runs: tokio::sync::Mutex<HashMap<String, RunHandle>>` + `graph_mgr: Arc<GraphMemoryManager<L>>` + `manager: Arc<SimulationManager>` + `cleanup_done: AtomicBool`. Class-dicts → one owned map of `RunHandle` (DECISION-17 §"Class-dicts → owned struct"). ported sub-cycle (b) 2026-06-17
- [x] S-600 · `unit:U-022` · `field` · `SimulationRunner.RUN_STATE_DIR` · `simulation_runner.py:208` · **rust-target:** `SimulationRunner::sim_data_dir: PathBuf` (constructor arg / `config.oasis_simulation_data_dir`); teri analog of `../../uploads/simulations`
- [≠] S-601 · `unit:U-022` · `field` · `SimulationRunner.SCRIPTS_DIR` · `simulation_runner.py:214` · **[≠] inexpressible-substrate:** locates `run_{twitter,reddit,parallel}_simulation.py`; teri runs the sim in-process (`SimEngine::run`), there are NO `run_*.py` scripts to locate. No observable output. (DECISION-17 §17.4, S-612-partial)
- [x] S-602 · `unit:U-022` · `field` · `SimulationRunner._run_states` · `simulation_runner.py:220` · **rust-target:** `RunHandle::state: SimulationRunState` (per-run, inside `runs` map)
- [x] S-603 · `unit:U-022` · `field` · `SimulationRunner._processes` · `simulation_runner.py:221` · **rust-target:** `RunHandle::task: tokio::task::JoinHandle<()>` — the in-process sim task (the Popen analog); `task.abort()` is the SIGKILL analog. `[≠]` Popen mechanism, lifecycle PORTED.
- [≠] S-604 · `unit:U-022` · `field` · `SimulationRunner._action_queues` · `simulation_runner.py:222` · **[≠] inexpressible-substrate:** `Queue` thread→thread handoff between the Popen monitor thread and main; in-process tokio uses the broadcast/oneshot channels directly — no second thread to hand off to. No observable. (DECISION-17 §17.4)
- [x] S-605 · `unit:U-022` · `field` · `SimulationRunner._monitor_threads` · `simulation_runner.py:223` · **rust-target:** `RunHandle::monitor: Option<JoinHandle<()>>`. Field placed in (b) (`None`); monitor task SPAWNED in `start_simulation` (sub-cycle c) via `spawn_monitor_task` and stored as `Some(handle)`. `terminate_handle` aborts+reaps it on stop/cleanup (daemon-thread teardown analog). ported sub-cycle (c) 2026-06-17, **parity VERIFIED 2026-06-17** (monitor spawned as `Some(handle)`; `terminate_handle` aborts+awaits it L1495-1498)
- [≠] S-606 · `unit:U-022` · `field` · `SimulationRunner._stdout_files` · `simulation_runner.py:224` · **[≠] inexpressible-substrate:** file handle existed ONLY to drain a child process's stdout pipe (avoid pipe-buffer deadlock); no child pipe in-process. No observable. (DECISION-17 §17.4)
- [≠] S-607 · `unit:U-022` · `field` · `SimulationRunner._stderr_files` · `simulation_runner.py:225` · **[≠] inexpressible-substrate:** as S-606 (stderr pipe drain). No observable. (DECISION-17 §17.4)
- [x] S-608 · `unit:U-022` · `field` · `SimulationRunner._graph_memory_enabled` · `simulation_runner.py:228` · **rust-target:** `RunHandle::graph_enabled: bool` (per-run); set by `start_simulation`, read by stop/cleanup to decide whether to `stop_updater`
- [ ] S-609 · `unit:U-022` · `method` · `SimulationRunner.get_run_state` · `simulation_runner.py:231`
- [x] S-610 · `unit:U-022` · `method` · `SimulationRunner._load_run_state` · `simulation_runner.py:243`
- [x] S-611 · `unit:U-022` · `method` · `SimulationRunner._save_run_state` · `simulation_runner.py:299`
- [x] S-612 · `unit:U-022` · `method` · `SimulationRunner.start_simulation` · spawns subprocess + monitor thread · `simulation_runner.py:313` · **rust-target:** `SimulationRunner::start_simulation(sim_id, platform, max_rounds, enable_graph, graph_id, inputs: RunInputs<L>, graph_for_updater) -> Result<SimulationRunState>`. PORTED: reject-if-running (Running/Starting → Err `模拟已在运行中`), config load + `total_rounds = int(total_hours*60/minutes_per_round)` (defaults 72/30) + missing-config Err (`模拟配置不存在`), `max_rounds` truncation (`min`+log), STARTING→persist, graph-updater create (enabled→require graph_id; create-fail = log+enabled=false, NOT abort), platform flags (twitter/reddit/parallel), spawn `tokio::spawn(SimEngine::run)` with cooperative `shutdown` installed, RUNNING→persist→register `RunHandle`, return running state; failure path → state Failed+persist. **[≠] partial:** Popen/`sys.executable`/script-path/`PYTHONUTF8`/`bufsize`/`start_new_session`/stdout→`simulation.log` (no interpreter/script — `tokio::spawn` of in-process `SimEngine::run`); `process_pid` value stays `null` (S-595). (DECISION-17 §17.4). ported sub-cycle (b) 2026-06-17
- [x] S-613 · `unit:U-022` · `method` · `SimulationRunner._monitor_simulation` · polls JSONL every 2s by file offset · `simulation_runner.py:482` · **rust-target:** `monitor_simulation(ctx: MonitorContext<L>, completion_rx)` (spawned task). PORTED: per-file byte-offset tail of `{sim_dir}/{platform}/actions.jsonl` (twitter+reddit) on the 2s `MONITOR_POLL_INTERVAL` cadence; `os.path.exists` file-guard per poll; `_save_run_state` after each poll; loop-exit on U-048 `subscribe_completion()` (replaces `process.poll()`); ONE FINAL read pass after the end signal (L518-522 no-trailing-action-loss); natural-end housekeeping clears `*_running` + persists (L545-547); `finally:` stops the graph updater when enabled (L549-557). **[≠] partial:** `daemon=True` OS-thread flag (tokio task tied to runtime; observable "monitor dies with the run" PORTED via abort on stop/cleanup); non-zero exit_code→FAILED branch + `simulation.log` tail (L524-544) is OS-exit-code mechanism (no OS exit code in-process; the COMPLETED-via-`simulation_end` observable IS ported). ported sub-cycle (c) 2026-06-17, **parity VERIFIED 2026-06-17** (tests `monitor_loop_does_final_read_after_completion`, `monitor_loop_already_completed_at_start_still_reads`, `monitor_loop_persists_run_state_json`; FINAL read pass proven to capture late actions; lock never held across `.await`; `[≠]` daemon/exit-code CONFIRMED inexpressible — no OS pid/exit-code in-process, the COMPLETED observable IS ported)
- [x] S-614 · `unit:U-022` · `method` · `SimulationRunner._read_action_log` · seek-and-readline · `simulation_runner.py:584` · **rust-target:** `read_action_log(log_path, position: u64, ctx, platform) -> u64` + `apply_log_record`. REALIZES U-047/S-1056 (offset tail): seek→read-delta→split complete (newline-terminated) lines only→advance offset past complete lines only→`f.tell()` analog returned; partial last line NOT consumed; no double-read; robust to growth + missing file + I/O error (returns `position` unchanged). Per-line dispatch EXACT: skip blank, skip `JSONDecodeError`, `simulation_end`→platform completed+gate, `round_end`→per-platform+global round/hours (max), other event_type→continue, action→`add_action`(S-596)+global round bump+graph-fire when enabled. **U-010↔U-022 field alignment:** maps U-010's JSONL `"round"`→`round_num`, supplies `platform` from dir (no `platform` key in the record), `agent_id`/`agent_name`/`action_type`/`action_args`/`result`/`success`/`timestamp` 1:1 — EXACT same map as Python `_read_action_log` L665-674. ported sub-cycle (c) 2026-06-17, **parity VERIFIED 2026-06-17** (tests `read_action_log_no_double_read_across_polls`, `read_action_log_partial_line_not_consumed`, `..._reads_new_lines_and_returns_offset`, `..._skips_blank_and_invalid_lines`, `..._event_types_not_added_as_actions`, `..._missing_file_returns_position_unchanged`; offset matches Python `f.tell()` byte-for-byte on writer-produced input — U-010 `PlatformActionLogger.log_action` writes `json+\n` in one write, so EOF is always at a complete-line boundary; partial-line handling is a strict-superset robustness, never a downgrade; **field map cross-checked against the U-010 PRODUCER** `src/sim/action_logger.rs:125-134` — producer writes `round` not `round_num`, no `platform` key, consumer maps both correctly = NO drift)
- [x] S-615 · `unit:U-022` · `method` · `SimulationRunner._check_all_platforms_completed` · both twitter+reddit must complete for dual · `simulation_runner.py:694` · **rust-target:** `check_all_platforms_completed(sim_data_dir, &state) -> bool`. PORTED EXACT (L706-717): platform enabled iff its `actions.jsonl` exists; enabled-but-not-`*_completed` → false; return `twitter_enabled || reddit_enabled` (≥1 enabled+completed). Single-platform = that one; dual = both required. ported sub-cycle (c) 2026-06-17, **parity VERIFIED 2026-06-17** (tests `check_completed_single_twitter_only`, `check_completed_dual_requires_both`, `check_completed_no_platforms_enabled_is_false`, `simulation_end_single_platform_marks_completed`, `simulation_end_dual_one_platform_not_completed`; single-platform completes alone, dual requires BOTH, no-platform is false-not-vacuously-true)
- [x] S-616 · `unit:U-022` · `method` · `SimulationRunner._terminate_process` · SIGTERM→pgid, SIGKILL after `timeout` (default 10s); Windows taskkill · `simulation_runner.py:721` · **rust-target:** `terminate_handle(&mut RunHandle, sim_id, grace: Duration)` (free fn). PORTED observable contract: cooperative `shutdown.store(true, Release)` (SIGTERM analog) → `tokio::time::timeout(grace, &mut task)` → on elapsed `task.abort()` (SIGKILL analog) + reap; aborts monitor task if present. **FAIL-1 FIX (sub-cycle b):** grace window is now a PARAMETER matching the source's per-caller `timeout` — Python `_terminate_process(process, timeout: int = 10)` (py:721) default 10s; `stop_simulation` passes no arg → 10s (`STOP_GRACE`), `cleanup_all` passes `timeout=5` → 5s (`CLEANUP_GRACE`). Was wrongly collapsed to a single 5s `TERMINATE_GRACE` (narrowed stop's 10s window). Regression test `terminate_grace_windows_match_python_defaults`. **[≠] partial:** `taskkill`/`killpg`/pgid/SIGTERM/SIGKILL/IS_WINDOWS Win-Unix branch (no OS process to signal). (DECISION-17 §17.4). ported sub-cycle (b) 2026-06-17, FAIL-1 fixed sub-cycle (b)
- [x] S-617 · `unit:U-022` · `method` · `SimulationRunner.stop_simulation` · `simulation_runner.py:777` · **rust-target:** `SimulationRunner::stop_simulation(sim_id) -> Result<SimulationRunState>`. PORTED: run-must-exist Err (`模拟不存在`), must-be-Running/Paused Err (`模拟未在运行`), STOPPING→persist, `terminate_handle(.., STOP_GRACE)` (**10s** grace then abort, S-616 — `_terminate_process(process)` with NO timeout arg → py:721 default 10s; FAIL-1 fixed, was 5s), STOPPED + clear platform flags + `completed_at`→persist, stop graph updater if enabled. Lock-discipline: handle taken OUT of the map before `.await` (never held across abort/join). ported sub-cycle (b) 2026-06-17, FAIL-1 fixed sub-cycle (b)
- [ ] S-618 · `unit:U-022` · `method` · `SimulationRunner._read_actions_from_file` · `simulation_runner.py:825`
- [ ] S-619 · `unit:U-022` · `method` · `SimulationRunner.get_all_actions` · `simulation_runner.py:894`
- [ ] S-620 · `unit:U-022` · `method` · `SimulationRunner.get_actions` · filter by platform/agent/round · `simulation_runner.py:955`
- [ ] S-621 · `unit:U-022` · `method` · `SimulationRunner.get_timeline` · by rounds · `simulation_runner.py:989`
- [ ] S-622 · `unit:U-022` · `method` · `SimulationRunner.get_agent_stats` · action type distribution · `simulation_runner.py:1060`
- [ ] S-623 · `unit:U-022` · `method` · `SimulationRunner.cleanup_simulation_logs` · `simulation_runner.py:1103`
- [x] S-624 · `unit:U-022` · `field` · `SimulationRunner._cleanup_done` · idempotency flag · `simulation_runner.py:1184` · **rust-target:** `SimulationRunner::cleanup_done: AtomicBool`; `compare_exchange(false→true, AcqRel/Acquire)` in `cleanup_all` (mirrors U-021 `stop_all` idempotency pattern, DECISION-13)
- [x] S-625 · `unit:U-022` · `method` · `SimulationRunner.cleanup_all_simulations` · terminates all, called by signal handlers · `simulation_runner.py:1187` · **rust-target:** `SimulationRunner::cleanup_all()` (async). PORTED: idempotent via `cleanup_done` (S-624), silent return if no runs+no updaters, `GraphMemoryManager::stop_all`, then for each drained handle. **FAIL-2 FIX (sub-cycle b):** the per-run terminate + STOPPED/error `run_state.json` write + secondary `state.json` write are ALL gated behind `!handle.is_finished()` (the in-process analog of Python's `if process.poll() is None:` at py:1219, which wraps the entire body L1219-1259). A FINISHED run is `continue`-skipped — drained from the map but its persisted state left INTACT (was wrongly clobbered to STOPPED+error for every handle). A RUNNING run: `terminate_handle(.., CLEANUP_GRACE)` (5s, `timeout=5` py:1224) + STOPPED/clear-flags/`completed_at`/error `服务器关闭，模拟被终止`→persist `run_state.json`, secondary `state.json` write via `SimulationManager::mark_state_json_stopped` (U-023, DECISION-17 §17.0 Area 4 — NOT raw json edit), per-run catch-log-continue. Drain ALL entries (Python `_processes.clear()` L1282). Regression tests `cleanup_all_preserves_finished_run_state`, `cleanup_all_stops_running_but_skips_finished`. ported sub-cycle (b) 2026-06-17, FAIL-2 fixed sub-cycle (b)
- [→U-049] S-626 · `unit:U-022` · `method` · `SimulationRunner.register_cleanup` · installs SIGTERM/SIGINT/SIGHUP + atexit · `simulation_runner.py:1288` · **DEFERRED to U-049 (NOT `[≠]`, NOT dropped):** the SIGTERM/SIGINT/SIGHUP/atexit/`WERKZEUG_RUN_MAIN` signal-installation is Flask-WSGI-specific; U-049 wires teri's existing `ctrl_c` graceful-shutdown (U-002's `axum::serve().with_graceful_shutdown`) to CALL `runner.cleanup_all()`. U-022 ships `cleanup_all` (S-625) as the callable U-049 invokes; NO signal handlers are installed here. (DECISION-17 §17.0 Area 4 / §17.5 row f)
- [x] S-627 · `unit:U-022` · `method` · `SimulationRunner.get_running_simulations` · `simulation_runner.py:1361` · **rust-target:** `SimulationRunner::get_running_simulations() -> Vec<String>`; ids where `!RunHandle::is_finished()` (`process.poll() is None` analog → `!task.is_finished()`). ported sub-cycle (b) 2026-06-17
- [ ] S-628 · `unit:U-022` · `method` · `SimulationRunner.check_env_alive` · delegates to IPC client · `simulation_runner.py:1374`
- [ ] S-629 · `unit:U-022` · `method` · `SimulationRunner.get_env_status_detail` · `simulation_runner.py:1392`
- [ ] S-630 · `unit:U-022` · `method` · `SimulationRunner.interview_agent` · via IPC, timeout=60s · `simulation_runner.py:1428`
- [ ] S-631 · `unit:U-022` · `method` · `SimulationRunner.interview_agents_batch` · via IPC, timeout=120s · `simulation_runner.py:1492`
- [ ] S-632 · `unit:U-022` · `method` · `SimulationRunner.interview_all_agents` · reads profiles + batch interviews all · `simulation_runner.py:1551`
- [ ] S-633 · `unit:U-022` · `method` · `SimulationRunner.close_simulation_env` · via IPC · `simulation_runner.py:1611`
- [ ] S-634 · `unit:U-022` · `method` · `SimulationRunner._get_interview_history_from_db` · reads SQLite twitter_simulation.db/reddit_simulation.db · `simulation_runner.py:1659`
- [ ] S-635 · `unit:U-022` · `method` · `SimulationRunner.get_interview_history` · `simulation_runner.py:1717`

---

## U-023 — `backend/app/services/simulation_manager.py`

- [x] S-636 · `unit:U-023` · `type` · `SimulationStatus` · enum 8 variants · `simulation_manager.py:25` → `teri::services::simulation_manager::SimulationStatus`
- [x] S-637 · `unit:U-023` · `variant` · `SimulationStatus.CREATED` · `simulation_manager.py:27` → `SimulationStatus::Created`
- [x] S-638 · `unit:U-023` · `variant` · `SimulationStatus.PREPARING` · `simulation_manager.py:28` → `SimulationStatus::Preparing`
- [x] S-639 · `unit:U-023` · `variant` · `SimulationStatus.READY` · `simulation_manager.py:29` → `SimulationStatus::Ready`
- [x] S-640 · `unit:U-023` · `variant` · `SimulationStatus.RUNNING` · `simulation_manager.py:30` → `SimulationStatus::Running`
- [x] S-641 · `unit:U-023` · `variant` · `SimulationStatus.PAUSED` · `simulation_manager.py:31` → `SimulationStatus::Paused`
- [x] S-642 · `unit:U-023` · `variant` · `SimulationStatus.STOPPED` · `simulation_manager.py:32` → `SimulationStatus::Stopped`
- [x] S-643 · `unit:U-023` · `variant` · `SimulationStatus.COMPLETED` · `simulation_manager.py:33` → `SimulationStatus::Completed`
- [x] S-644 · `unit:U-023` · `variant` · `SimulationStatus.FAILED` · `simulation_manager.py:34` → `SimulationStatus::Failed`
- [x] S-645 · `unit:U-023` · `type` · `PlatformType` · enum: TWITTER/REDDIT (2 variants only; ledger-summary BOTH was wrong) · `simulation_manager.py:37` → `teri::services::simulation_manager::PlatformType`
- [x] S-646 · `unit:U-023` · `variant` · `PlatformType.TWITTER` · `simulation_manager.py:39` → `PlatformType::Twitter`
- [x] S-647 · `unit:U-023` · `variant` · `PlatformType.REDDIT` · `simulation_manager.py:40` → `PlatformType::Reddit`
- [x] S-648 · `unit:U-023` · `type` · `SimulationState` · dataclass with to_dict/to_simple_dict · `simulation_manager.py:44` → `teri::services::simulation_manager::SimulationState`
- [x] S-649 · `unit:U-023` · `field` · `SimulationState.simulation_id` · `simulation_manager.py:46` → `SimulationState::simulation_id`
- [x] S-650 · `unit:U-023` · `field` · `SimulationState.project_id` · `simulation_manager.py:47` → `SimulationState::project_id`
- [x] S-651 · `unit:U-023` · `field` · `SimulationState.graph_id` · `simulation_manager.py:48` → `SimulationState::graph_id`
- [x] S-652 · `unit:U-023` · `field` · `SimulationState.enable_twitter` · `simulation_manager.py:51` → `SimulationState::enable_twitter`
- [x] S-653 · `unit:U-023` · `field` · `SimulationState.enable_reddit` · `simulation_manager.py:52` → `SimulationState::enable_reddit`
- [x] S-654 · `unit:U-023` · `field` · `SimulationState.status` · `simulation_manager.py:55` → `SimulationState::status`
- [x] S-655 · `unit:U-023` · `field` · `SimulationState.entities_count` · `simulation_manager.py:58` → `SimulationState::entities_count`
- [x] S-656 · `unit:U-023` · `field` · `SimulationState.profiles_count` · `simulation_manager.py:59` → `SimulationState::profiles_count`
- [x] S-657 · `unit:U-023` · `field` · `SimulationState.entity_types` · `simulation_manager.py:60` → `SimulationState::entity_types`
- [x] S-658 · `unit:U-023` · `field` · `SimulationState.config_generated` · `simulation_manager.py:63` → `SimulationState::config_generated`
- [x] S-659 · `unit:U-023` · `field` · `SimulationState.config_reasoning` · `simulation_manager.py:64` → `SimulationState::config_reasoning`
- [x] S-660 · `unit:U-023` · `field` · `SimulationState.current_round` · `simulation_manager.py:67` → `SimulationState::current_round`
- [x] S-661 · `unit:U-023` · `field` · `SimulationState.twitter_status` · `simulation_manager.py:68` → `SimulationState::twitter_status`
- [x] S-662 · `unit:U-023` · `field` · `SimulationState.reddit_status` · `simulation_manager.py:69` → `SimulationState::reddit_status`
- [x] S-663 · `unit:U-023` · `field` · `SimulationState.created_at` · `simulation_manager.py:72` → `SimulationState::created_at`
- [x] S-664 · `unit:U-023` · `field` · `SimulationState.updated_at` · `simulation_manager.py:73` → `SimulationState::updated_at`
- [x] S-665 · `unit:U-023` · `field` · `SimulationState.error` · `simulation_manager.py:76` → `SimulationState::error`
- [x] S-666 · `unit:U-023` · `method` · `SimulationState.to_dict` · `simulation_manager.py:78` → `SimulationState::to_dict` (17 keys, insertion order, status as string, error null/string)
- [x] S-667 · `unit:U-023` · `method` · `SimulationState.to_simple_dict` · `simulation_manager.py:100` → `SimulationState::to_simple_dict` (9 keys, insertion order)
- [x] S-668 · `unit:U-023` · `type` · `SimulationManager` · FS-backed manager (not singleton) · `simulation_manager.py:115` → `SimulationManager` struct with `Mutex<HashMap<String,SimulationState>>` cache + `sim_data_dir: PathBuf`; `new(path)` + `from_config(Config)` constructors; sub-cycle (c) 2026-06-17
- [x] S-669 · `unit:U-023` · `field` · `SimulationManager.SIMULATION_DATA_DIR` · `simulation_manager.py:127` → `SimulationManager::sim_data_dir`; uses `config.oasis_simulation_data_dir` (env `OASIS_SIMULATION_DATA_DIR`, default `"./uploads/simulations"`) — teri's equivalent of Python's module-relative `../../uploads/simulations`
- [x] S-670 · `unit:U-023` · `method` · `SimulationManager.__init__` · `simulation_manager.py:132` → `SimulationManager::new` + `SimulationManager::from_config`; creates dir lazily on first use (matching Python's per-call `os.makedirs`); initializes empty Mutex-guarded HashMap cache
- [x] S-671 · `unit:U-023` · `method` · `SimulationManager._get_simulation_dir` · `simulation_manager.py:139` → `SimulationManager::get_simulation_dir`; creates `{sim_data_dir}/{simulation_id}/` via `create_dir_all` then returns PathBuf
- [x] S-672 · `unit:U-023` · `method` · `SimulationManager._save_simulation_state` · `simulation_manager.py:145` → `SimulationManager::save_simulation_state`; bumps `state.updated_at` FIRST, then writes `state.json` (pretty JSON, 2-space indent, UTF-8 raw matching `ensure_ascii=False`), then updates Mutex cache — order faithful to Python L150-155
- [x] S-673 · `unit:U-023` · `method` · `SimulationManager._load_simulation_state` · `simulation_manager.py:157` → `SimulationManager::load_simulation_state`; cache-first; file missing→None; per-field `.get(key,default)` tolerance faithful to Python L171-189; invalid status string→Err (Python `SimulationStatus(str)` raises ValueError); caches on load
- [x] S-674 · `unit:U-023` · `method` · `SimulationManager.create_simulation` · uuid sim_id, creates FS dir · `simulation_manager.py:194` → `SimulationManager::create_simulation`; id = `"sim_"` + 12 lowercase hex chars (`uuid::Uuid::new_v4().simple().to_string()[..12]`); saves state.json via `save_simulation_state`; returns SimulationState
- [x] S-675 · `unit:U-023` · `method` · `SimulationManager.prepare_simulation` · 4-stage async: entities→profiles→config→READY · `simulation_manager.py:230` → `SimulationManager::prepare_simulation<L: LlmClient>` (`src/services/simulation_manager.rs`); `PrepareProgress<'a>` struct co-located. All 4 stages ported (reading→generating_profiles→generating_config→READY); zero-entity→Ok(FAILED); exception→Err(FAILED) via try_stage! macro; live parallel_count via buffer_unordered in oasis_profile_export; two independent if-branches for reddit+twitter saves. 7 tests in `prepare_tests` module. Parity pending (opus gate).
- [x] S-676 · `unit:U-023` · `method` · `SimulationManager.get_simulation` · `simulation_manager.py:459` → `SimulationManager::get_simulation`; thin delegation to `load_simulation_state`
- [x] S-677 · `unit:U-023` · `method` · `SimulationManager.list_simulations` · `simulation_manager.py:463` → `SimulationManager::list_simulations`; skips hidden (`.`-prefix) and non-dir entries; filters by project_id when `Some`; returns empty vec if sim_data_dir absent (matching Python's `os.path.exists` guard); unspecified order (matching Python's `os.listdir`)
- [x] S-678 · `unit:U-023` · `method` · `SimulationManager.get_profiles` · reads JSON/CSV · `simulation_manager.py:481` → `SimulationManager::get_profiles`; missing state→Err (Python `raise ValueError`); missing file→`Ok(vec![])` (NOT Err); file present→`Ok(Vec<Value>)`. Raise-vs-empty distinction faithfully preserved. `platform` arg selects `{platform}_profiles.json`.
- [x] S-679 · `unit:U-023` · `method` · `SimulationManager.get_simulation_config` · reads JSON · `simulation_manager.py:496` → `SimulationManager::get_simulation_config`; file missing→`Ok(None)`; file present→`Ok(Some(Value))`
- [x] S-680 · `unit:U-023` · `method` · `SimulationManager.get_run_instructions` · `simulation_manager.py:507` → `SimulationManager::get_run_instructions` PARTIAL — returns `RunInstructions{simulation_dir, config_file, substrate_note}`. [≠]-substrate: `scripts_dir`, `commands` (Python OASIS subprocess invocations), and `instructions` (conda activate steps) are genuinely INEXPRESSIBLE in teri's substrate (teri has no `scripts/run_*.py` and no conda env; it runs SimEngine in-process). This is NOT "won't use" — the strings cannot be faithfully constructed. Structural fields (paths) ported; command strings omitted with `substrate_note` directing callers to `SimEngine::run`.

---

## U-024 — `backend/app/services/report_agent.py`

- [~] S-681 · `unit:U-024` · `type` · `ReportLogger` · JSONL agent_log.jsonl writer · `report_agent.py:36` → `teri::report::logger::ReportLogger` (`src/report/logger.rs`)
- [~] S-682 · `unit:U-024` · `method` · `ReportLogger.__init__` · `report_agent.py:44` → `ReportLogger::new(report_id, upload_folder)` — dir creation, Instant start
- [~] S-683 · `unit:U-024` · `method` · `ReportLogger._ensure_log_file` · `report_agent.py:58` → folded into `ReportLogger::new` (create_dir_all)
- [~] S-684 · `unit:U-024` · `method` · `ReportLogger._get_elapsed_time` · `report_agent.py:63` → `self.start.elapsed().as_secs_f64()` inlined in `log()`; rounded via `round_half_even_2dp`
- [~] S-685 · `unit:U-024` · `method` · `ReportLogger.log` · `report_agent.py:67` → `ReportLogger::log` — builds 8-key entry in contractual order, compact JSON+`\n`, non-ASCII unescaped, append-open
- [~] S-686 · `unit:U-024` · `method` · `ReportLogger.log_start` · `report_agent.py:100` → `ReportLogger::log_start(simulation_id, graph_id, simulation_requirement)`
- [~] S-687 · `unit:U-024` · `method` · `ReportLogger.log_planning_start` · `report_agent.py:113` → `ReportLogger::log_planning_start()`
- [~] S-688 · `unit:U-024` · `method` · `ReportLogger.log_planning_context` · `report_agent.py:121` → `ReportLogger::log_planning_context(context: Value)`
- [~] S-689 · `unit:U-024` · `method` · `ReportLogger.log_planning_complete` · `report_agent.py:132` → `ReportLogger::log_planning_complete(outline_dict: Value)`
- [~] S-690 · `unit:U-024` · `method` · `ReportLogger.log_section_start` · `report_agent.py:143` → `ReportLogger::log_section_start(section_title, section_index)`
- [~] S-691 · `unit:U-024` · `method` · `ReportLogger.log_react_thought` · `report_agent.py:153` → `ReportLogger::log_react_thought(section_title, section_index, iteration, thought)`
- [~] S-692 · `unit:U-024` · `method` · `ReportLogger.log_tool_call` · `report_agent.py:167` → `ReportLogger::log_tool_call(section_title, section_index, tool_name, parameters: Value, iteration)`
- [~] S-693 · `unit:U-024` · `method` · `ReportLogger.log_tool_result` · `report_agent.py:189` → `ReportLogger::log_tool_result(section_title, section_index, tool_name, result, iteration)`
- [~] S-694 · `unit:U-024` · `method` · `ReportLogger.log_llm_response` · `report_agent.py:212` → `ReportLogger::log_llm_response(section_title, section_index, response, iteration, has_tool_calls, has_final_answer)`
- [~] S-695 · `unit:U-024` · `method` · `ReportLogger.log_section_content` · `report_agent.py:237` → `ReportLogger::log_section_content(section_title, section_index, content, tool_calls_count)`
- [~] S-696 · `unit:U-024` · `method` · `ReportLogger.log_section_full_complete` · `report_agent.py:258` → `ReportLogger::log_section_full_complete(section_title, section_index, full_content)`
- [~] S-697 · `unit:U-024` · `method` · `ReportLogger.log_report_complete` · `report_agent.py:281` → `ReportLogger::log_report_complete(total_sections, total_time_seconds)`
- [~] S-698 · `unit:U-024` · `method` · `ReportLogger.log_error` · `report_agent.py:293` → `ReportLogger::log_error(error_message, stage, section_title: Option<&str>)`
- [x] S-699 · `unit:U-024` · `type` · `ReportConsoleLogger` · attaches file handler to named loggers · `report_agent.py:307` → `ReportConsoleLayer` per-event target/level filter (`teri::report` exact + `teri::services::zep_tools` prefix, INFO+); parity-verified g2 (format dump shows correct capture)
- [x] S-700 · `unit:U-024` · `method` · `ReportConsoleLogger.__init__` · `report_agent.py:315` → `ReportConsoleLogger::new` (mkdir+append open+sink install); verified
- [x] S-701 · `unit:U-024` · `method` · `ReportConsoleLogger._ensure_log_file` · `report_agent.py:330` → mkdir inside `new`; verified by `test_mkdir_and_file_created`
- [x] S-702 · `unit:U-024` · `method` · `ReportConsoleLogger._setup_file_handler` · dynamically attaches handler · `report_agent.py:335` → sets `REPORT_CONSOLE_SINK`; verified by lifecycle install/emit
- [x] S-703 · `unit:U-024` · `method` · `ReportConsoleLogger.close` · detaches handler — must be called explicitly · `report_agent.py:366` → toggles sink off + flush; verified by post-close-not-captured + idempotence
- [x] S-704 · `unit:U-024` · `method` · `ReportConsoleLogger.__del__` · `report_agent.py:384` → `impl Drop` calls close; verified by `test_drop_is_idempotent`
- [~] S-705 · `unit:U-024` · `type` · `ReportStatus` · enum: PENDING/PLANNING/GENERATING/COMPLETED/FAILED · `report_agent.py:389` → `teri::report::ReportStatus` (`src/report/mod.rs`)
- [~] S-706 · `unit:U-024` · `variant` · `ReportStatus.PENDING` · `report_agent.py:391` → `teri::report::ReportStatus::Pending`
- [~] S-707 · `unit:U-024` · `variant` · `ReportStatus.PLANNING` · `report_agent.py:392` → `teri::report::ReportStatus::Planning`
- [~] S-708 · `unit:U-024` · `variant` · `ReportStatus.GENERATING` · `report_agent.py:393` → `teri::report::ReportStatus::Generating`
- [~] S-709 · `unit:U-024` · `variant` · `ReportStatus.COMPLETED` · `report_agent.py:394` → `teri::report::ReportStatus::Completed`
- [~] S-710 · `unit:U-024` · `variant` · `ReportStatus.FAILED` · `report_agent.py:395` → `teri::report::ReportStatus::Failed`
- [~] S-711 · `unit:U-024` · `type` · `ReportSection` · dataclass with to_dict/to_markdown · `report_agent.py:399` → `teri::report::ReportSection` (`src/report/mod.rs`)
- [~] S-712 · `unit:U-024` · `field` · `ReportSection.title` · `report_agent.py:401` → `ReportSection.title`
- [~] S-713 · `unit:U-024` · `field` · `ReportSection.content` · `report_agent.py:402` → `ReportSection.content`
- [~] S-714 · `unit:U-024` · `method` · `ReportSection.to_dict` · `report_agent.py:404` → `ReportSection::to_dict`
- [~] S-715 · `unit:U-024` · `method` · `ReportSection.to_markdown` · `report_agent.py:410` → `ReportSection::to_markdown(level)`
- [~] S-716 · `unit:U-024` · `type` · `ReportOutline` · dataclass: title,summary,sections · `report_agent.py:419` → `teri::report::ReportOutline` (`src/report/mod.rs`)
- [~] S-717 · `unit:U-024` · `field` · `ReportOutline.title` · `report_agent.py:421` → `ReportOutline.title`
- [~] S-718 · `unit:U-024` · `field` · `ReportOutline.summary` · `report_agent.py:422` → `ReportOutline.summary`
- [~] S-719 · `unit:U-024` · `field` · `ReportOutline.sections` · `report_agent.py:423` → `ReportOutline.sections`
- [~] S-720 · `unit:U-024` · `method` · `ReportOutline.to_dict` · `report_agent.py:425` → `ReportOutline::to_dict`
- [~] S-721 · `unit:U-024` · `method` · `ReportOutline.to_markdown` · `report_agent.py:432` → `ReportOutline::to_markdown`
- [~] S-722 · `unit:U-024` · `type` · `Report` · dataclass · `report_agent.py:442` → `teri::report::Report` (`src/report/mod.rs`)
- [~] S-723 · `unit:U-024` · `field` · `Report.report_id` · `report_agent.py:444` → `Report.report_id`
- [~] S-724 · `unit:U-024` · `field` · `Report.simulation_id` · `report_agent.py:445` → `Report.simulation_id`
- [~] S-725 · `unit:U-024` · `field` · `Report.graph_id` · `report_agent.py:446` → `Report.graph_id`
- [~] S-726 · `unit:U-024` · `field` · `Report.simulation_requirement` · `report_agent.py:447` → `Report.simulation_requirement`
- [~] S-727 · `unit:U-024` · `field` · `Report.status` · `report_agent.py:448` → `Report.status`
- [~] S-728 · `unit:U-024` · `field` · `Report.outline` · `report_agent.py:449` → `Report.outline`
- [~] S-729 · `unit:U-024` · `field` · `Report.markdown_content` · `report_agent.py:450` → `Report.markdown_content`
- [~] S-730 · `unit:U-024` · `field` · `Report.created_at` · `report_agent.py:451` → `Report.created_at`
- [~] S-731 · `unit:U-024` · `field` · `Report.completed_at` · `report_agent.py:452` → `Report.completed_at`
- [~] S-732 · `unit:U-024` · `field` · `Report.error` · `report_agent.py:453` → `Report.error`
- [~] S-733 · `unit:U-024` · `method` · `Report.to_dict` · `report_agent.py:455` → `Report::to_dict`
- [ ] S-734 · `unit:U-024` · `const` · `TOOL_DESC_INSIGHT_FORGE` · tool description string for ReACT prompt · `report_agent.py:476`
- [ ] S-735 · `unit:U-024` · `const` · `TOOL_DESC_PANORAMA_SEARCH` · `report_agent.py:494`
- [ ] S-736 · `unit:U-024` · `const` · `TOOL_DESC_QUICK_SEARCH` · `report_agent.py:511`
- [ ] S-737 · `unit:U-024` · `const` · `TOOL_DESC_INTERVIEW_AGENTS` · `report_agent.py:523`
- [x] S-738 · `unit:U-024` · `const` · `PLAN_SYSTEM_PROMPT` · LLM prompt for outline planning · `report_agent.py:552` → `src/report/mod.rs:92` — VERBATIM (691 chars byte-identical, parity 2026-06-18)
- [x] S-739 · `unit:U-024` · `const` · `PLAN_USER_PROMPT_TEMPLATE` · `report_agent.py:591` → `src/report/mod.rs:130` — VERBATIM (367 chars byte-identical) + all 6 slot substitutions verified (parity 2026-06-18)
- [x] S-740 · `unit:U-024` · `const` · `SECTION_SYSTEM_PROMPT_TEMPLATE` · `report_agent.py:615`
- [x] S-741 · `unit:U-024` · `const` · `SECTION_USER_PROMPT_TEMPLATE` · `report_agent.py:769`
- [x] S-742 · `unit:U-024` · `const` · `REACT_OBSERVATION_TEMPLATE` · `report_agent.py:796`
- [x] S-743 · `unit:U-024` · `const` · `REACT_INSUFFICIENT_TOOLS_MSG` · `report_agent.py:808`
- [x] S-744 · `unit:U-024` · `const` · `REACT_INSUFFICIENT_TOOLS_MSG_ALT` · `report_agent.py:813`
- [x] S-745 · `unit:U-024` · `const` · `REACT_TOOL_LIMIT_MSG` · `report_agent.py:818`
- [x] S-746 · `unit:U-024` · `const` · `REACT_UNUSED_TOOLS_HINT` · `report_agent.py:823`
- [x] S-747 · `unit:U-024` · `const` · `REACT_FORCE_FINAL_MSG` · `report_agent.py:825`
- [x] S-748 · `unit:U-024` · `const` · `CHAT_SYSTEM_PROMPT_TEMPLATE` · `report_agent.py:829` → `src/report/mod.rs:385` — PARITY PASS 2026-06-18 (sub-cycle i): stored with SINGLE braces for the JSON example; the real const + 3 sequential `.replace()` render is **byte-identical / SHA-256-matched** to Python `CHAT_SYSTEM_PROMPT_TEMPLATE.format(...)` (proven via Rust probe vs Python `.format()` on a fixed triple incl. CJK + stray `{x}` value; `.format()`'s `{{`→`{` unescape == teri's single-brace storage). 3 named slots filled; literal JSON braces render SINGLE.
- [x] S-749 · `unit:U-024` · `const` · `CHAT_OBSERVATION_SUFFIX` · `report_agent.py:857` → `src/report/mod.rs:414` — PARITY PASS 2026-06-18: `"\n\n请简洁回答问题。"` byte-identical; appended to the observation user msg in the chat ReACT loop (rust:2278).
- [ ] S-750 · `unit:U-024` · `type` · `ReportAgent` · ReACT-pattern report generator · `report_agent.py:865`
- [ ] S-751 · `unit:U-024` · `field` · `ReportAgent.MAX_TOOL_CALLS_PER_SECTION` · from Config · `report_agent.py:876`
- [ ] S-752 · `unit:U-024` · `field` · `ReportAgent.MAX_REFLECTION_ROUNDS` · from Config · `report_agent.py:879`
- [x] S-753 · `unit:U-024` · `field` · `ReportAgent.MAX_TOOL_CALLS_PER_CHAT` · `report_agent.py:882` → `src/report/mod.rs:417` (`const MAX_TOOL_CALLS_PER_CHAT: usize = 2`) — PARITY PASS 2026-06-18: value 2; enforced in chat loop (rust:2239-2241) as the `tool_calls_made.len() >= MAX` break; test (i)-4 `test_chat_i_max_tool_calls_cap` asserts the cap holds.
- [ ] S-754 · `unit:U-024` · `method` · `ReportAgent.__init__` · `report_agent.py:884`
- [ ] S-755 · `unit:U-024` · `method` · `ReportAgent._define_tools` · `report_agent.py:919`
- [ ] S-756 · `unit:U-024` · `method` · `ReportAgent._execute_tool` · dispatches insight_forge/panorama_search/quick_search/interview_agents · `report_agent.py:956`
- [ ] S-757 · `unit:U-024` · `field` · `ReportAgent.VALID_TOOL_NAMES` · set of allowed tool names · `report_agent.py:1065`
- [ ] S-758 · `unit:U-024` · `method` · `ReportAgent._parse_tool_calls` · parses `<tool_call>{...}</tool_call>` blocks + bare-JSON fallback · `report_agent.py:1067`
- [ ] S-759 · `unit:U-024` · `method` · `ReportAgent._is_valid_tool_call` · `report_agent.py:1114`
- [ ] S-760 · `unit:U-024` · `method` · `ReportAgent._get_tools_description` · `report_agent.py:1127`
- [x] S-761 · `unit:U-024` · `method` · `ReportAgent.plan_outline` · ZepTools.get_simulation_context + LLM → 2-5 section outline · `report_agent.py:1137` → `ReportAgent::plan_outline` (`src/report/mod.rs:388`) — PARITY PASS 2026-06-18: PLAN_SYSTEM_PROMPT+PLAN_USER_PROMPT_TEMPLATE verbatim, all 4 progress emissions (0/30/80/100; error path skips 80/100), entity_types Python list-repr, related_facts_json serde pretty≤10 byte-identical to json.dumps(ensure_ascii=False,indent=2), try/except boundary ({"sections":[]}→empty outline NOT fallback), 3-section fallback byte-identical on chat_json Err. 8 differential tests green; 1087/1087 lib tests pass. NOTE: upstream entity_types key-order randomness lives in zep_tools get_graph_statistics (HashMap), out of scope for this symbol.
- [x] S-762 · `unit:U-024` · `method` · `ReportAgent._generate_section_react` · up to 5 ReACT iterations per section; min 3 tool calls enforced · `report_agent.py:1221`
- [x] S-763 · `unit:U-024` · `method` · `ReportAgent.generate_report` · planning + sections + assembly · `report_agent.py:1532` → `src/report/mod.rs:1535` · **h2+h3 PASS** (h2 skeleton/planning/tails + failed-meta-retains-outline fix 2026-06-18; h3 per-section streaming loop parity-verified 2026-06-18 — TRAP #1 final-meta-content faithful, TRAP #2 sink-superset legal, progress arithmetic verbatim, save-section-immediate + real assemble confirmed); h4 (U-027 SseSink adapter seam) optional polish, NullSink covers parity
- [x] S-764 · `unit:U-024` · `method` · `ReportAgent.chat` · tool-assisted chat against report, max 2 tool calls · `report_agent.py:1766` → `src/report/mod.rs:2124` — **PARITY PASS 2026-06-18 (sub-cycle i)**. ALL three g1-class adversarial targets PROVEN: (1) CHAR truncation — `message[:50]`, `markdown_content[:15000]` (incl. `>15000` comparison), `result[:1500]` are ALL char-based (`.chars().take()` / `char_indices().nth()`); CJK differential confirms 15001 chars → 15000 kept + `\n\n... [报告内容已截断] ...` suffix, 15000 chars → NO suffix (Python `len()`/slice == Rust char count, byte-identical). (2) `.format()` brace — Rust render byte-identical/SHA-matched to Python `.format()`; literal JSON-example braces render SINGLE. (3) regex — `(?s)<tool_call>.*?</tool_call>` + `\[TOOL_CALL\].*?\)` byte-identical to Python `re.sub(...DOTALL)` (multiline strip + non-greedy `)` stop), then `.trim()`. ReACT control: max_iter=2, `take(1)`/round, MAX_TOOL_CALLS_PER_CHAT break, post-loop final chat+clean+return, no-tool early return; sources=`parameters["query"]` default ""; tool_calls_made = EXECUTED only. messages order system→history[-10:]→user (roles preserved). observation `"\n".join([{tool}结果]\n{result})`+suffix as user after assistant. ChatResponse.to_dict key order response/tool_calls/sources. [≠] fetchReportFailed except path: LEGIT — inexpressible (Option swallows I/O err, no log surface), observable None→empty→（暂无报告）preserved, no distinct observable output dropped. [!] interview_agents (U-020): honest unknown-tool err string tolerated as observation. llm-err convention (Err/Ok("")→"") consistent with (e); infallible-signature forces graceful empty (Python would raise — no behavior lost). 11 `test_chat_i_*` tests green; 1216/1216 lib tests pass single-threaded; clippy --all-targets clean.
- [x] S-765 · `unit:U-024` · `type` · `ReportManager` · FS-based report persistence · `report_agent.py:1884` → `src/report/manager.rs::ReportManager`
- [x] S-766 · `unit:U-024` · `field` · `ReportManager.REPORTS_DIR` · `report_agent.py:1903` → `ReportManager::reports_dir: PathBuf` (caller-constructed per DECISION-11; Python class-level → instance field)
- [x] S-767 · `unit:U-024` · `method` · `ReportManager._ensure_reports_dir` · `report_agent.py:1906` → `ReportManager::ensure_reports_dir(&self)`
- [x] S-768 · `unit:U-024` · `method` · `ReportManager._get_report_folder` · `report_agent.py:1911` → `ReportManager::get_report_folder(&self, report_id)`
- [x] S-769 · `unit:U-024` · `method` · `ReportManager._ensure_report_folder` · `report_agent.py:1916` → `ReportManager::ensure_report_folder(&self, report_id)`
- [x] S-770 · `unit:U-024` · `method` · `ReportManager._get_report_path` · `report_agent.py:1923` → `ReportManager::get_report_path(&self, report_id)`
- [x] S-771 · `unit:U-024` · `method` · `ReportManager._get_report_markdown_path` · `report_agent.py:1928` → `ReportManager::get_report_markdown_path(&self, report_id)`
- [x] S-772 · `unit:U-024` · `method` · `ReportManager._get_outline_path` · `report_agent.py:1933` → `ReportManager::get_outline_path(&self, report_id)`
- [x] S-773 · `unit:U-024` · `method` · `ReportManager._get_progress_path` · `report_agent.py:1938` → `ReportManager::get_progress_path(&self, report_id)`
- [x] S-774 · `unit:U-024` · `method` · `ReportManager._get_section_path` · `report_agent.py:1943` → `ReportManager::get_section_path(&self, report_id, section_index)`; `section_{NN:02}.md` format preserved
- [x] S-775 · `unit:U-024` · `method` · `ReportManager._get_agent_log_path` · `report_agent.py:1948` → `ReportManager::get_agent_log_path(&self, report_id)`
- [x] S-776 · `unit:U-024` · `method` · `ReportManager._get_console_log_path` · `report_agent.py:1953` → `ReportManager::get_console_log_path(&self, report_id)`
- [x] S-777 · `unit:U-024` · `method` · `ReportManager.get_console_log` · incremental, from_line param · `report_agent.py:1958` → `ReportManager::get_console_log(&self, report_id, from_line)` → `Map{logs,total_lines,from_line,has_more}`
- [x] S-778 · `unit:U-024` · `method` · `ReportManager.get_console_log_stream` · `report_agent.py:2005` → `ReportManager::get_console_log_stream(&self, report_id) -> Vec<String>`
- [x] S-779 · `unit:U-024` · `method` · `ReportManager.get_agent_log` · incremental, from_line param · `report_agent.py:2019` → `ReportManager::get_agent_log(&self, report_id, from_line)` → `Map{logs,total_lines,from_line,has_more}`; invalid JSON lines silently skipped
- [x] S-780 · `unit:U-024` · `method` · `ReportManager.get_agent_log_stream` · `report_agent.py:2067` → `ReportManager::get_agent_log_stream(&self, report_id) -> Vec<Value>`
- [x] S-781 · `unit:U-024` · `method` · `ReportManager.save_outline` · `report_agent.py:2081` → `ReportManager::save_outline(&self, report_id, outline)`; `serde_json::to_string_pretty` (indent=2, non-ASCII unescaped)
- [x] S-782 · `unit:U-024` · `method` · `ReportManager.save_section` · `report_agent.py:2095` → `ReportManager::save_section(&self, report_id, section_index, section)` → `PathBuf`; calls `clean_section_content`
- [x] S-783 · `unit:U-024` · `method` · `ReportManager._clean_section_content` · `report_agent.py:2132` → `ReportManager::clean_section_content(&self, content, section_title)` — heading regex, dup-title-in-5-lines drop, all-headings→bold, leading blank/separator strip
- [x] S-784 · `unit:U-024` · `method` · `ReportManager.update_progress` · `report_agent.py:2200` → `ReportManager::update_progress(&self, report_id, status, progress, message, current_section, completed_sections)`; writes `progress.json`. **h1 RE-VERIFY PASS 2026-06-18:** `progress` param widened `u32`→`i32` (failed-path `-1` parity fix, py:1753). Raw progress.json dumped from teri's own `update_progress` is BYTE-IDENTICAL to Python `json.dumps(ensure_ascii=False,indent=2)`: key order status/progress/message/current_section/completed_sections/updated_at (serde_json `preserve_order` ON → IndexMap insertion order == Python dict), `"progress": -1` as JSON integer (not string, not u32-overflow), `current_section: null`, `completed_sections: []`/array layout, `updated_at` naive isoformat (no offset) == `datetime.now().isoformat()`. Normal 0..100 unchanged. 2 new tests (negative_one_failed_path, normal_range_still_works) green.
- [x] S-785 · `unit:U-024` · `method` · `ReportManager.get_progress` · `report_agent.py:2229` → `ReportManager::get_progress(&self, report_id) -> Option<Map>`; returns `None` if file missing
- [x] S-786 · `unit:U-024` · `method` · `ReportManager.get_generated_sections` · `report_agent.py:2240` → `ReportManager::get_generated_sections(&self, report_id) -> Vec<Map>`; sorted by filename; returns `[]` if folder missing
- [x] S-787 · `unit:U-024` · `method` · `ReportManager.assemble_full_report` · `report_agent.py:2271` → `ReportManager::assemble_full_report(&self, report_id, outline) -> io::Result<String>`; writes `full_report.md`
- [x] S-788 · `unit:U-024` · `method` · `ReportManager._post_process_report` · `report_agent.py:2301` → `ReportManager::post_process_report(&self, content, outline)` — dup-heading (last-5 window), level1/2/3 handling, separator-after-heading skip, blank-collapse-to-2
- [x] S-789 · `unit:U-024` · `method` · `ReportManager.save_report` · `report_agent.py:2427` → `ReportManager::save_report(&self, report)` — writes meta.json + outline.json + full_report.md
- [x] S-790 · `unit:U-024` · `method` · `ReportManager.get_report` · `report_agent.py:2447` → `ReportManager::get_report(&self, report_id) -> Option<Report>`; old-format `{id}.json` fallback; markdown_content falls back to full_report.md
- [x] S-791 · `unit:U-024` · `method` · `ReportManager.get_report_by_simulation` · `report_agent.py:2500` → `ReportManager::get_report_by_simulation(&self, simulation_id) -> Option<Report>`; scans both folder and `.json` formats
- [x] S-792 · `unit:U-024` · `method` · `ReportManager.list_reports` · `report_agent.py:2521` → `ReportManager::list_reports(&self, simulation_id, limit) -> Vec<Report>`; sorted by created_at desc; old+new format
- [x] S-793 · `unit:U-024` · `method` · `ReportManager.delete_report` · `report_agent.py:2548` → `ReportManager::delete_report(&self, report_id) -> bool`; removes folder (new format) or flat files (old format)

---

## U-025 — `backend/app/api/graph.py` (Blueprint: `/api/graph`)

- [x] S-794 · `unit:U-025` · `route` · `GET /project/<project_id>` · handler `get_project` · `graph.py:37` · **PARITY-VERIFIED 2026-06-18 (opus PASS).** 200 `{success,data:to_dict}` (data key order byte-matches Python to_dict, 15 keys); 404 `{success,error}` (2-key, no traceback, msg=`api.projectNotFound` w/ id); corrupt-json→500 3-key (Flask no try/except → uncaught exception=500, faithful). Driven via real HTTP through `create_app`.
- [x] S-795 · `unit:U-025` · `route` · `GET /project/list` · handler `list_projects` · `graph.py:56` · **PARITY-VERIFIED 2026-06-18 (opus PASS).** 200 `{success,data:[…],count}` (key order success,data,count); `?limit` Flask `type=int` semantics replicated: absent→50, `?limit=abc`→50 (NOT 400), `?limit=N`→N; list order created_at desc (ProjectManager U-011). ROUTE-ORDER proven: `/project/list` resolves to `list_projects`, NOT `get_project("list")`, via axum static-before-capture through the real HTTP path.
- [x] S-796 · `unit:U-025` · `route` · `DELETE /project/<project_id>` · handler `delete_project` · `graph.py:71` · **PARITY-VERIFIED 2026-06-18 (opus PASS).** Ok(true)→200 `{success,message:api.projectDeleted w/id}`; Ok(false)→404 `{success,error:api.projectDeleteFailed w/id}` (2-key). Note: teri `delete_project`→`Ok(false)` on absent (NOT Err) per S-133 — faithful to Python `return False`.
- [x] S-797 · `unit:U-025` · `route` · `POST /project/<project_id>/reset` · handler `reset_project` · `graph.py:90` · **PARITY-VERIFIED 2026-06-18 (opus PASS).** Missing→404; else status machine `ontology.is_some()?OntologyGenerated:Created`, clears graph_id/graph_build_task_id/error, save, 200 `{success,message:api.projectReset,data:to_dict}` (key order success,message,data). Verified through HTTP: graph_id serializes null, status `"created"`/`"ontology_generated"`, data key order matches Python to_dict.
- [x] S-798 · `unit:U-025` · `route` · `POST /ontology/generate` · multipart: files+simulation_requirement+project_name · `graph.py:123`
- [x] S-799 · `unit:U-025` · `route` · `POST /build` · json: project_id, graph_name → build_graph_async · `graph.py:261`
- [x] S-800 · `unit:U-025` · `route` · `GET /task/<task_id>` · TaskManager status · `graph.py:535`
- [x] S-801 · `unit:U-025` · `route` · `GET /tasks` · list all tasks · `graph.py:554`
- [x] S-802 · `unit:U-025` · `route` · `GET /data/<graph_id>` · GraphBuilderService.get_graph_data · `graph.py:570` → `teri::api::graph::get_graph_data` (`src/api/graph.rs`). **PARITY-VERIFIED 2026-06-18 ROUND-2 (opus PASS).** MAP-ONTO graph_id==task_id; reshapes task result["graph"] into Python's {graph_id,nodes[exactly 6-key],edges[exactly 14-key],node_count,edge_count}; node_count/edge_count == array lengths. `[≠] U025-ZEP-TEMPORAL` defaults verified (node summary/attributes/created_at; edge fact/attributes/created_at/expired_at/episodes); valid_at/invalid_at from teri temporal model. **Edge uuid determinism FIXED + verified:** v5(NAMESPACE_OID, "{src}|{tgt}|{kind}|{valid_at_key}") — pure function of fixed inputs (independently cross-computed offline = `0c149838-c96f-5f70-80dc-c1e7123a93ed` for the seeded RelatedTo edge). New test `get_graph_data_edge_uuid_deterministic_across_requests` drives `GET /data/:id` TWICE via the real HTTP app (oneshot) and asserts `edges[].uuid` identical across responses (no longer per-request v4 nondeterminism that orphaned Vue :key). Parallel-edge collision (same src/tgt/kind/window → same uuid) documented + acceptable (indistinguishable in teri's model; not silent loss). `[!] U025-GRAPHSTORE` (task-result-backed, acceptable).
- [x] S-803 · `unit:U-025` · `route` · `DELETE /delete/<graph_id>` · handler `delete_graph` · `graph.py:598` → `teri::api::graph::delete_graph` (`src/api/graph.rs`). **PARITY-VERIFIED 2026-06-18 ROUND-2 (opus PASS).** ZEP guard 500 (2-key, no traceback) + success envelope {success,message:api.graphDeleted(id)} verified via HTTP. `[!] U025-GRAPHSTORE`: TaskManager has no remove_task → no-op delete (task persists; faithful to Python's fire-and-forget Zep delete that never 404s an absent graph). Acceptable `[!]` (substrate-inexpressible durable graph store; observable response output preserved).

---

## U-026 — `backend/app/api/simulation.py` (Blueprint: `/api/simulation`)

- [ ] S-804 · `unit:U-026` · `route` · `GET /entities/<graph_id>` · get_graph_entities · `simulation.py:49`
- [ ] S-805 · `unit:U-026` · `route` · `GET /entities/<graph_id>/<entity_uuid>` · get_entity_detail · `simulation.py:94`
- [ ] S-806 · `unit:U-026` · `route` · `GET /entities/<graph_id>/by-type/<entity_type>` · get_entities_by_type · `simulation.py:127`
- [ ] S-807 · `unit:U-026` · `route` · `POST /create` · create_simulation · `simulation.py:166`
- [ ] S-808 · `unit:U-026` · `route` · `POST /prepare` · prepare_simulation async task · `simulation.py:360`
- [ ] S-809 · `unit:U-026` · `route` · `POST /prepare/status` · get_prepare_status · `simulation.py:643`
- [ ] S-810 · `unit:U-026` · `route` · `GET /<simulation_id>` · get_simulation · `simulation.py:756`
- [ ] S-811 · `unit:U-026` · `route` · `GET /list` · list_simulations · `simulation.py:789`
- [ ] S-812 · `unit:U-026` · `route` · `GET /history` · get_simulation_history · `simulation.py:877`
- [ ] S-813 · `unit:U-026` · `route` · `GET /<simulation_id>/profiles` · get_simulation_profiles · `simulation.py:991`
- [ ] S-814 · `unit:U-026` · `route` · `GET /<simulation_id>/profiles/realtime` · get_simulation_profiles_realtime · `simulation.py:1029`
- [ ] S-815 · `unit:U-026` · `route` · `GET /<simulation_id>/config/realtime` · get_simulation_config_realtime · `simulation.py:1139`
- [ ] S-816 · `unit:U-026` · `route` · `GET /<simulation_id>/config` · get_simulation_config · `simulation.py:1259`
- [ ] S-817 · `unit:U-026` · `route` · `GET /<simulation_id>/config/download` · download_simulation_config · `simulation.py:1295`
- [ ] S-818 · `unit:U-026` · `route` · `GET /script/<script_name>/download` · download_simulation_script · `simulation.py:1324`
- [ ] S-819 · `unit:U-026` · `route` · `POST /generate-profiles` · generate_profiles · `simulation.py:1378`
- [ ] S-820 · `unit:U-026` · `route` · `POST /start` · start_simulation · `simulation.py:1452`
- [ ] S-821 · `unit:U-026` · `route` · `POST /stop` · stop_simulation · `simulation.py:1645`
- [ ] S-822 · `unit:U-026` · `route` · `GET /<simulation_id>/run-status` · get_run_status · `simulation.py:1706`
- [ ] S-823 · `unit:U-026` · `route` · `GET /<simulation_id>/run-status/detail` · get_run_status_detail · `simulation.py:1764`
- [ ] S-824 · `unit:U-026` · `route` · `GET /<simulation_id>/actions` · get_simulation_actions · `simulation.py:1865`
- [ ] S-825 · `unit:U-026` · `route` · `GET /<simulation_id>/timeline` · get_simulation_timeline · `simulation.py:1919`
- [ ] S-826 · `unit:U-026` · `route` · `GET /<simulation_id>/agent-stats` · get_agent_stats · `simulation.py:1959`
- [ ] S-827 · `unit:U-026` · `route` · `GET /<simulation_id>/posts` · get_simulation_posts · `simulation.py:1988`
- [ ] S-828 · `unit:U-026` · `route` · `GET /<simulation_id>/comments` · get_simulation_comments · `simulation.py:2066`
- [ ] S-829 · `unit:U-026` · `route` · `POST /interview` · interview_agent · `simulation.py:2143`
- [ ] S-830 · `unit:U-026` · `route` · `POST /interview/batch` · interview_agents_batch · `simulation.py:2272`
- [ ] S-831 · `unit:U-026` · `route` · `POST /interview/all` · interview_all_agents · `simulation.py:2410`
- [ ] S-832 · `unit:U-026` · `route` · `POST /interview/history` · get_interview_history · `simulation.py:2513`
- [ ] S-833 · `unit:U-026` · `route` · `POST /env-status` · get_env_status · `simulation.py:2585`
- [ ] S-834 · `unit:U-026` · `route` · `POST /close-env` · close_simulation_env · `simulation.py:2650`

---

## U-027 — `backend/app/api/report.py` (Blueprint: `/api/report`)

- [ ] S-835 · `unit:U-027` · `route` · `POST /generate` · generate_report async; pre-generates report_id · `report.py:26`
- [ ] S-836 · `unit:U-027` · `route` · `POST /generate/status` · get_generate_status · `report.py:204`
- [ ] S-837 · `unit:U-027` · `route` · `GET /<report_id>` · get_report · `report.py:278`
- [ ] S-838 · `unit:U-027` · `route` · `GET /by-simulation/<simulation_id>` · get_report_by_simulation · `report.py:320`
- [ ] S-839 · `unit:U-027` · `route` · `GET /list` · list_reports · `report.py:359`
- [ ] S-840 · `unit:U-027` · `route` · `GET /<report_id>/download` · download_report · `report.py:399`
- [ ] S-841 · `unit:U-027` · `route` · `DELETE /<report_id>` · delete_report · `report.py:445`
- [ ] S-842 · `unit:U-027` · `route` · `POST /chat` · chat_with_report_agent · `report.py:473`
- [ ] S-843 · `unit:U-027` · `route` · `GET /<report_id>/progress` · get_report_progress · `report.py:570`
- [ ] S-844 · `unit:U-027` · `route` · `GET /<report_id>/sections` · get_report_sections · `report.py:611`
- [ ] S-845 · `unit:U-027` · `route` · `GET /<report_id>/section/<int:section_index>` · get_single_section · `report.py:662`
- [ ] S-846 · `unit:U-027` · `route` · `GET /check/<simulation_id>` · check_report_status · `report.py:708`
- [ ] S-847 · `unit:U-027` · `route` · `GET /<report_id>/agent-log` · get_agent_log; from_line incremental · `report.py:759`
- [ ] S-848 · `unit:U-027` · `route` · `GET /<report_id>/agent-log/stream` · stream_agent_log · `report.py:818`
- [ ] S-849 · `unit:U-027` · `route` · `GET /<report_id>/console-log` · get_console_log; from_line incremental · `report.py:854`
- [ ] S-850 · `unit:U-027` · `route` · `GET /<report_id>/console-log/stream` · stream_console_log · `report.py:900`
- [ ] S-851 · `unit:U-027` · `route` · `POST /tools/search` · search_graph_tool · `report.py:936`
- [ ] S-852 · `unit:U-027` · `route` · `POST /tools/statistics` · get_graph_statistics_tool · `report.py:984`

---

## U-028 — `backend/scripts/run_twitter_simulation.py`

- [ ] S-853 · `unit:U-028` · `type` · `UnicodeFormatter` · logging formatter decoding Unicode escapes · `run_twitter_simulation.py:53`
- [ ] S-854 · `unit:U-028` · `field` · `UnicodeFormatter.UNICODE_ESCAPE_PATTERN` · `run_twitter_simulation.py:56`
- [ ] S-855 · `unit:U-028` · `method` · `UnicodeFormatter.format` · `run_twitter_simulation.py:58`
- [ ] S-856 · `unit:U-028` · `type` · `MaxTokensWarningFilter` · suppresses camel-ai max_tokens warnings · `run_twitter_simulation.py:70`
- [ ] S-857 · `unit:U-028` · `method` · `MaxTokensWarningFilter.filter` · `run_twitter_simulation.py:73`
- [ ] S-858 · `unit:U-028` · `fn` · `setup_oasis_logging` · configures OASIS library loggers to files · `run_twitter_simulation.py:84`
- [ ] S-859 · `unit:U-028` · `type` · `CommandType` · enum (local copy): INTERVIEW/BATCH_INTERVIEW/CLOSE_ENV · `run_twitter_simulation.py:139`
- [ ] S-860 · `unit:U-028` · `type` · `IPCHandler` · subprocess-side IPC handler · `run_twitter_simulation.py:146`
- [ ] S-861 · `unit:U-028` · `method` · `IPCHandler.__init__` · `run_twitter_simulation.py:149`
- [ ] S-862 · `unit:U-028` · `method` · `IPCHandler.update_status` · writes env_status.json · `run_twitter_simulation.py:162`
- [ ] S-863 · `unit:U-028` · `method` · `IPCHandler.poll_command` · scans ipc_commands/ · `run_twitter_simulation.py:170`
- [ ] S-864 · `unit:U-028` · `method` · `IPCHandler.send_response` · writes response JSON · `run_twitter_simulation.py:193`
- [ ] S-865 · `unit:U-028` · `method` · `IPCHandler.handle_interview` · calls OASIS env.interview_agent · `run_twitter_simulation.py:214`
- [ ] S-866 · `unit:U-028` · `method` · `IPCHandler.handle_batch_interview` · `run_twitter_simulation.py:248`
- [ ] S-867 · `unit:U-028` · `method` · `IPCHandler._get_interview_result` · `run_twitter_simulation.py:300`
- [ ] S-868 · `unit:U-028` · `method` · `IPCHandler.process_commands` · asyncio poll loop, 0.5s interval · `run_twitter_simulation.py:343`
- [ ] S-869 · `unit:U-028` · `type` · `TwitterSimulationRunner` · `run_twitter_simulation.py:385`
- [ ] S-870 · `unit:U-028` · `field` · `TwitterSimulationRunner.AVAILABLE_ACTIONS` · `run_twitter_simulation.py:389`
- [ ] S-871 · `unit:U-028` · `method` · `TwitterSimulationRunner.__init__` · `run_twitter_simulation.py:398`
- [ ] S-872 · `unit:U-028` · `method` · `TwitterSimulationRunner._load_config` · `run_twitter_simulation.py:414`
- [ ] S-873 · `unit:U-028` · `method` · `TwitterSimulationRunner._get_profile_path` · `run_twitter_simulation.py:419`
- [ ] S-874 · `unit:U-028` · `method` · `TwitterSimulationRunner._get_db_path` · `run_twitter_simulation.py:423`
- [ ] S-875 · `unit:U-028` · `method` · `TwitterSimulationRunner._create_model` · configures OASIS LLM model · `run_twitter_simulation.py:427`
- [ ] S-876 · `unit:U-028` · `method` · `TwitterSimulationRunner._get_active_agents_for_round` · filters by activity schedule · `run_twitter_simulation.py:462`
- [ ] S-877 · `unit:U-028` · `method` · `TwitterSimulationRunner.run` · async main loop; wait-for-commands mode after rounds · `run_twitter_simulation.py:531`
- [ ] S-878 · `unit:U-028` · `fn` · `main` · entry-point; sets up asyncio loop · `run_twitter_simulation.py:707`
- [ ] S-879 · `unit:U-028` · `fn` · `setup_signal_handlers` · SIGTERM/SIGINT/SIGHUP → graceful shutdown · `run_twitter_simulation.py:749`

---

## U-029 — `backend/scripts/run_reddit_simulation.py`

- [ ] S-880 · `unit:U-029` · `type` · `UnicodeFormatter` · (mirror of U-028) · `run_reddit_simulation.py:53`
- [ ] S-881 · `unit:U-029` · `field` · `UnicodeFormatter.UNICODE_ESCAPE_PATTERN` · `run_reddit_simulation.py:56`
- [ ] S-882 · `unit:U-029` · `method` · `UnicodeFormatter.format` · `run_reddit_simulation.py:58`
- [ ] S-883 · `unit:U-029` · `type` · `MaxTokensWarningFilter` · `run_reddit_simulation.py:70`
- [ ] S-884 · `unit:U-029` · `method` · `MaxTokensWarningFilter.filter` · `run_reddit_simulation.py:73`
- [ ] S-885 · `unit:U-029` · `fn` · `setup_oasis_logging` · `run_reddit_simulation.py:84`
- [ ] S-886 · `unit:U-029` · `type` · `CommandType` · `run_reddit_simulation.py:139`
- [ ] S-887 · `unit:U-029` · `type` · `IPCHandler` · `run_reddit_simulation.py:146`
- [ ] S-888 · `unit:U-029` · `method` · `IPCHandler.__init__` · `run_reddit_simulation.py:149`
- [ ] S-889 · `unit:U-029` · `method` · `IPCHandler.update_status` · `run_reddit_simulation.py:162`
- [ ] S-890 · `unit:U-029` · `method` · `IPCHandler.poll_command` · `run_reddit_simulation.py:170`
- [ ] S-891 · `unit:U-029` · `method` · `IPCHandler.send_response` · `run_reddit_simulation.py:193`
- [ ] S-892 · `unit:U-029` · `method` · `IPCHandler.handle_interview` · `run_reddit_simulation.py:214`
- [ ] S-893 · `unit:U-029` · `method` · `IPCHandler.handle_batch_interview` · `run_reddit_simulation.py:248`
- [ ] S-894 · `unit:U-029` · `method` · `IPCHandler._get_interview_result` · `run_reddit_simulation.py:300`
- [ ] S-895 · `unit:U-029` · `method` · `IPCHandler.process_commands` · `run_reddit_simulation.py:343`
- [ ] S-896 · `unit:U-029` · `type` · `RedditSimulationRunner` · `run_reddit_simulation.py:385`
- [ ] S-897 · `unit:U-029` · `field` · `RedditSimulationRunner.AVAILABLE_ACTIONS` · `run_reddit_simulation.py:389`
- [ ] S-898 · `unit:U-029` · `method` · `RedditSimulationRunner.__init__` · `run_reddit_simulation.py:405`
- [ ] S-899 · `unit:U-029` · `method` · `RedditSimulationRunner._load_config` · `run_reddit_simulation.py:421`
- [ ] S-900 · `unit:U-029` · `method` · `RedditSimulationRunner._get_profile_path` · `run_reddit_simulation.py:426`
- [ ] S-901 · `unit:U-029` · `method` · `RedditSimulationRunner._get_db_path` · `run_reddit_simulation.py:430`
- [ ] S-902 · `unit:U-029` · `method` · `RedditSimulationRunner._create_model` · `run_reddit_simulation.py:434`
- [ ] S-903 · `unit:U-029` · `method` · `RedditSimulationRunner._get_active_agents_for_round` · `run_reddit_simulation.py:469`
- [ ] S-904 · `unit:U-029` · `method` · `RedditSimulationRunner.run` · `run_reddit_simulation.py:523`
- [ ] S-905 · `unit:U-029` · `fn` · `main` · `run_reddit_simulation.py:695`
- [ ] S-906 · `unit:U-029` · `fn` · `setup_signal_handlers` · `run_reddit_simulation.py:737`

---

## U-030 — `backend/scripts/run_parallel_simulation.py`

- [ ] S-907 · `unit:U-030` · `type` · `MaxTokensWarningFilter` · `run_parallel_simulation.py:106`
- [ ] S-908 · `unit:U-030` · `method` · `MaxTokensWarningFilter.filter` · `run_parallel_simulation.py:109`
- [ ] S-909 · `unit:U-030` · `fn` · `disable_oasis_logging` · `run_parallel_simulation.py:120`
- [ ] S-910 · `unit:U-030` · `fn` · `init_logging_for_simulation` · `run_parallel_simulation.py:141`
- [ ] S-911 · `unit:U-030` · `const` · `TWITTER_ACTIONS` · `run_parallel_simulation.py:178`
- [ ] S-912 · `unit:U-030` · `const` · `REDDIT_ACTIONS` · `run_parallel_simulation.py:188`
- [ ] S-913 · `unit:U-030` · `type` · `CommandType` · `run_parallel_simulation.py:210`
- [ ] S-914 · `unit:U-030` · `type` · `ParallelIPCHandler` · handles interview commands for both platforms · `run_parallel_simulation.py:217`
- [ ] S-915 · `unit:U-030` · `method` · `ParallelIPCHandler.__init__` · `run_parallel_simulation.py:224`
- [ ] S-916 · `unit:U-030` · `method` · `ParallelIPCHandler.update_status` · `run_parallel_simulation.py:246`
- [ ] S-917 · `unit:U-030` · `method` · `ParallelIPCHandler.poll_command` · `run_parallel_simulation.py:256`
- [ ] S-918 · `unit:U-030` · `method` · `ParallelIPCHandler.send_response` · `run_parallel_simulation.py:279`
- [ ] S-919 · `unit:U-030` · `method` · `ParallelIPCHandler._get_env_and_graph` · `run_parallel_simulation.py:300`
- [ ] S-920 · `unit:U-030` · `method` · `ParallelIPCHandler._interview_single_platform` · `run_parallel_simulation.py:317`
- [ ] S-921 · `unit:U-030` · `method` · `ParallelIPCHandler.handle_interview` · `run_parallel_simulation.py:345`
- [ ] S-922 · `unit:U-030` · `method` · `ParallelIPCHandler.handle_batch_interview` · `run_parallel_simulation.py:416`
- [ ] S-923 · `unit:U-030` · `method` · `ParallelIPCHandler._get_interview_result` · `run_parallel_simulation.py:517`
- [ ] S-924 · `unit:U-030` · `method` · `ParallelIPCHandler.process_commands` · `run_parallel_simulation.py:560`
- [ ] S-925 · `unit:U-030` · `fn` · `load_config` · `run_parallel_simulation.py:604`
- [ ] S-926 · `unit:U-030` · `const` · `FILTERED_ACTIONS` · `run_parallel_simulation.py:611`
- [ ] S-927 · `unit:U-030` · `const` · `ACTION_TYPE_MAP` · `run_parallel_simulation.py:614`
- [ ] S-928 · `unit:U-030` · `fn` · `get_agent_names_from_config` · `run_parallel_simulation.py:633`
- [ ] S-929 · `unit:U-030` · `fn` · `fetch_new_actions_from_db` · polls SQLite for new action rows · `run_parallel_simulation.py:657`
- [ ] S-930 · `unit:U-030` · `fn` · `_enrich_action_context` · enriches action_args with post/user content from DB · `run_parallel_simulation.py:749`
- [ ] S-931 · `unit:U-030` · `fn` · `_get_post_info` · `run_parallel_simulation.py:857`
- [ ] S-932 · `unit:U-030` · `fn` · `_get_user_name` · `run_parallel_simulation.py:903`
- [ ] S-933 · `unit:U-030` · `fn` · `_get_comment_info` · `run_parallel_simulation.py:938`
- [ ] S-934 · `unit:U-030` · `fn` · `create_model` · configures OASIS model · `run_parallel_simulation.py:984`
- [ ] S-935 · `unit:U-030` · `fn` · `get_active_agents_for_round` · `run_parallel_simulation.py:1040`
- [ ] S-936 · `unit:U-030` · `type` · `PlatformSimulation` · dataclass: env + runner per platform · `run_parallel_simulation.py:1093`
- [ ] S-937 · `unit:U-030` · `method` · `PlatformSimulation.__init__` · `run_parallel_simulation.py:1095`
- [ ] S-938 · `unit:U-030` · `fn` · `run_twitter_simulation` · coroutine for asyncio.gather · `run_parallel_simulation.py:1101`
- [ ] S-939 · `unit:U-030` · `fn` · `run_reddit_simulation` · coroutine for asyncio.gather · `run_parallel_simulation.py:1293`
- [ ] S-940 · `unit:U-030` · `fn` · `main` · asyncio.gather both platforms · `run_parallel_simulation.py:1492`
- [ ] S-941 · `unit:U-030` · `fn` · `setup_signal_handlers` · `run_parallel_simulation.py:1653`

---

## U-031 — `frontend/src/api/index.js`

- [ ] S-942 · `unit:U-031` · `fn` · `requestWithRetry` · retry wrapper (exponential back-off, default 3 retries) · `api/index.js:56`
- [ ] S-943 · `unit:U-031` · `const` · `service` (default export) · axios instance; baseURL=VITE_API_BASE_URL; Accept-Language interceptor · `api/index.js:5`

---

## U-032 — `frontend/src/api/graph.js`

- [ ] S-944 · `unit:U-032` · `fn` · `generateOntology` · POST /graph/ontology/generate (multipart) · `api/graph.js:8`
- [ ] S-945 · `unit:U-032` · `fn` · `buildGraph` · POST /graph/build · `api/graph.js:26`
- [ ] S-946 · `unit:U-032` · `fn` · `getTaskStatus` · GET /graph/task/<taskId> · `api/graph.js:41`
- [ ] S-947 · `unit:U-032` · `fn` · `getGraphData` · GET /graph/data/<graphId> · `api/graph.js:53`
- [ ] S-948 · `unit:U-032` · `fn` · `getProject` · GET /graph/project/<projectId> · `api/graph.js:65`

---

## U-033 — `frontend/src/api/simulation.js`

- [ ] S-949 · `unit:U-033` · `fn` · `createSimulation` · POST /simulation/create · `api/simulation.js:7`
- [ ] S-950 · `unit:U-033` · `fn` · `prepareSimulation` · POST /simulation/prepare · `api/simulation.js:15`
- [ ] S-951 · `unit:U-033` · `fn` · `getPrepareStatus` · POST /simulation/prepare/status · `api/simulation.js:23`
- [ ] S-952 · `unit:U-033` · `fn` · `getSimulation` · GET /simulation/<id> · `api/simulation.js:31`
- [ ] S-953 · `unit:U-033` · `fn` · `getSimulationProfiles` · GET /simulation/<id>/profiles · `api/simulation.js:40`
- [ ] S-954 · `unit:U-033` · `fn` · `getSimulationProfilesRealtime` · GET /simulation/<id>/profiles/realtime · `api/simulation.js:49`
- [ ] S-955 · `unit:U-033` · `fn` · `getSimulationConfig` · GET /simulation/<id>/config · `api/simulation.js:57`
- [ ] S-956 · `unit:U-033` · `fn` · `getSimulationConfigRealtime` · GET /simulation/<id>/config/realtime · `api/simulation.js:66`
- [ ] S-957 · `unit:U-033` · `fn` · `listSimulations` · GET /simulation/list · `api/simulation.js:74`
- [ ] S-958 · `unit:U-033` · `fn` · `startSimulation` · POST /simulation/start · `api/simulation.js:83`
- [ ] S-959 · `unit:U-033` · `fn` · `stopSimulation` · POST /simulation/stop · `api/simulation.js:91`
- [ ] S-960 · `unit:U-033` · `fn` · `getRunStatus` · GET /simulation/<id>/run-status · `api/simulation.js:99`
- [ ] S-961 · `unit:U-033` · `fn` · `getRunStatusDetail` · GET /simulation/<id>/run-status/detail · `api/simulation.js:107`
- [ ] S-962 · `unit:U-033` · `fn` · `getSimulationPosts` · GET /simulation/<id>/posts · `api/simulation.js:118`
- [ ] S-963 · `unit:U-033` · `fn` · `getSimulationTimeline` · GET /simulation/<id>/timeline · `api/simulation.js:130`
- [ ] S-964 · `unit:U-033` · `fn` · `getAgentStats` · GET /simulation/<id>/agent-stats · `api/simulation.js:142`
- [ ] S-965 · `unit:U-033` · `fn` · `getSimulationActions` · GET /simulation/<id>/actions · `api/simulation.js:151`
- [ ] S-966 · `unit:U-033` · `fn` · `closeSimulationEnv` · POST /simulation/close-env · `api/simulation.js:159`
- [ ] S-967 · `unit:U-033` · `fn` · `getEnvStatus` · POST /simulation/env-status · `api/simulation.js:167`
- [ ] S-968 · `unit:U-033` · `fn` · `interviewAgents` · POST /simulation/interview · `api/simulation.js:175`
- [ ] S-969 · `unit:U-033` · `fn` · `getSimulationHistory` · GET /simulation/history · `api/simulation.js:184`

---

## U-034 — `frontend/src/api/report.js`

- [ ] S-970 · `unit:U-034` · `fn` · `generateReport` · POST /report/generate · `api/report.js:7`
- [ ] S-971 · `unit:U-034` · `fn` · `getReportStatus` · POST /report/generate/status · `api/report.js:15`
- [ ] S-972 · `unit:U-034` · `fn` · `getAgentLog` · GET /report/<id>/agent-log?from_line= · `api/report.js:24`
- [ ] S-973 · `unit:U-034` · `fn` · `getConsoleLog` · GET /report/<id>/console-log?from_line= · `api/report.js:33`
- [ ] S-974 · `unit:U-034` · `fn` · `getReport` · GET /report/<id> · `api/report.js:41`
- [ ] S-975 · `unit:U-034` · `fn` · `chatWithReport` · POST /report/chat · `api/report.js:49`

---

## U-035 — `frontend/src/store/pendingUpload.js`

- [ ] S-976 · `unit:U-035` · `fn` · `setPendingUpload` · sets reactive state files+requirement+isPending=true · `store/pendingUpload.js:13`
- [ ] S-977 · `unit:U-035` · `fn` · `getPendingUpload` · returns {files, simulationRequirement, isPending} · `store/pendingUpload.js:19`
- [ ] S-978 · `unit:U-035` · `fn` · `clearPendingUpload` · resets state · `store/pendingUpload.js:27`
- [ ] S-979 · `unit:U-035` · `const` · `state` (default export) · reactive store object · `store/pendingUpload.js:7`

---

## U-036 — `frontend/src/i18n/index.js`

- [ ] S-980 · `unit:U-036` · `const` · `availableLocales` · array of {key, label} from languages.json · `i18n/index.js:26`
- [ ] S-981 · `unit:U-036` · `const` · `i18n` (default export) · vue-i18n instance; fallbackLocale=zh; reads localStorage · `i18n/index.js:19`

---

## U-037 — `frontend/src/router/index.js`

- [ ] S-982 · `unit:U-037` · `route` · `/` · Home · `router/index.js:10`
- [ ] S-983 · `unit:U-037` · `route` · `/process/:projectId` · MainView (props:true) · `router/index.js:16`
- [ ] S-984 · `unit:U-037` · `route` · `/simulation/:simulationId` · SimulationView (props:true) · `router/index.js:22`
- [ ] S-985 · `unit:U-037` · `route` · `/simulation/:simulationId/start` · SimulationRunView (props:true) · `router/index.js:28`
- [ ] S-986 · `unit:U-037` · `route` · `/report/:reportId` · ReportView (props:true) · `router/index.js:34`
- [ ] S-987 · `unit:U-037` · `route` · `/interaction/:reportId` · InteractionView (props:true) · `router/index.js:40`
- [ ] S-988 · `unit:U-037` · `const` · `router` (default export) · createWebHistory router · `router/index.js:47`

---

## U-038 — `frontend/src/views/Home.vue`

- [ ] S-989 · `unit:U-038` · `const` · `formData` · reactive simulation_requirement + files state · `Home.vue:10`
- [ ] S-990 · `unit:U-038` · `const` · `files` · ref([]) file list · `Home.vue:15`
- [ ] S-991 · `unit:U-038` · `const` · `loading` · `Home.vue:18`
- [ ] S-992 · `unit:U-038` · `const` · `error` · `Home.vue:19`
- [ ] S-993 · `unit:U-038` · `const` · `isDragOver` · `Home.vue:20`
- [ ] S-994 · `unit:U-038` · `const` · `canSubmit` · computed: formData.simulation_requirement && files.length > 0 · `Home.vue:26`
- [ ] S-995 · `unit:U-038` · `fn` · `triggerFileInput` · `Home.vue:31`
- [ ] S-996 · `unit:U-038` · `fn` · `handleFileSelect` · `Home.vue:38`
- [ ] S-997 · `unit:U-038` · `fn` · `handleDragOver` · `Home.vue:44`
- [ ] S-998 · `unit:U-038` · `fn` · `handleDragLeave` · `Home.vue:50`
- [ ] S-999 · `unit:U-038` · `fn` · `handleDrop` · `Home.vue:54`
- [ ] S-1000 · `unit:U-038` · `fn` · `addFiles` · validates ext, max 10 files, deduplicates · `Home.vue:63`
- [ ] S-1001 · `unit:U-038` · `fn` · `removeFile` · `Home.vue:72`
- [ ] S-1002 · `unit:U-038` · `fn` · `startSimulation` · calls setPendingUpload then routes to /process/new · `Home.vue:85`

---

## U-039 — `frontend/src/views/MainView.vue`

- [ ] S-1003 · `unit:U-039` · `const` · `viewMode` · split/graph/workbench · `MainView.vue:17`
- [ ] S-1004 · `unit:U-039` · `const` · `currentStep` · 1-5 · `MainView.vue:20`
- [ ] S-1005 · `unit:U-039` · `const` · `stepNames` · computed from i18n · `MainView.vue:21`
- [ ] S-1006 · `unit:U-039` · `fn` · `handleNextStep` · advances currentStep; routes for step >= 3 · `MainView.vue:86`
- [ ] S-1007 · `unit:U-039` · `fn` · `handleGoBack` · `MainView.vue:98`
- [ ] S-1008 · `unit:U-039` · `fn` · `initProject` · `MainView.vue:107`
- [ ] S-1009 · `unit:U-039` · `fn` · `handleNewProject` · uploads via generateOntology then polls · `MainView.vue:116`
- [ ] S-1010 · `unit:U-039` · `fn` · `loadProject` · `MainView.vue:156`
- [ ] S-1011 · `unit:U-039` · `fn` · `updatePhaseByStatus` · maps project status → currentPhase · `MainView.vue:188`
- [ ] S-1012 · `unit:U-039` · `fn` · `startBuildGraph` · `MainView.vue:198`
- [ ] S-1013 · `unit:U-039` · `fn` · `startGraphPolling` · `MainView.vue:219`
- [ ] S-1014 · `unit:U-039` · `fn` · `fetchGraphData` · `MainView.vue:225`
- [ ] S-1015 · `unit:U-039` · `fn` · `startPollingTask` · `MainView.vue:243`
- [ ] S-1016 · `unit:U-039` · `fn` · `pollTaskStatus` · `MainView.vue:248`
- [ ] S-1017 · `unit:U-039` · `fn` · `loadGraph` · `MainView.vue:284`
- [ ] S-1018 · `unit:U-039` · `fn` · `refreshGraph` · `MainView.vue:302`
- [ ] S-1019 · `unit:U-039` · `fn` · `stopPolling` · `MainView.vue:309`
- [ ] S-1020 · `unit:U-039` · `fn` · `stopGraphPolling` · `MainView.vue:316`
- [ ] S-1021 · `unit:U-039` · `fn` · `toggleMaximize` · `MainView.vue:78`

---

## U-040 — `frontend/src/views/SimulationView.vue`

- [ ] S-1022 · `unit:U-040` · `const` · `viewMode` · `SimulationView.vue:21`
- [ ] S-1023 · `unit:U-040` · `fn` · `handleGoBack` · checks env status, closes env if running, routes back · `SimulationView.vue:77`
- [ ] S-1024 · `unit:U-040` · `fn` · `handleNextStep` · routes to /simulation/<id>/start · `SimulationView.vue:86`
- [ ] S-1025 · `unit:U-040` · `fn` · `checkAndStopRunningSimulation` · `SimulationView.vue:117`
- [ ] S-1026 · `unit:U-040` · `fn` · `forceStopSimulation` · `SimulationView.vue:163`
- [ ] S-1027 · `unit:U-040` · `fn` · `loadSimulationData` · `SimulationView.vue:176`
- [ ] S-1028 · `unit:U-040` · `fn` · `loadGraph` · `SimulationView.vue:206`
- [ ] S-1029 · `unit:U-040` · `fn` · `refreshGraph` · `SimulationView.vue:221`
- [ ] S-1030 · `unit:U-040` · `fn` · `toggleMaximize` · `SimulationView.vue:69`

---

## U-041 — `frontend/src/views/SimulationRunView.vue`

- [ ] S-1031 · `unit:U-041` · `fn` · `handleGoBack` · stops sim env if still running before routing · `SimulationRunView.vue:82`
- [ ] S-1032 · `unit:U-041` · `fn` · `handleNextStep` · routes to /report/<report_id> · `SimulationRunView.vue:130`
- [ ] S-1033 · `unit:U-041` · `fn` · `loadSimulationData` · loads sim, config, project · `SimulationRunView.vue:137`
- [ ] S-1034 · `unit:U-041` · `fn` · `loadGraph` · `SimulationRunView.vue:178`
- [ ] S-1035 · `unit:U-041` · `fn` · `refreshGraph` · `SimulationRunView.vue:200`
- [ ] S-1036 · `unit:U-041` · `fn` · `startGraphRefresh` · `SimulationRunView.vue:209`
- [ ] S-1037 · `unit:U-041` · `fn` · `stopGraphRefresh` · `SimulationRunView.vue:216`
- [ ] S-1038 · `unit:U-041` · `fn` · `toggleMaximize` · `SimulationRunView.vue:74`
- [ ] S-1039 · `unit:U-041` · `const` · `isSimulating` · computed from currentStatus · `SimulationRunView.vue:58`

---

## U-042 — `frontend/src/views/ReportView.vue`

- [ ] S-1040 · `unit:U-042` · `fn` · `loadReportData` · loads report → sim → project + graph · `ReportView.vue:80`
- [ ] S-1041 · `unit:U-042` · `fn` · `loadGraph` · `ReportView.vue:119`
- [ ] S-1042 · `unit:U-042` · `fn` · `refreshGraph` · `ReportView.vue:135`
- [ ] S-1043 · `unit:U-042` · `fn` · `toggleMaximize` · `ReportView.vue:71`
- [ ] S-1044 · `unit:U-042` · `fn` · `addLog` · `ReportView.vue:58`
- [ ] S-1045 · `unit:U-042` · `fn` · `updateStatus` · `ReportView.vue:66`

---

## U-043 — `frontend/src/views/InteractionView.vue`

- [ ] S-1046 · `unit:U-043` · `fn` · `loadReportData` · `InteractionView.vue:81`
- [ ] S-1047 · `unit:U-043` · `fn` · `loadGraph` · `InteractionView.vue:120`
- [ ] S-1048 · `unit:U-043` · `fn` · `refreshGraph` · `InteractionView.vue:136`
- [ ] S-1049 · `unit:U-043` · `fn` · `toggleMaximize` · `InteractionView.vue:72`
- [ ] S-1050 · `unit:U-043` · `fn` · `addLog` · `InteractionView.vue:59`
- [ ] S-1051 · `unit:U-043` · `fn` · `updateStatus` · `InteractionView.vue:67`

---

## U-044 — `frontend/src/App.vue` + `frontend/src/main.js`

- [ ] S-1052 · `unit:U-044` · `const` · `App` (default export) · root Vue component, hosts RouterView · `App.vue:1`
- [ ] S-1053 · `unit:U-044` · `fn` · `main` (IIFE) · createApp(App).use(router).use(i18n).mount('#app') · `main.js:1`

---

## U-045 — Runtime: Subprocess Isolation Contract

- [ ] S-1054 · `unit:U-045` · `const` · `SUBPROCESS_ISOLATION_CONTRACT` · each simulation runs as a separate OS process group (pgid); killed via os.killpg(pgid, SIGTERM)→SIGKILL after 5 s; fully isolated from Flask process · `simulation_runner.py:390`

---

## U-046 — Runtime: IPC Polling Contract

- [ ] S-1055 · `unit:U-046` · `const` · `IPC_POLLING_CONTRACT` · filesystem-based IPC: parent writes JSON command files to ipc_commands/; subprocess polls 0.5 s interval; subprocess writes response + status JSON; no sockets, no pipes · `run_twitter_simulation.py:170`

---

## U-047 — Runtime: JSONL Tail-Read Contract

- [x] S-1056 · `unit:U-047` · `const` · `JSONL_TAIL_CONTRACT` · simulation output read by file offset (seek); server persists last_offset per reader; client polls with from_line param; never re-reads entire file · `simulation_runner.py:563` · **REALIZED HERE in U-022 sub-cycle (c)** via `read_action_log(log_path, position: u64, ...) -> u64` (`src/services/simulation_runner.rs`): per-file `u64` byte offset persisted across polls (the monitor's `twitter_position`/`reddit_position`), seek-to-offset + read-delta, only newline-terminated (complete) lines consumed, offset advances only past complete lines (partial last line preserved for next poll), never re-reads, never loses a line. Tests: `read_action_log_no_double_read_across_polls`, `read_action_log_partial_line_not_consumed`, `read_action_log_missing_file_returns_position_unchanged`, growth-between-polls. realized sub-cycle (c) 2026-06-17, **U-047 REALIZATION VERIFIED 2026-06-17** (all four invariants — no-double-read, partial-line-safety, offset-monotonic, missing-file/growth/IO-error robustness — proven by passing differential tests; offset == Python `f.tell()` on writer-produced input)

---

## U-048 — Runtime: Report Streaming Contract / in-band sim_end terminal signal (extend-Y)

- [x] S-1057 · `unit:U-048` · `const` · `REPORT_STREAMING_CONTRACT` · report sections generated one at a time (serial); each section appended to JSON on disk as completed; client polls /sections and /section/<n> to see partial progress; SSE log streams via /agent-log/stream + /console-log/stream · `report_agent.py:2100` · Rust: `src/api/mod.rs::TickStreamEvent::sim_end` + `src/sim/mod.rs::SimCompletion` + `src/sim/mod.rs::SimEngine::subscribe_completion` (extend-Y: additive completion channel on SimEngine; sim_end TickStreamEvent constructor; SSE wiring deferred to U-026)
- [x] S-1057-A · `unit:U-048` · `struct` · `SimCompletion` · terminal signal payload emitted by SimEngine::run() after the last snapshot; mirrors MiroFish action_logger.log_simulation_end (~line 105) / simulation_runner monitor (~line 623) · `simulation_runner.py:623` · Rust: `src/sim/mod.rs::SimCompletion` (`total_ticks: u32`, Serialize/Deserialize/Clone/PartialEq)
- [x] S-1057-B · `unit:U-048` · `method` · `SimEngine::subscribe_completion` · returns watch::Receiver<Option<SimCompletion>>; watch chosen so late subscribers (after run() returns) always observe the terminal Some(...) without a race; _completion_anchor keeps the channel alive so send() in run() always persists · `simulation_runner.py:623` · Rust: `src/sim/mod.rs::SimEngine::subscribe_completion`
- [x] S-1057-C · `unit:U-048` · `method` · `TickStreamEvent::sim_end` · constructor for the in-band SSE terminal frame: tick=total_ticks, data={"sim_end":true,"total_ticks":n}, event_id="sim-end"; mirrors lag_gap's sentinel-in-data encoding so SSE wire format stays uniform · `action_logger.py:105` · Rust: `src/api/mod.rs::TickStreamEvent::sim_end`

---

## U-049 — Runtime: Background Thread Concurrency Contract

- [ ] S-1058 · `unit:U-049` · `const` · `BACKGROUND_THREAD_CONTRACT` · SimulationRunner runs status-monitor daemon thread (non-blocking); SimulationManager runs prepare tasks via ThreadPoolExecutor; report generation runs via separate thread; Flask threads access shared state under threading.Lock · `simulation_manager.py:89`

---

## U-050 — Runtime: Signal Handling Contract

- [ ] S-1059 · `unit:U-050` · `const` · `SIGNAL_CONTRACT` · SIGTERM/SIGINT/SIGHUP all routed to graceful shutdown; subprocess runners register signal handlers before asyncio.run(); Flask SIGTERM propagates kill to child pgid then exits · `run_twitter_simulation.py:749`

---

## SWEEP-1 — Frontend Components (8 Vue SFCs)

- [ ] S-1060 · `unit:SWEEP-1` · `component` · `GraphPanel` · D3 force-graph; props: graphData, isSimulating, viewMode; emits: refresh, toggle-maximize; ~100 reactive symbols including renderGraph, entityTypes computed, drag handlers, self-loop expansion · `components/GraphPanel.vue:1`
- [ ] S-1061 · `unit:SWEEP-1` · `component` · `HistoryDatabase` · project history panel; animated card fan; ~75 reactive symbols including getCardStyle, containerStyle, loadProjects, deleteProject · `components/HistoryDatabase.vue:1`
- [ ] S-1062 · `unit:SWEEP-1` · `component` · `LanguageSwitcher` · dropdown for locale; 7 symbols: open, switcherRef, currentLabel, toggleDropdown, switchLocale, onClickOutside · `components/LanguageSwitcher.vue:1`
- [ ] S-1063 · `unit:SWEEP-1` · `component` · `Step1GraphBuild` · ontology/graph viewer; props: graphData, projectData; ~14 symbols including handleEnterEnvSetup, selectOntologyItem, graphStats · `components/Step1GraphBuild.vue:1`
- [ ] S-1064 · `unit:SWEEP-1` · `component` · `Step2EnvSetup` · profile generation + config; props: simulationId; emits: go-back, next-step; ~65 symbols including startPrepareSimulation, startPolling, profile display · `components/Step2EnvSetup.vue:1`
- [ ] S-1065 · `unit:SWEEP-1` · `component` · `Step3Simulation` · simulation run control; props: simulationId, maxRounds, minutesPerRound; emits: go-back, next-step; ~60 symbols including doStartSimulation, fetchRunStatus, actionFeed · `components/Step3Simulation.vue:1`
- [ ] S-1066 · `unit:SWEEP-1` · `component` · `Step4Report` · report progress + viewer; props: reportId; emits: add-log, update-status; ~252 symbols including section rendering, SSE polling, parseInsightForge, markdown render · `components/Step4Report.vue:1`
- [ ] S-1067 · `unit:SWEEP-1` · `component` · `Step5Interaction` · chat + batch survey; props: reportId; emits: add-log, update-status; ~90 symbols including selectChatTarget, sendMessage, surveyAgents, renderMarkdown · `components/Step5Interaction.vue:1`

---

## SWEEP-2 — Locales (`locales/en.json`, `locales/zh.json`)

- [ ] S-1068 · `unit:SWEEP-2` · `const` · `locale:common` · 20 leaf keys: cancel, confirm, save, close, delete, create, edit, loading, error, success, download, preview, search, refresh, back, next, done, required, optional, unknown · `locales/en.json:common`
- [ ] S-1069 · `unit:SWEEP-2` · `const` · `locale:meta` · app title / brand strings · `locales/en.json:meta`
- [ ] S-1070 · `unit:SWEEP-2` · `const` · `locale:nav` · navigation labels · `locales/en.json:nav`
- [ ] S-1071 · `unit:SWEEP-2` · `const` · `locale:home` · home page UI strings · `locales/en.json:home`
- [ ] S-1072 · `unit:SWEEP-2` · `const` · `locale:main` · main layout + step names · `locales/en.json:main`
- [ ] S-1073 · `unit:SWEEP-2` · `const` · `locale:step1` · graph build step strings · `locales/en.json:step1`
- [ ] S-1074 · `unit:SWEEP-2` · `const` · `locale:step2` · env setup step strings · `locales/en.json:step2`
- [ ] S-1075 · `unit:SWEEP-2` · `const` · `locale:step3` · simulation run step strings · `locales/en.json:step3`
- [ ] S-1076 · `unit:SWEEP-2` · `const` · `locale:step4` · report step strings · `locales/en.json:step4`
- [ ] S-1077 · `unit:SWEEP-2` · `const` · `locale:step5` · interaction step strings · `locales/en.json:step5`
- [ ] S-1078 · `unit:SWEEP-2` · `const` · `locale:graph` · graph panel strings · `locales/en.json:graph`
- [ ] S-1079 · `unit:SWEEP-2` · `const` · `locale:history` · history database strings · `locales/en.json:history`
- [ ] S-1080 · `unit:SWEEP-2` · `const` · `locale:api` · API error/status message strings · `locales/en.json:api`
- [ ] S-1081 · `unit:SWEEP-2` · `const` · `locale:progress` · progress indicator strings · `locales/en.json:progress`
- [ ] S-1082 · `unit:SWEEP-2` · `const` · `locale:log` · log panel strings · `locales/en.json:log`
- [ ] S-1083 · `unit:SWEEP-2` · `const` · `locale:report` · report viewer strings · `locales/en.json:report`
- [ ] S-1084 · `unit:SWEEP-2` · `const` · `locale:console` · console log panel strings · `locales/en.json:console`
- [ ] S-1085 · `unit:SWEEP-2` · `const` · `locale:zh_parity` · zh.json must supply identical key tree (629 leaves, 17 namespaces); any missing/extra key is a parity failure · `locales/zh.json`

---

## SWEEP-3 — OASIS Library Contract (external dependency)

- [ ] S-1086 · `unit:SWEEP-3` · `const` · `OASIS_CONTRACT` · OASIS TwitterChannel/RedditChannel env: step_env(active_agents), interview_agent(agent_id, query), close(); ActionType enum with 6 Twitter + 13 Reddit variants; model constructed via create_model(model_type, model_config_dict). Teri port must provide a compatible abstraction layer; do NOT vendor OASIS internals · `backend/scripts/run_twitter_simulation.py:1`

---

## SWEEP-4 — `backend/scripts/test_profile_format.py`

- [ ] S-1087 · `unit:SWEEP-4` · `fn` · `test_profile_format` · DEFERRED — test utility only; not part of production surface · `backend/scripts/test_profile_format.py:1`


---

## GAP-1 / OQ-2 — Relation temporal validity (cycle-2 additions, src/graph/mod.rs)

- [x] S-G1-001 · `unit:GAP-1` · `field` · `Relation.valid_at` · temporal window `Option<(u64, Option<u64>)>`; `#[serde(default)]` for backward-compat · `src/graph/mod.rs` → `teri::graph::Relation.valid_at` · PARITY-VERIFIED 2026-06-14: serde backward-compat proven (`test_relation_serde_backward_compat_no_valid_at_field`: old `{"kind":"RelatedTo","weight":0.5}` → `valid_at=None`)
- [x] S-G1-002 · `unit:GAP-1` · `fn` · `Relation::with_validity` · constructor with explicit valid_at; same weight validation as `::new` · `src/graph/mod.rs` → `teri::graph::Relation::with_validity` · PARITY-VERIFIED 2026-06-14: weight-validation parity with `::new` (`test_relation_with_validity_weight_validation`)
- [x] S-G1-003 · `unit:GAP-1` · `fn` · `Relation::is_active_at` · returns bool; handles None/open-ended/closed window cases · `src/graph/mod.rs` → `teri::graph::Relation::is_active_at` · PARITY-VERIFIED 2026-06-14: all 3 branches (None/open/half-open `[start,end)`) reproduce Zep active-vs-historical contract
- [x] S-G1-004 · `unit:GAP-1` · `fn` · `KnowledgeGraph::partition_edges_at` · splits edges into (active, historical) at timestamp t; powers panorama_search classification · `src/graph/mod.rs` → `teri::graph::KnowledgeGraph::partition_edges_at` · PARITY-VERIFIED 2026-06-14: maps onto `panorama_search` active/historical split (`test_partition_edges_at`)
- [x] S-G1-005 · `unit:GAP-1` · `fn` · `parse_valid_at_from_json` · free fn; parses valid_at/valid_from/valid_until from LLM JSON; array and object forms; graceful None on missing · `src/graph/mod.rs` → `teri::graph::parse_valid_at_from_json` · PARITY-VERIFIED 2026-06-14: array+object forms + graceful-None (`test_parse_valid_at_from_json_array_form`/`_object_form`)
- [x] S-G1-006 · `unit:GAP-1` · `type` · `EdgeTriple` · type alias `(Uuid, Uuid, Relation)` for clippy complexity · `src/graph/mod.rs` → `teri::graph::EdgeTriple` · PARITY-VERIFIED 2026-06-14: type alias exercised via `partition_edges_at`/`get_all_edges` return shape

---

## GAP-2 / OQ-3 — query_vec_similarity cosine search (cycle-2 additions, src/memory/mod.rs)

- [x] S-G2-001 · `unit:GAP-2` · `fn` · `MemoryStore::query_vec_similarity` · async cosine-similarity search over stored vec entries; spawn_blocking redb scan; dimension-mismatch skip; zero-norm skip; top_k sort · `src/memory/mod.rs` → `teri::memory::MemoryStore::query_vec_similarity` · PARITY-VERIFIED 2026-06-14: cosine math proven genuine (magnitude-normalized, not dot — independent differential: wrong-direction high-magnitude vector loses to aligned one); 6 branches tested (empty→Ok([]), ranking, top_k limiting, top_k≥avail→all, dim-mismatch skip, identical→sim≈1.0); reproduces SEARCH half of Zep insight_forge/quick_search ranked-results contract

---

## GAP-OQ3-EMBED — Embedding generation (blocked, substrate decision needed)

- [!] S-EMBED-001 · `unit:GAP-OQ3-EMBED` · BLOCKED · Embedding generation (text → vector) has no backend. shimmy has no /v1/embeddings route. Decision: add /v1/embeddings to shimmy OR add EmbeddingClient trait in teri. Do NOT implement a fake/random embedder.

---

## GAP-ACTION-TAXONOMY — MiroFish/OASIS social-media action taxonomy (cycle-3 additions)

Sources: `backend/app/config.py` (OASIS_TWITTER_ACTIONS/OASIS_REDDIT_ACTIONS), `backend/app/services/zep_graph_memory_updater.py` (AgentActivity.to_episode_text 12-type dispatch).
Rust target: `src/sim/mod.rs` + `src/agent/mod.rs`.
Status: `- [x]` (cycle-3 RE-VERIFY PASS 2026-06-14 — both defects resolved, differentially confirmed; S-TAX-001..019+021 → `- [x]`, S-TAX-020 stays `- [≠]`). FIX-1: `SocialAction::Trend` added (no-arg variant, parser arm "TREND"/"trend", Display "Performed trend operation", importance 0.25). FIX-2: `TargetKind {Post,Comment}` discriminant added to `Like` and `Dislike`; LIKE_POST→Post, LIKE_COMMENT→Comment, DISLIKE_POST→Post, DISLIKE_COMMENT→Comment; Display produces "Liked post:" vs "Liked comment:" matching to_episode_text distinct render paths. REFRESH omission remains CORRECT (`- [≠]`).

- [x] S-TAX-001 · `unit:GAP-ACTION-TAXONOMY` · `type` · `SocialAction` · enum with 13 OASIS social action variants (12 active + DoNothing) · `src/sim/mod.rs` → `teri::sim::SocialAction`
- [x] S-TAX-002 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::CreatePost` · args: content · `src/sim/mod.rs` → `teri::sim::SocialAction::CreatePost`; source: CREATE_POST
- [x] S-TAX-003 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::Like` · args: target_kind: TargetKind, target_id · LIKE_POST→Post, LIKE_COMMENT→Comment · `src/sim/mod.rs` → `teri::sim::SocialAction::Like` · FIX-2 applied: post-vs-comment discriminant restored via `TargetKind` enum; Display "Liked post: X" vs "Liked comment: Y" matching to_episode_text :70-81 vs :153-164.
- [x] S-TAX-004 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::Dislike` · args: target_kind: TargetKind, target_id · DISLIKE_POST→Post, DISLIKE_COMMENT→Comment · `src/sim/mod.rs` → `teri::sim::SocialAction::Dislike` · FIX-2 applied: same post-vs-comment discriminant as Like.
- [x] S-TAX-005 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::Repost` · args: post_id · source: REPOST · `src/sim/mod.rs` → `teri::sim::SocialAction::Repost`
- [x] S-TAX-006 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::Quote` · args: post_id, content · source: QUOTE_POST · `src/sim/mod.rs` → `teri::sim::SocialAction::Quote`
- [x] S-TAX-007 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::Follow` · args: user_id · source: FOLLOW · `src/sim/mod.rs` → `teri::sim::SocialAction::Follow`
- [x] S-TAX-008 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::Comment` · args: post_id, content · source: CREATE_COMMENT · `src/sim/mod.rs` → `teri::sim::SocialAction::Comment`
- [x] S-TAX-009 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::SearchPosts` · args: query · source: SEARCH_POSTS · `src/sim/mod.rs` → `teri::sim::SocialAction::SearchPosts`
- [x] S-TAX-010 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::SearchUser` · args: query · source: SEARCH_USER · `src/sim/mod.rs` → `teri::sim::SocialAction::SearchUser`
- [x] S-TAX-011 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::Mute` · args: user_id · source: MUTE · `src/sim/mod.rs` → `teri::sim::SocialAction::Mute`
- [x] S-TAX-012 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::DoNothing` · no args · source: DO_NOTHING · `src/sim/mod.rs` → `teri::sim::SocialAction::DoNothing`
- [x] S-TAX-013 · `unit:GAP-ACTION-TAXONOMY` · `impl` · `SocialAction::fmt (Display)` · 13 arms producing readable English descriptions (Like/Dislike each have 2 TargetKind arms) · `src/sim/mod.rs`
- [x] S-TAX-014 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `Action::Social(SocialAction)` · wrapper variant on `Action` enum; 5 generic variants intact · `src/sim/mod.rs`
- [x] S-TAX-015 · `unit:GAP-ACTION-TAXONOMY` · `impl` · `Action::fmt (Display)` · Social arm delegates to SocialAction Display · `src/sim/mod.rs`
- [x] S-TAX-016 · `unit:GAP-ACTION-TAXONOMY` · `method` · `Agent::parse_and_validate_action` · OASIS SCREAMING_SNAKE_CASE name matching + bare-value/key=value arg parsing for all 13 social names · `src/agent/mod.rs`
- [x] S-TAX-017 · `unit:GAP-ACTION-TAXONOMY` · `method` · `Agent::parse_social_action` · private helper; LIKE_POST→Like{Post,…}, LIKE_COMMENT→Like{Comment,…}, DISLIKE_POST→Dislike{Post,…}, DISLIKE_COMMENT→Dislike{Comment,…}, "TREND"/"trend"→Trend · `src/agent/mod.rs`
- [x] S-TAX-018 · `unit:GAP-ACTION-TAXONOMY` · `method` · `Agent::store_action_in_memory` · Social arm with 13 sub-arms; Like/Dislike each have 2 TargetKind arms (0.30 each); Trend 0.25; weights: 0.85 CreatePost, 0.75 Follow/Mute, 0.70 Quote/Comment, 0.65 Repost, 0.30 Like/Dislike, 0.25 Search*/Trend, 0.05 DoNothing · `src/agent/mod.rs`
- [x] S-TAX-019 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `SocialAction::Trend` · no args · source: TREND (ACTION_TYPE_MAP run_parallel_simulation.py:627, NOT in FILTERED_ACTIONS, agent_action.py:507) · `src/sim/mod.rs` → `teri::sim::SocialAction::Trend` · FIX-1 applied: parser "TREND"/"trend"→Trend; Display "Performed trend operation"; importance 0.25; apply no-panic; 4 new tests.
- [x] S-TAX-021 · `unit:GAP-ACTION-TAXONOMY` · `type` · `TargetKind` · `{Post, Comment}` discriminant enum for Like/Dislike · `src/sim/mod.rs` → `teri::sim::TargetKind` · FIX-2 applied.
- [≠] S-TAX-020 · `unit:GAP-ACTION-TAXONOMY` · `variant` · `REFRESH` intentionally NOT represented · source: REFRESH · INTENTIONAL DIVERGENCE (justified): REFRESH is in `FILTERED_ACTIONS = {'refresh','sign_up'}` (run_parallel_simulation.py:611) → never reaches actions.jsonl / `AgentActivity` / `to_episode_text`. Omission is correct and not a downgrade.

---

## GAP-SOCIAL-WORLDSTATE — Rich social world-state (deferred; not a downgrade)

- [!] S-SWS-001 · `unit:GAP-SOCIAL-WORLDSTATE` · DEFERRED · `WorldState::apply`/`apply_at` records `Action::Social(...)` generically (same event-push path as generic actions). Rich social-world-state (timeline, post store, engagement counts, follower graph, comment threads) is the dedicated work of U-022/U-028/U-029/U-030. Those units stay `- [ ]`. This item is a scope-boundary marker, not a missing implementation.
