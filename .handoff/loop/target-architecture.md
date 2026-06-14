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
