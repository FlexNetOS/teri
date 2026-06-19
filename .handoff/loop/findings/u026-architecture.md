# U-026 — Simulation HTTP API routes — Target Architecture (port-fresh)

**Unit:** U-026 · source `MiroFish/backend/app/api/simulation.py` (2716 lines, **33** `@simulation_bp.route`s — the prompt said 31; `env-status` + `close-env` are the +2) · mounted at `/api/simulation` (`backend/app/api/__init__.py:8,12`).
**Destination:** new `src/api/simulation.rs` with `simulation_router(state) -> Router` + handlers; one-line nest in `src/server.rs:197-200`.
**Class:** `port-fresh` (no Y route layer to extend — the U-025 seam is the *shared infra*, not a sibling route file).
**Inherited seam (U-025, LANDED — reuse, do not redesign):** `ApiError::{client,client_with,server}` + `IntoResponse` (`src/api/mod.rs:166-232`); `build_llm(&Config)->OpenAiAdapter` (`src/api/mod.rs:246`); `Result<Json<Value>, ApiError>` handler pattern; per-route `ApiState` via `State(state): State<Arc<ApiState>>`; CORS/`/api/*` nest + Accept-Language middleware (`src/server.rs:184-214`); graph-load-by-id convention `graph_id == task_id -> TaskManager::global().get_task(id) -> task.result["graph"]` (`src/api/graph.rs:933-946`, DECISION-9/U025-GRAPHSTORE).

---

## 1. THE CENTRAL DECISION — `ApiState` runtime-state extension (DECISION-U026-1)

### The tension (grounded)
- `SimulationRunner<L: LlmClient + Send + Sync + 'static>` is **genuinely generic** over the LLM: it owns `graph_mgr: Arc<GraphMemoryManager<L>>` (`src/services/simulation_runner.rs:979`) and spawns `SimEngine::run::<L>` inside `start_simulation` (`:1167`). It is NOT a "holds-no-LLM, takes-it-per-call" shape — option (c) is **false**. The `runs` map of live handles (`:976`) holds per-run state that MUST survive across requests.
- `SimulationManager` holds `cache: Mutex<HashMap<String, SimulationState>>` (`src/services/simulation_manager.rs:790`). The cache is the Python `self._simulations` class-singleton; a per-handler `from_config` instance would lose cross-request cache coherence (a `create_simulation` write would be invisible to a later `get_simulation` on a fresh instance — though file-backed reads paper over it, the cache contract S-670 is a shared-state contract).
- DECISION-U025-1 (`src/api/mod.rs:242-243`): `LlmClient` has generic methods → **not dyn-compatible**; **axum state cannot be generic**. So no `Arc<dyn LlmClient>`, no `ApiState<L>`.

### The resolution — option (b): concrete monomorphization at the state boundary
`build_llm()` **always** returns `crate::llm::OpenAiAdapter` (`src/api/mod.rs:246-248`). Therefore `SimulationRunner<OpenAiAdapter>` is a **single concrete type** and CAN live in non-generic `ApiState`. The generic is monomorphized *at the state-construction boundary*, not erased — DECISION-U025-1 is preserved verbatim (no `dyn`, no generic state).

```rust
// src/api/mod.rs — EXTEND ApiState (sub-cycle a)
pub struct ApiState {
    pub config: crate::Config,
    // U-026: shared runtime registry — concrete monomorphization (DECISION-U026-1).
    pub sim_manager: std::sync::Arc<crate::services::SimulationManager>,
    pub sim_runner:  std::sync::Arc<crate::services::SimulationRunner<crate::llm::OpenAiAdapter>>,
}
```

Construction (in `ApiState::new` / wherever `serve` builds state): build the `GraphMemoryManager<OpenAiAdapter>` from `build_llm(&config)` wrapped in `Arc`, then
`SimulationManager::from_config(&config)` → `Arc`, then
`SimulationRunner::new(sim_data_dir, graph_mgr.clone(), sim_manager.clone())` → `Arc`
(constructor sig confirmed `src/services/simulation_runner.rs:991-1003`).

