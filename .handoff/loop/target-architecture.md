# MiroFish → teri: Target Architecture (Port-and-Map / DISCOVER)

**Architect:** rust-port-architect · **Date:** 2026-06-14
**Source X:** `/home/drdave/Desktop/meta/MiroFish` (Python Flask + OASIS subprocess + Zep Cloud)
**Dest/Target Y == rust_target:** `teri` @ `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri` (branch `port/mirofish`).
The port lands Rust **directly into teri** modules — "port" and "merge" collapse into one landing.
**Substrate:** `shimmy` (Airframe, OpenAI-compatible at `/v1/chat/completions` + `/v1/models`) — replaces ollama; inference DONE.

> Spot-verified on develop@c894de8 (2026-06-14). All 6 critical claims CONFIRMED with file:line below.
> Principle: **full-feature, no-downgrade, nothing left behind.** teri substrates REPLACE Zep/OASIS/ollama as UPGRADES, but every MiroFish behavior is mapped/extended/ported — never dropped. `reuse-Y` = "verify-only vs MiroFish", not "trust".

---

## Spot-verification of the 6 critical claims (file:line)

1. **`src/graph/mod.rs:223-237`** — `KnowledgeGraph::build(doc)` IS a placeholder. Body comment line 224 "Minimal placeholder build: create a single entity from document metadata"; creates ONE `Entity { kind: EntityKind::Other }` from title/filename/id (lines 226-235). The extraction pipeline (the existing `entity_extraction_prompt`/`parse_entities_json`/`relation_extraction_prompt`/`parse_relations_json` helpers) is NOT wired into `build()`. CONFIRMED.
2. **`src/memory/mod.rs:294-300`** — `query_vec_similarity()` returns `Err(TeriError::Memory("vector similarity search not yet implemented"))` (line 300). Test `test_query_vec_similarity_returns_not_implemented` (line 538) asserts the stub. CONFIRMED.
3. **`src/api/mod.rs`** — types ONLY. Grep for `Router|axum::|.route` over `src/api/mod.rs` = 0 matches. No axum Router, no handlers. CONFIRMED.
4. **`src/main.rs`** — `run_cmd()` (line 45) returns `Err(TeriError::Unknown("Pipeline not yet implemented"))` (line 96); `serve_cmd()` (line 99) returns `Err(TeriError::Unknown("API server not yet implemented"))` (line 112). CONFIRMED.
5. **`src/sim/mod.rs:9-15`** — `Action` enum = `Speak/Move/Interact/Observe/Think` (each `(String)`). `src/agent/mod.rs:14-19` — `Persona { name, background, traits: Vec<String>, role }` — generic, NON-platform (no handle/follower/platform fields). CONFIRMED. (Note: `Action` is defined in `sim/mod.rs`, re-used by agent — the agent parser/match arms are in `agent/mod.rs:290-312`.)
6. **`src/llm.rs`** — 3 adapters: `OpenAiAdapter` (line 29), `AnthropicAdapter` (line 227), `GeminiAdapter` (line 417), each with `complete`/`complete_json`/`stream` (trait lines 13-15) + `max_retries` server-error/timeout retry (lines 35,70,81,233,280,291,423). **`<think>` stripping does NOT exist** in `src/llm.rs` (grep for `think` strip/regex = 0 matches) — this is the one MiniMax-M2.5 behavior to ADD (cf. `llm_client.py:67`). CONFIRMED.

---

## (a) Per-unit classification table — ALL U-001..U-050 + SWEEP-1..4

Columns: **unit | class | landing | teri target symbol | rationale (evidence-cited)**.
Classes: `port-fresh` | `extend-Y` | `reuse-Y` | `map-onto-substrate`.

### Layer 0 — Config / Bootstrap
| unit | class | landing | teri target | rationale |
|------|-------|---------|-------------|-----------|
| U-001 config.Config | extend-Y | merge-into `teri::config` | `AppConfig` | teri `config.rs` exists (env-driven, no files). ADD OASIS action lists, ALLOWED_EXTENSIONS, MAX_CONTENT_LENGTH, ZEP-replaced fields. No Zep key needed (graph is native). |
| U-002 run.py main | extend-Y | merge-into `teri::main` | `serve_cmd` | `main.rs:99 serve_cmd` is a stub; wire host/port from env, call create_app router. |
| U-003 create_app | port-fresh | new module `src/server/mod.rs` | `create_app()->Router` | No axum Router exists (claim 3). Build the app factory: /health, 3 route groups, request-logging middleware. |

### Layer 1 — Utilities
| unit | class | landing | teri target | rationale |
|------|-------|---------|-------------|-----------|
| U-004 logger | reuse-Y | (verify-only) `teri` tracing | `tracing` setup in main.rs | teri already uses `tracing` (`main.rs:111` `tracing::info!`). Verify rotating-file parity; add file appender if absent. |
| U-005 locale i18n | port-fresh | new module `src/i18n/mod.rs` | `i18n::t`, `set_locale` | No i18n in teri. Port `t()` + zh/en + `get_language_instruction()`. task-local locale (replaces thread-local). |
| U-006 retry | reuse-Y | (verify-only) `src/llm.rs` | `max_retries` backoff (llm.rs:35) | teri llm has exponential backoff retry already. Generic decorator → reuse the pattern; no standalone util needed. |
| U-007 zep_paging | map-onto-substrate | map-onto `src/graph` (native) | `KnowledgeGraph` iteration | Zep paging is an API-cursor artifact; native petgraph has no paging. Behavior preserved = "retrieve all nodes/edges" via `get_all_entities`/`get_all_edges`. verify-only vs X (no fresh port). |
| U-008 llm_client | reuse-Y | (verify-only) `src/llm.rs` | `OpenAiAdapter` | teri llm is a strict superset (claim 6). One ADD: `<think>` strip → folds into U-008 as extend (see Decision-3). verify chat/chat_json map to complete/complete_json. |
| U-009 file_parser | reuse-Y | (verify-only) `src/seed` | `SeedIngestor::from_file` | teri `seed/mod.rs` parses .txt/.pdf/.json/.md + chunking. Superset of FileParser. verify chunk/overlap + encoding fallback parity. |
| U-010 action_logger | port-fresh | new module `src/sim/action_logger.rs` | `PlatformActionLogger` | JSONL per-platform action log. No teri equiv; needed by native SimEngine output (Decision-2). |

### Layer 2 — Models
| unit | class | landing | teri target | rationale |
|------|-------|---------|-------------|-----------|
| U-011 ProjectManager | port-fresh | new module `src/models/project.rs` | `ProjectManager` | FS-backed project persistence (uploads/projects). No teri equiv. |
| U-012 TaskManager | port-fresh | new module `src/models/task.rs` | `TaskManager` | In-memory async task registry (status/progress). Use `Arc<RwLock<HashMap>>` (idiom: singleton→shared state). Underpins all async API tasks. |

### Layer 3 — External Integration Services
| unit | class | landing | teri target | rationale |
|------|-------|---------|-------------|-----------|
| U-013 text_processor | reuse-Y | (verify-only) `src/seed` | seed chunk/preprocess | split_text/preprocess/stats already in seed. verify. |
| U-014 ontology_generator | port-fresh | new module `src/graph/ontology.rs` | `OntologyGenerator` | OQ-5 = add `EntityKind::Custom(String)` + port ontology gen (no-downgrade). LLM → entity/edge type set. Maps to teri petgraph dynamic kinds. |
| U-015 graph_builder (Zep) | map-onto-substrate | map-onto `src/graph` (petgraph) | `KnowledgeGraph::build` (wire pipeline) | Decision-1. Zep create_graph/set_ontology/add_text/wait → native build(): extraction prompts + LLM + parsers (helpers EXIST, build() is stub claim 1). Async build via TaskManager. |
| U-016 zep_entity_reader | map-onto-substrate | map-onto `src/graph` | `get_entity_with_context`,`get_entities_by_type` | Zep node/edge read → petgraph queries (`get_subgraph`, label filter). Decision-1. verify-only vs X. |
| U-017 zep_tools | map-onto-substrate | map-onto `src/graph`+`src/report` | `graph::search`,`panorama_search`,`insight_forge` | Decision-1. insight_forge/panorama_search/quick_search → graph search + OQ-3 vec similarity (shimmy embeddings). interview_agents → U-020 native IPC. |
| U-018 oasis_profile_generator | extend-Y | merge-into `src/agent` | `PersonaGenerator` + social fields | teri `PersonaGenerator` (agent/mod.rs:470) exists. EXTEND Persona (line 14) with platform handle/followers/style; add reddit-JSON/twitter-CSV emit. |
| U-019 simulation_config_generator | port-fresh | new module `src/sim/config_generator.rs` | `SimConfigGenerator` | LLM(seed+query)→SimConfig (agent count/ticks/parallelism + per-platform). Simpler than OASIS config (no subprocess). |
| U-020 simulation_ipc | port-fresh | new module `src/sim/ipc.rs` | `InterviewBus` | File-IPC → in-process tokio channels (idiom: file-IPC→channels). Native SimEngine = same process, so interview = inject + capture next prepare_action. |
| U-021 zep_graph_memory_updater | map-onto-substrate | map-onto `src/graph` | `KnowledgeGraph::append_episode` | Decision-1. Live sim→graph write-back. ADD `append_episode()` (absent) + `AgentActivity::to_episode_text()` for 12 action types. tokio task + batching (replaces daemon thread). |

### Layer 4-6 — Runner / Manager / Report
| unit | class | landing | teri target | rationale |
|------|-------|---------|-------------|-----------|
| U-022 simulation_runner | map-onto-substrate | map-onto `src/sim::SimEngine` | `SimRunner` (native) | Decision-2. OASIS subprocess orchestration → native SimEngine (UPGRADE: pure Rust, no Popen). JSONL polling → broadcast subscribe; pgid-kill → CancellationToken. |
| U-023 simulation_manager | port-fresh | new module `src/sim/manager.rs` | `SimulationManager` | Multi-sim queue, status (CREATED→PREPARING→READY→FAILED), MAX_CONCURRENT. Home = `ApiState`. Drives prepare pipeline. |
| U-024 report_agent | extend-Y | merge-into `src/report` | `ReportAgent` (ReACT) | teri `report/mod.rs generate_stream` is the right SSE substrate. ADD plan_outline phase, per-section ReACT loop w/ graph tool calls, chat mode, section file writes. Graph parts = Decision-1. |

### Layer 7 — HTTP API
| unit | class | landing | teri target | rationale |
|------|-------|---------|-------------|-----------|
| U-025 graph.py blueprint | port-fresh | new module `src/api/graph.rs` | axum routes `/api/graph/*` | No Router (claim 3). ontology/build/task/data/project routes. Uses `streaming.rs` TickBuffer for progress. |
| U-026 simulation.py blueprint | port-fresh | new module `src/api/simulation.rs` | axum routes `/api/simulation/*` | 30+ routes incl. SSE stream, interview/batch, start/stop. Largest API unit. Consumes `StreamAdapter`. |
| U-027 report.py blueprint | port-fresh | new module `src/api/report.rs` | axum routes `/api/report/*` | generate(async, pre-mint report_id), agent-log/console-log incremental + stream, chat, graph-search. |

### Layer 8 — Simulation Scripts (subprocess executables)
| unit | class | landing | teri target | rationale |
|------|-------|---------|-------------|-----------|
| U-028 run_twitter_simulation | map-onto-substrate | map-onto `src/sim::SimEngine` | `TwitterPlatform` impl | Decision-2. OASIS twitter subprocess → native SimEngine + Twitter action set. No subprocess. UPGRADE. |
| U-029 run_reddit_simulation | map-onto-substrate | map-onto `src/sim::SimEngine` | `RedditPlatform` impl | Decision-2. Mirror of U-028 for Reddit. |
| U-030 run_parallel_simulation | map-onto-substrate | map-onto `src/sim` top-runner | `MultiPlatformRunner` | Decision-2 + OQ-1. Dual-platform → top-level runner owning TWO `SimEngine` instances (tokio::join), platform as agent attribute. action-arg enrichment from DB → native in-memory enrichment. |

