# Cross-Repo References (MiroFish → teri port) — LIGHT (target==dest==teri)

**Architect:** rust-port-architect · **Date:** 2026-06-14
Because `rust_target == dest_repo == teri`, there is no X-repo↔Y-repo symbol merge map: every landing is intra-teri.
The only true cross-repo edge is **teri ↔ shimmy** (the inference substrate). The rest of this file is the
**intra-teri blast radius** for each `extend-Y` unit — what existing teri symbols it modifies and which teri tests must not regress.

---

## (a) teri ↔ shimmy substrate contract

**Edge:** teri `OpenAiAdapter` (HTTP client) → shimmy OpenAI-compatible server.

**Confirmed present (file:line):**
- shimmy server exposes the routes teri targets: `/v1/chat/completions` and `/v1/models`
  (`shimmy/src/server.rs:56-57`, `:122-123`, `:149-152` — `.route("/v1/models", get(openai_compat::models))`).
- shimmy OpenAI-compat module: `shimmy/src/openai_compat/mod.rs` (+ `types.rs`).
- teri side: `OpenAiAdapter` at `src/llm.rs:29`; `preflight_check_backend(&config.llm)` at `src/main.rs:65`
  refuses a stub backend before any simulation runs (the "no GGUF/stub" guard from teri CLAUDE.md).