### Per-handler vs in-state, per primitive (decision table)
| Primitive | Placement | Why |
|---|---|---|
| `SimulationManager` | **in-state** `Arc<SimulationManager>` | Mutex cache (`:790`) must be cross-request-coherent; also the SAME instance the runner holds (`runner.manager`, `:982`) so `mark_state_json_stopped` writes are consistent. |
| `SimulationRunner<OpenAiAdapter>` | **in-state** `Arc<…>` | `runs` HashMap (`:976`) holds live run handles — a sim started by POST `/start` MUST be visible to GET `/:id/run-status` and POST `/stop` on later requests. THE reason U-026 needs shared runtime state (unlike U-025). |
| `ProjectManager` | **per-handler** `from_config(&state.config)` | Stateless file-store, matches U-025 (`src/api/graph.rs:121`). |
| `KnowledgeGraphEntityReader<'a>` | **per-handler**, borrow-facade over a freshly-loaded `KnowledgeGraph` | Reader is `new(graph: &'a KnowledgeGraph)` (`src/services/entity_reader.rs:532`) — borrow, no `Arc`. Idiomatic minimum. |
| `ReportManager` | **per-handler** `ReportManager::new(upload_folder)` | Stateless file scan (`src/report/manager.rs:58`); history route only reads. |
| `TaskManager` | **process global** `TaskManager::global()` | Already a singleton (U-025, `src/api/graph.rs:94`), no state needed. |
| per-request LLM (prepare spawn) | **per-handler** `build_llm(&state.config)` | `manager.prepare_simulation::<L>` is generic; pass a fresh `OpenAiAdapter`. |

> **Note (test impact):** the 30+ existing `create_app(state)` test call-sites (`src/api/graph.rs`) construct `ApiState` — adding two non-`Option` fields changes `ApiState::new`'s signature. Mitigation recorded for porter: give `ApiState::new` a back-compat constructor OR have `new(config)` build the runner/manager internally from `config` (preferred — keeps the 30 call-sites compiling; the runner needs only `config` + `build_llm`). **This is a `- [!]` wiring item for sub-cycle (a), NOT a blocker** — blast radius is one constructor, all callers go through `ApiState::new`.

**U-027 inherits this**: report routes that touch a running sim reuse the SAME `Arc<SimulationRunner<OpenAiAdapter>>` from state.


---

## 2. Streaming / SSE decision — **NONE in U-026** (all 33 routes are request/response JSON)

Investigated every candidate "realtime/streaming" route against source. **No route in U-026 streams.** The `/realtime`, `/run-status`, and `/run-status/detail` routes are **poll-based one-shot snapshots**, by explicit source design:
- `GET /:id/run-status` (`simulation.py:1705-1760`) — docstring `用于前端轮询` ("for frontend polling"). Returns one `jsonify(run_state.to_dict())` snapshot. No generator, no `text/event-stream`.
- `GET /:id/profiles/realtime` (`:1028-1135`) — reads `reddit_profiles.json`/`twitter_profiles.csv` from disk + metadata (`file_modified_at`, `is_generating`, `count`, `total_expected`); one JSON. "realtime" = *fresh file read bypassing the manager cache*, not a stream.
- `GET /:id/config/realtime` (`:1138-1257`) — same shape over `simulation_config.json` + `generation_stage`.
- `GET /:id/run-status/detail` (`:1763+`) — one-shot snapshot with embedded actions list (uses the U-047 tail reader).

**Decision:** map ALL 33 routes to `async fn(...) -> Result<Json<Value>, ApiError>`. **Do NOT** use `axum::response::sse` or `api::streaming::StreamAdapter` anywhere in U-026. The `TickStreamEvent`/SSE machinery (`src/api/mod.rs:61-118`, `src/api/streaming.rs`) is for **U-027** (report `agent-log/stream`, `console-log/stream`) and the future live-sim stream — **not** this unit. Recording this prevents a porter from inventing SSE for `/realtime` (a capability *change*, not a port).