### Layer 9 — Frontend (OQ-4: IN SCOPE, Vue-kept)
| unit | class | landing | teri target | rationale |
|------|-------|---------|-------------|-----------|
| U-031 api/index.js | port-fresh | keep-Vue `frontend/src/api/index.js` | axios client | OQ-4 lock: Vue kept, re-pointed at teri's axum API (baseURL→teri port). Accept-Language header drives U-005 locale. |
| U-032 api/graph.js | port-fresh | keep-Vue | graph client | Re-point to teri `/api/graph`. Contract preserved (multipart ontology, build, task poll). |
| U-033 api/simulation.js | port-fresh | keep-Vue | simulation client | Re-point to teri `/api/simulation`. ~20 fns; contracts must match U-026 routes. |
| U-034 api/report.js | port-fresh | keep-Vue | report client | Re-point to teri `/api/report`; from_line log polling preserved. |
| U-035 pendingUpload store | reuse-Y | keep-Vue (unchanged) | Vue reactive store | Frontend-internal; no teri change. verify-only. |
| U-036 i18n | extend-Y | keep-Vue + `src/i18n` | vue-i18n + locales/*.json | Frontend i18n kept; backend U-005 mirrors same locales/*.json (SWEEP-2). |
| U-037 router | reuse-Y | keep-Vue (unchanged) | vue-router | Frontend-internal. verify-only. |
| U-038 Home.vue | reuse-Y | keep-Vue (unchanged) | view | Re-points via U-032 only; view logic unchanged. verify-only. |
| U-039 MainView.vue | reuse-Y | keep-Vue | view | Unchanged but verify task-poll shapes vs teri TaskManager. |
| U-040 SimulationView.vue | reuse-Y | keep-Vue | view | verify config/profile shapes vs U-019/U-018. |
| U-041 SimulationRunView.vue | extend-Y | keep-Vue | view | Switch run-status polling → teri SSE tick stream (UPGRADE available); polling fallback kept. |
| U-042 ReportView.vue | reuse-Y | keep-Vue | view | verify agent-log from_line shape vs U-027. |
| U-043 InteractionView.vue | reuse-Y | keep-Vue | view | verify chat shape vs U-027 chat route. |
| U-044 main.js+App.vue | reuse-Y | keep-Vue (unchanged) | bootstrap | verify-only. |

### Layer 10 — Runtime / Concurrency contracts
| unit | class | landing | teri target | rationale |
|------|-------|---------|-------------|-----------|
| U-045 thread management | map-onto-substrate | map-onto tokio runtime | tokio tasks | daemon threads → tokio tasks (task/monitor/memory-worker). Native SimEngine already tokio. |
| U-046 subprocess lifecycle | map-onto-substrate | map-onto SimEngine | CancellationToken | NO subprocess in teri (UPGRADE). pgid-kill → `tokio_util::CancellationToken`; 5s grace → timeout. |
| U-047 file-IPC protocol | map-onto-substrate | map-onto channels | `InterviewBus` (U-020) | file-IPC dirs → tokio mpsc/oneshot. Same-process. |
| U-048 JSONL action streaming | reuse-Y | (verify-only) `src/sim` broadcast | `subscribe_with_history` | teri SimEngine has broadcast + history (sim/mod.rs). Tail-by-offset → broadcast subscribe. verify ordering/no-loss. |
| U-049 signal/graceful shutdown | extend-Y | merge-into `src/main`+`src/server` | shutdown handler | axum graceful shutdown + CancellationToken cleanup of all SimEngines. Replaces SIGTERM/atexit subprocess kill. |
| U-050 locale propagation | port-fresh | new `src/i18n` task-local | `LOCALE` task-local | thread-local→tokio task-local; capture before spawn. Folds with U-005. |

### Sweep items
| unit | class | landing | teri target | rationale |
|------|-------|---------|-------------|-----------|
| SWEEP-1 Vue components | reuse-Y | keep-Vue | 6 step components | OQ-4 Vue-kept; verify against re-pointed clients. |
| SWEEP-2 locales/*.json | port-fresh | `src/i18n/locales` + keep-Vue | zh/en keys | ALL `t()` keys (backend+frontend) ported; shared json source of truth. |
| SWEEP-3 OASIS contract | map-onto-substrate | map-onto `src/sim::SimEngine` | native SimEngine | **RECLASSIFIED** vs parity-ledger (ledger said "teri calls Python OASIS via subprocess, same as MiroFish"). Per Decision-2 + owner no-downgrade-as-upgrade: teri REIMPLEMENTS OASIS natively (pure Rust, no Python). All OASIS behaviors → SimEngine. |
| SWEEP-4 test_profile_format | — | (drop, non-prod) | — | `- [≠]` intentional: utility test script, not production surface. Owner-noted, not a behavior drop. |

---

## (b) Port-and-map locked decision blocks

### DECISION-1 — Zep Cloud graph+memory → MAP-ONTO teri petgraph (`src/graph`) + redb (`src/memory`)
**Units:** U-007, U-015, U-016, U-017, U-021 + graph parts of U-024.
**Decision:** Replace Zep Cloud SaaS with teri's native petgraph KnowledgeGraph + redb store. No external SaaS, no API keys, no async server round-trips. The LLM extraction helpers already exist in `graph/mod.rs`; `build()` (claim 1) and `append_episode()` are the wiring gaps.
**Behaviors preserved (each must be expressible):**
1. Episodic facts + entity/edge extraction → wire `entity_extraction_prompt`+LLM+`parse_entities_json` and relation equivalents into `build()` (helpers exist; orchestration is the stub).
2. Temporal validity (valid/expired facts) → **GAP**, see below.
3. Paged retrieval (fetch_all_nodes/edges) → `get_all_entities`/`get_all_edges`/`get_subgraph` (no paging needed; native iteration).
4. Semantic/vector search (insight_forge) → **GAP**, see below.
5. Live graph-memory update from sim (`zep_graph_memory_updater`) → ADD `KnowledgeGraph::append_episode()` + `AgentActivity::to_episode_text()` for the 12 action types; called from SimEngine tick via a tokio task.
6. Hybrid search + reranker fallback to local keyword (`zep_tools._local_search`) → teri `query_ltm` substring is the local fallback; semantic path = GAP-3.
**Substrate gaps (each → an explicit ledger flag, NO silent drop):**
- **GAP-1 `Relation.valid_at`** — `Relation` (graph/mod.rs:507 has only `kind`+`weight`) has no temporal window. ADD `valid_at: Option<(u64, Option<u64>)>` (OQ-2). Ledger: `- [!] U-017/U-021 valid_at window — panorama_search active/historical classification needs it`.
- **GAP-2 vector similarity** — `query_vec_similarity` is a stub (claim 2). Implement via shimmy embeddings (OQ-3). Ledger: `- [!] U-017 insight_forge semantic search — depends on query_vec_similarity impl`.
- **GAP-3 dynamic ontology** — fixed `EntityKind` enum. ADD `EntityKind::Custom(String)` (OQ-5) so ontology generator (U-014) types survive. Ledger: `- [!] U-014 custom entity kinds`.
**Parity note:** parity is proven by differential test: feed the same document + sim-activity stream through (a) a recorded Zep golden (from MiroFish fixtures) and (b) teri's native graph, comparing the SET of entities/edges and the active/historical classification — not Zep's internal IDs. `reuse`-grade only after build() pipeline + append_episode + valid_at land.

### DECISION-2 — OASIS subprocess sim → MAP-ONTO teri native `SimEngine` (`src/sim`) = REIMPLEMENT-in-teri (UPGRADE)
**Units:** U-022, U-028, U-029, U-030 + SWEEP-3.
**Decision:** teri does NOT spawn Python OASIS. The native `SimEngine::run()` (sim/mod.rs:337-405, already a full tick loop) IS the simulation. **This RECLASSIFIES SWEEP-3** from the ledger's "call Python OASIS via subprocess" to "reimplement OASIS natively" — justified by owner's no-downgrade-as-upgrade mandate (pure Rust, single binary, no Python dependency, no IPC files).
**Behaviors preserved:**
1. Social-media action types: CREATE_POST/LIKE_POST/COMMENT/RETWEET/REPOST/QUOTE/FOLLOW/SEARCH_POSTS/MUTE/DO_NOTHING → EXTEND `Action` enum (sim/mod.rs:9, currently Speak/Move/Interact/Observe/Think) with these variants (additive; keep generic variants).
2. Per-platform agents → platform as a `Persona` attribute (U-018) + per-platform action validity.
3. Dual-platform (Twitter+Reddit) execution → OQ-1: top-level `MultiPlatformRunner` owning two `SimEngine` instances via `tokio::join!` (U-030).
4. Round/tick semantics → SimEngine ticks (already present); map OASIS "round" → tick.
5. Action-log streaming → `subscribe_with_history` broadcast (sim/mod.rs) + `PlatformActionLogger` JSONL (U-010).
6. Mid-sim agent interview/IPC → native `InterviewBus` (U-020): inject prompt for a specific agent via `inject_fn`, capture next `prepare_action()` output, return it (in-process, no file-IPC).
**Substrate gaps:**
- **GAP-4 platform action validity matrix** — which actions are legal per platform (Twitter vs Reddit). Encode as a per-platform allowed-action set on the platform impl. Ledger: `- [!] U-028/U-029 per-platform action set`.
- **GAP-5 action-arg enrichment** — OASIS parallel script enriches action_args with post/user/comment content from SQLite before logging. Native: SimEngine WorldState holds posts in-memory; enrich at commit time. Ledger note on U-030.
**Parity note:** differential test = feed a fixed seed+config+RNG-seed through MiroFish's recorded OASIS action log (golden JSONL fixtures) and teri's SimEngine; compare the DISTRIBUTION of action types and per-agent action legality, not exact stochastic outputs (LLM-driven, so compare structure/legality, not token-identity). Interview parity = same question → a well-formed agent response.

### DECISION-3 — ollama/OpenAI endpoint → MAP-ONTO shimmy via teri `OpenAiAdapter` (DONE — confirm only)
**Units:** capability #13, U-008 inference path.
**Decision:** All local LLM inference maps onto shimmy's OpenAI-compatible API via teri's `OpenAiAdapter`. CONFIRMED present and tested: shimmy exposes `/v1/chat/completions` + `/v1/models` (`shimmy/src/server.rs:56-57,122-123,149-152`); teri `OpenAiAdapter` (llm.rs:29) + `preflight_check_backend` (main.rs:65) refuses stub backends.
**Behaviors preserved:** chat completion, streaming, JSON mode — all in teri llm (superset of MiroFish `LLMClient`).
**Substrate gap:** **GAP-6 `<think>` stripping** — MiroFish `llm_client.py:67` strips `<think>...</think>` (MiniMax-M2.5/DeepSeek-R1 reasoning models). NOT in teri llm (claim 6 grep = 0). ADD a strip step in `OpenAiAdapter::complete`/`complete_json`. Ledger: `- [!] U-008 <think> strip`. Also OQ-3 embeddings (shimmy `/v1/embeddings` — verify shimmy exposes it; if not, that is an owner-flagged substrate gap, not a teri drop).
**Parity note:** confirm-only — golden test a `<think>...</think>`-wrapped response → stripped output equals MiroFish's.

---

## (c) OQ resolutions (locked toward NO-DOWNGRADE / full-feature)

- **OQ-1 dual-platform** → SUPPORT BOTH. Platform as a `Persona` attribute + a top-level `MultiPlatformRunner` owning per-platform `SimEngine` instances (`tokio::join!`). Single-platform = a runner with one engine. (UPGRADE: no subprocesses.) Drives U-030.
- **OQ-2 episodic fact validity** → ADD `valid_at: Option<(u64, Option<u64>)>` to `Relation`. Required by `panorama_search` active/historical (GAP-1). Breaking schema change → touches all Relation constructors + graph tests (see cross-repo-refs blast radius).
- **OQ-3 semantic search** → IMPLEMENT `query_vec_similarity` via shimmy embeddings (cosine over stored `agent:{uuid}:vec:*` rows + a graph-entity embedding index). Backend = shimmy `/v1/embeddings` (verify exposed; else owner-flag). Unblocks `insight_forge`.
- **OQ-4 Vue frontend** → IN SCOPE, **Vue KEPT consuming teri's axum API** (NOT Rust/WASM). Justification: the Vue SPA + i18n + 5-step workflow is mature, working source; re-pointing axios baseURL at teri is far lower-risk and zero-feature-loss vs a WASM rewrite, and keeps teri's "single binary" backend clean (frontend is a separate static deploy served by axum or standalone). Drives U-031..U-044 as keep-Vue/re-point.
- **OQ-5 dynamic ontology** → ADD `EntityKind::Custom(String)` + PORT the ontology generator (U-014). No-downgrade: MiroFish's domain-specific entity typing is preserved as Custom kinds; fixed variants stay the fast path.
- **OQ-6 doc drift** → FIX `ARCHITECTURE.md`/`TODO.md` (RocksDB→redb, rayon→futures::stream::buffered, "LLM/agent pending"→implemented). Documentation debt; folds into SWEEP/cleanup, no owner decision.

> No OQ is a genuine 50/50 high-cost-to-reverse — all lock cleanly toward no-downgrade. `proposed-upgrades.md` not required for OQ-1..6. (If shimmy lacks `/v1/embeddings`, OQ-3's backend choice becomes the only owner escalation — noted as a substrate gap, not a feature drop.)

---

## (d) New teri module layout (port-fresh) + idiom map + dependency-equivalents

### New / extended module tree (concrete paths under `src/`)
```
src/
  config.rs                 (extend: U-001)
  main.rs                   (extend: U-002, U-049)
  server/mod.rs             (NEW: U-003 create_app, axum Router, middleware, /health)
  i18n/mod.rs               (NEW: U-005, U-050 — t(), locales, LOCALE task-local)
    locales/{en,zh}.json    (SWEEP-2)
  models/
    project.rs              (NEW: U-011)
    task.rs                 (NEW: U-012 TaskManager = Arc<RwLock<HashMap>>)
  graph/
    mod.rs                  (extend: wire build() U-015, add valid_at, append_episode, Custom kind, vec search)
    ontology.rs             (NEW: U-014)
  memory/mod.rs             (extend: implement query_vec_similarity U-017/OQ-3)
  agent/mod.rs              (extend: Persona social fields U-018)
  sim/
    mod.rs                  (extend: Action enum social variants; native runner U-022)
    action_logger.rs        (NEW: U-010)
    config_generator.rs     (NEW: U-019)
    ipc.rs                  (NEW: U-020 InterviewBus = tokio channels)
    manager.rs              (NEW: U-023 SimulationManager)
    platform.rs             (NEW: U-028/U-029 Twitter/Reddit platform impls)
    multi_runner.rs         (NEW: U-030 MultiPlatformRunner, OQ-1)
  report/mod.rs             (extend: ReACT loop, plan_outline, chat, section writes U-024)
  api/
    mod.rs                  (extend: keep types; add ApiState fields)
    graph.rs                (NEW: U-025 routes)
    simulation.rs           (NEW: U-026 routes)
    report.rs               (NEW: U-027 routes)
    streaming.rs            (reuse: TickBuffer/StreamAdapter already done)
frontend/                   (keep-Vue, re-point: U-031..U-044, SWEEP-1)
```

### Idiom map
| MiroFish (Python) | teri (Rust) |
|---|---|
| Flask Blueprint / `app.run` | axum `Router` + `tokio` server (`src/server`) |
| `threading.Thread(daemon=True)` | `tokio::spawn` task (no join; CancellationToken for cleanup) |
| file-IPC (`ipc_commands/`, `ipc_responses/`) | tokio `mpsc`/`oneshot` channels (`InterviewBus`) OR axum HTTP for cross-boundary |
| OASIS Python subprocess (`subprocess.Popen`) | native `SimEngine` (no subprocess — UPGRADE) |
| `subprocess.Popen` + pgid kill | `tokio::task` + `tokio_util::sync::CancellationToken` + timeout grace |
| Pydantic models / dataclasses | `serde` structs (`#[derive(Serialize,Deserialize)]`) |
| `asyncio` / `asyncio.gather` | `tokio` / `futures::join!` / `stream::buffered` (already used in SimEngine) |
| thread-local locale | `tokio::task_local!` `LOCALE` |
| Singleton (double-checked lock) | `Arc<RwLock<T>>` shared state / `OnceCell` |
| Zep Cloud SaaS graph | native petgraph `KnowledgeGraph` + redb |
| JSONL tail-by-offset polling | `SimEngine::subscribe_with_history` broadcast |
| `re.sub('<think>...')` | string strip in `OpenAiAdapter` |
| TaskManager in-memory dict | `models::task::TaskManager` (Arc<RwLock<HashMap>>) |

### Dependency-equivalent table
| MiroFish lib | teri Rust crate | note |
|---|---|---|
| Flask | axum (+ tower middleware) | already a teri dep target |
| openai SDK | teri `OpenAiAdapter` (reqwest) | present |
| zep_cloud | petgraph + redb | native (Decision-1) |
| camel-oasis / camel-ai | native `SimEngine` | reimplement (Decision-2) |
| pydantic | serde / serde_json | present |
| python-dotenv | std env + teri config | present (no .env files; envctl) |
| PyMuPDF (fitz) | pdfium_render | present in seed |
| chardet/charset-normalizer | encoding fallback in seed | verify parity |
| requests | reqwest | present |
| asyncio | tokio | present |
| chunk text util | seed split | present |
| vue-i18n (frontend) | vue-i18n (kept) + backend i18n module | OQ-4 |
| SQLite (OASIS internal) | redb / in-memory WorldState | no separate DB |
| shimmy embeddings (`/v1/embeddings`) | reqwest call | OQ-3 (verify exposed) |

---

## (e) Recommended ITERATE order

**Phase A — reuse-Y verify-only quick wins (cheap parity confirmations, no fresh port):**
U-004, U-006, U-008, U-009, U-013, U-035, U-037, U-038, U-039, U-040, U-042, U-043, U-044, U-048.
(Confirm teri's existing superset matches MiroFish; flag GAP-6 `<think>` as the one ADD in U-008.)

**Phase B — extend-Y (additive on verified-green teri modules):**
U-001 (config) → U-018 (Persona social fields) → sim `Action` enum variants (Decision-2 core) → U-015 (`build()` pipeline wiring) → U-024 (report ReACT) → U-036/U-041/U-049.
Then the cross-cutting schema changes: **OQ-2 `Relation.valid_at`** and **OQ-3 `query_vec_similarity`** (highest blast radius — land early in B so port-fresh consumers build on the final shapes).

**Phase C — port-fresh, leaf→entrypoint:**
U-005/U-050 i18n → U-012 TaskManager → U-011 → U-010 action_logger → U-020 InterviewBus → U-014 ontology → U-019 config_gen → U-023 manager → U-022/U-028/U-029/U-030 native sim platforms → U-003 server → U-025/U-026/U-027 routes → U-002 serve_cmd → frontend re-point (U-031..U-034).

**Highest-unlock units (do these to maximize downstream progress):**
1. **U-015 wire `build()`** — unblocks the entire graph→report→ontology chain (claim 1; helpers already exist, ~highest-ROI).
2. **OQ-2 `Relation.valid_at` + OQ-3 `query_vec_similarity`** — the two cross-cutting schema/stub changes; every graph/report consumer depends on the final shapes, so landing them early avoids rework.
3. **`Action` enum extension + native SimEngine platform layer (Decision-2)** — unlocks U-022/U-028/U-029/U-030 and the whole simulation entrypoint.

---

## DECISION-7 — parameterized chat (system+user, temperature, max_tokens) → extend-Y on U-008 (additive superset; GAP-6 lineage)

**Trigger:** U-014 `OntologyGenerator.generate()` and several other pending units (`SimulationConfigGenerator`, `ReportAgent`, `OasisProfileGenerator`) call MiroFish `LLMClient.chat_json(messages=[{system},{user}], temperature, max_tokens)` / `chat(...)` (`backend/app/utils/llm_client.py:35-102`). teri's `LlmClient` trait (`src/llm.rs:194-204`) only exposes `complete(&str)` / `complete_json::<T>(&str)` / `stream(&str)`: single user-role prompt, HARDCODED temperature (`complete`=0.7, `complete_json`=0.0), no system role, no `max_tokens`. A faithful port of `generate()` cannot currently express (a) a distinct system message, (b) `temperature=0.3`, (c) `max_tokens=4096`. Folding system+user into one prompt and accepting temp=0.0/no-cap is an **observable downgrade** (temperature + max_tokens are explicit source choices; role separation matters for some models).

**Class:** **extend-Y on U-008** (additive superset; GAP-6/Decision-3 lineage). Adds capability, never narrows. U-008 is already parity-verified `[x]` — this MUST NOT regress it.

### 1. New trait surface (chosen: option (a) — message-vector + options struct; reuses MiroFish's own `chat`/`chat_json` shape 1:1)

Add to `src/llm.rs` (new public type defs + two new trait methods):

```rust
/// One chat message. Role is a closed set (system|user|assistant) so a typo
/// can't silently produce an unknown role; serializes to the lowercase wire
/// string each provider expects.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

/// Optional tuning. None = adapter's existing default (so options-less callers
/// are unchanged). Mirrors MiroFish chat() kwargs temperature/max_tokens.
#[derive(Debug, Clone, Default)]
pub struct ChatOptions {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self { Self { role: ChatRole::System, content: content.into() } }
    pub fn user(content: impl Into<String>)   -> Self { Self { role: ChatRole::User,   content: content.into() } }
}
```

Two new trait methods (additive — the existing three are byte-identical, NOT touched):

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    // --- UNCHANGED (U-008 verified) ---
    async fn complete(&self, prompt: &str) -> Result<String>;
    async fn complete_json<T: DeserializeOwned>(&self, prompt: &str) -> Result<T>;
    async fn stream(&self, prompt: &str)
        -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>>;

    // --- NEW (DECISION-7): parameterized chat. system+user vector, opt temp/max_tokens ---
    async fn chat(&self, messages: &[ChatMessage], opts: &ChatOptions) -> Result<String>;
    async fn chat_json<T: DeserializeOwned>(
        &self,
        messages: &[ChatMessage],
        opts: &ChatOptions,
    ) -> Result<T>;
}
```

Rejected options: (b) builder — heavier API surface for no gain over an options struct; (c) one fat `ChatRequest` struct passed by value — fine but `&[ChatMessage] + &ChatOptions` keeps the message vector borrowable and matches MiroFish's `(messages, temperature, max_tokens)` arg shape exactly, easing differential testing.

### 2. No-regression mechanism — REQUIRED on all 3 adapters (NOT a default-impl)

A default trait impl is **forbidden here**: a no-op/single-prompt default would silently drop system role + temperature + max_tokens — a downgrade. Each of the 3 adapters implements `chat`/`chat_json` properly, building its own provider-native payload. The existing `complete`/`complete_json`/`stream` bodies are **unchanged** (byte-identical) — porter does additive `impl` blocks only.

Post-processing parity (reuse existing free fns — do NOT reimplement): `chat` applies `strip_think` to the extracted text; `chat_json` applies `strip_json_fence(&strip_think(raw))` then `serde_json::from_str`. Identical to the existing `complete`/`complete_json` post-processing (`llm.rs:302-303,331-335`) and to MiroFish `chat`/`chat_json` (`llm_client.py:67,94-100`). The retry/back-off path is inherited for free: each adapter's `chat*` calls the SAME `call_api(serde_json::Value)` the existing methods use — no new retry code.

**Per-adapter payload (each `chat_json` = build payload → `call_api` → extract → `strip_json_fence(strip_think(..))` → parse; `chat` = same minus fence/parse):**

- **OpenAiAdapter** (production / shimmy path, `llm.rs:279`): `messages[]` carries system+user roles verbatim; add `temperature` only when `opts.temperature.is_some()`, `max_tokens` only when `opts.max_tokens.is_some()`; `chat_json` also sets `"response_format": {"type":"json_object"}` (matches existing `complete_json` and MiroFish `chat_json`). Reuses `self.call_api(payload)` (llm.rs:236) unchanged. Extract `choices[0].message.content`.
  *(Defaults when `opts` field is None: omit the key → server/shimmy default. To exactly preserve the current `complete`/`complete_json` temps for any internal caller that migrates, the caller passes `temperature: Some(0.7)` / `Some(0.0)` explicitly — the new method does not hardcode.)*

- **AnthropicAdapter** (`llm.rs:494`): Anthropic does NOT take a `system` message role — it is a **top-level `system` string param**. Partition `messages`: concatenate all `System` contents into the top-level `"system"` field (join with `"\n\n"`), put `User`/`Assistant` into `messages[]`. `max_tokens` is **REQUIRED by the Anthropic API** — when `opts.max_tokens` is None, default to `4096` (matches the value already hardcoded at `llm.rs:505,547` and MiroFish's `max_tokens=4096` default). Add `temperature` when `opts.temperature.is_some()`. Reuses `self.call_api` (llm.rs ~460). Extract `content[0].text`. *(API facts confirmed against the claude-api skill: `system` is top-level, `max_tokens` is required, `temperature` accepted on current models.)*

- **GeminiAdapter** (`llm.rs:693`): system prompt → top-level `"systemInstruction": {"parts":[{"text": <joined system msgs>}]}`; user/assistant → `"contents":[{"role","parts":[{"text"}]}]` (Gemini role is `"user"`/`"model"` — map `Assistant`→`"model"`). Tuning → `"generationConfig": { "temperature": <if some>, "maxOutputTokens": <if some> }`. For `chat_json`, also set `"generationConfig.responseMimeType": "application/json"` (Gemini's JSON mode; the existing `complete_json` instead appends a "Respond with valid JSON only." sentinel — the new method may keep that sentinel as belt-and-suspenders, but the mime-type is the faithful expression of `response_format`). Reuses `self.call_api` (llm.rs:648). Extract `candidates[0].content.parts[0].text`.

### 3. Existing single-prompt callers — UNCHANGED (the no-regression answer: YES, confirmed)

All current callers of `complete`/`complete_json(&str)` keep calling them verbatim. DECISION-7 only ADDS `chat`/`chat_json`. No call site migrates unless it genuinely needs system+temp+max_tokens. The 496 existing tests touch only the unchanged methods.

### 4. Classification & invariants

- **extend-Y on U-008** (additive superset, GAP-6/Decision-3 lineage) — adding capability, never narrowing. NOT a new unit; NOT a `[≠]`.
- **No-regression guarantee:** existing `complete`/`complete_json`/`stream` bodies are byte-identical; new methods are purely additive `impl` items + new pub types. U-008's verified behavior and the 496 existing tests are untouched. New differential test for DECISION-7: feed `[system, user] + {temp:0.3, max_tokens:4096}` through MiroFish `chat_json` and teri `chat_json`; assert the system message is delivered as a distinct system input, temperature/max_tokens appear in the request payload (golden-capture the outbound JSON per provider), and the parsed JSON shape matches.
- **Built once, reused by:** U-014 `OntologyGenerator` (the trigger), plus `SimulationConfigGenerator` (U-019), `ReportAgent` (U-024), `OasisProfileGenerator` (U-018/agent persona generation) — every pending unit that calls MiroFish `chat_json`/`chat` with a `get_language_instruction()` system prompt + tuned temperature. Port this method ONCE in U-008-extend, before U-014.

**Idiom-map addendum** (append to the §(d) idiom table): `LLMClient.chat(messages=[{system},{user}], temperature, max_tokens)` → `LlmClient::chat(&[ChatMessage], &ChatOptions)` (system role: OpenAI=messages[], Anthropic/Gemini=top-level system param).

---

## DECISION-8 — U-015 `build_graph_async` (S-189) + `set_ontology` (S-192) → MAP-ONTO teri native async build over the real petgraph 2-pass pipeline

**Units:** U-015 (S-189 `build_graph_async`, rolling up S-192 `set_ontology`). DECISION-1 lineage (Zep→petgraph). The 5 existing `KnowledgeGraph::build` tests (`graph/mod.rs:945-1102`) MUST NOT regress — everything below is **additive**.

**Source contract (graph_builder.py):** `build_graph_async(text, ontology, graph_name, chunk_size=500, chunk_overlap=50, batch_size=3) -> task_id`. Spawns a daemon thread; the worker drives a Zep build with milestones 5/10/15/20/20-60/60-90/90/100 and reports `{graph_id, graph_info{node_count,edge_count,entity_types[]}, chunks_processed}`. `set_ontology` registers dynamic Zep Pydantic entity/edge classes from the validated dict.

### Q1 — `EntityKind::Custom(String)` (OQ-5/GAP-3): exact shape + blast radius (ADDITIVE, no serde regression)

**Decision:** add a single new tuple variant `Custom(String)` carrying the **PascalCase ontology entity-type name** verbatim (the post-`validate_and_process` `name`, e.g. `"MediaOutlet"`).

```rust
pub enum EntityKind {
    Person, Organization, Location, Concept, Event, Other,
    Custom(String),   // NEW — carries the ontology's PascalCase type name
}
```

- **Display** (`graph/mod.rs:24`): add arm `EntityKind::Custom(name) => write!(f, "{name}")`. Rationale: the 6 built-ins Display as lowercase tokens; a custom kind has no canonical lowercase token, so it Displays as its own PascalCase name (this is what every `entity.kind.to_string()` consumer in `agent/mod.rs` will emit — lines 888/922/1239/1274 — and matches MiroFish's Zep node-label string, which is the raw type name).
- **serde:** externally-tagged by default (existing variants serialize as bare strings `"Person"`; `Custom(s)` serializes as `{"Custom": s}`). **This is additive and non-regressing** — no existing variant's representation changes, so every previously-serialized `Entity`/graph (JSON + bincode) still round-trips identically (confirmed: `SerializableKnowledgeGraph` carries `Vec<Entity>`; only the new variant adds a new wire shape). The differential/golden parity is on the SET of entity *type names*, not on serde internals, so the tagged form is fine.
- **Parse** (`parse_entities_json`, `graph/mod.rs:693-701`): the `match kind_str` keeps its 6 named arms; the wildcard `_ => EntityKind::Other` is **narrowed** so a kind string matching a registered ontology type name becomes `Custom(kind_str)` (see Q2 for how the registered set reaches the parser). A kind string that is neither a built-in nor a registered custom type stays `Other` — unchanged fallback.
- **Blast radius (VERIFIED exhaustive — additive, 2 sites change):**
  - `graph/mod.rs:24` `Display` impl — add one arm (REQUIRED — non-exhaustive match won't compile).
  - `graph/mod.rs:693` `parse_entities_json` match — narrow the wildcard (Q2).
  - **No other exhaustive `match EntityKind` exists in the tree.** All non-test consumers (`agent/mod.rs:888,922,1239,1274`) use `entity.kind.to_string()` / `{}` Display — covered by the new Display arm; they need **zero** changes.
  - All `EntityKind::Person|Organization|…` *construction* sites (agent tests, etc.) are untouched (additive variant).
  - `Entity` derives `PartialEq, Eq, Hash` — `Custom(String)` satisfies all three (String is Hash+Eq). No derive breaks.
  - **Confirmed additive → no serialization regression for existing data; the 5 build tests + all agent tests stay green.**

### Q2 — `set_ontology` (S-192) faithful port: store registered type names on the graph; LLM extraction can emit `Custom`

**Decision:** Option **(a)** — `set_ontology` records the ontology's custom entity-type names on the `KnowledgeGraph` so Pass-1 extraction can (i) prompt the LLM with the allowed custom types and (ii) parse a returned custom type into `EntityKind::Custom(name)`. This is the **observable behavioral port** of "register dynamic entity types": in MiroFish, `set_ontology` constrains which entity types Zep will emit as node labels; in teri, it constrains/expands which `EntityKind`s `build()` can produce. **That effect on the resulting graph's entity types is contractual and IS ported.**

```rust
// New state on KnowledgeGraph (additive field; Default = empty):
ontology_entity_types: Vec<String>,   // PascalCase names from validate_and_process
ontology_edge_types:   Vec<String>,   // UPPER_SNAKE_CASE names (recorded; see [≠] note)

// New method (S-192 port):
/// Registers the entity/edge type names from a validated ontology dict
/// (output of `OntologyGenerator::validate_and_process`). Pulls the `name`
/// field from each `entity_types[]` / `edge_types[]` object. Idempotent set.
pub fn set_ontology(&mut self, ontology: &serde_json::Value);
```

- Pass-1 entity prompt is EXTENDED: when `ontology_entity_types` is non-empty, the allowed-kind list in `entity_extraction_prompt` becomes the 6 built-ins **plus** the registered custom names (so the LLM emits e.g. `"MediaOutlet"`); `parse_entities_json` maps any `kind_str` present in `ontology_entity_types` → `Custom(kind_str)`. **The 5 existing build tests do not set an ontology → `ontology_entity_types` is empty → the prompt + parse are byte-identical to today → no regression.**
- **What MUST match MiroFish (ported):** the *set of registered entity type names* drives which entity types appear in the built graph and in the result's `entity_types[]` rollup. This is the only externally observable effect of `set_ontology` on the graph.
- **What is legitimately `[≠]` (genuinely inexpressible / Zep-SDK-specific, non-contractual):**
  - The dynamic **Pydantic `EntityModel`/`EdgeModel` class synthesis** (`type(name, (EntityModel,), attrs)`) — a Zep SDK registration mechanism with no observable output other than "Zep now emits these labels," which teri reproduces via the recorded-name-set + `Custom`. `- [≠]` legal: inexpressible substrate (no Zep client), and the *behavior* (constrained type set) is ported, not dropped.
  - `RESERVED_NAMES` / `safe_attr_name` (`uuid|name|group_id|name_embedding|summary|created_at` → `entity_*`) — a guard against **Zep's reserved attribute-key collision**. teri's `Entity{id,name,kind}` has no per-entity attribute bag and no Zep key namespace, so there is no collision to guard. `- [≠]` legal: non-contractual (guards a Zep-internal namespace that teri does not have). NOTE: the source ontology prompt ALREADY forbids these names at generation time (ontology.rs:221), so the validated dict won't carry them anyway.
  - `Field(description=…, default=None)` + the pydantic `UserWarning` suppression — Zep-SDK API requirements; no teri analogue. `- [≠]` legal: inexpressible substrate.
  - **`ontology_edge_types` / edge attribute registration:** teri's relation extraction (Pass-2) matches `kind_str` against the **fixed** `RelationKind` set; it does NOT yet emit custom relation kinds. Recording `ontology_edge_types` is done (so the data is not dropped) but custom-edge-kind *emission* is **out of scope for U-015** and is flagged `- [!] U-015 custom edge kinds — Pass-2 RelationKind is fixed; ontology edge_types recorded but not yet emitted as relation kinds (needs a RelationKind::Custom follow-up, mirror of OQ-5)`. This is a `- [!]` (deferred-but-tracked), NOT a `- [≠]` (it IS expressible, just not in this unit).

### Q3 — `build()` progress-callback: ADDITIVE via a delegating `build_with_progress` (5 tests unchanged)

**Decision:** keep `build()` byte-identical; add a new method `build_with_progress` that carries the pipeline, and make `build()` delegate to it with a no-op callback. This is the **lowest-regression** option: zero existing call sites or tests change (they keep calling `build(doc, llm)`), and the closure-vs-trait-object decision stays internal.

```rust
// UNCHANGED signature — delegates with a no-op sink (existing 5 tests call THIS):
pub async fn build<L: LlmClient>(doc: &SeedDocument, llm: &L) -> Result<Self> {
    Self::build_with_progress(doc, llm, &mut |_p, _m| {}).await
}

// NEW — same pipeline, plus a progress sink and (optional) registered ontology.
// `progress` is `&mut dyn FnMut(i64, String)` (chosen over Option<cb>: no Option
// branching, the no-op closure is zero-cost, and it matches the worker's
// `update_task(progress, message)` shape 1:1).
pub async fn build_with_progress<L: LlmClient>(
    doc: &SeedDocument,
    llm: &L,
    progress: &mut dyn FnMut(i64, String),
) -> Result<Self>;
```

Rationale for `&mut dyn FnMut(i64, String)` over `Option<callback>`: the worker emits *monotonic* (progress, message) pairs at fixed milestones; a single callback type with a no-op default closure means `build()`'s body needs no `if let Some` guards and the existing tests' code path is provably identical (they route through the no-op). The ontology, if any, is applied to the graph *before* `build_with_progress` (caller does `g.set_ontology(&ont)`), OR `build_graph_async` accepts the ontology and applies it — see Q4.

### Q4 — `build_graph_async` (S-189) teri shape: signature, spawn, milestone map, result

```rust
// On the graph build service (new fn; lives where U-015 lands — graph module or a
// GraphBuildService). Returns the task_id immediately, like MiroFish.
pub fn build_graph_async<L: LlmClient + Clone + Send + Sync + 'static>(
    llm: L,
    text: String,
    ontology: serde_json::Value,        // validated dict (OntologyGenerator output)
    graph_name: String,                 // kept for parity; flows into result + task metadata
    chunk_size: usize,                  // default 500
    chunk_overlap: usize,               // default 50
    // batch_size: parity-only — see [≠] note (no Zep batching). Accept + ignore-with-record.
) -> String;                            // task_id
```

- **Spawn:** `let task_id = TaskManager::global().create_task("graph_build", Some(meta{graph_name, chunk_size, text_length}));` then capture `let locale = i18n::get_locale();` and `tokio::spawn(async move { i18n::with_locale(locale, async move { /* worker */ }).await });` — the tokio-task + task-local-locale idiom replacing MiroFish's daemon thread + `set_locale(locale)` (idiom-map rows already in §d). Return `task_id` immediately.
- **Worker body** = port of `_build_graph_worker`: build a `KnowledgeGraph`, `g.set_ontology(&ontology)`, then `KnowledgeGraph::build_with_progress(&doc, &llm, &mut |p,m| TaskManager::global().update_task(&task_id, Some(Processing), Some(p), Some(m), None, None, None))`, on `Ok` `complete_task`, on `Err` `fail_task(task_id, err.to_string())` (mirrors the `try/except traceback` → `fail_task`).
- **Milestone port/≠ table** (teri pipeline = set_ontology → split → 2-pass extraction):

  | MiroFish milestone | % | teri mapping | port / `[≠]` |
  |---|---|---|---|
  | startBuildingGraph | 5 | emit `t("progress.startBuildingGraph")` at worker start | **PORT** |
  | create_graph (Zep) | 10 | — (no Zep graph object; native graph is in-memory) | **`[≠]`** inexpressible: no Zep `graph.create`. No observable teri output. Do NOT emit `graphCreated`. |
  | set_ontology | 15 | `g.set_ontology(&ontology)`; emit `t("progress.ontologySet")` | **PORT** (Q2) |
  | text split | 20 | `split_text(text, chunk_size, chunk_overlap)`; emit `t_args("progress.textSplit",{count})` | **PORT** |
  | add_text_batches (Zep) | 20–60 | the **2-pass LLM extraction** (`build_with_progress` Pass-1+Pass-2) IS teri's text→graph step; map extraction progress into 20–60% (e.g. Pass-1 over N chunks → 20-40, Pass-2 → 40-60). May emit `t_args("progress.sendingBatch",…)` per chunk-group for shape parity. | **PORT** (re-mapped onto real pipeline) |
  | wait_for_episodes (Zep) | 60–90 | — (no async Zep episode processing; extraction is synchronous-await) | **`[≠]`** inexpressible: no Zep episode queue. Optional single 90% bridge tick; do NOT emit `waitingEpisodes`/`zepProcessing`. |
  | fetch_graph_info (Zep) | 90 | compute `node_count/edge_count/entity_types[]` directly from the in-memory graph; emit `t("progress.fetchingGraphInfo")` | **PORT** (native rollup) |
  | complete | 100 | `complete_task(task_id, result)` (sets `t("progress.taskComplete")`) | **PORT** |

- **Result shape (teri-native — no Zep `graph_id`):**

  ```json
  {
    "graph_name": "<graph_name>",
    "graph_info": { "node_count": <usize>, "edge_count": <usize>, "entity_types": ["<distinct EntityKind Display tokens, excluding built-in Other>"] },
    "chunks_processed": <usize>,
    "graph": <SerializableKnowledgeGraph JSON>   // teri persists the built graph IN the result (replaces Zep's server-side graph_id handle)
  }
  ```
  Rationale: MiroFish returns a `graph_id` that is a **Zep server handle** to fetch the graph later. teri has no remote handle, so the built graph is serialized into the task result (and/or persisted to redb by the caller). `graph_id` is `[≠]` inexpressible (Zep-server artifact); the *retrievable graph* it pointed to IS preserved (embedded). `entity_types[]` mirrors MiroFish's `_get_graph_info` label-set (distinct kinds, excluding the `Entity`/`Node`/`Other` filler — teri excludes `Other`).
- **`[≠]` notes recorded:** `batch_size` — Zep-batching artifact (rate-limited `graph.add_batch` + `time.sleep(1)`); teri's LLM calls are per-chunk with the adapter's own retry/backoff, no Zep batch endpoint. Accept the param for call-shape parity, record `- [≠] U-015 batch_size — Zep add_batch pacing artifact; teri has no batch endpoint (non-contractual, no observable output difference)`. The `_wait_for_episodes` 600s timeout + 3s poll is the same Zep-queue artifact, `[≠]` inexpressible.

### Q5 — i18n keys: NONE TO ADD (already present), and which Zep-only keys are NOT needed

**Decision: ZERO new `progress.*` keys.** VERIFIED — all 14 keys the worker uses already exist in BOTH `src/i18n/locales/{en,zh}.json` (byte-present): `startBuildingGraph, graphCreated, ontologySet, textSplit, sendingBatch, batchFailed, waitingZepProcess, waitingEpisodes, episodesTimeout, zepProcessing, processingComplete, noEpisodesWait, fetchingGraphInfo, taskComplete` (+ `taskFailed`). SWEEP-2 already byte-copied the MiroFish progress block.

- **Keys teri WILL emit** (ported milestones): `startBuildingGraph`, `ontologySet`, `textSplit`, optionally `sendingBatch`, `fetchingGraphInfo`, `taskComplete`, `taskFailed` (failure path).
- **Keys present but NOT emitted by teri's U-015 path** (Zep-milestone `[≠]`, retained in the locale because they are MiroFish-faithful strings and may be consumed by other units/frontend — do NOT delete): `graphCreated` (10% create_graph `[≠]`), `waitingZepProcess`/`waitingEpisodes`/`zepProcessing`/`episodesTimeout`/`noEpisodesWait` (60-90% Zep-episode-wait `[≠]`), `batchFailed` (only if batch-shape emit is kept). Retaining the key strings is correct (they are not a behavior; the locale is the shared source of truth per SWEEP-2) — the `[≠]` is on *emitting the milestone*, not on the string's existence.

---

### DECISION-8 — 6-line actionable summary

1. **`EntityKind::Custom(String)`** carrying the PascalCase ontology type name — ADDITIVE; only 2 sites change (`Display` arm @ graph/mod.rs:24, `parse_entities_json` wildcard @ :693); all `agent/mod.rs` consumers use Display and need zero change; no serde/data regression; 5 build tests + agent tests stay green.
2. **`set_ontology(&mut self, &Value)`** (S-192 port) records `ontology_entity_types`/`ontology_edge_types` on the graph → Pass-1 prompt+parse can emit `Custom`; Zep Pydantic-class synthesis / `RESERVED_NAMES`/`safe_attr_name` / `Field(default=None)` are legit `[≠]` (inexpressible Zep-SDK, non-contractual); custom EDGE kinds are `- [!]` deferred (RelationKind fixed).
3. **`build()` stays byte-identical**, delegating to new `build_with_progress(doc, llm, &mut dyn FnMut(i64,String))` (no-op closure) — 5 existing tests unchanged.
4. **`build_graph_async(llm, text, ontology, graph_name, chunk_size, chunk_overlap) -> task_id`** spawns `tokio::spawn(with_locale(locale, …))`, drives `set_ontology`→`build_with_progress`, milestones map 5/15/20/20-60/90/100 onto the REAL pipeline; create_graph(10)/wait_for_episodes(60-90) are `[≠]` (no Zep); result is teri-native `{graph_name, graph_info{node_count,edge_count,entity_types[]}, chunks_processed, graph:<serialized>}` (no Zep `graph_id`).
5. **i18n: ADD NOTHING** — all 14 worker `progress.*` keys already exist in both locales (SWEEP-2); teri emits the ported subset, leaves the Zep-only keys present-but-unemitted.
6. **Parity gate:** differential on the SET of entity type names (incl. Custom) + node/edge counts + the milestone progress sequence teri DOES emit; every `[≠]` above is challenged by the gate — they pass because each is Zep-server/SDK-inexpressible with no observable teri output, never a portable-feature skip.

---

## DECISION-9 — U-016 `ZepEntityReader` machinery (S-214..S-219) → `KnowledgeGraphEntityReader` over native petgraph

Source: `backend/app/services/zep_entity_reader.py` L71-437. DTOs `EntityNode`/`FilteredEntities` (L22-68) already ported & parity-verified at `src/services/entity_reader.rs` — **do not redesign them**. This decision covers ONLY the reader methods `_call_with_retry`, `get_all_nodes`, `get_all_edges`, `get_node_edges`, `filter_defined_entities`, `get_entity_with_context`, `get_entities_by_type`.

**Substrate confirmed (read of `src/graph/mod.rs`):** entity source is native `KnowledgeGraph` (petgraph), not Zep. `Entity{id:Uuid, name:String, kind:EntityKind}` — NO summary, NO attributes, single `kind`. `Relation{kind:RelationKind, weight:f32, valid_at}` — NO uuid, NO name, NO fact, NO attributes. Read surface: `get_all_entities()->Vec<&Entity>`, `get_all_edges()->Vec<(Uuid,Uuid,Relation)>` (`EdgeTriple`, endpoints carry **Entity.id**), `get_neighbor_relations(Uuid)->Result<Vec<(&Entity,&Relation,bool)>>` (bool = is_outgoing), `get_subgraph`, `entity_count()`, `get_entity(name)`. `index_by_id` exists internally but there is **no public `get_entity_by_id(Uuid)`** — see Q6.

**Consumer evidence (the no-downgrade pivot — verified, not assumed):** the ONLY consumers of the reader's output are U-018 `oasis_profile_generator._build_entity_context` (L414-473) and U-019 `simulation_config_generator`. `_build_entity_context` reads, per related_edge, `fact` FIRST and falls back to `edge_name`+`direction` when `fact` is empty (L439-450); per related_node it reads `name`/`labels`/`summary`; it reads `entity.attributes` (L426). `entity.summary` feeds `_generate_profile_with_llm` (L243) and is the `persona` fallback `entity.summary or "A {type} named {name}"` (L261). So every "missing" Zep field has an EXPLICIT consumer-side graceful fallback — that is what makes a `[≠]` legal here (the observable persona/context output is still produced via the fallback path), and what tells us where a derive is preferable to an empty.

### Q1 — Reader shape: borrowed struct over `&KnowledgeGraph`

**DECISION: `pub struct KnowledgeGraphEntityReader<'a> { graph: &'a KnowledgeGraph }`** with `pub fn new(graph: &'a KnowledgeGraph) -> Self`. New file `src/services/entity_reader.rs` (extend the existing module — DTOs already live there). No `api_key`, no Zep client, no `graph_id` parameter on any method (the graph IS the bound reference).

- Rationale: an in-process read borrows the graph; free functions would lose the `'a` cohesion and the obvious "this reader reads THIS graph" contract. `graph_id: str` on every Python method is a Zep server-graph selector — **`[≠]` inexpressible** (no remote graph handle); the bound `&KnowledgeGraph` is the teri selector.
- `__init__(api_key)` + `ZEP_API_KEY` validation + `Zep(api_key=…)` client construction → **`[≠]` inexpressible** (Zep-auth for a network client; an in-process petgraph read has no auth, no client). Owner-rule: genuinely inexpressible, non-contractual (no observable output), not a feature skip.

### Q2 — `EntityNode` field-mapping from teri `Entity` (the critical no-downgrade table)

| EntityNode field | ← teri source | verdict | owner-rule justification |
|---|---|---|---|
| `uuid` | `entity.id.to_string()` | **PORT** | clean 1:1. |
| `name` | `entity.name.clone()` | **PORT** | clean 1:1. |
| `labels` | `vec![entity.kind.to_string()]` | **PORT (mapped)** | teri has a single `kind`, not a Vec. Map to a 1-element label vec carrying the Display token (built-ins lowercase e.g. `"person"`; `Custom` = PascalCase type name verbatim). Do **NOT** synthesize the Zep base `"Entity"`/`"Node"` labels — see Q3. The observable downstream use of `labels` is `get_entity_type()` (first non-`Entity`/`Node` label) and U-018's custom-label filter; with a 1-element real-kind vec both yield exactly that kind. Faithful: the *entity type* the consumer extracts is identical. |
| `summary` | `String::new()` | **`[≠]` inexpressible — RECORDED with consumer impact** | teri's extraction (`build()`) genuinely produces no per-entity summary; Zep auto-generates it server-side during ingestion. There is **no portable source on `Entity`** to derive a faithful per-node summary from (deriving from relation facts would itself be synthetic, and `fact` is also `[≠]` — Q4). Consumer impact (MUST be recorded, not hidden): U-018 `_generate_profile_with_llm` receives `entity_summary=""`; the `persona` fallback becomes `"A {type} named {name}"` (L261); `_build_entity_context` simply omits the `### 实体属性`/summary-derived lines. The persona is still produced (graceful), at reduced input richness. This is a substrate gap, not a downgrade-by-laziness. **Porter MUST emit `summary=""` only WITH this ledger line, never silently.** |
| `attributes` | `serde_json::Map::new()` (empty) | **`[≠]` inexpressible** | teri `Entity` has no attributes map; Zep attributes are server-extracted KV pairs. No portable source. Consumer (U-018 L426-432) guards `if entity.attributes:` → simply skips the attributes block. Non-emitting is the consumer's existing graceful path. Record alongside `summary`. |
| `related_edges` | built from edges (Q4) | **PORT** | see Q4. |
| `related_nodes` | built from edge endpoints (Q4) | **PORT** | see Q4. |

### Q3 — The `{Entity,Node}`-only skip filter: **always-pass in teri (observable-equivalent)**

MiroFish skips nodes whose only labels are `{Entity, Node}` (a node Zep created but never typed). teri entities ALWAYS carry a real `kind` (one of 6 built-ins or `Custom`), so `labels = [kind.to_string()]` NEVER reduces to only `{Entity,Node}` (none of the Display tokens equal the exact-case strings `"Entity"`/`"Node"` — built-ins are lowercase; `Custom` is a distinct ontology name). **DECISION: keep the filter code verbatim (port the `custom_labels = labels - {Entity,Node}; if not custom_labels: continue` logic), which in teri is simply always-pass.** Porting the logic (not deleting it) is correct: it is the faithful expression, it costs nothing, and it stays correct if a future label-set ever includes those tokens. The skip being a no-op is an observable *consequence* of teri's typed entities, not a dropped behavior — `filtered_count` == count-of-typed-entities in both systems. NOT a `[≠]`; it is a PORT whose branch is unreachable given teri's data.

### Q4 — Edge field mapping (`related_edges` / `get_all_edges` / `get_node_edges`)

teri `Relation` has no uuid/name/fact/attributes. The edge dicts MiroFish builds come in two shapes — the **full** dict (`get_all_edges`/`get_node_edges`: `{uuid,name,fact,source_node_uuid,target_node_uuid,attributes}`) and the **related_edges** dict (`{direction, edge_name, fact, target_node_uuid|source_node_uuid}`). Map per the consumer's read path (L439-450 reads `fact`→fallback `edge_name`+`direction`):

| edge field | ← teri source | verdict | justification |
|---|---|---|---|
| `source_node_uuid` / `target_node_uuid` | endpoint `Entity.id.to_string()` (from `EdgeTriple`/`get_neighbor_relations`) | **PORT** | clean; endpoints are real entity ids. |
| `edge_name` (related_edges) / `name` (full) | `relation.kind.to_string()` | **PORT (mapped)** | RelationKind Display (`"WorksFor"`, `"RelatedTo"`, … or `Custom` verbatim). This is exactly the fallback the consumer uses when `fact` is empty (L446-450). Faithful: the relation label the consumer renders is identical. |
| `direction` (related_edges) | the `is_outgoing` bool from `get_neighbor_relations` → `"outgoing"`/`"incoming"` | **PORT** | 1:1 with MiroFish's source/target comparison; same string literals. |
| `fact` | `String::new()` (empty) | **`[≠]` inexpressible** | Zep's `fact` is an LLM-generated natural-language sentence about the edge produced during Zep ingestion; teri stores only `(kind, weight)`. Consumer is graceful: empty `fact` → renders the `edge_name`-template line instead (L446-450), which teri DOES produce. So the observable "relationships" section is still emitted (via the fallback). **Porter emits `fact=""`; the `edge_name`/`direction` path carries the observable output.** (A derived `"{from} {kind} {to}"` sentence is *optional* and NOT required — the consumer's own fallback already covers it; do not invent a fact that diverges from MiroFish's empty-fact→template behavior. Keep `fact=""`.) |
| `uuid` (full dict only) | `String::new()` | **`[≠]` non-contractual** | edge uuid is read by NO consumer of `get_all_edges`/`get_node_edges` in MiroFish (it is dict-shape filler); teri Relation has no uuid; synthesizing one would be a fabricated observable with no reader. Emit `""`. (Do NOT synthesize a deterministic uuid — there is no consumer to satisfy and it would be a divergence from MiroFish's `edge.uuid_ or ""` which is itself usually empty for these reads.) |
| `attributes` (full dict only) | empty Map | **`[≠]` inexpressible** | same as node attributes; no source, no consumer reads it. |

`related_nodes` entries `{uuid,name,labels,summary}` are built from the related entity via the SAME Entity→fields mapping as Q2 (so `summary=""` there too, consumer-graceful at L467-470: no summary → renders `**{name}**{label_str}` without the summary suffix).

### Q5 — `filter_defined_entities` logic onto `KnowledgeGraph`

Signature: `pub fn filter_defined_entities(&self, defined_entity_types: Option<&[String]>, enrich_with_edges: bool) -> FilteredEntities`. (No `graph_id`; Q1.)

- Iterate `self.graph.get_all_entities()`; `total_count = graph.entity_count() as i64` (== all entities).
- Per entity build `labels = [kind.to_string()]`; compute `custom_labels = labels - {Entity,Node}` (Q3: always == labels). `if custom_labels.is_empty() { continue }` (unreachable in teri, ported verbatim).
- If `defined_entity_types` is `Some(types)`: `matching = custom_labels ∩ types`; `if matching.is_empty() { continue }`; `entity_type = matching[0]`. Else `entity_type = custom_labels[0]`. Match is against the **EntityKind Display string** (built-in lowercase token or Custom name) — so `get_entities_by_type("person")` matches `EntityKind::Person`, `get_entities_by_type("MediaOutlet")` matches `Custom("MediaOutlet")`. **Note for callers:** built-in matches require the lowercase Display token (Q-flag for parity: MiroFish matched Zep PascalCase labels; teri built-ins Display lowercase. This is the SAME divergence already accepted in DECISION-8 for `EntityKind::Display`; `Custom` names match verbatim. The parity gate compares against teri's own Display contract, consistent with DECISION-8.)
- `entity_types_found: HashSet<String>` ← inserted `entity_type` per kept entity.
- Build `EntityNode` (Q2 mapping). If `enrich_with_edges`: **use `get_neighbor_relations(entity.id)`** (NOT a full `get_all_edges` O(n·e) rescan) to produce `related_edges` (direction + edge_name + fact="" + the opposite-endpoint id) and the `related_node_uuids` set; then resolve each related uuid to its `{uuid,name,labels,summary}` via the entity lookup (Q6). **Equivalence proof:** MiroFish scans `all_edges` filtering `source==node.uuid` (→outgoing) / `target==node.uuid` (→incoming); `get_neighbor_relations` returns exactly the outgoing-then-incoming incident edges with the `is_outgoing` flag — same set, same direction labels, same opposite endpoints, but O(degree) per node instead of O(e). Output is identical; only the traversal cost differs (a strict efficiency improvement, allowed). `related_nodes` dedup: collect endpoint ids into a set first (matching MiroFish's `related_node_uuids` set semantics) before resolving, so a multi-edge pair yields one related_node.
- `filtered_count = filtered_entities.len() as i64`. Return `FilteredEntities{entities, entity_types: entity_types_found, total_count, filtered_count}`.

`get_entities_by_type(&self, entity_type: &str, enrich_with_edges: bool) -> Vec<EntityNode>` = `self.filter_defined_entities(Some(&[entity_type.to_string()]), enrich_with_edges).entities` (1:1 port of L413-435).

### Q6 — Entity-by-id lookup (resolving related_node uuids & `get_entity_with_context`)

`filter_defined_entities` resolves related-node uuids → entity, and `get_entity_with_context` takes an `entity_uuid`. teri exposes `get_entity(name)` and internal `index_by_id` but **no public `get_entity_by_id(Uuid)`**. **DECISION: add a small public accessor `pub fn get_entity_by_id(&self, id: Uuid) -> Option<&Entity>` to `KnowledgeGraph`** (reads existing `index_by_id`). This is **ADDITIVE** (new pub fn, reads an existing private field) — does NOT change `Entity`/`Relation`/any existing signature, zero blast radius on verified types, no existing caller affected. (Alternative — building an id→entity map inside the reader by scanning `get_all_entities` — is also acceptable and needs no graph change; pick the accessor for O(1) lookup and reuse by `get_entity_with_context`. Porter may choose the in-reader map if avoiding any graph edit is preferred — both are observ­ably identical. **Flagging the accessor as the recommended path; it is additive, not a type change.**)

### Q7 — `get_entity_with_context(&self, entity_uuid: &str) -> Option<EntityNode>`

Parse `entity_uuid` → `Uuid`; `get_entity_by_id` (Q6). **`if None → return None`** (parse failure OR missing id both yield `None`). Build the EntityNode with its edges via `get_neighbor_relations(id)` + related_nodes (same as Q5 enrich path). Wrap the whole body so any internal error → `None`.

- **except→None contract: PORTED (contractual).** MiroFish wraps the Zep calls in `try/except → return None` (L409-411) and `if not node: return None` (L355). teri: a bad/unknown uuid → `None`, never a panic or `Err`. This is observable error behavior and IS preserved. (`get_all_nodes(graph_id)` re-fetch inside the Python version L362 is a Zep round-trip; teri already has the whole graph bound — just resolve ids directly. The `node_map` is the bound graph. No behavior lost.)

### Q8 — `get_node_edges(&self, node_uuid: &str) -> Vec<Value>` (or `Vec<EdgeDict>`)

Parse uuid; if missing/unparseable → **`Vec::new()`** (the `except → return []` contract, L211-213). Otherwise return the full-shape edge dicts (Q4) for that node's incident edges via `get_neighbor_relations`.

- **except→[] contract: PORTED (contractual).** A missing node → empty vec, never an error.

### Q9 — `_call_with_retry`: **`[≠]` non-contractual** (retry dropped); error-fallback **PORTED** (contractual)

**Separate the two behaviors the Python conflates:**
- **Retry/exponential-backoff** (3 attempts, delay 2.0×2, `time.sleep`): this exists ONLY to survive transient Zep network/API failures. An in-process petgraph read has **no I/O, no transient failure** — a `HashMap`/`petgraph` lookup either succeeds or the key is absent (and absence is handled by `None`/`[]`, not retried — retrying a missing uuid would loop pointlessly). **DECISION: `_call_with_retry` is `[≠]` inexpressible/non-contractual** — there is no fallible network call to wrap; retrying produces no different observable outcome. Do NOT port a retry loop, do NOT add artificial backoff. (Owner-rule: genuinely inexpressible — the failure mode it guards cannot occur in-process; non-contractual — no observable output depends on it.)
- **The `except → None` / `except → []` FALLBACKS** that `_call_with_retry`'s callers wrap around it (Q7, Q8): these ARE observable error contracts and **ARE PORTED** (a bad uuid → None / empty, deterministically). The retry is dropped; the graceful-degradation outcome is kept. This is the precise split the unit-spec demanded.

### Method signature summary (the porter's contract)

```rust
// src/services/entity_reader.rs  (extends the module that already holds EntityNode/FilteredEntities)
pub struct KnowledgeGraphEntityReader<'a> { graph: &'a KnowledgeGraph }

impl<'a> KnowledgeGraphEntityReader<'a> {
    pub fn new(graph: &'a KnowledgeGraph) -> Self;
    pub fn get_all_nodes(&self) -> Vec<Value>;                 // node dicts {uuid,name,labels,summary="",attributes={}}
    pub fn get_all_edges(&self) -> Vec<Value>;                 // full edge dicts {uuid="",name,fact="",source_node_uuid,target_node_uuid,attributes={}}
    pub fn get_node_edges(&self, node_uuid: &str) -> Vec<Value>;          // except→[] ; retry [≠]
    pub fn filter_defined_entities(&self, defined_entity_types: Option<&[String]>, enrich_with_edges: bool) -> FilteredEntities;
    pub fn get_entity_with_context(&self, entity_uuid: &str) -> Option<EntityNode>;   // except→None ; retry [≠]
    pub fn get_entities_by_type(&self, entity_type: &str, enrich_with_edges: bool) -> Vec<EntityNode>;
}
// ADDITIVE on KnowledgeGraph (graph/mod.rs): pub fn get_entity_by_id(&self, id: Uuid) -> Option<&Entity>
```
(`get_all_nodes`/`get_all_edges` return `Vec<Value>` to preserve the Python dict-shape contract these methods promise; the in-reader code uses typed entities directly and only materializes dicts at these public boundaries. They have no MiroFish consumer beyond `filter_defined_entities` internally, but are ported as public per S-214/S-215.)

### Blast-radius flag (read before the porter runs)

- **NO change to the verified `Entity` / `Relation` / `EntityKind` / `RelationKind` types.** All field "gaps" (`summary`, `attributes`, `fact`, edge `uuid`) are emitted as empty/`[≠]` at the reader boundary, NOT by extending the verified types. The earlier-considered "extend `Entity` with a `summary` field" is **explicitly rejected** — teri's extraction produces no summary data to populate it, so the field would be uniformly empty (a no-op type change that touches a parity-verified struct for zero benefit). Recorded as `[≠]` instead.
- **ONE additive change to `KnowledgeGraph`:** `get_entity_by_id` (new pub fn, reads existing `index_by_id`). Zero blast radius — no signature/field change, no existing caller affected. Porter may avoid even this by building an in-reader id→entity map. Either way the verified types are untouched.

### DECISION-9 — 6-line actionable summary

1. **`KnowledgeGraphEntityReader<'a>{ graph:&'a KnowledgeGraph }`** in `src/services/entity_reader.rs` (extends the DTO module); no api_key/client/`graph_id` — Zep-auth + graph_id are `[≠]` inexpressible. The bound `&KnowledgeGraph` is the selector.
2. **EntityNode mapping:** `uuid←id.to_string()`, `name←name`, `labels←[kind.to_string()]` (PORT-mapped, 1-elem real kind); `summary←""` and `attributes←{}` are **`[≠]` inexpressible** — RECORDED with consumer impact (U-018 persona falls back to `"A {type} named {name}"`; context omits attr/summary lines — graceful, reduced richness, NOT silent).
3. **Edge mapping:** `source/target_node_uuid←endpoint id`, `edge_name/name←kind.to_string()`, `direction←is_outgoing` are **PORT**; `fact←""` (`[≠]` Zep-LLM, consumer falls back to the edge_name template it already renders), edge `uuid←""`/`attributes←{}` (`[≠]` non-contractual, no reader). Keep `fact=""` — do NOT invent a fact.
4. **`filter_defined_entities`** ports the `{Entity,Node}`-skip verbatim (always-pass in teri — typed entities — NOT a `[≠]`, a PORT with an unreachable branch); type-match against EntityKind Display (lowercase built-ins / Custom verbatim — same divergence DECISION-8 accepted); enrich via **`get_neighbor_relations`** (O(degree), provably the same set as MiroFish's O(n·e) `all_edges` scan — efficiency upgrade); `total_count=entity_count`, `filtered_count=kept`.
5. **Retry vs error-contract split:** `_call_with_retry` is **`[≠]` non-contractual** (no in-process I/O can transiently fail; no observable difference) — NOT ported. The `except→None` (`get_entity_with_context`, incl. bad/unknown uuid) and `except→[]` (`get_node_edges`) fallbacks ARE observable contracts and **ARE PORTED**.
6. **Blast radius:** verified `Entity`/`Relation`/`EntityKind`/`RelationKind` **UNCHANGED** (rejected extending `Entity` with `summary` — no data to fill it); ONE additive `KnowledgeGraph::get_entity_by_id(Uuid)->Option<&Entity>` recommended (reads existing `index_by_id`, zero blast radius), or an in-reader id-map if no graph edit is wanted. Parity gate: differential on the kept-EntityNode set, entity_types, related_edges/nodes context, total/filtered counts, and the None/[] error paths; each `[≠]` is Zep-server/SDK-inexpressible with a verified consumer-side graceful fallback — never a portable-feature skip.

---

## DECISION-10 — U-018 OASIS profile-export layer un-`[≠]`'d + ported (S-367,S-369,S-370,S-371,S-372,S-373); U-023 sub-cycle order

**Trigger:** scoping U-023 `SimulationManager.prepare_simulation` surfaced a NO-DOWNGRADE violation. Stage 2 of `prepare_simulation` calls `OasisProfileGenerator.generate_profiles_from_entities(...)` then **saves `reddit_profiles.json` + `twitter_profiles.csv`**; `get_profiles(sim_id, platform)` READS those files; the U-026 HTTP API `GET /<id>/profiles` SERVES them. teri ALSO already ships i18n keys (en+zh, `src/i18n/locales/{en,zh}.json:657/659`) `loadedRedditProfiles`/`loadedTwitterProfiles` that name those exact files. These were `[≠]`'d in symbol-map (S-367/S-369/S-370/S-371/S-372/S-373) as "batch is orchestrator's job" / "OASIS file export not needed". Per the owner NO-DOWNGRADE rule and the `[≠]` bar, a feature with a **distinct observable output** (a JSON file, a CSV file, a List[profile] returned to a discrete caller) is PORTED, not `[≠]`-skipped. (Direct precedent: U-018 `to_reddit_format`/`to_twitter_format`/`to_dict` were already un-`[≠]`'d and ported for exactly this reason — and that skip had also hidden a bio+persona field collapse.)

### §1 — Profile DATA-FLOW ruling: the files ARE contractual (CONFIRMED, agree with the prior)

**RULING: the profile data flows through FILES in teri, matching MiroFish.** `reddit_profiles.json` + `twitter_profiles.csv` are **CONTRACTUAL observable outputs** that MUST be produced. Evidence (all three independently sufficient, jointly decisive):
1. **The U-026 API serves them** — `GET /<id>/profiles` returns the parsed file contents; the file shape IS the API response contract (an external observable). Skipping the files would force the API to fabricate a different in-process shape → an observable divergence on a public HTTP route.
2. **`get_profiles(sim_id, platform)` READS the files** (`simulation_manager.py` ~L481-495) — the read side is the documented contract; a reader with nothing to read is a downgrade.
3. **teri's own i18n keys already anticipate them** — `loadedRedditProfiles`("Loaded {count} profiles from reddit_profiles.json") / `loadedTwitterProfiles`(twitter_profiles.csv) exist in BOTH locales. teri's UI/log layer already expects to announce loading from these exact filenames. The keys are present-and-emittable; the files must exist for the message to be true.

**The native-SimEngine objection is REFUTED, not accepted.** The locked substrate decision (OASIS Python subprocess → teri native `src/sim/` SimEngine) changes only *who consumes the profiles to drive the simulation* (MiroFish: the OASIS subprocess reads the files; teri: the SimEngine reads in-process). It does **NOT** remove the file contract, because the API + `get_profiles` + the i18n keys consume the files **regardless of what the sim engine consumes**. The file outputs are an export/persistence contract on the *manager*, orthogonal to the engine's input path. teri SimEngine MAY additionally take the `Vec<SocialProfile>` in-process (efficiency, no re-parse) — that is a strict-superset additive convenience and does not relieve the obligation to write the files. **Files: contractual. In-process Vec to the engine: additive, allowed, does not replace the files.**

`save_profiles_to_json` (S-373) is the deprecated alias delegating to `save_profiles` — it emits the SAME `reddit_profiles.json`. It is the same observable output; port it as a thin delegating alias (it costs ~3 lines and keeps the public surface faithful), OR fold callers onto `save_profiles` if no caller uses the alias — see §3 verdict.

### §2 — Per-symbol PORT-vs-`[≠]` verdict table (S-367..S-373)

| Symbol | Method | Verdict | Owner-rule justification |
|---|---|---|---|
| **S-367** | `generate_profiles_from_entities` | **PORT (un-`[≠]`)** | U-023 calls it as a **discrete method** producing a `List[OasisAgentProfile]` that is then file-saved AND fed to config-gen. The `[≠]` rationale "AgentPool::spawn does it" was wrong: the **parallel orchestration** (ThreadPoolExecutor / `parallel_count`) is the caller's concern, but the **method itself** — sequential-or-parallel loop over `generate_profile_from_entity`, per-profile fallback on error, progress_callback `(current,total,msg)`, realtime-save hook, returns ordered `Vec` — is a real observable unit `prepare_simulation` invokes. Port a real `generate_profiles_from_entities` returning `Vec<SocialProfile>`. See §3 for signature. |
| **S-368** | `_print_generated_profile` | **STAYS `[≠]` (legitimate)** | Console pretty-print of a profile (`【简介】`/`【详细人设】`/`【基本属性】` blocks) to stdout. Non-contractual: no caller reads stdout; it is a human-tracing convenience, not an observable output any consumer (API/get_profiles/SimEngine) depends on. teri's tracing/logging layer covers progress via `progress_callback` + the i18n `progress.profileGenerated` key (already SWEEP-ported). Genuinely a debug-print → legal `[≠]`. (If a future trace contract names this exact format, re-flag — for now non-contractual.) |
| **S-369** | `save_profiles(profiles, file_path, platform)` | **PORT (un-`[≠]`)** | The platform-dispatch writer (`platform=="twitter"`→CSV else→JSON). Produces the contractual files (§1). Was `[≠]`'d "not needed" — refuted by API+get_profiles+i18n. PORT. |
| **S-370** | `_save_twitter_csv` | **PORT (un-`[≠]`)** | Produces `twitter_profiles.csv` — a contractual file with a **specific OASIS column contract** the API serves. NOTE the writer is NOT just `to_twitter_format` dumped: header is exactly `['user_id','name','username','user_char','description']`; `user_id` is the **CSV row index** (0-based, NOT `profile.user_id`); `user_char = bio` or `"{bio} {persona}"` when persona≠bio, with `\n`/`\r`→space; `description = bio` with `\n`/`\r`→space. This column shape is the OASIS contract — must port the writer, cannot substitute the serializer. PORT. |
| **S-371** | `_normalize_gender` | **PORT — its re-flag FIRES** | The 中文→en map (`男`→male,`女`→female,`机构`/`其他`→other, en passthrough, default→other). Symbol-map S-371 explicitly re-flagged it: "if OASIS Reddit/Twitter export is ever ported, MUST port WITH it (contractual to that output)." We are now porting `_save_reddit_json` (its only call site) → **the dependency fires; PORT it.** It is contractual to the reddit JSON `gender` field. |
| **S-372** | `_save_reddit_json` | **PORT (un-`[≠]`)** | Produces `reddit_profiles.json`. **Critical: this writer is NOT `to_reddit_format`.** It FORCES OASIS-mandatory defaults that `to_reddit_format` conditionally omits: `user_id` = `profile.user_id ?? row_idx`; `bio` = `bio[:150]` (truncate to 150 chars) or `"{name}"` fallback; `persona` = `persona` or `"{name} is a participant in social discussions."`; `karma` = `karma or 1000`; **`age` = `age or 30`**, **`gender` = `_normalize_gender(gender)`** (always present), **`mbti` = `mbti or "ISTJ"`**, **`country` = `country or "中国"`** — these four are UNCONDITIONAL with hard defaults (OASIS `agent_graph.get_agent()` requires them), whereas `to_reddit_format` omits them when falsy. Optional `profession`/`interested_topics` only when truthy. The `user_id` field is load-bearing (OASIS matches `initial_posts.poster_agent_id`). **Must port the dedicated writer** — reusing `to_reddit_format` would drop the mandatory-default contract = a downgrade. PORT. |
| **S-373** | `save_profiles_to_json` | **PORT as thin alias (un-`[≠]`)** | Deprecated alias: logs a deprecation warning then delegates to `save_profiles(...)`. Same observable output (`reddit_profiles.json`). Port as a ~3-line delegating fn (emit the deprecation log via `tracing::warn!`, call `save_profiles`) to keep the public surface faithful. If the left-behind sweep confirms NO caller (U-023 uses `save_profiles` directly), the porter MAY record it as a `[≠]` "deprecated-alias, zero callers, superseded by save_profiles" — but **default to porting the alias**; the per-symbol cost is trivial and it removes any doubt. |

**Net:** S-367, S-369, S-370, S-371, S-372 → **PORT** (un-`[≠]`). S-373 → **PORT (thin alias)**, downgradeable to a documented zero-caller `[≠]` only if the sweep proves no caller. S-368 → **legitimately STAYS `[≠]`** (non-contractual stdout debug-print; progress is carried by `progress_callback` + the already-ported `progress.profileGenerated` i18n key).

### §3 — Where the ported export layer lives + signatures

**Module: NEW `src/services/oasis_profile_export.rs`** (a new file in the existing `src/services/` module tree; add `pub mod oasis_profile_export;` to `src/services/mod.rs`). Rationale: this is a **service/persistence** concern (FS writers + a batch orchestrator over the generator), not a method on `PersonaGenerator` (which owns single-profile *generation*, not multi-profile *export*). Keeping it out of `src/agent/mod.rs` means **zero edits to the parity-verified `SocialProfile` / `PersonaGenerator` / serializer surface** — the export layer CONSUMES them additively. This also mirrors MiroFish's own split (generator vs the save_* methods live on the same class there, but in Rust the borrow/ownership story is cleaner with a free-function service module that takes `&[SocialProfile]`).

**Reuse (additive only — NO change to verified types):** `SocialProfile` (all fields present incl. `user_id,user_name,bio,persona,karma,age,gender,mbti,country,profession,interested_topics,created_at`); `Persona::to_reddit_format`/`to_twitter_format`/`to_dict` (already verified — used by `realtime` JSON path where MiroFish uses `to_reddit_format`); `agent::generate_username(name)`; the single-profile `PersonaGenerator::generate_social<L>(...)`; `EntityNode::get_entity_type()`.

Signatures (the porter's contract):

```rust
// src/services/oasis_profile_export.rs
use crate::agent::{PersonaGenerator, SocialProfile};
use crate::services::entity_reader::EntityNode;

/// Output platform for the OASIS export (selects file format).
pub enum OutputPlatform { Reddit, Twitter }

/// Batch-generate profiles from filtered entities (S-367).
/// Sequential or bounded-parallel loop over generate_social; per-entity fallback
/// profile on generation error (mirrors MiroFish's try/except → fallback OasisAgentProfile);
/// progress_callback invoked (current, total, message) after each; optional realtime
/// file write after each completion. Returns profiles in ENTITY ORDER (Vec index == idx).
///
/// `parallel_count` mapping (MiroFish ThreadPoolExecutor): see §4 — caller (prepare_simulation)
/// chooses sequential (parallel_count<=1) vs tokio JoinSet (parallel_count>1). The realtime-save
/// + ordered-result + per-error-fallback semantics are identical either way.
pub async fn generate_profiles_from_entities<L: crate::llm::LlmClient>(
    generator: &PersonaGenerator,
    llm: &L,
    entities: &[EntityNode],
    graph: Option<&crate::graph::KnowledgeGraph>,   // for build_entity_context enrichment
    use_llm: bool,
    parallel_count: usize,
    realtime_output: Option<(&std::path::Path, OutputPlatform)>,  // realtime save hook
    progress_callback: &mut dyn FnMut(i64, i64, String),
) -> Vec<SocialProfile>;

/// Platform-dispatch writer (S-369): twitter→CSV else→JSON.
pub fn save_profiles(profiles: &[SocialProfile], file_path: &std::path::Path, platform: OutputPlatform) -> std::io::Result<()>;

/// Reddit JSON writer (S-372) — FORCES OASIS-mandatory defaults (NOT to_reddit_format):
///   user_id = profile.user_id (fallback row idx); bio = bio[:150] or "{name}";
///   persona = persona or "{name} is a participant in social discussions.";
///   karma = karma|1000; age = age|30; gender = normalize_gender(gender) [ALWAYS];
///   mbti = mbti|"ISTJ"; country = country|"中国"; optional profession/interested_topics if truthy.
fn save_reddit_json(profiles: &[SocialProfile], file_path: &std::path::Path) -> std::io::Result<()>;

/// Twitter CSV writer (S-370) — header ['user_id','name','username','user_char','description'];
///   user_id = ROW INDEX (0-based, not profile.user_id);
///   user_char = bio, or "{bio} {persona}" when persona != bio, with \n/\r -> space;
///   description = bio with \n/\r -> space. Force .csv extension (replace .json).
fn save_twitter_csv(profiles: &[SocialProfile], file_path: &std::path::Path) -> std::io::Result<()>;

/// 中文/en gender normalization to OASIS {male,female,other} (S-371).
///   None/"" -> "other"; 男->male, 女->female, 机构/其他/other->other; male/female passthrough; default other.
fn normalize_gender(gender: Option<&str>) -> &'static str;
```

- **`save_profiles_realtime`** (S-368-adjacent inner closure) is NOT a separate public symbol — it is the `realtime_output: Option<...>` hook inside `generate_profiles_from_entities`: after each profile completes, if `Some`, re-serialize all completed-so-far profiles and overwrite the file (reddit→`to_reddit_format` JSON array; twitter→`to_twitter_format` CSV). Mirror MiroFish's "write the full current set each time" semantics + its `except → log warning, continue` (a realtime-write failure must NOT abort the batch — `tracing::warn!` and proceed).
- **`save_profiles_to_json`** (S-373) → optional thin alias `pub fn save_profiles_to_json(...) { warn!("deprecated…"); save_profiles(...) }` per §2.
- **Error model:** writers return `std::io::Result<()>` (idiomatic; `prepare_simulation` maps to `TeriError`); the realtime-hook swallows-and-logs (matches MiroFish). Per CLAUDE.md prefer `TeriError` variants over `anyhow` at the manager boundary — the export layer surfaces `io::Result` and the caller wraps.

### §4 — U-023 decomposition + sub-cycle ORDER

The export layer (§3) is a **hard dependency of `prepare_simulation` stage 2** — it must land BEFORE the stage that calls it. Recommended order (4 cycles):

1. **Cycle A — EXPORT LAYER (re-opens U-018).** Port S-367/S-369/S-370/S-371/S-372 (+S-373 alias) into `src/services/oasis_profile_export.rs` per §3. Un-`[≠]` those symbol-map rows (flip to `[ ]`→port→`[x]`); S-371's re-flag fires here; S-368 documented as legitimately-staying-`[≠]`. **Differential parity gate:** golden-compare the `reddit_profiles.json` bytes (field set incl. forced defaults, `bio[:150]`, `user_id`), the `twitter_profiles.csv` (header + the `user_char`/`description`/row-index columns), `normalize_gender` over {男,女,机构,其他,male,female,"",None,garbage}, and the batch `Vec` ordering + per-error fallback. This cycle is self-contained and verifiable WITHOUT the manager. **Do this first.**
2. **Cycle B — U-023(a): state types.** `SimulationStatus` + `PlatformType` enums; `SimulationState` struct + `to_dict`/`to_simple_dict` serializers (serde). FS-persistence-shape only; no behavior yet.
3. **Cycle C — U-023(b): manager skeleton + FS persistence + getters.** `SimulationManager` struct; `_get_simulation_dir`/`_save_simulation_state`/`_load_simulation_state`; `create_simulation`; and the getters `get_simulation`/`list_simulations`/**`get_profiles`** (reads the files Cycle A writes)/`get_simulation_config`/`get_run_instructions`. `get_profiles` parity now provable because Cycle A produces the files it reads.
4. **Cycle D — U-023(c): `prepare_simulation` 4-stage async task.** Wire stage 1 (`filter_defined_entities`, already ported S-356/U-016 reader) → stage 2 (`generate_profiles_from_entities` + `save_profiles` for BOTH platforms, from Cycle A) → stage 3 (`generate_config`, U-019) → stage 4 (state READY). Async via `tokio::spawn`; FS-state-machine PENDING→PREPARING→READY/FAILED; `parallel_profile_count>1` → `tokio::JoinSet`/`join` inside `generate_profiles_from_entities` (the parallel variant is THIS caller's concern, exactly as the `[≠]` rationale anticipated — but now it drives a real ported method, not a missing one).

**Order rationale:** Cycle A (export) is a leaf with no manager dependency and is the stage-2 dependency, so it lands first and is independently gated. B before C (types before the struct that holds them). C before D (`prepare_simulation` writes state via C's `_save_simulation_state` and its stage-2 output is read back by C's `get_profiles`). Estimate 4 cycles; Cycle A may split into A1 (writers+normalize_gender) / A2 (batch generator) if the batch async/parallel mapping proves heavy.

### §5 — Blast-radius flag

- **ADDITIVE ONLY. Zero edits to parity-verified types.** `SocialProfile`, `Persona::{to_reddit_format,to_twitter_format,to_dict}`, `PersonaGenerator::generate_social`, `agent::generate_username`, `EntityNode::get_entity_type` are all **CONSUMED, not modified**. The export layer is a new file (`src/services/oasis_profile_export.rs`) + one `pub mod` line in `src/services/mod.rs`. No existing signature changes; no existing caller affected.
- The `to_reddit_format`/`to_twitter_format` serializers are reused **only on the realtime-save path** (where MiroFish uses them). The FINAL `save_reddit_json`/`save_twitter_csv` writers are dedicated (forced-defaults / OASIS column contract) and do NOT route through the serializers — this is faithful to MiroFish (its realtime closure uses `to_*_format`; its `save_*` methods build their own dicts).
- Symbol-map edits required (Cycle A): flip S-367/S-369/S-370/S-371/S-372/S-373 from `[≠]` toward port; annotate each with "un-`[≠]`'d DECISION-10 (contractual file/return output)". Keep S-368 `[≠]` with the updated non-contractual-stdout note. merge-ledger U-018 row stays `extend-Y` (the export layer extends the U-018 surface in teri).

### DECISION-10 — 6-line actionable summary

1. **Files ARE contractual** — `reddit_profiles.json` + `twitter_profiles.csv` are served by the U-026 API, read by `get_profiles`, and named by teri's existing i18n keys; the native SimEngine substrate does NOT remove the file contract (engine input is orthogonal to the export/persistence contract). The data flows through files in teri, matching MiroFish.
2. **Un-`[≠]` and PORT S-367, S-369, S-370, S-371, S-372, S-373**; **keep S-368 `[≠]`** (non-contractual stdout debug-print, progress carried by the already-ported `progress_callback`/`progress.profileGenerated` key).
3. **S-371's re-flag FIRES** (`_normalize_gender` ports because its only caller `_save_reddit_json` is now ported); **`_save_reddit_json` is NOT `to_reddit_format`** — it forces OASIS-mandatory defaults (age=30, normalized gender ALWAYS, mbti=ISTJ, country=中国, bio[:150], user_id-or-rowidx), so port the dedicated writer.
4. **New module `src/services/oasis_profile_export.rs`** (free-fn service): `generate_profiles_from_entities` (async, ordered Vec, per-error fallback, progress_callback, realtime hook), `save_profiles` (dispatch), `save_reddit_json`/`save_twitter_csv` (dedicated writers — twitter header `['user_id','name','username','user_char','description']`, user_id=row-index), `normalize_gender` — all reusing `SocialProfile` + the verified serializers.
5. **Sub-cycle order: A (export layer, re-opens U-018, gated standalone) → B (state types) → C (manager + FS persistence + getters incl. get_profiles) → D (prepare_simulation 4-stage async task, parallel via JoinSet).** Export-layer FIRST because it is stage-2's dependency and is independently verifiable.
6. **Blast radius: additive only** — new file + one `pub mod` line; zero edits to the parity-verified `SocialProfile`/`PersonaGenerator`/serializers; the dedicated writers do NOT route through `to_*_format` (faithful to MiroFish's own realtime-vs-save split). Parity gate: golden-byte-compare both files, `normalize_gender` map, batch ordering + fallback.

---

## DECISION-11 — U-023 sub-cycle (d) `prepare_simulation` (S-675) → COMPLETES U-023

**Trigger:** the convergence point of the service layer (`simulation_manager.py:230-458`). Handoff flagged it
likely-architect. Resolved by SOURCE evidence (the handoff's own predictions were inaccurate — see corrections).

**Source-authoritative corrections to the handoff (the handoff guessed; source is the contract):**
- **NO `task_id` / `tokio::spawn` at this level.** Source `prepare_simulation` is a **synchronous** method that
  runs all stages inline and `return`s `SimulationState`. The `task_manager.create_task` + `threading.Thread` +
  `task_id`-returned-immediately wrapping lives in the **API ROUTE** layer (`api/simulation.py:360-618`) = **U-026**,
  not here. (Carry-forward to U-026 already logged.)
- **NO `force_regenerate` parameter.** Source signature is `(simulation_id, simulation_requirement, document_text,
  defined_entity_types=None, use_llm_for_profiles=True, progress_callback=None, parallel_profile_count=3)`. There is
  **no** stage-skipping / already-completed-files check. Do NOT fabricate one.

**The 5 locked decisions:**
1. **Async mapping (sync→async):** `pub async fn prepare_simulation<L: LlmClient>(&self, simulation_id: &str,
   simulation_requirement: &str, document_text: &str, defined_entity_types: Option<&[String]>,
   use_llm_for_profiles: bool, parallel_profile_count: usize, llm: &L, graph: &KnowledgeGraph,
   generator: &PersonaGenerator, progress_callback: Option<&mut dyn FnMut(PrepareProgress<'_>)>)
   -> Result<SimulationState>`. SimulationManager holds NO llm/graph/persona — caller provides them (teri's
   caller-constructs-clients design; the `&KnowledgeGraph` IS the substrate for `state.graph_id` per DECISION-9).
2. **Concurrency — the LIVE knob (no-downgrade):** port REAL bounded concurrency into
   `oasis_profile_export::generate_profiles_from_entities` via `futures::stream::iter(...).buffer_unordered(
   parallel_count.max(1))` (precedent: `src/sim/mod.rs:502`). This is the faithful map of Python's
   `ThreadPoolExecutor(max_workers=parallel_count)` + `as_completed`: each future returns `(idx, profile, error)`;
   the consumer loop writes to the pre-allocated indexed `Vec` (order-preserving final result), then does
   realtime-save + the monotonic 1-based progress callback per completion — exactly Python's `as_completed` body.
   This **re-opens U-018's `generate_profiles_from_entities`** (`_parallel_count` → live `parallel_count`); it drops
   to `[~]` and re-verifies. Borrowed `&` refs are fine (buffer_unordered polls on the current task, no `'static`/
   spawn). Final Vec + files are deterministic regardless of `parallel_count`; only wall-clock + race-dependent
   intermediate realtime-file/progress ordering differ (non-contractual even in Python). Making the knob live (vs
   the deferred sequential adjudication) satisfies the owner "PORT a feature with observable surface / WHEN IN DOUBT
   PORT IT" rule — `parallel_profile_count` is a real parameter, not dead.
3. **No `force_regenerate`** — every stage always runs (source has no skip path).
4. **FS state machine (faithful):** `load_simulation_state` → `None` ⇒ `Err("模拟不存在: {id}")`. Then:
   PREPARING + save → **stage1** `KnowledgeGraphEntityReader::new(graph).filter_defined_entities(defined_entity_types,
   enrich_with_edges=true)`; set `entities_count`/`entity_types` (= `list(filtered.entity_types)`); **if
   `filtered_count == 0` ⇒ status FAILED + error "没有找到符合条件的实体，请检查图谱是否正确构建" + save + `return
   Ok(state)`** (Python `return state`, NOT raise) → **stage2** realtime path (`reddit_profiles.json` if
   enable_reddit else `twitter_profiles.csv` if enable_twitter) + `generate_profiles_from_entities(...,
   parallel_profile_count, realtime_output, ...)`; set `profiles_count`; then `save_profiles` reddit (if enable_reddit)
   + twitter CSV (if enable_twitter) — both dedicated writers → **stage3** `SimulationConfigGenerator::new(llm)
   .generate_config(simulation_id, project_id, graph_id, simulation_requirement, document_text, &entities,
   enable_twitter, enable_reddit, None)`; write `simulation_config.json` via `sim_params.to_json()`;
   set `config_generated=true` + `config_reasoning = sim_params.generation_reasoning` → status READY + save +
   return Ok(state). **Any error in the body ⇒ status FAILED + `error = e.to_string()` + save + return `Err`**
   (Python `except: … raise`). The 0-entities case is `Ok`, the exception case is `Err` — distinct.
5. **Progress callback — full observable surface (no SSE downgrade):** `PrepareProgress<'a> { stage: &'a str,
   progress: i64, message: String, current: Option<i64>, total: Option<i64> }` — carries every kwarg Python's
   `progress_callback(stage, progress, message, current=, total=, item_name=)` passes (these feed the U-026 SSE
   stream; dropping them would downgrade the route contract). Stage labels verbatim: `"reading"` /
   `"generating_profiles"` / `"generating_config"`. All 10 `progress.*` i18n keys verified present in zh.json.

**Blast radius:** new `prepare_simulation` method in `simulation_manager.rs`; re-touch
`oasis_profile_export::generate_profiles_from_entities` (sequential→buffer_unordered, knob live — U-018 re-verify).
Parity gate: differential vs source on (a) 0-entity FAILED-Ok path, (b) exception FAILED-Err path, (c) stage
ordering + state.json after each stage, (d) `parallel_count` final-output determinism, (e) reddit/twitter file
gating by enable flags. **S-675 `[x]` ⇒ U-023 COMPLETE (all S-636..S-680).**

---

## DECISION-12 — U-007 `zep_paging.py` (S-049..S-055) → map-onto-substrate (subsumed by `KnowledgeGraph` in-process reads)

**Class:** map-onto-substrate. **New production code: NONE** — the observable contract is already satisfied by
U-016's parity-verified reader symbols. U-007 is a verification + `[≠]`-adjudication unit.

**Source (`backend/app/utils/zep_paging.py`, 143 lines):** the entire module is Zep-Cloud **network** pagination —
`fetch_all_nodes`/`fetch_all_edges` page the Zep graph via `client.graph.node/edge.get_by_graph_id` with a UUID
`uuid_cursor`, `page_size=100`, per-page retry on transient network errors (`ConnectionError`/`TimeoutError`/
`OSError`/`InternalServerError`) with exponential backoff (`delay*=2`), capping nodes at `_MAX_NODES=2000`.

**Substrate reality:** teri's graph is in-process petgraph (`KnowledgeGraph`). There is **no network, no cursor, no
pages, no transient I/O**. The consumers MiroFish feeds with `fetch_all_*` are already ported to read the graph
directly: U-016 `KnowledgeGraphEntityReader::get_all_nodes`/`get_all_edges` (`entity_reader.rs:560/585`, returning
the same node/edge-dict shapes), built on `KnowledgeGraph::get_all_entities` (`graph/mod.rs:1046`) /
`get_all_edges` (`graph/mod.rs:830`). U-015 build reads the graph directly too.

**Per-symbol mapping (the gate confirms each):**
- **S-054 `fetch_all_nodes` / S-055 `fetch_all_edges`** → `[x]` map-onto: subsumed by `KnowledgeGraphEntityReader::
  get_all_nodes`/`get_all_edges` (already parity-verified in U-016). Observable contract "return ALL nodes/edges of
  the graph" is met (the full in-memory set, in petgraph insertion order — deterministic, a parity improvement over
  Zep's cursor-order).
- **S-053 `_fetch_page_with_retry`** → `[≠]` inexpressible: retries network/IO transient errors; an in-memory
  `Vec`/`HashMap` read cannot raise `ConnectionError`/`InternalServerError` — nothing to retry. (Consistent with the
  U-016 adjudication of Zep `_call_with_retry` as `[≠]` non-contractual.)
- **S-049 `_DEFAULT_PAGE_SIZE=100` / S-051 `_DEFAULT_MAX_RETRIES=3` / S-052 `_DEFAULT_RETRY_DELAY=2.0`** → `[≠]`
  inexpressible: page-size/retry knobs for a pagination+retry loop that does not exist in-process.
- **S-050 `_MAX_NODES=2000`** → `[≠]` strict-SUPERSET: the cap exists ONLY to bound unbounded Zep network paging
  (a safety limit), with NO in-process analog. teri returns the full in-memory set; applying `.take(2000)` would
  ARTIFICIALLY truncate valid data teri already holds in RAM — removing capability the cap was never meant to remove.
  Returning all nodes is MORE complete (superset), not a downgrade. (This is the rare legitimate `[≠]`-superset under
  the owner rule: not "the destination won't use it" — the destination returns *more*, and the source behavior is a
  network-safety artifact, not a semantic contract. The gate must confirm no downstream consumer asserts `len<=2000`.)

**Gate:** confirm (1) `get_all_nodes`/`get_all_edges` return every graph node/edge (no silent drop), (2) the
node/edge-dict shapes match what MiroFish's `fetch_all_*` feed consumers (already shown in U-016), (3) no teri
consumer depends on the 2000-cap. On PASS: S-054/055 `[x]`, S-049/050/051/052/053 `[≠]` ⇒ **U-007 COMPLETE**,
unblocking U-017/U-021.

---

## DECISION-13 — U-021 `zep_graph_memory_updater.py` (S-493..S-539) → 3 sub-cycles (map-onto-substrate: Zep graph.add → teri KnowledgeGraph)

**Trigger:** newly-unblocked-by-U-007 unit, 554L/47 symbols — too large for one cycle. Decompose:

- **sub-cycle (a) — `AgentActivity` + `to_episode_text` + 12 `_describe_*` (S-493..S-514).** PURE PORT (no substrate
  decision). `AgentActivity` is a distinct loggable record (`action_type: String`, `action_args: serde_json::Map`,
  + platform/agent_id/agent_name/round_num/timestamp) — faithful to MiroFish reading dicts from `actions.jsonl`; do
  NOT couple it to teri's `SocialAction` enum (action_type is a dispatch string). `to_episode_text` dispatches on the
  action_type string to 12 describers producing **byte-exact Chinese NL** ("{agent_name}: {description}"); unknown
  action_type → `_describe_generic` ("执行了{action_type}操作"). `action_args.get("k", "")` → serde_json
  `.get("k").and_then(Value::as_str).unwrap_or("")`. New module `src/services/graph_memory.rs`. Byte-exact differential
  testable per describer (all the if/elif content+author combinations). **THIS CYCLE.**
- **sub-cycle (b) — `ZepGraphMemoryUpdater` (S-515..S-53x).** The batching worker. **NEEDS THE SUBSTRATE DECISION
  (architect when reached):** MiroFish `client.graph.add(graph_id, type="text", data=combined_text)` sends NL text to
  Zep Cloud which runs ITS OWN server-side LLM extraction to add entities/edges. teri's in-process equivalent is the
  **U-015 KnowledgeGraph LLM-extraction pipeline** (`build_with_progress`/extend) — so `_send_batch_activities` →
  combined_text → teri graph extract-and-merge into the existing `KnowledgeGraph`. Threading model (Queue + daemon
  worker + BATCH_SIZE=5 per-platform buffers + SEND_INTERVAL=0.5 + MAX_RETRIES=3) → tokio (mpsc channel + spawned
  task, OR a simpler async accumulator). The retry on `client.graph.add` failure is `[≠]`-adjudicate (in-process
  extraction failure modes differ from Zep-network). `start()`'s thread-spawn is a **U-050 site → use `with_locale`**.
  The BATCH merge-as-one-text (combined_text = "\n".join) IS observable (affects extraction grouping) → port faithfully.
- **sub-cycle (c) — `ZepGraphMemoryManager` (S-53x..S-539).** Class-level registry keyed by simulation_id
  (create_updater/get_updater/stop_updater/stop_all/get_all_stats + `_stop_all_done` idempotency flag). Maps onto a
  teri singleton/struct holding `Mutex<HashMap<String, ZepGraphMemoryUpdater>>`. **Feeds U-049** (`stop_all` is called
  by register_cleanup) — record the carry-forward.

**Owner-rule notes:** the platform display names {twitter:'世界1', reddit:'世界2'} (S-517) are console-display only →
likely `[≠]` non-contractual (verify no consumer reads them). get_stats (S-538) IS observable (served via U-049/API) →
port. DO_NOTHING skip in add_activity IS contractual (filters before queueing) → port.

---

## DECISION-14 — U-021 sub-cycle (b) `ZepGraphMemoryUpdater` (S-515..S-53x) → MAP-ONTO teri in-process graph extract-merge + tokio (refines DECISION-13 §b)

**Lineage:** DECISION-1 (Zep→petgraph), DECISION-13 §b (sub-cycle split). Sub-cycle (a) (`AgentActivity` + `to_episode_text`, S-493..S-514) is DONE & tested in `src/services/graph_memory.rs`. This refines DECISION-13 §b into an implementable, no-downgrade design the porter executes verbatim.

**Verified teri facts (read this cycle, file:line):**
- `KnowledgeGraph::build_with_progress_and_ontology` (`graph/mod.rs:545`) is the 2-pass pipeline: Pass-1 per-chunk entity extraction with merge `if !graph.index.contains_key(&entity.name) { graph.add_entity(...) }` (`:613-618`); Pass-2 per-chunk relation extraction added by name-lookup, skipping relations whose `from`/`to` aren't in the graph (`:626-701`). It builds a **fresh** `KnowledgeGraph::new()` (`:561`).
- `add_entity(&mut self, Entity) -> Result<NodeIndex>` (`:272`) **REJECTS duplicates by name** (`Err` if `self.index.contains_key(&entity.name)`, `:273-278`). So a merge MUST gate on `contains_key` first (never call `add_entity` on a present name) — this is exactly what Pass-1 already does.
- `add_relation(&mut self, from, to, Relation)` (`:286`) appends blindly (no edge-dedup) — matches Pass-2.
- `i18n::with_locale(locale: String, future) -> T` (`i18n/mod.rs:143`, async, `LOCALE.scope`), `get_locale() -> String` (`:174`, defaults "zh"). This is the U-050 site for `start()`'s pre-spawn locale capture.
- Ownership idiom locked by DECISION-11: **caller-constructs-clients**; `&KnowledgeGraph` + `&L: LlmClient` are passed per-call; service structs hold NO llm/graph. The updater must follow this.
- `Relation { kind: RelationKind, weight: f32, valid_at: Option<(u64,Option<u64>)> }` (`:82-93`); `get_all_entities -> Vec<&Entity>` (`:1046`), `get_all_edges -> Vec<(Uuid,Uuid,Relation)>` (`:830`), `entity_count` (`:1050`).

---

### Decision 1 — `graph.add(text)` → ADDITIVE `KnowledgeGraph::extend_from_text` (Option A), reusing the U-015 extraction pipeline

**CHOSEN: Option (A).** Add ONE additive method to `src/graph/mod.rs`:

```rust
/// Merge stats returned by `extend_from_text` (observable surface for the updater's
/// counters + differential parity).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtendStats {
    pub entities_added: usize,    // names not already present → add_entity'd
    pub entities_merged: usize,   // names already present → reused existing node (NOT added)
    pub relations_added: usize,   // edges added in Pass-2
}

/// Extends an EXISTING graph with entities/relations extracted from free NL `text`,
/// running the SAME 2-pass LLM extraction pipeline as `build_with_progress_and_ontology`,
/// but merging into `self` instead of a fresh graph. The in-process analogue of Zep's
/// server-side `graph.add(type="text", data=...)`.
///
/// Merge semantics (no-downgrade, matched as closely to Zep's entity-resolution as the
/// substrate allows):
///   - entity merge key = entity NAME (case-sensitive; same key the build pipeline + add_entity use).
///     present name  → reuse existing node, do NOT mutate it (entities_merged += 1);
///     absent name   → add_entity (entities_added += 1).
///   - relations: Pass-2 over `self`'s post-merge entity set; an edge whose from/to name
///     is not in `self` is skipped (identical to build's :664-672); else add_relation
///     (relations_added += 1, no edge-dedup — matches build).
/// Additive: `build`/`build_with_progress*`/`add_entity` are UNCHANGED; the existing 5 build
/// tests + all graph tests stay green.
pub async fn extend_from_text<L: LlmClient>(
    &mut self,
    text: &str,
    llm: &L,
) -> Result<ExtendStats>;
```

**Why A over B/C (faithful + lowest blast radius):**
- **Faithfulness.** Zep `graph.add(text)` runs server-side LLM entity/relation extraction and MERGES into the existing graph by entity identity. teri's `build_with_progress_and_ontology` ALREADY embodies exactly that extraction + merge-by-name — it merges across chunks into one accumulating graph. Extending `&mut self` instead of a fresh graph is the same operation with a different target. The combined_text grouping (Decision 3) drives the same per-batch extraction Zep would run on the same NL blob.
- **Lowest blast radius.** Option B (build a throwaway subgraph via `build()` then merge its nodes/edges) re-extracts then re-walks the subgraph to re-add — duplicating merge logic AND losing the Pass-2 "relations reference the merged entity set" property (B's Pass-2 only sees the subgraph's entities, not `self`'s, so a relation from a NEW entity to an ALREADY-PRESENT one would be dropped — a silent downgrade). A keeps Pass-2 against `self`'s full post-merge entity set, so cross-batch relations are preserved. B also can't carry `valid_at` cleanly. **Reject B.**
- **Implementation (porter):** refactor the per-chunk extract+merge body of `build_with_progress_and_ontology` (`:580-701`) into a private `async fn extract_and_merge_into(&mut self, chunks, llm, custom_entity_kinds, custom_edge_kinds, progress, stats: &mut ExtendStats)` that operates on `&mut self`. Then `build_with_progress_and_ontology` becomes: `let mut g = KnowledgeGraph::new(); g.ontology_*=...; g.extract_and_merge_into(...).await?; Ok(g)` (its return is byte-identical → 5 build tests untouched, confirmed because the merge gate `if !contains_key` is already what it does). `extend_from_text` = `self.extract_and_merge_into(split_text(text,500,50), llm, &self.ontology_entity_types.clone(), &self.ontology_edge_types.clone(), &mut |_,_|{}, &mut stats).await?`. **Ontology types are read from `self`** (a graph that had `set_ontology` called keeps emitting Custom kinds on extend — faithful to Zep where set_ontology persists for subsequent adds). Chunking params reuse the pipeline constants (CHUNK_SIZE=500/OVERLAP=50) — the combined_text per batch is typically < 500 chars so it's a single chunk, but chunking is kept for parity with the build path and large batches.

**Substrate limit (→ `[≠]`, see Decision 4):** Zep's server-side entity resolution does fuzzy/embedding-based coreference ("AI" ≡ "artificial intelligence"). teri merges by EXACT case-sensitive name only. Where two NL mentions resolve to the same Zep entity but to distinct teri names, teri creates two nodes. This is a **genuine substrate inexpressibility** (teri has no entity-resolution model at merge time), adjudicated `[≠]` — NOT a silent drop (every extracted entity IS added; the limit is only that exact-name is the resolution key). The differential parity (DECISION-1 note) compares the SET of entity *names* and edge structure, not Zep's internal coreference, so this is the correct boundary.

---

### Decision 2 — What the updater HOLDS (ownership/sharing) — follow DECISION-11 caller-constructs idiom

The updater is in-process, so the Zep `graph_id: str` (a by-id remote handle) becomes an **explicit owned graph handle**. The extraction needs an `LlmClient`. Because `extend_from_text` takes `&mut self` on the graph AND the background flush task needs to outlive individual calls, the updater holds **shared, mutable** handles:

```rust
pub struct GraphMemoryUpdater {
    graph: Arc<Mutex<KnowledgeGraph>>,        // the target graph (was Zep graph_id) — tokio::sync::Mutex
    llm:   Arc<dyn LlmClient>,                 // extraction client (was Zep server-side pipeline)
    graph_label: String,                       // for log lines (was graph_id; the "graph_{...}" string)
    // ... buffers / stats / channel (Decision 3) ...
}
```

- **`Arc<tokio::sync::Mutex<KnowledgeGraph>>`** (NOT `std::sync::Mutex`): the flush calls `extend_from_text(&mut *guard, ...).await` which holds the lock ACROSS an `.await` (LLM round-trip) — requires an async-aware mutex. This is acceptable here because flushes are serialized per-updater anyway (one worker task) and the sim writers (U-022) only `add_activity` (which touches buffers, not the graph). The graph lock is contended only between this updater's flush and any external graph reader; the sim itself does not read the graph during ticks.
- **`Arc<dyn LlmClient>`**: matches teri's existing `Arc<dyn LlmClient>` usage; clonable into the spawned task. (`LlmClient: Send + Sync` per `llm.rs`.)
- **Composability with U-022 / `prepare_simulation`:** the U-022 social-sim constructs the `KnowledgeGraph` (or receives `&KnowledgeGraph` per DECISION-11) and the `Arc<dyn LlmClient>`, wraps the graph in `Arc<Mutex<_>>`, and constructs the updater with clones. The updater is then handed `AgentActivity` records as the sim emits actions. This keeps service structs free of llm/graph fields at construction-of-manager time (DECISION-11) — only the updater, which IS the write-back binding, holds them, and only for its lifetime. The `graph_id`→graph resolution Zep does by-id is replaced by passing the actual `Arc<Mutex<KnowledgeGraph>>` at `GraphMemoryUpdater::new`.
- **Constructor:** `GraphMemoryUpdater::new(graph: Arc<Mutex<KnowledgeGraph>>, llm: Arc<dyn LlmClient>, graph_label: String) -> Self`. No `api_key`, no `ZEP_API_KEY` check (S-516 `__init__` ValueError on missing key is **dropped — substrate-absent**, native graph needs no key; `[≠]`, Decision 4).

---

### Decision 3 — Concurrency model: tokio `mpsc` + ONE spawned worker task (faithful async background drain)

**CHOSEN:** true async-spawned worker (mpsc channel + spawned task), NOT a synchronous flush-on-threshold. Justification: the observable contract includes `running` (S-538 get_stats), `queue_size`, and the start()/stop() lifecycle with `_flush_remaining` on stop — these only make sense with a real background drain. More decisively, U-022's sim loop calls `add_activity` on the hot tick path; making it block on an LLM extraction round-trip at every 5th activity would change sim timing and couple the sim to extraction latency (a behavior change). Python deliberately decouples via Queue + daemon thread; tokio mpsc + spawned task is the faithful map. The final graph state + stats are identical either way, but the decoupling IS part of the contract (the daemon thread exists precisely so the producer never blocks on the network/LLM).

**Structure (maps each Python member):**
```
Python                                  teri
------------------------------------    --------------------------------------------------
Queue _activity_queue                   tokio::sync::mpsc::UnboundedSender<AgentActivity> (producer side)
threading.Thread _worker_thread         tokio::task::JoinHandle<()> (the worker)
_platform_buffers: Dict[str,List]       HashMap<String, Vec<AgentActivity>>   (OWNED BY THE WORKER TASK —
                                          no _buffer_lock needed; single owner. seeds {"twitter":[],"reddit":[]})
_buffer_lock                            (eliminated — buffers live solely in the worker; idiom: shared-lock→ownership)
_running                               AtomicBool in Arc (read by get_stats; set false on stop)
BATCH_SIZE = 5                          const BATCH_SIZE: usize = 5
SEND_INTERVAL = 0.5                     [≠] dropped (Decision 4)
MAX_RETRIES / RETRY_DELAY               [≠]→ but extraction-error handling kept (Decision 4)
stats (5 counters)                      Arc<Mutex<UpdaterStats>> OR AtomicU64 set (shared; read by get_stats)
```

**Worker loop (faithful to `_worker_loop` :364-394 + flush-on-threshold :381-387):**
- Wrapped in `i18n::with_locale(captured_locale, async move { ... }).await` so the whole worker runs under the locale captured at `start()` (U-050; mirrors `_worker_loop(locale)` + `set_locale(locale)` :366). `start()` captures `get_locale()` BEFORE spawn (mirrors :281).
- Loop: `while let Some(activity) = rx.recv().await { push to buffers[platform.to_lowercase()] (insert empty Vec if new platform — :376-378); if buffers[platform].len() >= BATCH_SIZE { drain first BATCH_SIZE into a batch (:382-383), call flush_batch(batch, platform).await } }`. When the sender drops (stop), `recv()` returns `None` → exit loop → run `_flush_remaining` equivalent. **No SEND_INTERVAL sleep** between flushes (Decision 4).
- `flush_batch(activities, platform)`: `combined_text = activities.iter().map(to_episode_text).collect::<Vec<_>>().join("\n")` (S-?? `_send_batch_activities` :407-409 — combined_text is OBSERVABLE, drives extraction grouping → exact). Then `{ let mut g = self.graph.lock().await; g.extend_from_text(&combined_text, &*self.llm).await }`. On Ok → `total_sent += 1; total_items_sent += activities.len()` + info log w/ display_name (Decision 4 on display_name). On Err → `failed_count += 1` + error log (Decision 4 on retry).

**`add_activity` (S-?? :310-338) — runs on the PRODUCER side (sim thread), faithful:**
- `if activity.action_type == "DO_NOTHING" { skipped_count += 1; return }` — **contractual filter BEFORE queueing** (verified DECISION-13 note). Then `total_activities += 1; self.tx.send(activity)` (ignore send error if worker gone). Counter increments happen on the producer side exactly as Python (`_total_activities` incremented in `add_activity`, not the worker).
- `add_activity_from_dict(data: &serde_json::Value /*or Map*/, platform: &str)` (:340-362): **`if data.get("event_type").is_some() { return }`** (skip event-type entries — contractual, :349-350); else construct `AgentActivity` from the dict fields (`agent_id`/`agent_name`/`action_type`/`action_args`/`round`→round_num/`timestamp` with `datetime.now().isoformat()` default → use chrono RFC3339 now) and call `add_activity`. NOTE: the `round` key maps to `round_num` (Python `data.get("round", 0)`).

**`start()` / `stop()` lifecycle:**
- `start(&mut self)`: if already running, return (`:277-278`). Capture `let locale = get_locale();` (:281). Set `running=true`. Build the mpsc channel, move buffers + stats + graph + llm into the worker, `spawn(with_locale(locale, worker_future))`, store the `JoinHandle` + keep the `tx`.
- `stop(&mut self) -> ` (async): set `running=false`; **drop the `tx`** (signals the worker `recv()` returns None → it runs the final flush of sub-BATCH_SIZE leftovers = `_flush_remaining` :435-458, sending each non-empty per-platform buffer even if < BATCH_SIZE); then `join_handle.await` (Python `join(timeout=10)` :301 — the timeout is a thread-join guard; in tokio `.await` the handle; a `tokio::time::timeout(10s, handle)` MAY be used to mirror the bound, but the 10s is a safety cap not a contract — note it, don't over-engineer). Final info log w/ all counters (:303-308). **`_flush_remaining` semantics:** since buffers live in the worker, the "drain queue into buffers then flush each buffer" two-step (:437-458) collapses to: worker drains remaining `rx` messages into buffers (the `while recv` already does this until channel empty+closed), then flushes every non-empty per-platform buffer once. The observable result (all leftover activities sent in per-platform sub-batches) is identical.

**`get_stats()` (S-538 — OBSERVABLE, served via API/U-049 → PORT):** returns a serde struct/Map with `graph_id`(→graph_label), `batch_size`, `total_activities`, `batches_sent`(=total_sent), `items_sent`(=total_items_sent), `failed_count`, `skipped_count`, `queue_size`, `buffer_sizes`(per-platform map), `running`. `queue_size` = mpsc has no public len(); track an `AtomicUsize queued` (incr on send, decr when worker pops) OR document queue_size as best-effort. `buffer_sizes`: the worker owns buffers, so expose via a shared snapshot (worker writes a `Arc<Mutex<HashMap<String,usize>>>` after each buffer mutation) OR fold buffer_sizes into the shared stats updated by the worker. Keep the JSON key names byte-identical to Python (`batches_sent`, `items_sent`, etc.) for API contract parity.

---

### Decision 4 — `[≠]` adjudications (each under the owner's sharpened bar: legal only if substrate-inexpressible / non-contractual-unobservable / strict-superset)

| Item (source) | Verdict | Justification |
|---|---|---|
| **`SEND_INTERVAL=0.5s` sleep** between flushes (:387) | **`[≠]` non-contractual** | A client-side rate-limit for a network SaaS API. In-process extraction has no remote rate to throttle; the sleep produces NO observable output (only wall-clock pacing). Omitting it changes nothing in the final graph or stats. LEGAL `[≠]`. Ledger: `- [≠] U-021 SEND_INTERVAL — Zep network rate-limit, non-contractual in-process`. |
| **`MAX_RETRIES=3` / `RETRY_DELAY` retry loop** on `graph.add` (:412-433) | **SPLIT: retry `[≠]`, error-handling PORTED** | The retry *mechanism* (3 attempts + backoff) targets transient Zep NETWORK failures — substrate-absent in-process → `[≠]`. BUT the *observable outcome* — on final failure, increment `failed_count` and do NOT crash the worker (Python catches & continues :392-394, 433) — IS contractual (failed_count is in get_stats) and IS PORTED: `flush_batch` Err → `failed_count += 1` + error log + worker continues. An LLM extraction call CAN fail (timeout/parse), so the *resilience* is real; only the literal 3×-retry-with-2s-backoff is the network-shaped part dropped. Ledger: `- [≠] U-021 MAX_RETRIES/RETRY_DELAY literal retry-loop — Zep-network transient-retry; in-process keeps the failed_count + continue-on-error contract, drops the network retry cadence`. (If the verifier deems extraction-retry contractual, a single retry is a cheap re-port — flagged.) |
| **`PLATFORM_DISPLAY_NAMES {twitter:'世界1', reddit:'世界2'}`** (S-517) + `_get_platform_display_name` (:271-273) | **PORT (do NOT `[≠]`)** | Used ONLY inside `logger.info(...)` lines (:422-423, :453-454) — console/log output. Per DECISION-13 note "verify no consumer reads them": confirmed only consumed by log strings, no return value / API / file. Log content is a thin observable surface but the owner rule says "do not `[≠]`-skip anything with an observable output" and a log line IS output. **PORT it** as a `fn platform_display_name(p) -> &str` mapping twitter→"世界1"/reddit→"世界2"/else→p, used in the same log lines, to stay safely inside the bar. Cheap, removes any disguised-skip risk. |
| **`__init__` `ZEP_API_KEY` ValueError** (S-516, :243-244) | **`[≠]` substrate-absent** | Native graph requires no API key; the guard guards a credential that no longer exists. Genuinely inexpressible (nothing to validate). LEGAL `[≠]`. Ledger: `- [≠] U-021 ZEP_API_KEY check — native graph is keyless; substrate-absent`. |
| **Zep server-side fuzzy entity-resolution / coreference** (implicit in `graph.add`) | **`[≠]` substrate-inexpressible** | Decision 1 limit: teri merges by exact case-sensitive name; Zep does embedding/LLM coreference. teri has no entity-resolution model at merge time → genuinely inexpressible. NO entity is dropped (all extracted entities are added); only the *resolution key* differs. Differential parity compares name-set + edge structure, not Zep coreference. LEGAL `[≠]`, verifier-adjudicated. Ledger: `- [≠] U-021 Zep coreference entity-resolution — teri merges by exact name; no resolution model. No entity dropped.` |

**Nothing with a return value, file, API field, or distinct NL output is `[≠]`'d.** combined_text join, DO_NOTHING skip, event_type skip, all 5 stat counters, get_stats, _flush_remaining leftovers, platform display names → ALL PORTED.

---

### Decision 5 — Symbol coverage + placement; split assessment

**Placement:** all of sub-cycle (b) lands in `src/services/graph_memory.rs` (alongside the DONE `AgentActivity`). The ONE additive graph method (`extend_from_text` + `ExtendStats` + private `extract_and_merge_into` refactor) lands in `src/graph/mod.rs` (additive; re-touches the build pipeline by extraction only — 5 build tests must stay green; this is a `[~]` re-open of U-015's `build_with_progress_and_ontology` body, re-verified).

**Symbol map (S-515..S-53x, `unit:U-021`):**
| Source symbol (zep_graph_memory_updater.py) | Lines | teri target |
|---|---|---|
| S-515 `ZepGraphMemoryUpdater` class | 202 | `GraphMemoryUpdater` struct (graph_memory.rs) |
| S-516 `__init__` | 232-269 | `GraphMemoryUpdater::new(graph, llm, graph_label)` — ZEP_API_KEY check `[≠]` |
| S-517 `PLATFORM_DISPLAY_NAMES` + `_get_platform_display_name` | 220-223,271-273 | `fn platform_display_name(&str)->&str` (PORTED) |
| S-518 `start` | 275-291 | `start(&mut self)` — `get_locale()` capture + `spawn(with_locale(...))` (U-050) |
| S-519 `stop` | 293-308 | `stop(&mut self)` async — drop tx + join + final-flush + log |
| S-520 `add_activity` | 310-338 | `add_activity(&self, AgentActivity)` — DO_NOTHING skip + send (producer side) |
| S-521 `add_activity_from_dict` | 340-362 | `add_activity_from_dict(&self, &Value, &str)` — event_type skip |
| S-522 `_worker_loop` | 364-394 | the spawned worker future (recv → buffer → threshold flush) |
| S-523 `_send_batch_activities` | 396-433 | `flush_batch(&self, Vec<AgentActivity>, &str)` — combined_text join + `extend_from_text` + counters; retry `[≠]` |
| S-524 `_flush_remaining` | 435-458 | worker drain-on-close + flush each non-empty buffer |
| S-525 `get_stats` | 460-476 | `get_stats(&self) -> UpdaterStats` (serde; byte-identical JSON keys) |
| (graph-side, U-015 re-open) | — | `KnowledgeGraph::extend_from_text` + `ExtendStats` + private `extract_and_merge_into` (graph/mod.rs) |

(Exact S-numbers within 515..53x to be reconciled against the symbol-map rows by the porter; the class spans 5 methods + 2 helpers + the dunder init, all mapped above.)

**Split assessment:** sub-cycle (b) is **borderline but does NOT need b1/b2** if the `extend_from_text` graph refactor is treated as a tight, pre-landed `[~]` re-open of U-015 (it's a mechanical extract-method refactor + one new public method, ~40 LOC, fully covered by reusing the existing pipeline). The updater itself is ~5 methods of straightforward tokio plumbing. **Recommended order within the cycle:** (b-i) land `extend_from_text` in graph/mod.rs first, re-verify the 5 build tests + add an extend-merge unit test (build a graph, extend with text mentioning a present + a new entity → assert entities_merged/added + cross-batch relation kept); THEN (b-ii) the `GraphMemoryUpdater` tokio worker. If the porter finds the graph refactor contentious (e.g. the `extract_and_merge_into` signature fights the borrow checker on `self.ontology_*.clone()`), THEN split b1=graph method / b2=updater — but the default is one cycle.

**Parity (differential) for (b):** feed a fixed sequence of `AgentActivity` records (incl. one DO_NOTHING, one event_type dict, ≥5 same-platform to trigger a batch, leftovers < 5) through (a) a recorded MiroFish golden (the combined_text blobs MiroFish would have sent to Zep, captured from fixtures) and (b) teri: assert (1) the combined_text per batch is byte-identical (join "\n", per-platform grouping at 5), (2) DO_NOTHING/event_type are skipped (skipped_count + absent from any batch), (3) final stats counters match (total_activities/total_sent/total_items_sent/skipped_count; failed_count=0 on happy path), (4) leftovers flushed on stop, (5) the resulting graph's entity-NAME set ⊇ the entities extractable from the combined_text (extraction is LLM-stochastic → compare name-set membership + that no batch was dropped, not exact entity counts — same boundary as DECISION-1's parity note). The Zep-coreference `[≠]` is the only adjudicated divergence.

### DECISION-14 — 6-line actionable summary
1. `graph.add(text)` → ADD `KnowledgeGraph::extend_from_text(&mut self, text, &L) -> Result<ExtendStats>` (Option A): refactor the U-015 2-pass body into `extract_and_merge_into(&mut self,...)`, merge by EXACT entity NAME (`if !index.contains_key → add_entity`, else reuse), Pass-2 relations against self's full set; additive, 5 build tests stay green.
2. Updater holds `Arc<tokio::sync::Mutex<KnowledgeGraph>>` + `Arc<dyn LlmClient>` + `graph_label` (caller-constructs per DECISION-11; graph_id-by-id → explicit handle); no API key.
3. Concurrency: tokio `mpsc::UnboundedSender` + ONE spawned worker task that OWNS the per-platform buffers (no lock), flushes at BATCH_SIZE=5, `combined_text = episode_texts.join("\n")` (observable, exact), `extend_from_text` under the graph lock; producer-side `add_activity` does the DO_NOTHING skip + counter; `stop` drops tx → worker final-flushes leftovers.
4. U-050: `start()` captures `get_locale()` then `spawn(with_locale(locale, worker))`.
5. `[≠]`: SEND_INTERVAL (network rate-limit), literal MAX_RETRIES retry-loop (network-transient; failed_count+continue-on-error IS ported), ZEP_API_KEY check (keyless substrate), Zep coreference entity-resolution (no resolution model; no entity dropped). PORTED (not `[≠]`): platform display names (log output), get_stats, all 5 counters, event_type skip.
6. All in `src/services/graph_memory.rs` (+ graph method in `src/graph/mod.rs`); ONE cycle (split b1/b2 only if the graph refactor fights the borrow checker).

---

## DECISION-15 — U-020 `simulation_ipc.py` (S-453..S-492) → 2 sub-cycles (map-onto-substrate: subprocess-file-IPC → in-process)

**Substrate context (LOCKED):** OASIS Python subprocess sim → teri native in-process `SimEngine` (`src/sim`). MiroFish's
IPC is file-based ONLY because it bridges two OS PROCESSES (Flask writes command JSON to `ipc_commands/`, the sim
subprocess polls + replies in `ipc_responses/`, liveness via `env_status.json`). teri has NO second process → the file
*transport* is a subprocess artifact with no in-process analog (same class as Zep-network→petgraph U-007/U-016).
But the **command/response PROTOCOL** (CommandType, the interview/batch_interview/close_env arg shapes, IPCResponse
status/result/error + JSON round-trip) IS the observable contract U-022/U-047 consume → PORTED.

- **sub-cycle (a) — protocol types (S-453..S-476).** PURE PORT, transport-agnostic, NO substrate decision.
  `CommandType` (str enum: interview/batch_interview/close_env), `CommandStatus` (pending/processing/completed/failed),
  `IPCCommand {command_id, command_type, args: Map, timestamp}` + `to_dict`(4-key)/`from_dict`(.get tolerance),
  `IPCResponse {command_id, status, result: Option<Map>, error: Option<str>, timestamp}` + `to_dict`(5-key:
  command_id/status/result/error/timestamp, result&error → JSON null when None, ensure_ascii=False)/`from_dict`.
  timestamp default = `python_isoformat_local()`. serde with preserve_order for key order. New module
  `src/services/simulation_ipc.rs`. Byte-exact differential testable (to_dict/from_dict round-trip, enum .value strings,
  null result/error). **THIS CYCLE.**
- **sub-cycle (b) — `SimulationIPCClient` + `SimulationIPCServer` (S-477..S-492).** **NEEDS THE SUBSTRATE DECISION
  (architect when reached):** map the file-based request/response onto IN-PROCESS delivery. Client `send_command`
  (write cmd file → poll response file w/ timeout/poll_interval → cleanup) → in-process send + await-with-timeout
  (tokio `mpsc` + `oneshot` reply, or a handler trait the SimEngine implements). Server `poll_commands` (scan dir by
  mtime) / `send_response`/`send_success`/`send_error` → the SimEngine-side command receive+reply loop. `start`/`stop`/
  `_update_env_status`(env_status.json) / `check_env_alive` → in-process liveness (a shared running flag). The
  PROTOCOL methods (send_interview timeout=60 / send_batch_interview timeout=120 / send_close_env timeout=30 — the
  arg dicts) are CONTRACTUAL → ported; the file paths (ipc_commands/ipc_responses/env_status.json), os.remove cleanup,
  mtime-ordered polling, JSONDecodeError-retry are subprocess-transport `[≠]` (no second process). Wires into U-022
  (the sim loop driving interviews) — compose the channel with how SimEngine runs as a tokio task.

**Owner-rule note:** the file-transport `[≠]` rests on the LOCKED OASIS-subprocess→in-process-SimEngine substrate
decision (genuine inexpressibility — no second process to bridge), NOT on "the destination won't use it". The
observable interview/close protocol is fully ported. Sub-cycle (b) verifier must confirm no protocol behavior is lost.

---

## DECISION-16 — U-020 sub-cycle (b): `SimulationIPCClient` + `SimulationIPCServer` (S-477..S-492) → in-process async transport (refines DECISION-15 §b)

**Lineage:** DECISION-15 §b (sub-cycle split; substrate LOCKED), DECISION-14 (the async-worker mpsc+spawned-task
precedent for `ZepGraphMemoryUpdater`). Protocol types (S-453..S-476) are DONE + parity-verified in
`src/services/simulation_ipc.rs` — **REUSE verbatim** (`IPCCommand`/`IPCResponse`/`CommandType`/`CommandStatus`).
Sub-cycle (b) adds the **transport** (client + server) in the **same file**, below the protocol types.

### 16.0 The transport map-onto decision — **CHOSEN: (A) `mpsc<Envelope>` + per-command embedded `oneshot` reply.**

Rejected: **(B) shared `Mutex<HashMap<cmd_id, slot>>` + `Notify`** — reimplements id-correlation, liveness scanning,
and wakeup that the channel pair gives for free; it is a transliteration of the file-map, not idiomatic. **(C) keep
the file impl verbatim** — would manufacture a filesystem round-trip between two halves of ONE process: pure overhead,
race-prone (the very `JSONDecodeError`-on-partial-write the source defends against), and not faithful (the file
transport exists ONLY to cross the OS-process boundary that teri does not have).

**(A) is the faithful + idiomatic map-onto.** The OBSERVABLE protocol contract is: *a command of a given `command_type`
+ `args`, submitted with a `timeout`, yields exactly one `IPCResponse` (matching `command_id`, with `status`/`result`/
`error`), OR a timeout error.* That request-response-with-timeout shape is **exactly** `tokio::sync::oneshot` for the
reply embedded in an `mpsc`-delivered command envelope:

- **client `send_command`** = build `IPCCommand` (with a fresh `command_id`) → create a `oneshot::channel()` → send
  `Envelope { command: IPCCommand, reply: oneshot::Sender<IPCResponse> }` on the mpsc tx → `tokio::time::timeout(timeout,
  reply_rx).await`. Elapsed → timeout error; `Ok(Ok(resp))` → the `IPCResponse`.
- **server `poll_commands`** = `try_recv()` (non-blocking, for the between-ticks sim loop) from the mpsc rx, returning
  `Option<Envelope>` (the loop holds the live `reply` sender to fire later).
- **server `send_response`/`send_success`/`send_error`** = fire the `oneshot` reply for that envelope.

This preserves every observable: the command type+args reach the server, the response status/result/error return to the
caller, and the timeout is a **real elapsed-time await** (`tokio::time::timeout` over the same default seconds:
interview 60, batch 120, close 30). **Genuine inexpressibility confirmed:** the file paths exist solely to bridge two OS
processes via the filesystem; teri runs the sim in-process, so there is no filesystem boundary to bridge — the transport
files are not "unused", they are **structurally absent** (same class as Zep-network→petgraph). This is the owner-rule
"genuinely inexpressible given the locked substrate" case, NOT a "won't use it" skip.

### 16.1 Handle / ownership split (client holds tx, server holds rx)

```rust
/// The command envelope crossing the in-process channel. Carries the protocol
/// IPCCommand PLUS the oneshot reply sink (replaces the {cmd_id}.json response file).
pub struct IpcEnvelope {
    pub command: IPCCommand,
    reply: tokio::sync::oneshot::Sender<IPCResponse>,
}

pub struct SimulationIPCClient {
    tx: tokio::sync::mpsc::Sender<IpcEnvelope>,     // [≠] replaces commands_dir
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,  // [≠] replaces env_status.json read
}

pub struct SimulationIPCServer {
    rx: tokio::sync::mpsc::Receiver<IpcEnvelope>,   // [≠] replaces commands_dir scan
    running: std::sync::Arc<std::sync::atomic::AtomicBool>, // [≠] replaces env_status.json write + _running
}

/// Factory — the in-process analog of "both halves point at the same simulation_dir".
/// Replaces `__init__(simulation_dir)` on BOTH sides: the shared directory pair becomes
/// the shared channel + shared AtomicBool. Returns the paired client+server.
pub fn channel(buffer: usize) -> (SimulationIPCClient, SimulationIPCServer) { … }
```

**Ownership:** client owns the **mpsc `Sender`** (clonable → multiple Flask-route-equivalent callers can submit; matches
MiroFish where many requests write into one `commands_dir`); server owns the **mpsc `Receiver`** (single consumer = the
sim loop, matching the single sim subprocess). The `oneshot::Sender` is created per-command by the client and **moved
into the envelope** to the server — so reply correlation is automatic (no id-matching needed; see §16.4). Liveness is a
shared `Arc<AtomicBool>` written by the server (`start`→true, `stop`→false) and read by the client (`check_env_alive`),
replacing `env_status.json`. Buffer size: a bounded channel (e.g. 64, matching the SimEngine `broadcast` buffer) for
backpressure parity; `send` is `.await`ed.

### 16.2 Standalone testability + U-022 wiring (does NOT require U-022 to land/test)

The server is a **self-contained handle**: it owns the rx and exposes `poll_commands(&mut self) -> Option<IpcEnvelope>`
(`try_recv`) plus the `start`/`stop`/`send_*` methods. **It does not reference `SimEngine` at all.** Therefore (b) lands
+ tests NOW against a **mock loop**: a test spawns the server in a `tokio::task` that `start()`s, loops calling
`poll_commands()`, and replies `send_success`/`send_error`; the client (on the test task) calls `send_interview` /
`send_batch_interview` / `send_close_env` and asserts the returned `IPCResponse` (and a timeout when the mock declines to
reply). No second process, no SimEngine, no filesystem.

**U-022 wiring (later, no API change needed):** `SimEngine::run` (`src/sim/mod.rs:493`) already runs as an async task
with `broadcast`/`watch` channels. U-022 holds the `SimulationIPCServer` and, **between rounds** (after the Phase-2
commit, alongside the existing `inject_fn` hook at `mod.rs:542`), drains commands:
```text
while let Some(env) = ipc_server.poll_commands() {
    match env.command.command_type {
        Interview      => { run interview over agents; env reply send_success(result) }
        BatchInterview => { … }
        CloseEnv       => { env reply send_success({}); break the tick loop }
    }
}
```
`poll_commands` returning `Option` (not blocking) is exactly why the loop can interleave IPC servicing with ticking —
the source's `poll_commands` was likewise a single non-blocking dir scan called inside the sim's own loop. U-022 owns the
*command-handler* logic (what an interview DOES); (b) owns only the *transport + dispatch surface*.

### 16.3 Per-symbol mapping (S-477..S-492)

**Client (S-477..S-483):**
| S | Python | teri |
|---|--------|------|
| S-477 | `class SimulationIPCClient` | `pub struct SimulationIPCClient { tx, alive }` |
| S-478 | `__init__(simulation_dir)` (mk `commands_dir`/`responses_dir`) | via `channel()` factory; dirs → channel handle + `alive` `[≠]` |
| S-479 | `send_command(command_type, args, timeout=60.0, poll_interval=0.5)` | `async fn send_command(&self, command_type, args, timeout: Duration) -> Result<IPCResponse>`; build `IPCCommand`+fresh `command_id`, oneshot, `tx.send(env).await`, `tokio::time::timeout(timeout, reply_rx)`. `poll_interval` `[≠]` (irrelevant to a channel). Elapsed → `Err(TeriError::Sim("…timeout… (N s)"))` (mirrors `TimeoutError`; same `Result` error path callers already handle) |
| S-480 | `send_interview(agent_id, prompt, platform=None, timeout=60.0)` | `async fn send_interview(&self, agent_id: i64, prompt: &str, platform: Option<&str>, timeout: Duration) -> Result<IPCResponse>`; args map `{agent_id, prompt, [platform]}` — platform key inserted **only when Some** (matches `if platform:`); `send_command(Interview, …, 60s)` |
| S-481 | `send_batch_interview(interviews, platform=None, timeout=120.0)` | `async fn send_batch_interview(&self, interviews: Vec<Value>/Map, platform: Option<&str>, timeout)`; args `{interviews, [platform]}`; `send_command(BatchInterview, …, 120s)` |
| S-482 | `send_close_env(timeout=30.0)` | `async fn send_close_env(&self, timeout) -> Result<IPCResponse>`; args `{}`; `send_command(CloseEnv, …, 30s)` |
| S-483 | `check_env_alive() -> bool` (read `env_status.json`, `status=="alive"`) | `fn check_env_alive(&self) -> bool` = `self.alive.load(Ordering::SeqCst)`; the `env_status.json` file → in-process `AtomicBool` `[≠]` |

Default-timeout fidelity: keep the per-method defaults (60/120/30 s). Recommended ergonomic form: `timeout: Duration`
with the source defaults documented; a thin `_default` wrapper or `Option<Duration>` (None → default) may be used so the
call sites stay terse — verifier checks the *effective* default seconds, not the signature shape.

**Server (S-484..S-492):**
| S | Python | teri |
|---|--------|------|
| S-484 | `class SimulationIPCServer` | `pub struct SimulationIPCServer { rx, running }` |
| S-485 | `__init__(simulation_dir)` (mk dirs; `_running=False`) | via `channel()` factory; `running` starts `false` |
| S-486 | `start()` (`_running=True`; `_update_env_status("alive")`) | `fn start(&self)` = `running.store(true)`; the `"alive"` status → the bool `[≠]` |
| S-487 | `stop()` (`_running=False`; `_update_env_status("stopped")`) | `fn stop(&self)` = `running.store(false)` |
| S-488 | `_update_env_status(status)` (write `env_status.json` w/ timestamp) | **`[≠]`** — collapses into the `AtomicBool` store; no file, no timestamp (file artifact) |
| S-489 | `poll_commands() -> Optional[IPCCommand]` (mtime-sorted dir scan, oldest first) | `fn poll_commands(&mut self) -> Option<IpcEnvelope>` = `rx.try_recv().ok()`. **FIFO preserves "oldest first"** — mpsc delivers in send order, the same ordering the mtime-sort imposed. mtime scan/`JSONDecodeError`-retry `[≠]` (file-race artifacts). NOTE: returns `IpcEnvelope` (command + reply sink), not bare `IPCCommand`, because the reply target travels with the command (file impl re-derived it from `command_id`+`responses_dir`) |
| S-490 | `send_response(response)` (write `{cmd_id}.json`; rm command file) | replies via the envelope's `oneshot`. Either a free `send_response(env: IpcEnvelope, resp: IPCResponse)` or methods on a held envelope; `env.reply.send(resp)`. os.remove cleanup `[≠]` (the oneshot consumes itself) |
| S-491 | `send_success(command_id, result)` | `fn send_success(env: IpcEnvelope, result: Map)` → fires `IPCResponse{command_id: env.command.command_id, status: Completed, result: Some(result), error: None}`. **command_id preserved** for protocol/log fidelity (§16.4) |
| S-492 | `send_error(command_id, error)` | `fn send_error(env: IpcEnvelope, error: String)` → `IPCResponse{…, status: Failed, error: Some(error), result: None}` |

### 16.4 command_id — stays meaningful

With the embedded oneshot, reply correlation is **automatic** (the reply sink is bound to the request; no
id-→-file matching). But `command_id` **stays in `IPCCommand`/`IPCResponse`** (it already does, S-463/S-470, parity-
verified): it is part of the serialized protocol contract, it is logged on both send + receive in the source
(`logger.info(… command_id=…)`), and `send_success`/`send_error` echo it back into the response so the response is
self-describing. So: correlation no longer *needs* it, but the protocol *keeps* it — porter populates
`response.command_id = envelope.command.command_id` and preserves the send/receive log lines.

### 16.5 `[≠]` list (each justified under the owner's sharpened rule — all rest on the LOCKED in-process substrate, none on "won't use it")

| `[≠]` artifact | Source role | Why genuinely inexpressible in-process (not a feature skip) |
|----------------|-------------|------------------------------------------------------------|
| `ipc_commands/` + `ipc_responses/` dirs (+ `os.makedirs`) | filesystem channel between 2 processes | one process → no FS boundary to bridge; replaced by the mpsc+oneshot (same delivery, no observable change) |
| `env_status.json` file + `_update_env_status` timestamp | cross-process liveness signal | liveness is now a shared in-memory `AtomicBool`; the file is purely the cross-process delivery of the same boolean |
| `os.remove` cleanup (command/response files) | reclaim files after delivery | nothing to clean up — mpsc consumes the envelope, oneshot consumes itself |
| mtime-ordered directory scan | impose oldest-first over an unordered FS dir | mpsc is already FIFO → oldest-first **preserved**, the *mechanism* (mtime sort) is moot |
| `poll_interval` (0.5 s) | how often to re-scan the FS for a reply | a channel wakes the awaiter immediately; there is nothing to poll. The OBSERVABLE (`timeout`) is preserved as a real await |
| `JSONDecodeError`-retry-on-partial-file | defend against reading a half-written file mid-write | a file-write-race artifact; in-process values are moved whole, never partially observable |

**PORTED (the contract — NOT `[≠]`):** command types + arg shapes (interview/batch_interview/close_env, the
`{agent_id, prompt, platform?}` / `{interviews, platform?}` / `{}` dicts incl. the conditional-platform-key behavior);
the timeouts as REAL elapsed awaits with the source defaults (60/120/30); `IPCResponse` status/result/error
construction; the **FIFO oldest-first** delivery ordering; `command_id` round-trip + the send/receive log lines;
`check_env_alive` semantics; success/error response construction.

### 16.6 Coverage + cycle count

**16/16 symbols mapped** (S-477..S-492; client 7, server 9). All in **one cycle** — this is a single cohesive file
addition (~one struct pair + a factory + ~9 methods, all REUSING the verified protocol types), standalone-testable with
a mock loop, no dependency on U-022. **No split needed.**

### DECISION-16 — actionable summary for the porter
1. Add `SimulationIPCClient`, `SimulationIPCServer`, `IpcEnvelope`, and `pub fn channel(buffer)` to
   `src/services/simulation_ipc.rs`, **below** the existing (reused) protocol types.
2. Transport = `tokio::sync::mpsc<IpcEnvelope>` (client tx, server rx) + per-command `tokio::sync::oneshot<IPCResponse>`
   embedded in the envelope; liveness = shared `Arc<AtomicBool>`.
3. `send_command` = build cmd → oneshot → `tx.send` → `tokio::time::timeout(timeout, reply_rx)`; elapsed → `TeriError::Sim`
   timeout message. `poll_commands` = `rx.try_recv().ok()`. `send_success`/`send_error` = fire the envelope oneshot.
4. Apply the per-symbol table (§16.3) and the `[≠]` table (§16.5) exactly. command_id stays populated (§16.4).
5. Tests: mock-loop task (start → poll → reply) + client assertions, incl. a timeout case. No SimEngine, no files.

---

## DECISION-17 — U-022 `SimulationRunner` (S-540..S-635, 96 symbols) → MAP-ONTO in-process tokio runner (refines DECISION-2; wires DECISION-16 IPC channel + U-021 graph mgr + U-010 logger + U-023 manager)

**Lineage:** DECISION-2 (OASIS subprocess → native `SimEngine`, REIMPLEMENT-as-UPGRADE; substrate LOCKED), DECISION-16
(`channel()`→`SimulationIPCClient`/`SimulationIPCServer` in-process transport — U-022's interview methods delegate here),
DECISION-13/14 (`GraphMemoryManager<L>`/`GraphMemoryUpdater<L>` async worker — the monitor fires it), U-023 (`SimulationManager`
owns `{sim_data_dir}/{sim_id}/` FS layout + `SimulationState`/`state.json` — the run-state ties here). Source-authoritative:
read against `simulation_runner.py` in full (1768 lines).

### 17.0 Landing + the central substrate map (the four owner-posed areas)

teri runs the sim **in-process** (`SimEngine::run` is a tokio future, `src/sim/mod.rs:492`). MiroFish's `SimulationRunner` is
a process **supervisor**: it `subprocess.Popen`s `run_{twitter,reddit,parallel}_simulation.py`, polls their JSONL output
files, and process-group-kills them. The supervisor's *transport* (OS subprocess, Popen, pgid, JSONL files between processes)
is structurally absent in teri — same inexpressibility class as Zep-network→petgraph (DECISION-1) and file-IPC→channel
(DECISION-16). But the supervisor's **observable contract** (start→running state, stop→terminate+clean, dual-platform needs
both, idempotent cleanup, per-action graph-memory firing, `simulation_end`→completed, action/timeline/stat readers,
interview round-trip, run-state `to_dict` shapes) is FULLY PORTED. New module `src/services/simulation_runner.rs`.

**Class-level `_run_states`/`_processes`/… dicts → an OWNED `SimulationRunner` struct** (NOT process-global statics — same
forcing function as U-021/U-023: generic-over-`L: LlmClient` statics are impossible, `LlmClient` not dyn-safe; class-level
singleton → one instance in app state, observable "one registry per process" contract preserved). The struct holds a
`Mutex<HashMap<String, RunHandle>>` where each `RunHandle` bundles what the six Python dicts keyed by `simulation_id`:

```text
struct RunHandle {
    state:        SimulationRunState,                  // _run_states[id]   (S-602)
    task:         tokio::task::JoinHandle<()>,         // _processes[id]    (S-603) — the sim task, NOT a Popen
    shutdown:     Arc<AtomicBool>,  // or watch::Sender<bool> — cooperative stop signal  ([≠] replaces pgid/SIGTERM)
    ipc_client:   SimulationIPCClient,                 // DECISION-16 — interview/close delegate target
    monitor:      tokio::task::JoinHandle<()>,         // _monitor_threads[id] (S-605) — the JSONL/snapshot monitor task
    graph_enabled: bool,                               // _graph_memory_enabled[id] (S-608)
}
```

`_action_queues` (S-604), `_stdout_files`/`_stderr_files` (S-606/S-607) are **subprocess-transport `[≠]`** (see §17.4):
`Queue` was thread→thread handoff (in-process tokio uses the broadcast/oneshot channels directly); the stdout/stderr file
handles existed only to drain a child process's pipes — there is no child pipe in-process.

**(Area 1) `_processes`/Popen lifecycle → JoinHandle + cooperative shutdown.**
| Observable contract (PORTED) | OS-subprocess mechanism (`[≠]` inexpressible) |
|---|---|
| `start_simulation` returns a state with `runner_status=RUNNING`, `twitter_running`/`reddit_running` set per platform, persisted to `state.json` (S-612) | `subprocess.Popen(cmd, start_new_session=True)`, `sys.executable`, `run_*_simulation.py` script path, `PYTHONUTF8`/`PYTHONIOENCODING` env, `bufsize=1`, stdout→`simulation.log` file — there is no child process or interpreter to spawn (S-612 partial) |
| `stop_simulation` transitions RUNNING→STOPPING→STOPPED, sets `twitter/reddit_running=false`, `completed_at`, stops graph updater, persists (S-617) | `_terminate_process` (S-616): `taskkill /F /T` (Windows), `os.killpg(pgid, SIGTERM)` then `SIGKILL` after 5s (Unix), `os.getpgid` — **no OS process to signal**; replaced by `shutdown.store(true)` (cooperative) + `task.abort()` (hard, the SIGKILL analog) + `task.await` bounded by `tokio::time::timeout(5s)` (the SIGTERM-then-SIGKILL-after-5s timing IS observable → preserve the *timing semantics* via a graceful-then-abort window) |
| dual-platform requires BOTH twitter+reddit to terminate (S-615 `_check_all_platforms_completed`) | (pure logic — fully ported) |
| `cleanup_all_simulations` idempotent (S-624 `_cleanup_done` flag), terminates ALL, stops `GraphMemoryManager.stop_all`, sets each state STOPPED + error "服务器关闭，模拟被终止", persists (S-625) | per-process `taskkill`/`killpg`; the `state.json` secondary write (S-625 L1244-1259) is U-023's `SimulationState` file — write via the `SimulationManager`, not a raw json edit |
| `get_running_simulations` lists ids whose task is not finished (S-627) | `process.poll() is None` → `!handle.task.is_finished()` |

`_terminate_process` (S-616): the **observable** is "the running simulation stops within ~5s, gracefully if possible else
forcibly." Map: set `shutdown`→true (sim loop checks between ticks, like the existing `inject_fn` hook point at
`mod.rs:542`), `tokio::time::timeout(Duration::from_secs(5), &mut task)`; on elapsed → `task.abort()` (the SIGKILL analog).
The Windows/Unix branch split, `taskkill`/`killpg`/`SIGTERM`/`SIGKILL`/pgid are **`[≠]` inexpressible** (no OS process) — but
the **5s grace-then-force window is contractual and IS preserved.** `IS_WINDOWS` (S-540) → `[≠]` (no cross-platform process
API needed; teri's stop is OS-agnostic).

**(Area 2) `_monitor_simulation` (S-613) + `_read_action_log` (S-614) → tail U-010's `actions.jsonl` by offset AND/OR consume SimEngine channels.**
The monitor's three observable duties, each PRESERVED:
1. **per-action graph-memory firing** when enabled (L683-684: `graph_updater.add_activity_from_dict(action_data, platform)`)
   → `GraphMemoryUpdater::add_activity_from_dict(&data, platform)` (already ported, U-021 S-... ; non-blocking mpsc send).
2. **`simulation_end` → COMPLETED** (L622-640) → **REUSE U-048 (already PARITY-PASSED): `SimEngine::subscribe_completion()`
   `watch::Receiver<Option<SimCompletion>>`** is the in-band terminal signal; `_check_all_platforms_completed` (S-615) gates
   COMPLETED on both platforms' completion. `round_end` event → `current_round`/`simulated_hours` updates (L642-661).
3. **`add_action` into `recent_actions`** (cap 50, S-596) + per-platform counts (L665-680).

**Monitor source decision — CHOSEN: tail U-010 `actions.jsonl` by byte offset (realizes U-047 JSONL_TAIL_CONTRACT here).**
Rationale: the monitor's job is to *re-derive run-state from the action log the sim writes* — exactly MiroFish's design
(`f.seek(position)` → readline → `f.tell()`, L610-688). teri's `SimEngine` writes the SAME `actions.jsonl` via
`PlatformActionLogger` (U-010, `log_action`/`log_round_end`/`log_simulation_end`). So U-022's monitor:
- spawns a `tokio::task` per run that, in a `loop { … tokio::time::sleep(Duration::from_secs(2)).await }` (the 2s poll IS
  observable cadence — preserve), tails `{sim_dir}/twitter/actions.jsonl` and `{sim_dir}/reddit/actions.jsonl` from a
  per-file `u64` offset, parses each new line, and dispatches event_type/action exactly as `_read_action_log` does;
- terminates the loop when `subscribe_completion()` fires (U-048) OR the sim task finishes — then does ONE final tail pass
  (mirrors L518-522 "进程结束后最后读取一次") so no trailing action is lost.

**This realizes U-047 `JSONL_TAIL_CONTRACT` HERE** (do not defer): the offset-tailing reader (seek + readline + tell, never
re-read a processed line, no line lost) is U-022's monitor. **Preserve exactly:** offset monotonic per file, partial last
line not consumed until newline-terminated (mirror `json.loads` skipping `JSONDecodeError`), graph-memory fired once per
action, `simulation_end`→completed, `add_action`'s 50-cap + counts. The 2s `sleep` + final-pass-after-end are contractual.
*Why tail-the-file over consume-the-broadcast:* the file is the single source MiroFish derives state from; the broadcast
channel carries `WorldState` snapshots (tick-grain), not per-action JSONL records — tailing the log is the faithful map and
keeps the offset-no-double-read invariant the source guarantees. (U-048's `subscribe_completion` is used ONLY for the
loop-exit signal, replacing `process.poll()`.)

**(Area 3) `get_interview_history` (S-635) + `_get_interview_history_from_db` (S-634) → SQLite `trace` table read.**
Source reads `{sim_dir}/{platform}_simulation.db` SQLite, `SELECT user_id, info, created_at FROM trace WHERE action='interview'`
ordered desc, limit, optional `user_id=agent_id` filter, JSON-decodes `info` → `{agent_id, response, prompt, timestamp, platform}`.
**Substrate decision — CHOSEN: the in-process IPC interview store (a teri-side interview-trace log), NOT redb, NOT raw SQLite.**
Rationale + boundary (this is a **carry-forward gate**, NOT a `[≠]`): in teri the interview round-trip is the in-process IPC
path (DECISION-16). MiroFish's OASIS subprocess WROTE each interview to its SQLite `trace` table as a side effect; the history
read is "give me the interviews that happened." teri has no OASIS SQLite — but the interview RESULT is observable and
contractual (it is served to the frontend by U-026), so it is **PORTED, not dropped.** Decision: U-022's interview handler
(sub-cycle e/f) appends each completed interview to a teri-native interview-trace sink — **realize as a JSONL trace file
`{sim_dir}/{platform}_interviews.jsonl`** (consistent with U-010's JSONL substrate; avoids pulling a SQLite dep solely to
mimic OASIS's storage), and `get_interview_history` reads/merges/sorts/limits from THAT. The `trace`-table SQL, `sqlite3`
import, `user_id`/`info`/`created_at` column names are **`[≠]` inexpressible** (OASIS's schema, no OASIS DB) — but the
returned dict shape `{agent_id, response, prompt, timestamp, platform}`, the desc-sort, the per-platform + merged-limit
behavior (L1760-1766) are PORTED and differential-testable. **CARRY-FORWARD GATE → sub-cycle (f) + U-026:** the
interview-history reader is only meaningful if the interview HANDLER writes the trace; the parity gate for (f) MUST verify a
write-then-read round-trip (interview an agent → it appears in `get_interview_history`), or the history contract is a hollow
`[≠]`. Until the handler writes, `get_interview_history` returns `[]` faithfully (matches Python when no DB exists, L1669).

**(Area 4) `register_cleanup` (S-626) + `cleanup_all_simulations` (S-625) → U-049 boundary.**
Source `register_cleanup` installs SIGTERM/SIGINT/SIGHUP signal handlers + `atexit`, all calling `cleanup_all_simulations`;
guards on `WERKZEUG_RUN_MAIN`/`FLASK_DEBUG` reloader detection; `_cleanup_registered` global idempotency. **Boundary
decision:** U-022 OWNS `cleanup_all_simulations` (S-625) — it is the *what-to-clean* logic (terminate all run tasks, stop
`GraphMemoryManager`, persist STOPPED states) and lives on the `SimulationRunner` struct as `async fn cleanup_all`. U-049
OWNS the *signal-handler installation* (`register_cleanup` S-626) — it wires teri's existing graceful-shutdown path
(U-002's `axum::serve().with_graceful_shutdown(ctrl_c)`, parity-verified) to CALL `runner.cleanup_all().await`. So:
- **S-625 `cleanup_all_simulations` → U-022** (`SimulationRunner::cleanup_all`, idempotent via `AtomicBool` compare_exchange
  mirroring `_cleanup_done`, mirroring U-021's `stop_all` idempotency pattern — DECISION-13).
- **S-626 `register_cleanup` → routed to U-049** as a `[deferred-to-U-049]` symbol (NOT `[≠]`, NOT dropped): the
  SIGTERM/SIGINT/SIGHUP/atexit/`WERKZEUG_RUN_MAIN` mechanism is Flask-WSGI-specific; teri's analog is one tokio `ctrl_c`
  (already in U-002) extended (in U-049) to also fire `runner.cleanup_all()` + `graph_mgr.stop_all()`. The DOUBLE-signal /
  SIGHUP / atexit-backup are `[≠]` only insofar as tokio's single `signal::unix::SignalKind` set covers them (U-049 decides;
  SIGHUP IS expressible via `tokio::signal::unix`). U-022's DONE gate does NOT require S-626 to ship — it ships in U-049 — but
  U-022 MUST expose `cleanup_all` as the callable U-049 wires. Record S-626 in U-022's ledger as `[→U-049]` so the symbol is
  not lost.

### 17.1 Run-state types — byte-exact `to_dict` shapes (S-541..S-598)

`RunnerStatus` (S-541..S-549): `str` enum → `#[derive(Serialize)] #[serde(rename_all="lowercase")] enum RunnerStatus { Idle,
Starting, Running, Paused, Stopping, Stopped, Completed, Failed }` with `.value` strings `idle/starting/running/paused/
stopping/stopped/completed/failed` byte-exact (the `to_dict` writes `.value`, L163).
`AgentAction` (S-550..S-560): struct + `to_dict` 9-key map (`round_num, timestamp, platform, agent_id, agent_name,
action_type, action_args, result, success`) — `result: Option<String>` → JSON null when None; `action_args: Map`. **Byte-exact
differential-testable.** Note this `AgentAction` is U-022's own (distinct from U-010's logger record shape — they share the
JSONL field set, which is WHY the monitor can parse the logger's output; verify field-name alignment with U-010 in (c)).
`RoundSummary` (S-561..S-570): + `to_dict` with the computed `actions_count: len(actions)` key AND nested `actions: [..to_dict]`.
`SimulationRunState` (S-571..S-598): the big one. `add_action` (S-596): insert-at-front, truncate to `max_recent_actions=50`,
bump per-platform count, refresh `updated_at`. `to_dict` (S-597): note the **computed** `progress_percent =
round(current_round / max(total_rounds,1) * 100, 1)` and `total_actions_count = twitter+reddit` keys (not stored fields —
compute them) — these are observable outputs, PORT exactly incl. the `round(…,1)` one-decimal rounding. `to_detail_dict`
(S-598): `to_dict` + `recent_actions: [..to_dict]` + `rounds_count: len(rounds)`. `_load_run_state`/`_save_run_state`
(S-610/S-611): persist `to_detail_dict()` to `{sim_dir}/run_state.json` (this is U-022's OWN file, distinct from U-023's
`state.json` — both coexist, source writes both, L302-310 vs L1244-1259). `get_run_state` (S-609): memory-cache-then-file-load.
`process_pid` field (S-595): kept in the struct + `to_dict` for shape parity, but **populated as `[≠]`** — there is no OS pid;
serialize as `null` (faithful: Python sets it to `process.pid`, an OS artifact; the FIELD is contractual in `to_dict`, the
VALUE is OS-mechanism). Flag for verifier: this is shape-preserving, value-`[≠]`.

### 17.2 Action readers (S-618..S-622) — pure file-read logic, fully PORTED

`_read_actions_from_file` (S-618), `get_all_actions` (S-619), `get_actions` (S-620, limit/offset pagination), `get_timeline`
(S-621, round-grouped aggregation with `active_agents` set, `action_types` histogram, first/last time), `get_agent_stats`
(S-622, per-agent histogram sorted desc by total). **All pure JSONL-read + aggregation — NO substrate decision, NO `[≠]`.**
Reads the SAME `{sim_dir}/{platform}/actions.jsonl` U-010 writes; preserve: skip `event_type` records, skip records w/o
`agent_id`, `default_platform` fallback, twitter-then-reddit order, legacy single-file fallback (L938-947), timestamp-desc
sort. `cleanup_simulation_logs` (S-623): deletes `run_state.json`/`simulation.log`/`stdout.log`/`stderr.log`/`*_simulation.db`/
`env_status.json` + `{platform}/actions.jsonl`, returns `{success, cleaned_files, errors}` — PORT the file-deletion set
(the `.db` / `stdout.log` / `stderr.log` / `env_status.json` filenames are deleted-if-present, harmless if teri never created
them — keep the names for forward-compat with logs from a mixed run; `success = errors empty`).

### 17.3 Interview wiring (S-628..S-633) — delegate to DECISION-16 IPC

`check_env_alive` (S-628) → `ipc_client.check_env_alive()` (AtomicBool read). `get_env_status_detail` (S-629): source reads
`env_status.json` → `{status, twitter_available, reddit_available, timestamp}`; teri has no env_status.json (DECISION-16
replaced it with the `alive` AtomicBool) → **derive the same dict from the live `RunHandle`** (status from
`state.runner_status`, twitter/reddit_available from `state.twitter_running`/`reddit_running`, timestamp = now) — the
`env_status.json` FILE is `[≠]`, the returned dict shape is PORTED. `interview_agent` (S-630, timeout 60s),
`interview_agents_batch` (S-631, timeout 120s), `close_simulation_env` (S-633, timeout 30s) → delegate to
`ipc_client.send_interview`/`send_batch_interview`/`send_close_env` (DECISION-16, real `tokio::time::timeout`); PORT the
result-dict shapes (`{success, agent_id, prompt, result, timestamp}` etc.) and the `check_env_alive`-guard-raises-ValueError
precondition (→ `Err(TeriError::Sim)`). `interview_all_agents` (S-632) → reads `simulation_config.json` `agent_configs`,
builds the interview list, delegates to `interview_agents_batch` (timeout 180s). **The interview HANDLER (what the sim loop
DOES on receiving an Interview command — inject prompt, capture next `prepare_action`) is U-022 sub-cycle (e/f) and writes the
interview-trace (Area 3).**

### 17.4 Candidate `[≠]` symbols — WITH per-symbol inexpressible/non-contractual/superset justification

| S | Symbol | `[≠]` class | Justification (verifier re-checks adversarially) |
|---|--------|-------------|--------------------------------------------------|
| S-540 | `IS_WINDOWS` | non-contractual | platform-branch selector for `taskkill` vs `killpg`; teri's stop is OS-agnostic (cooperative+abort), so no branch exists. No observable output. |
| S-604 | `_action_queues` (`Queue`) | inexpressible-substrate | thread→thread handoff between Popen-monitor-thread and main; in-process tokio uses the broadcast/oneshot channels directly — there is no second thread to hand off to. No observable. |
| S-606/S-607 | `_stdout_files`/`_stderr_files` | inexpressible-substrate | file handles existed ONLY to drain a child process's stdout/stderr pipes (avoid pipe-buffer deadlock, L426-428); no child pipe in-process. No observable output (the `simulation.log` content is OASIS's own logging, not a contract teri reproduces — teri logs via its own tracing). |
| S-616 (partial) | `_terminate_process` Win/Unix branches, `taskkill`/`killpg`/pgid/SIGTERM/SIGKILL | inexpressible-substrate | no OS process to signal. **The 5s grace-then-force WINDOW is PORTED** (shutdown-signal then `task.abort()` after `timeout(5s)`); only the signal MECHANISM is `[≠]`. |
| S-612 (partial) | `start_simulation` Popen/`sys.executable`/script-path/`PYTHONUTF8`/`bufsize`/`start_new_session` | inexpressible-substrate | no interpreter/script to spawn; teri spawns a tokio task running `SimEngine::run`. **The returned RUNNING state + platform flags + persistence are PORTED.** |
| S-595 (value only) | `SimulationRunState.process_pid` value | non-contractual value, shape PORTED | field stays in struct + `to_dict` (shape parity); value is `null` (no OS pid). |
| S-629 (file only) | `get_env_status_detail` reading `env_status.json` | inexpressible-substrate, dict PORTED | the FILE is the subprocess-liveness artifact (DECISION-16 `[≠]`); the returned dict is derived from live state. |
| S-634/S-635 (SQL only) | `_get_interview_history_from_db` `sqlite3`/`trace`-table SQL/column names | inexpressible-substrate, dict + sort + merge PORTED | no OASIS SQLite DB; teri's interview-trace is a JSONL sink (Area 3). The returned record shape, desc-sort, per-platform+merged-limit are PORTED. **Carry-forward gate: handler must write the trace.** |
| S-626 | `register_cleanup` SIGTERM/SIGINT/SIGHUP/atexit/`WERKZEUG_RUN_MAIN` | `[→U-049]` (NOT `[≠]`, deferred) | the signal-installation belongs to U-049, which wires teri's `ctrl_c` shutdown to call `cleanup_all`. U-022 ships the callable; the installer ships in U-049. |

**Everything not in this table is PORTED** (no other downgrades). In particular: every `to_dict`/`to_detail_dict` shape, all
action/timeline/stat readers, all interview delegations, dual-platform completion logic, the 2s monitor cadence, the offset-no-
double-read tailing, the per-action graph-memory firing, the 5s grace window, `cleanup_simulation_logs`, idempotent cleanup.

### 17.5 Sub-cycle decomposition (6 cycles; one porter cycle each, opus-gated)

| Cycle | Symbols (S) | What | Reuse points | Risk / escalation |
|-------|-------------|------|--------------|-------------------|
| **(a)** run-state types | S-541..S-598 (RunnerStatus, AgentAction, RoundSummary, SimulationRunState + add_action/to_dict/to_detail_dict/_load/_save/get_run_state) | the dataclasses + byte-exact serde shapes + FS run_state.json persistence | U-023 `SimulationManager` dir layout (`{sim_data_dir}/{id}/`); U-010 JSONL field set (align `AgentAction` fields) | LOW. Watch `progress_percent` rounding + computed `total_actions_count`; `process_pid` null-value `[≠]`. |
| **(b)** lifecycle | S-599..S-603, S-608, S-612, S-616, S-617, S-624, S-625, S-627 (struct, RunHandle, start/stop/_terminate/cleanup_all/get_running) | spawn `SimEngine::run` as tokio task + cooperative-shutdown+abort lifecycle; idempotent cleanup | DECISION-16 `channel()` (build ipc_client per run); U-021 `GraphMemoryManager::create_updater`/`stop_updater`/`stop_all`; U-023 manager for the `state.json` secondary write; `SimEngine::run`/`subscribe_completion` | MED. The shutdown signal must be honored by `SimEngine`'s tick loop — needs a `shutdown` check at the `inject_fn` hook point (`mod.rs:542`). If SimEngine has no cooperative-stop hook, ESCALATE (may need a tiny SimEngine `watch<bool>` shutdown input — additive, no downgrade). The 5s grace-then-abort timing. |
| **(c)** monitor + tail + graph-fire | S-613, S-614, S-615 (_monitor_simulation, _read_action_log, _check_all_platforms_completed) | per-run monitor task: 2s offset-tail of both `actions.jsonl`, event/action dispatch, graph-memory fire, completion detect | U-010 `PlatformActionLogger` output (the file it tails); U-021 `GraphMemoryUpdater::add_activity_from_dict`; U-048 `subscribe_completion` (loop-exit signal); realizes **U-047** JSONL_TAIL_CONTRACT | MED-HIGH. Offset-no-double-read + final-pass-after-end + partial-line safety are the parity-critical invariants. Confirm U-010 writes the exact field names `AgentAction` parses (round/timestamp/agent_id/agent_name/action_type/action_args/result/success + event_type/round_end/simulation_end). If field drift, ESCALATE to align U-010. |
| **(d)** action readers | S-618..S-623 (_read_actions_from_file, get_all_actions, get_actions, get_timeline, get_agent_stats, cleanup_simulation_logs) | pure JSONL read + aggregation + pagination + log cleanup | U-010 `actions.jsonl` layout | LOW. No substrate decision. Preserve sort order, event/no-agent_id skips, legacy single-file fallback. |
| **(e)** interview wiring | S-628, S-630, S-631, S-633 (check_env_alive, interview_agent, interview_agents_batch, close_simulation_env) | delegate to DECISION-16 IPC client; result-dict shapes; check-alive-guard | DECISION-16 `SimulationIPCClient::check_env_alive`/`send_interview`/`send_batch_interview`/`send_close_env` | LOW-MED. Pure delegation; the HANDLER side (what the loop does on a command) is the (b)/(c) sim-loop dispatch (DECISION-16 §16.2) — confirm the loop services commands. |
| **(f)** interview history + env-status + cleanup boundary | S-629, S-632, S-634, S-635, S-626 (get_env_status_detail, interview_all_agents, _get_interview_history_from_db, get_interview_history, register_cleanup→U-049) | interview-trace JSONL sink + history reader; derived env-status; interview_all (config read→batch); S-626 boundary to U-049 | U-023 `get_simulation_config`/`get_profiles` (agent_configs for interview_all); the interview-trace sink wired in (e) | MED. **Carry-forward gate:** verify interview write→read round-trip (Area 3). S-626 lands in U-049, recorded `[→U-049]` here. |

### 17.6 First sub-cycle recommendation — **(a) run-state types FIRST.**

Dependency order: (a) has zero upstream deps (pure dataclasses + serde + FS persistence on U-023's already-built dir layout)
and EVERY later cycle consumes its types — (b)/(c) read/write `SimulationRunState`, (d) returns `AgentAction`, (e)/(f) return
interview dicts that reference run-state. It is also the highest byte-exact differential-test value (the `to_dict`/`to_detail_dict`
shapes are the frontend contract via U-026) and lowest risk, so it locks the observable JSON surface before the
substrate-heavy lifecycle/monitor cycles build on it. Then (b) lifecycle (needs (a)'s state + the SimEngine-task spawn — the
central substrate work + the one likely escalation), (c) monitor (needs (b)'s RunHandle + (a)'s add_action), (d) readers
(independent, can interleave), (e) interviews (needs the (b) IPC-client-per-run), (f) history+boundary last.

### DECISION-17 — 6-line actionable summary
1. New `src/services/simulation_runner.rs`; class-dicts → owned `SimulationRunner { Mutex<HashMap<String, RunHandle>> }`; sim
   = tokio task running `SimEngine::run`, NOT a subprocess (LOCKED by DECISION-2).
2. Lifecycle: `_processes`/Popen → `JoinHandle` + cooperative `shutdown` flag + `task.abort()` after `timeout(5s)`; the 5s
   grace-then-force window is PORTED, the taskkill/killpg/pgid/SIGTERM/SIGKILL mechanism is `[≠]` (no OS process).
3. Monitor (c) tails U-010's `actions.jsonl` by byte offset every 2s (realizes U-047), fires U-021 `add_activity_from_dict`,
   uses U-048 `subscribe_completion` for loop-exit; offset-no-double-read + final-pass are parity-critical.
4. Interviews delegate to DECISION-16 IPC client; `get_interview_history` reads a teri-native interview-trace JSONL sink
   (SQLite `trace`-table SQL is `[≠]`, the record shape/sort/merge are PORTED) — carry-forward gate: handler must write it.
5. `cleanup_all` (S-625) owned by U-022 (idempotent AtomicBool); `register_cleanup` (S-626) deferred `[→U-049]` which wires
   teri's `ctrl_c` to call it — NOT a `[≠]`, NOT dropped.
6. Port in order **(a)→(b)→(c)→(d)→(e)→(f)**; start with **(a) run-state types** (zero deps, all cycles consume it, byte-exact
   frontend contract). `[≠]` set is exactly §17.4 — everything else PORTED.