**Contract terms:**
1. teri sends OpenAI-shaped `POST /v1/chat/completions` (chat, streaming SSE, JSON mode) — shimmy must accept and return OpenAI-shaped responses. CONFIRMED route present.
2. teri preflight hits shimmy `/v1/models` (or `/health`) and refuses if shimmy reports stub mode. Substrate must report non-stub when Airframe engine is loaded.
3. **OPEN — embeddings (OQ-3 / GAP-2):** `query_vec_similarity` will call shimmy for embeddings. **Verify shimmy exposes `/v1/embeddings`.** Grep at DISCOVER found `/v1/chat/completions` + `/v1/models` in `server.rs` but did NOT confirm `/v1/embeddings`. If shimmy lacks an embeddings endpoint, that is an **owner-escalation substrate gap** (choose: add `/v1/embeddings` to shimmy, OR use teri's OpenAiAdapter against a remote embeddings provider) — it is NOT grounds to drop semantic search.

**No-downgrade gate (shimmy side):** teri's `llm.rs` test suite (~20 tests incl. `test_openai_complete`, `test_openai_stream`) is the substrate-contract proof; they run against `httpmock`, so shimmy changes do not regress them, but a shimmy route rename WOULD break the live path — keep `/v1/chat/completions` stable.

---

## (b) Intra-teri blast radius per extend-Y unit

Each extend-Y unit modifies EXISTING teri symbols. Below: symbols touched + teri tests that MUST NOT regress (the no-downgrade-of-Y gate; teri baseline = GREEN, 142 tests).

### U-001 — config (AppConfig)
- **Modifies:** `src/config.rs` `AppConfig` (add OASIS action lists, ALLOWED_EXTENSIONS, MAX_CONTENT_LENGTH; remove/repurpose ZEP_API_KEY — graph is native).
- **Callers (blast):** `main.rs` (`run_cmd`/`serve_cmd`), every module reading config. Additive fields = low risk; field REMOVAL (ZEP key) needs caller sweep.
- **Tests must not regress:** `config.rs` env-driven config tests; `main.rs` keyless `--help` discipline.

### U-018 — Persona social fields
- **Modifies:** `src/agent/mod.rs` `Persona { name, background, traits, role }` (line 14) → add platform handle, follower/following counts, posting style, platform enum. `PersonaGenerator` (line 470) + `templates/persona_gen.jinja`.
- **Callers (blast):** `AgentPool::spawn`, `Agent::step` context builder, any `Persona` constructor in tests. Additive fields with `#[serde(default)]` = low risk; constructors that build `Persona` literally must add fields.
- **Tests must not regress:** `test_persona_generator`, `test_agent_pool_spawn`, persona-dedup tests in `agent/mod.rs`.

### Action enum extension (Decision-2; folded into sim work, drives U-018/U-022/U-028/U-029)
- **Modifies:** `src/sim/mod.rs` `Action` enum (line 9 — `Speak/Move/Interact/Observe/Think`) → add `CreatePost/LikePost/Comment/Retweet/Repost/Quote/Follow/SearchPosts/Mute/DoNothing` (keep generic variants — additive).
- **Callers (blast — HIGH):** `Action::Display` impl (`sim/mod.rs:17-27`), agent action parser + match arms (`agent/mod.rs:290-294` parse, `:305-312` apply — these are **exhaustive matches** that WILL fail to compile until new arms added), `Agent::commit_action`, every action-parser test (`agent/mod.rs:1182-1224`).
- **Tests must not regress:** `test_action_parser_*` (agent/mod.rs), `test_sim_engine_run` (sim/mod.rs). Exhaustive-match compile errors are the safety net — update ALL arms.

### U-015 — wire KnowledgeGraph::build() (map-onto, but edits existing build)
- **Modifies:** `src/graph/mod.rs` `build()` (line 223 placeholder) → orchestrate `entity_extraction_prompt`+LLM+`parse_entities_json`+relation equivalents. Signature likely changes from `fn build(doc) -> Result<Self>` to `async fn build(doc, llm) -> Result<Self>` (needs LLM).
- **Callers (blast):** any caller of `KnowledgeGraph::build` (currently none in pipeline — `run_cmd` is a stub, claim 4) + `test_knowledge_graph_build`. Sync→async signature change = update callers + test to `.await`.
- **Tests must not regress:** `test_knowledge_graph_build`, `test_entity_extraction`, `test_relation_extraction`, `test_get_subgraph`.

### OQ-2 — Relation.valid_at (highest blast radius)
- **Modifies:** `src/graph/mod.rs` `Relation { kind, weight }` (constructed at line 507) → add `valid_at: Option<(u64, Option<u64>)>`.
- **Callers (blast — HIGH):** EVERY `Relation { .. }` literal — `parse_relations_json` (line 507), serialization (`SerializableKnowledgeGraph`), `add_relation`, and all graph tests that build relations. Use `#[serde(default)]` so existing serialized graphs still deserialize.
- **Tests must not regress:** all `graph/mod.rs` relation tests; bincode/JSON roundtrip tests (schema change — ensure backward-compatible default).

### OQ-3 — query_vec_similarity impl (memory)
- **Modifies:** `src/memory/mod.rs` `query_vec_similarity` (line 294 stub) → real cosine over `agent:{uuid}:vec:*` rows + shimmy embeddings.
- **Callers (blast):** the stub-asserting test `test_query_vec_similarity_returns_not_implemented` (line 538) — this test MUST be REPLACED (it asserts the not-implemented error; implementing the function intentionally inverts it). Note in U-017/OQ-3 work: replace, don't "regress".
- **Tests must not regress:** other `memory/mod.rs` tests (`test_memory_store_write_read`, `test_concurrent_writes`, `test_snapshot_roundtrip`) — vec impl is additive to those.

### U-024 — Report ReACT loop
- **Modifies:** `src/report/mod.rs` `ReportAgent` (add `plan_outline`, per-section ReACT, `chat`, section file writes) — built ON existing `generate_stream`.
- **Callers (blast):** `generate`/`generate_stream` callers; `PredictionReport` struct may gain fields (additive). 
- **Tests must not regress:** `report/mod.rs` streaming tests (initial partial + buffer + early JSON parse ordering) — the existing `generate_stream` behavior is the no-downgrade surface; extend, don't replace.

### U-036 — i18n (frontend + backend mirror)
- **Modifies:** keeps Vue `vue-i18n`; adds backend `src/i18n` reading same `locales/*.json`.
- **Blast:** frontend unchanged (re-point only); backend new module — no existing teri symbol modified.

### U-041 — SimulationRunView SSE switch
- **Modifies:** Vue view only (polling → teri SSE). No teri Rust symbol changed; depends on U-026 SSE route shape.

### U-049 — graceful shutdown
- **Modifies:** `src/main.rs` + new `src/server` — add axum graceful shutdown + CancellationToken cleanup of running SimEngines.
- **Blast:** `main.rs` entrypoint; new server module. Low — additive.

---

## Summary: highest-blast-radius changes (land early, on green)
1. **`Action` enum extension** — exhaustive matches in `agent/mod.rs` + `sim/mod.rs` Display force compile-time coverage; touches sim+agent+all action tests.
2. **`Relation.valid_at`** — every Relation constructor + graph serialization + graph tests; use `#[serde(default)]` for back-compat.
3. **`query_vec_similarity` impl** — intentionally replaces the stub-asserting test; coordinate so it reads as "implemented", not "regressed".
4. **`build()` sync→async** — signature change ripples to callers + `test_knowledge_graph_build`.

teri baseline (GREEN, 142 tests) is the gate: after each extend-Y landing, the full teri suite must stay green except the two **intentionally-inverted** stub tests (`query_vec_similarity` not-implemented assertion), which are replaced, not regressed.