> `[≠] U026-MTIME`: `file_modified_at` is `datetime.fromtimestamp(st_mtime).isoformat()`. Port via `std::fs::metadata(..).modified()` → chrono local ISO-8601. The VALUE format (local naive ISO, no tz) must match teri's existing `created_at` convention (the manager already emits these — reuse `SimulationState`'s timestamp formatter). Contractual shape preserved; not a divergence if the formatter matches.

---

## 3. Per-route X→Y map (Python route → teri handler → landed primitive)

All routes return the U-025 envelope. `OK` = primitive landed & verified; `[!]` = gap; `[≠]` = intentional divergence (challenged at the parity gate).

### Group b — entities (3 routes) — primitive: `KnowledgeGraphEntityReader` (U-016, OK)
| Python (file:line) | teri handler | maps onto |
|---|---|---|
| `GET /entities/<graph_id>` `get_graph_entities` (:48) | `get_graph_entities` | load graph by `graph_id`→task (graph.rs:933 convention) → `KnowledgeGraphEntityReader::new(&g)` → `filter_defined_entities(entity_types, enrich)` (entity_reader.rs:661). Query: `entity_types` (csv), `enrich` (default true). `result.to_dict()`. |
| `GET /entities/<graph_id>/<entity_uuid>` `get_entity_detail` (:93) | `get_entity_detail` | `…get_entity_with_context(uuid)` (entity_reader.rs:755); `None`→404 `api.entityNotFound`. |
| `GET /entities/<graph_id>/by-type/<entity_type>` `get_entities_by_type` (:126) | `get_entities_by_type` | `…get_entities_by_type(type, enrich)` (entity_reader.rs:794); `{entity_type,count,entities:[…]}`. |

> `[≠] U026-ZEPKEY`: source guards each with `if not Config.ZEP_API_KEY: 500 api.zepApiKeyMissing` (Zep cloud reads). teri reads the LOCAL `KnowledgeGraph` (no Zep API). **The 500-guard is a strict-superset removal** — teri has no remote key to be missing. CHALLENGE at gate: is the `api.zepApiKeyMissing` 500 an observable contract a client relies on? It is an *error* path triggered only by a missing remote key teri does not use → non-reproducible in teri's model → `[≠]` legal (really a `[!]`-class inexpressibility: there is no Zep key). Record both i18n keys still present (U-005). Porter must still resolve `graph_id`→graph; a missing graph → 404 (the teri-native failure).

### Group c — create/get/list (3 routes) — primitive: `SimulationManager` (U-023, OK; in-state)
| Python | teri handler | maps onto |
|---|---|---|
| `POST /create` `create_simulation` (:165) | `create_simulation` | `ProjectManager::from_config` get_project (400 `requireProjectId`, 404 `projectNotFound`); `graph_id = body.graph_id or project.graph_id` (400 `graphNotBuilt`); `state.sim_manager.create_simulation(project_id, graph_id, enable_twitter=true, enable_reddit=true)` (manager.rs +899:123). `state.to_dict()`. |
| `GET /<simulation_id>` `get_simulation` (:755) | `get_simulation` | `state.sim_manager.get_simulation(id)` (manager.rs:516); `None`→404 `simulationNotFound`; else `state.to_dict()`. |
| `GET /list` `list_simulations` (:788) | `list_simulations` | `state.sim_manager.list_simulations(project_id?)` (manager.rs:591), optional `?project_id`, `?limit`; `{data:[to_dict],count}`. |

### Group d — prepare + prepare/status (2 routes) — primitive: `SimulationManager::prepare_simulation` (U-023, OK) + `TaskManager` + spawn
| Python | teri handler | maps onto |
|---|---|---|
| `POST /prepare` `prepare_simulation` (:359) | `prepare_simulation` | 404 if no sim; `force_regenerate` skip-check via `_check_simulation_prepared` (PORT helper, see §4); sync entity-count preview via reader; `TaskManager::global().create_task("simulation_prepare", metadata)`; set status PREPARING; **`tokio::spawn`** running `manager.prepare_simulation::<OpenAiAdapter>(…, progress_callback)` with `build_llm(&config)` — mirrors `graph.rs:802 build_graph_async_with_completion`. Returns `{simulation_id,task_id,status:"preparing",already_prepared:false,expected_entities_count,entity_types}` immediately. |
| `POST /prepare/status` `get_prepare_status` (:642) | `prepare_status` | Query a task by `task_id` OR by `simulation_id` → `TaskManager::global().get_task` → task dict. (Read §4 source for both-modes shape.) |

> The progress-stage weighting (`reading 0-20, generating_profiles 20-70, generating_config 70-90, copying_scripts 90-100`, `simulation.py:524-528`) + `progress_detail` payload (`:556-565`) is a **contractual closure** the porter ports inside the spawned task's `progress_callback`. `copying_scripts` stage: `[≠] U026-SCRIPTS` — teri has no `run_*.py` to copy (S-601, already `[≠]` in U-022); the stage still emits 90-100% progress (the weight band is observable) but copies nothing.

### Group e — profiles/config reads (6 routes) — primitives: `SimulationManager` + direct file reads
| Python | teri handler | maps onto |
|---|---|---|
| `GET /<id>/profiles` (:990) | `get_profiles` | `state.sim_manager.get_profiles(id, platform)` (manager.rs:642), `?platform`. |
| `GET /<id>/profiles/realtime` (:1028) | `get_profiles_realtime` | **direct file read** of `{sim_dir}/reddit_profiles.json`|`twitter_profiles.csv`; metadata `{count,total_expected,is_generating,file_exists,file_modified_at,profiles}`; 404 if sim_dir absent. Reuse `Config::oasis_simulation_data_dir`. |
| `GET /<id>/config` (:1258) | `get_config` | `state.sim_manager.get_simulation_config(id)` (manager.rs:675). |
| `GET /<id>/config/realtime` (:1138) | `get_config_realtime` | direct file read `simulation_config.json` + `{file_exists,file_modified_at,is_generating,generation_stage,config}`. |
| `GET /<id>/config/download` (:1294) | `download_config` | `send_file` of `simulation_config.json` → axum file response (Content-Disposition attachment). |
| `GET /script/<script_name>/download` (:1323) | `download_script` | `[≠] U026-SCRIPTDL`: serves `backend/scripts/run_*.py`. teri has no run scripts (S-601). CHALLENGE at gate: this is a portable *file sink* (a download endpoint with observable output) — but the file it serves DOES NOT EXIST in teri (no `run_*.py` produced anywhere). Genuinely inexpressible → `[≠]`/`[!]`. Owner-decision: return 404 `scriptNotFound`, OR drop the route. FLAG for owner; **do not silently 200 with empty**. |

### Group f — generate-profiles (1 route)
| `POST /generate-profiles` (:1377) | `generate_profiles` | Read source §1377-1450 for exact shape (LLM profile (re)generation for a sim). Maps onto `OasisProfileGenerator` (Python `oasis_profile_generator`) — **verify teri equivalent** `src/services/oasis_profile_export.rs` covers `generate`; if it only EXPORTS (not generates), this is `[!] GAP-U026-PROFGEN`. See §4 open items. |

### Group g — start/stop (2 routes) — primitive: `SimulationRunner` (U-022, OK; in-state)
| `POST /start` `start_simulation` (:1451) | `start_simulation` | Full contract §below. `state.sim_runner.start_simulation(id, platform, max_rounds, enable_graph_memory_update, graph_id?, graph_for_updater?)` (runner.rs:1054). Validates `platform∈{twitter,reddit,parallel}` (400 `invalidPlatform`), `max_rounds>0` (400). `force`/`force_restarted` cleanup branch (`cleanup_simulation_logs`). Post-start: `manager` status→RUNNING. Response adds `max_rounds_applied`, `graph_memory_update_enabled`, `force_restarted`, `graph_id?`. |
| `POST /stop` `stop_simulation` (:1644) | `stop_simulation` | `state.sim_runner.stop_simulation(id)` (runner.rs:1234); then `manager` status→PAUSED; `run_state.to_dict()`. `ValueError`→400. |

### Group h — run-status/detail (2 routes) — primitive: `SimulationRunner.get_run_state` + `to_dict`/`to_detail_dict`
| `GET /<id>/run-status` (:1705) | `run_status` | `state.sim_runner.get_run_state(id)` (runner.rs:1012); `None`→**200** with the idle-stub dict (`runner_status:"idle"`, zeros — NOT 404, simulation.py:1735-1747); else `run_state.to_dict()`. |
| `GET /<id>/run-status/detail` (:1763) | `run_status_detail` | `get_run_state` + `to_detail_dict()` (runner.rs:645) with embedded recent actions (reads U-047 tail). Query params per source §1763+. |

### Group i — world-state actions/timeline/agent-stats (3 routes) — primitive: `SimulationRunner` readers (U-022(d), OK)
| `GET /<id>/actions` (:1864) | `get_actions` | `state.sim_runner.get_actions(id, limit, offset, platform?, agent_id?, round_num?)` (runner.rs:3859). `{actions:[AgentAction.to_dict],count,...}`. |
| `GET /<id>/timeline` (:1918) | `get_timeline` | `…get_timeline(id, start_round, end_round?)` (runner.rs:3882). |
| `GET /<id>/agent-stats` (:1958) | `get_agent_stats` | `…get_agent_stats(id)` (runner.rs:3933). |

> Producer-wiring caveat (from loop_state HEAD note): `SimEngine::run` does not yet WRITE `actions.jsonl` via `PlatformActionLogger`. The readers are faithful/no-op on a missing log. So actions/timeline/agent-stats will return **empty** until U-028/029/030 wire the producer. This is **correct port behavior** (matches a sim that produced no actions) — NOT a U-026 bug. Note in sub-cycle (i) `- [!] PRODUCER-PENDING` (informational; routes still port + verify against the empty-log contract).

### Group j — posts/comments (2 routes) — **`[!] GAP-SOCIAL-WORLDSTATE` — BLOCKED**
| `GET /<id>/posts` (:1987) | `get_posts` | reads `{sim_dir}/{platform}_simulation.db` (SQLite) `SELECT * FROM post ORDER BY created_at DESC LIMIT ? OFFSET ?` + `COUNT(*)`; missing DB → **200** `{platform,count:0,posts:[],message:dbNotExist}`. |
| `GET /<id>/comments` (:2065) | `get_comments` | reads `reddit_simulation.db` `SELECT * FROM comment …` + optional `post_id` filter; missing DB → 200 `{count:0,comments:[]}`. |

**GAP:** the `{platform}_simulation.db` is written by the **OASIS social-sim engine** (U-028 twitter / U-029 reddit / U-030 parallel — all `- [ ]` deferred; `loop_state.md:310` GAP-SOCIAL-WORLDSTATE). teri has **no SQLite social-world reader** (confirmed: zero hits for a social-DB reader in `src/`). `rusqlite` exists as an **optional** dep behind the `sqlite` feature (`Cargo.toml:91,110`) — not enabled.
**Decision (no silent drop):** sub-cycle (j) ports the **route shapes + missing-DB branch** now (both routes 200-return the empty contract when no DB — which is ALWAYS true until U-028/029/030 land the producer). The SQLite `SELECT` path is `- [!]` deferred: a `[!] GAP-U026-SOCIALDB` row blocks the *non-empty* branch on U-028/029/030 + enabling the `sqlite` feature. The empty-branch IS the current faithful behavior (no producer → no DB → empty), so the route is portable + verifiable TODAY against that contract; the populated branch is flagged, not hand-waved.

### Group k — interview ×4 (4 routes) — primitives: `SimulationRunner` interview methods (U-022(e), OK) + U-020 IPC
| `POST /interview` `interview_agent` (:2142) | `interview_agent` | `optimize_interview_prompt(prompt)` (PORT helper §4) then `state.sim_runner.interview_agent(id, agent_id, prompt, platform?, timeout)` (runner.rs:3997). |
| `POST /interview/batch` (:2271) | `interview_batch` | `…interview_agents_batch(id, interviews, platform?, timeout=120s)` (runner.rs:4020). |
| `POST /interview/all` (:2409) | `interview_all` | `…interview_all_agents(id, prompt, platform?, timeout=180s)` (runner.rs:4114). |
| `POST /interview/history` (:2512) | `interview_history` | reads interview history (per source §2512). Verify a `get_interview_history` reader exists on the runner; if not → `[!] GAP-U026-IVHIST` (likely a file read of an interview log). |

> `[!] U020-IPC-DEP`: interview routes require a **live env** (`runs[id].ipc_client()`) — `interview_agent` errors `Simulation not found` if no run is registered (runner.rs:4008). That is the faithful no-running-sim error. The U-020 IPC honest-err surface: confirm `send_interview`/`send_batch_interview` return a structured `IPCResponse` (not a stub). Spot-checked landed (runner.rs:4009 delegates to `handle.ipc_client().send_interview`). The actual IPC server (U-020) must be running inside the spawned sim — if U-028/029/030 (the producers that start the IPC server) aren't landed, interviews error "not found" / "env not alive". Flag `- [!] IPC-PRODUCER-PENDING` (same family as group i).

### Group l — env-status / close-env (2 routes — the +2 beyond the prompt's 31)
| `POST /env-status` `get_env_status` (:2584) | `env_status` | `state.sim_runner.get_env_status_detail(id)` (runner.rs:4062) — pure file read of `env_status.json`, default stub if absent. **No producer dep** — portable today. |
| `POST /close-env` `close_env` (:2649) | `close_env` | `state.sim_runner.close_simulation_env(id, timeout=30s)` (runner.rs:4041) via IPC. Same `IPC-PRODUCER-PENDING` family as group k. |

### Group m — history + downloads (history route; downloads folded into e)
| `GET /history` `get_simulation_history` (:876) | `get_history` | `state.sim_manager.list_simulations()[:limit]`; per sim enrich: `get_simulation_config` (requirement, total_simulation_hours, recommended_rounds), `sim_runner.get_run_state` (current_round/runner_status/total_rounds, idle-fallback), `ProjectManager.get_project` (files[:3]), **`report_id` via `ReportManager::get_report_by_simulation(id)`** (manager.rs:830 — the teri equivalent of `_get_report_id_for_simulation`, returns latest by created_at), `version:"v1.0.2"`, `created_date = created_at[:10]`. `{data:[…],count}`. |

> `report_id` linkage is **NOT blocked on the full ReportAgent (U-024 `[~]`)** — it only needs `ReportManager::get_report_by_simulation` which is **landed** (`src/report/manager.rs:830`, scans `{report_id}/meta.json` for `simulation_id`). Confirmed reuse. Take `.report_id` off the returned `Report`, `None`→null.

---

## 4. Module-private helpers to PORT (not on any landed primitive) + resolved verifications

| Helper | Source | Port target | Notes |
|---|---|---|---|
| `optimize_interview_prompt(prompt)` | `simulation.py:28-43` | private fn in `src/api/simulation.rs` | Prepends `INTERVIEW_PROMPT_PREFIX` ("结合你的人设…直接用文本回复我：") unless already prefixed. Pure string. Port verbatim (the CJK prefix is a literal — preserve exactly; it is observable in the interview prompt sent to the agent). |
| `_check_simulation_prepared(sim_id)` | `simulation.py:240-356` | private fn | Checks `{sim_dir}` exists + required files `[state.json, simulation_config.json, reddit_profiles.json, twitter_profiles.csv]`; reads `state.json` status∈`[ready,preparing,running,completed,stopped,failed]` AND `config_generated`; **side effect**: auto-upgrades `preparing→ready` by rewriting `state.json` + `updated_at` (`:324-334`). Returns `(bool, info-dict)`. Used by `/prepare` (skip) + `/start` (readiness). Port the FULL closure incl. the auto-upgrade write — it is observable (subsequent reads see `ready`). |
| `_get_report_id_for_simulation(sim_id)` | `simulation.py:817-873` | **REUSE** `ReportManager::get_report_by_simulation(id).map(|r| r.report_id)` | manager.rs:830 — landed, scans `{report_id}/meta.json` for `simulation_id`, latest by `created_at`. Confirmed equivalent — no new helper. |

### Verifications resolved (were "verify" in §3)
- **generate-profiles primitive — OK.** `generate_profiles_from_entities<L: LlmClient>` landed (`oasis_profile_export.rs:379`) + `to_reddit_format`/`to_twitter_format` realtime formatters (`:541,:552`). Group f is NOT a gap. Handler: load graph by `graph_id` → reader `filter_defined_entities(types, enrich=true)` → 400 `noMatchingEntities` if `filtered_count==0` → `generate_profiles_from_entities(entities, use_llm)` with `build_llm` → format per `platform`. (This is `async` — the generator is async; handler awaits, no spawn.)
- **interview-history primitive — landed but `[!] sqlite-gated.** `get_interview_history` (`runner.rs:4266`) is `#[cfg(feature="sqlite")]` and reads `{platform}_simulation.db` (`:4282`). Same social-DB family as posts/comments. See gap below.

### Consolidated GAP / divergence flags (no silent drop)
- `- [!] GAP-U026-SOCIALDB` — routes **posts, comments, interview/history** read `{platform}_simulation.db` (OASIS SQLite). Blocked on: (1) `sqlite` cargo feature enabled (`Cargo.toml:110`, currently off), AND (2) the DB **producer** = U-028/U-029/U-030 social-sim (all `- [ ]`). **Port now**: route shapes + the missing-DB empty branch (posts/comments 200-return empty — the faithful current behavior). **Defer**: the populated `SELECT` branch. interview/history under `#[cfg(feature="sqlite")]`: with the feature off, port a graceful empty/`[]` response matching "no DB" + flag. Ties to `loop_state.md:310` GAP-SOCIAL-WORLDSTATE.
- `- [!] PRODUCER-PENDING` (informational, NOT blocking the port) — actions/timeline/agent-stats (group i) read `actions.jsonl` not yet written by `SimEngine::run` (loop_state HEAD note); interview/env (groups k,l) need a live IPC server started by the sim producer. Until U-028/029/030, these return the faithful empty/not-found contract. Routes port + verify against THAT contract now.
- `- [≠] U026-ZEPKEY` (group b) — the `Config.ZEP_API_KEY` 500-guard is removed; teri reads the local graph (no remote key). Inexpressible-in-teri class. Gate will challenge — the i18n key stays present (U-005).
- `- [≠] U026-SCRIPTDL` (`/script/<name>/download`) — serves nonexistent `run_*.py` (S-601). **Owner-decision flag**: 404 `scriptNotFound` vs drop. Do NOT 200-empty.
- `- [≠] U026-SCRIPTS` (prepare `copying_scripts` stage) — emits 90-100% progress band (observable) but copies nothing (no run scripts). Already `[≠]` in U-022.
- `- [≠] U026-MTIME` (realtime routes) — `file_modified_at` value: port `st_mtime` → local ISO-8601; match teri's `created_at` formatter (contractual shape preserved).

---

## 5. Ordered sub-cycle decomposition (13 sub-cycles; one portable+parity-verifiable per loop cycle)

Each sub-cycle is independently portable, has a single shared primitive/concern, and ends at byte/JSON-parity PASS + commit. Deps are intra-unit unless noted.

| # | Sub-cycle | Routes | Primitive(s) | Dep | Risk flags |
|---|---|---|---|---|---|
| **a** | **ApiState runtime-state extension + `simulation_router` skeleton + nest** | (router only) | DECISION-U026-1: add `Arc<SimulationManager>` + `Arc<SimulationRunner<OpenAiAdapter>>` to `ApiState`; build them in `ApiState::new`; add `.nest("/simulation", simulation_router(state.clone()))` to `server.rs:198` | — | `- [!]` ApiState::new sig change (30 `create_app` call-sites — mitigate by building from `config` internally; blast radius = 1 constructor) |
| **b** | entities ×3 | `/entities/*` | `KnowledgeGraphEntityReader` (U-016) + graph-load-by-id (U-025 f) | a | `- [≠] U026-ZEPKEY` |
| **c** | create/get/list ×3 | `/create`, `/<id>`, `/list` | `SimulationManager` (in-state) + `ProjectManager` | a | — |
| **d** | prepare + prepare/status ×2 | `/prepare`, `/prepare/status` | `SimulationManager::prepare_simulation` + `TaskManager` + `tokio::spawn` + `build_llm` | a, c | `- [≠] U026-SCRIPTS`; port progress-stage closure |
| **e** | profiles/config reads + downloads ×5 | `/profiles`, `/profiles/realtime`, `/config`, `/config/realtime`, `/config/download` | `SimulationManager` + direct file reads | a, c | `- [≠] U026-MTIME` |
| **e2** | script download ×1 | `/script/<name>/download` | (none) | a | `- [≠] U026-SCRIPTDL` — owner-decision (404 vs drop) |
| **f** | generate-profiles ×1 | `/generate-profiles` | `generate_profiles_from_entities` (async) + reader + `build_llm` | a, b | — (verified landed) |
| **g** | start/stop ×2 | `/start`, `/stop` | `SimulationRunner` (in-state) + `_check_simulation_prepared` helper | a, c | port `_check_simulation_prepared` closure (auto-upgrade write); `force` cleanup branch |
| **h** | run-status/detail ×2 | `/<id>/run-status`, `/run-status/detail` | `SimulationRunner.get_run_state` + `to_dict`/`to_detail_dict` | a, g | idle-stub 200 (not 404) |
| **i** | world-state actions/timeline/agent-stats ×3 | `/actions`, `/timeline`, `/agent-stats` | `SimulationRunner` readers (U-022 d) | a, g | `- [!] PRODUCER-PENDING` (empty until U-028/029/030; verify vs empty-log contract) |
| **j** | posts/comments ×2 | `/posts`, `/comments` | direct SQLite (`sqlite` feature) | a | `- [!] GAP-U026-SOCIALDB` — port empty-branch now; populated branch deferred to U-028/029/030 |
| **k** | interview ×4 + history (sqlite) | `/interview`, `/interview/batch`, `/interview/all`, `/interview/history` | `SimulationRunner` IPC interview (U-022 e / U-020) + `optimize_interview_prompt` helper | a, g | `- [!] IPC-PRODUCER-PENDING`; `/interview/history` `- [!] GAP-U026-SOCIALDB` (sqlite-gated) |
| **l** | env-status/close-env ×2 | `/env-status`, `/close-env` | `get_env_status_detail` (pure file) + `close_simulation_env` (IPC) | a, g | env-status portable today; close-env `- [!] IPC-PRODUCER-PENDING` |
| **m** | history ×1 | `/history` | `SimulationManager.list_simulations` + `get_simulation_config` + runner `get_run_state` + `ProjectManager` + `ReportManager::get_report_by_simulation` | a, c, g, h | report_id reuse confirmed (manager.rs:830) |

**Route count check:** b(3)+c(3)+d(2)+e(5)+e2(1)+f(1)+g(2)+h(2)+i(3)+j(2)+k(4)+l(2)+m(1) = **31 handler routes + 2 (env/close in l)** = **33**. ✓ (a is router-only.)

**Critical-path ordering:** a → c → {b, e, e2, f} parallelizable-after-c → g → {h, i, j, k, l, m}. Sub-cycle (a) MUST land first (it is the ApiState/state decision every other handler depends on). Groups (h,i,k,l,m) depend on (g) only for the shared in-state runner being wired + the `_check_simulation_prepared` helper landing in (g).

**Recommended FIRST cycle for the porter:** sub-cycle **(a)** — the ApiState runtime-state extension + router skeleton + nest. It is the smallest unit that proves DECISION-U026-1 compiles (the `SimulationRunner<OpenAiAdapter>` monomorphization in axum state) without touching any route logic. Mirrors U-025's (a) skeleton-first landing. Then (c), then fan out.
