//! Simulation API route handlers — port of `backend/app/api/simulation.py` (MiroFish,
//! 2716 lines, 33 `@simulation_bp.route`s) mounted at `/api/simulation`.
//!
//! Unit **U-026** (`port-fresh`). Decomposed into 13 sub-cycles (a–m) per
//! `.handoff/loop/findings/u026-architecture.md`. This file lands incrementally; each
//! sub-cycle adds its routes and is parity-verified before commit.
//!
//! ## Sub-cycle (a) — ApiState runtime-state extension + router skeleton + nest
//!
//! DECISION-U026-1: `ApiState` now carries the shared simulation runtime registry
//! (`sim_manager: Arc<SimulationManager>` + `sim_runner: Arc<SimulationRunner<OpenAiAdapter>>`)
//! — concrete monomorphization at the state-construction boundary (see `src/api/mod.rs`).
//! `simulation_router` is nested under `/api/simulation` in `server.rs`.
//!
//! ## Sub-cycle (c) — THIS landing: `POST /create`, `GET /:id`, `GET /list`
//!
//! Three `SimulationManager`-backed routes.
//!
//! DECISION-U026-1: uses `state.sim_manager` (the shared in-state `Arc<SimulationManager>`)
//! rather than constructing a fresh `SimulationManager()` as Python does per-request. This is
//! the faithful mapping: Python's per-request `manager = SimulationManager()` shares the same
//! on-disk `state.json` files; teri's in-state Arc is the Rust equivalent because teri's
//! manager carries a cross-request in-memory cache backed by the same FS root.
//!
//! DECISION-U026-2: `GET /:simulation_id` includes `run_instructions` when status == READY,
//! using the NATIVE teri run-guidance (HTTP start endpoint) not the inexpressible Python-script
//! subprocess commands. The `run_instructions.to_dict()` key order:
//! `simulation_dir, config_file, commands{twitter,reddit,parallel}, instructions, substrate_note`.
//!
//! ## Route → handler map (33 routes, filled in by sub-cycles b–m)
//!
//! | Sub-cycle | Routes | Primitive |
//! |-----------|--------|-----------|
//! | b  | `GET /entities/:graph_id`, `/entities/:graph_id/:uuid`, `/entities/:graph_id/by-type/:type` | `KnowledgeGraphEntityReader` (U-016) |
//! | c  | `POST /create`, `GET /:id`, `GET /list` | `SimulationManager` (in-state) |
//! | d  | `POST /prepare`, `POST /prepare/status` | `prepare_simulation` + `TaskManager` + spawn |
//! | e  | `GET /:id/profiles`, `/profiles/realtime`, `/config`, `/config/realtime`, `/config/download` | `SimulationManager` + file reads |
//! | e2 | `GET /script/:name/download` | `[≠] U026-SCRIPTDL` (owner-decision) |
//! | f  | `POST /generate-profiles` | `generate_profiles_from_entities` (async) |
//! | g  | `POST /start`, `POST /stop` | `SimulationRunner` (in-state) |
//! | h  | `GET /:id/run-status`, `/run-status/detail` | `SimulationRunner.get_run_state` |
//! | i  | `GET /:id/actions`, `/timeline`, `/agent-stats` | `SimulationRunner` readers (`[!]` PRODUCER-PENDING) |
//! | j  | `GET /:id/posts`, `/comments` | SQLite (`[!]` GAP-U026-SOCIALDB; empty-branch now) |
//! | k  | `POST /interview`, `/interview/batch`, `/interview/all`, `/interview/history` | IPC interview (`[!]` IPC-PRODUCER-PENDING) |
//! | l  | `POST /env-status`, `POST /close-env` | `get_env_status_detail` + `close_simulation_env` |
//! | m  | `GET /history` | `SimulationManager` + `ReportManager::get_report_by_simulation` |
//!
//! All handlers return the U-025 envelope: `Result<Json<Value>, ApiError>` (`src/api/mod.rs`).
//! No SSE in U-026 — every route is a request/response JSON snapshot (the `/realtime` and
//! `/run-status` routes are poll-based by source design; SSE lands in U-027).
//!
//! # `[≠]` / `[!]` flags for sub-cycle (c)
//!
//! - `[≠] U025-TRACEBACK`: 500 body carries Rust string traceback, not Python stack.
//!   The 3-key shape `{success, error, traceback}` is preserved.
//! - `[≠] U026-2-SCRIPTS_DIR`: `run_instructions.to_dict()` omits `scripts_dir`
//!   (teri has no `backend/scripts/` dir; inexpressible). All other Python keys are
//!   NATIVE-EXPRESSED (DECISION-U026-2).
//! - `[!] U026-ROUTE-ORDER`: `GET /list` is registered BEFORE `GET /:simulation_id`
//!   (axum 0.7 static-before-capture rule; same pattern as U-025's `/project/list`).

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{Json, Response},
    routing::{get, post},
};
use serde_json::Value;

use crate::api::{ApiError, ApiState};
use crate::graph::KnowledgeGraph;
use crate::models::project::python_isoformat_local;
use crate::services::simulation_ipc::{CommandStatus, IPCResponse};
use crate::services::simulation_manager::SimulationStatus;
use crate::services::simulation_runner::{AgentStats, RunInputs, RunnerStatus, TimelineEntry};
use std::time::Duration;

// ---------------------------------------------------------------------------
// Router factory — sub-cycle (c) adds the 3 create/get/list routes.
//
// `[!] U026-ROUTE-ORDER`: `/list` (static) is registered BEFORE `/:simulation_id`
// (capture). Axum 0.7 ranks static segments above captures, so `GET /list` resolves
// to `list_simulations` — NOT `get_simulation("list")`.  The test
// `route_order_list_not_matched_as_simulation_id` asserts this invariant.
// ---------------------------------------------------------------------------

/// Build the `/api/simulation` sub-router.
///
/// Sub-cycle (a): skeleton — `with_state` bound so subsequent sub-cycles attach
/// handlers typed `State<Arc<ApiState>>` without re-threading state.
///
/// Sub-cycle (c): adds `POST /create`, `GET /list`, `GET /:simulation_id`.
pub fn simulation_router(state: Arc<ApiState>) -> Router {
    Router::new()
        // Sub-cycle (b): entity-read routes (U-026 sub-cycle b).
        //
        // `[!] U026-ROUTE-ORDER-ENTITIES`: segment-count disambiguation:
        //   `/entities/:graph_id`                  — 2 segments → get_graph_entities
        //   `/entities/:graph_id/:entity_uuid`     — 3 segments, both captures → get_entity_detail
        //   `/entities/:graph_id/by-type/:entity_type` — 4 segments, 3rd is static "by-type"
        //                                             → get_entities_by_type
        // Axum 0.7 static-before-capture rule applies within each segment count.  The 3-segment
        // `/entities/:graph_id/by-type/:entity_type` is DISTINCT from the 3-segment
        // `/entities/:graph_id/:entity_uuid` because axum routes by full path; a request to
        // `/entities/G/by-type/X` carries 4 segments (entities, G, by-type, X) and matches
        // `/entities/:graph_id/by-type/:entity_type`, NOT `/entities/:graph_id/:entity_uuid`
        // (which is only 3 segments: entities, G, U).  No order dependency needed, but we
        // register the static sub-path first for clarity.
        .route("/entities/:graph_id", get(get_graph_entities))
        .route("/entities/:graph_id/by-type/:entity_type", get(get_entities_by_type))
        .route("/entities/:graph_id/:entity_uuid", get(get_entity_detail))
        // Sub-cycle (c): simulation create/get/list routes.
        // IMPORTANT: /list (static) BEFORE /:simulation_id (capture) — axum 0.7 route order.
        .route("/create", post(create_simulation))
        .route("/list", get(list_simulations))
        // Sub-cycle (m): history list with project enrichment. Static 1-segment `/history`
        // registered BEFORE `/:simulation_id` (capture) — axum 0.7 ranks static above capture,
        // same `[!] U026-ROUTE-ORDER` rule as `/list`.
        .route("/history", get(get_simulation_history))
        // Sub-cycle (d): async prepare lifecycle. Both POST static paths (no capture conflict);
        // /prepare/status is a 2-segment static distinct from /prepare by full-path match.
        .route("/prepare", post(prepare_simulation_route))
        .route("/prepare/status", post(prepare_status_route))
        .route("/:simulation_id", get(get_simulation))
        // Sub-cycle (e): profiles/config read routes.
        //
        // `[!] U026-ROUTE-ORDER-E`: static suffixes (/profiles, /config, /config/download,
        // /profiles/realtime, /config/realtime) are 2-segment paths under /:simulation_id.
        // Axum 0.7 selects by full path; each route is distinct by its static suffix.
        // No capture conflicts: /:simulation_id alone is a 1-segment capture.
        //
        // Note: /config/download and /config/realtime both begin "/:simulation_id/config"
        // and then diverge by a second static segment — axum 0.7 resolves these correctly
        // by full path match (3-segment paths, 3rd segment static).  Register
        // /config/download and /config/realtime BEFORE /config so axum's 3-segment paths
        // shadow the 2-segment /:simulation_id/config.  Within 3-segment paths the static
        // wins over a capture (not relevant here — all three are static).
        .route("/:simulation_id/profiles", get(get_profiles))
        .route("/:simulation_id/profiles/realtime", get(get_profiles_realtime))
        .route("/:simulation_id/config/realtime", get(get_config_realtime))
        .route("/:simulation_id/config/download", get(download_config))
        .route("/:simulation_id/config", get(get_config))
        // Sub-cycle (f): generate profiles directly from a graph (no simulation setup needed).
        // Static path — no conflict with any capture route.  Python: `POST /generate-profiles`.
        .route("/generate-profiles", post(generate_profiles))
        // Sub-cycle (g): lifecycle — start + stop.  Both are static paths; no capture conflicts.
        .route("/start", post(start_simulation))
        .route("/stop", post(stop_simulation))
        // Sub-cycle (h): poll-based run-status snapshots (NOT streaming).
        // Both under /:simulation_id; the 3-segment `/run-status/detail` (static seg2 "detail")
        // is distinct from the 2-segment `/run-status` by full-path match (axum 0.7).
        .route("/:simulation_id/run-status", get(run_status))
        .route("/:simulation_id/run-status/detail", get(run_status_detail))
        // Sub-cycle (i): world-state read routes (actions/timeline/agent-stats). Distinct static
        // 2nd segments under /:simulation_id; no capture conflicts.
        .route("/:simulation_id/actions", get(get_simulation_actions))
        .route("/:simulation_id/timeline", get(get_simulation_timeline))
        .route("/:simulation_id/agent-stats", get(get_simulation_agent_stats))
        // Sub-cycle (j): social-DB read routes (posts/comments). Distinct static 2nd segments.
        .route("/:simulation_id/posts", get(get_simulation_posts))
        .route("/:simulation_id/comments", get(get_simulation_comments))
        // Sub-cycle (k): interview routes. All POST, static paths (simulation_id in the body),
        // no capture conflicts. `/interview/{batch,all,history}` are 2-segment statics distinct
        // from the 1-segment `/interview`.
        .route("/interview", post(interview_agent_route))
        .route("/interview/batch", post(interview_batch_route))
        .route("/interview/all", post(interview_all_route))
        .route("/interview/history", post(interview_history_route))
        // Sub-cycle (l): env-status (pure read) + close-env (IPC). Both POST static paths.
        .route("/env-status", post(env_status_route))
        .route("/close-env", post(close_env_route))
        // Sub-cycle (e2): simulation-script download.  Static FIRST segment "script" — axum 0.7
        // ranks a static segment above the `/:simulation_id/...` capture at position 0, so
        // `/script/<name>/download` is unambiguous against the 3-segment `/:simulation_id/config/*`
        // routes (whose middle segment is the static "config", which never equals a script name).
        .route("/script/:script_name/download", get(download_script))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Sub-cycle (b) private helper — load graph from TaskManager by graph_id
//
// Shared prologue for all 3 entity-read routes.  Steps:
//   1. ZEP guard (matches Python's `if not Config.ZEP_API_KEY: 500` + U-025 precedent)
//   2. Resolve graph_id → task → result["graph"]
//   3. Deserialize into KnowledgeGraph (via serialize_to_json round-trip)
//
// Returns the OWNED KnowledgeGraph.  Callers construct the reader (which borrows &graph)
// after this call — lifetime discipline: reader cannot outlive the owned graph the caller holds.
//
// `[≠] U026-ZEPKEY`: ZEP guard KEPT — matches source (`if not Config.ZEP_API_KEY: 500
//   api.zepApiKeyMissing`) AND U-025 precedent in get_graph_data/delete_graph.  The
//   architecture doc floated removing it as a [≠]-superset, but U-025 KEPT it — consistency.
// ---------------------------------------------------------------------------

/// Resolve a `graph_id` to an owned `KnowledgeGraph`, applying the ZEP guard and all
/// task-resolution failure modes as 500 ApiErrors.
///
/// Used by: `get_graph_entities`, `get_entity_detail`, `get_entities_by_type`,
/// and (U-027 reuse) `crate::api::report`'s tools/chat/generate handlers.
/// `pub(crate)` so report.rs reuses this exact helper rather than duplicating it
/// (no-downgrade: one graph-resolution path, one ZEP guard).
pub(crate) async fn load_entity_reader_graph(
    state: &ApiState,
    graph_id: &str,
) -> Result<KnowledgeGraph, ApiError> {
    // Step 1: ZEP guard — KEEP IT (matches Python source + U-025 precedent)
    // `[≠] U026-ZEPKEY`: guard KEPT; teri config carries zep_api_key.
    if state.config.zep_api_key.as_deref().unwrap_or("").is_empty() {
        return Err(ApiError::client(
            StatusCode::INTERNAL_SERVER_ERROR,
            crate::i18n::t("api.zepApiKeyMissing"),
        ));
    }

    // Step 2: Resolve graph_id → task → result["graph"]
    let task = crate::task::TaskManager::global()
        .get_task(graph_id)
        .ok_or_else(|| ApiError::server("graph not found: task not found for graph_id"))?;

    let result = task
        .result
        .as_ref()
        .ok_or_else(|| ApiError::server("graph not found: task has no result yet"))?;

    let graph_json = result
        .get("graph")
        .ok_or_else(|| ApiError::server("graph not found: task result missing 'graph' key"))?;

    // Step 3: Reconstruct KnowledgeGraph via JSON round-trip.
    // graph_json is a serde_json::Value embedded in the task result; re-stringify and deserialize.
    let graph_str = serde_json::to_string(graph_json)
        .map_err(|e| ApiError::server(format!("failed to re-serialize graph JSON: {e}")))?;

    let graph = KnowledgeGraph::deserialize_from_json(&graph_str)
        .map_err(|e| ApiError::server(format!("failed to deserialize graph: {e}")))?;

    Ok(graph)
}

// ---------------------------------------------------------------------------
// Sub-cycle (b) Route 1 — GET /entities/:graph_id  (simulation.py:48-90)
//
// Source steps:
//   1. ZEP guard — 500 api.zepApiKeyMissing
//   2. entity_types_str = request.args.get('entity_types', '')
//      entity_types = [t.strip() for t in s.split(',') if t.strip()] if s else None
//   3. enrich = request.args.get('enrich','true').lower() == 'true'
//   4. reader = ZepEntityReader(); result = reader.filter_defined_entities(graph_id, types, enrich)
//   5. return {"success": True, "data": result.to_dict()}
//
// teri MAP: ZepEntityReader() → load_entity_reader_graph + KnowledgeGraphEntityReader::new(&graph)
// `[≠] U025-TRACEBACK`: outer except → ApiError::server (3-key 500 shape).
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_graph_entities` (simulation.py:48-90).
async fn get_graph_entities(
    State(state): State<Arc<ApiState>>,
    Path(graph_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    // Steps 1-3 shared prologue via helper (ZEP guard + graph-load)
    let graph = load_entity_reader_graph(&state, &graph_id).await?;

    // Step 2: entity_types query param (simulation.py:66-67)
    // Split on ',', strip each segment, drop empties; None if param absent/empty.
    let entity_types: Option<Vec<String>> = {
        let s = params.get("entity_types").map(|s| s.as_str()).unwrap_or("");
        if s.is_empty() {
            None
        } else {
            let v: Vec<String> =
                s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
            if v.is_empty() { None } else { Some(v) }
        }
    };

    // Step 3: enrich param (simulation.py:68) — default "true"; false iff value != "true" (case-insensitive)
    let enrich = params.get("enrich").map(|s| s.to_lowercase() == "true").unwrap_or(true);

    // Step 4: filter entities
    let reader = crate::services::entity_reader::KnowledgeGraphEntityReader::new(&graph);
    let result = reader.filter_defined_entities(entity_types.as_deref(), enrich);

    // Step 5: return success envelope
    Ok(Json(serde_json::json!({
        "success": true,
        "data": result.to_dict()
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (b) Route 2 — GET /entities/:graph_id/:entity_uuid  (simulation.py:93-123)
//
// Source steps:
//   1. ZEP guard — 500 api.zepApiKeyMissing
//   2. reader = ZepEntityReader(); entity = reader.get_entity_with_context(graph_id, entity_uuid)
//   3. None → 404 api.entityNotFound {id: entity_uuid}
//   4. Some → {"success": True, "data": entity.to_dict()}
//
// `[≠] U025-TRACEBACK`: outer except → ApiError::server (3-key 500 shape).
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_entity_detail` (simulation.py:93-123).
async fn get_entity_detail(
    State(state): State<Arc<ApiState>>,
    Path((graph_id, entity_uuid)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    // Shared prologue (ZEP guard + graph-load)
    let graph = load_entity_reader_graph(&state, &graph_id).await?;

    // Step 2: get entity with context
    let reader = crate::services::entity_reader::KnowledgeGraphEntityReader::new(&graph);
    let entity = reader.get_entity_with_context(&entity_uuid);

    // Step 3: None → 404
    match entity {
        None => Err(ApiError::client(
            StatusCode::NOT_FOUND,
            crate::i18n::t_args("api.entityNotFound", &[("id", &entity_uuid)]),
        )),
        // Step 4: found → success envelope
        Some(e) => Ok(Json(serde_json::json!({
            "success": true,
            "data": e.to_dict()
        }))),
    }
}

// ---------------------------------------------------------------------------
// Sub-cycle (b) Route 3 — GET /entities/:graph_id/by-type/:entity_type  (simulation.py:126-160)
//
// Source steps:
//   1. ZEP guard — 500 api.zepApiKeyMissing
//   2. enrich = request.args.get('enrich','true').lower() == 'true'
//   3. reader = ZepEntityReader(); entities = reader.get_entities_by_type(graph_id, type, enrich)
//   4. return {"success": True, "data": {"entity_type": type, "count": len(entities),
//                                        "entities": [e.to_dict() for e in entities]}}
//
// KEY ORDER inside data: entity_type, count, entities — Python dict insertion order preserved.
// `[≠] U025-TRACEBACK`: outer except → ApiError::server (3-key 500 shape).
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_entities_by_type` (simulation.py:126-160).
async fn get_entities_by_type(
    State(state): State<Arc<ApiState>>,
    Path((graph_id, entity_type)): Path<(String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    // Shared prologue (ZEP guard + graph-load)
    let graph = load_entity_reader_graph(&state, &graph_id).await?;

    // Step 2: enrich param (same parse rule as Route 1)
    let enrich = params.get("enrich").map(|s| s.to_lowercase() == "true").unwrap_or(true);

    // Step 3: get entities by type
    let reader = crate::services::entity_reader::KnowledgeGraphEntityReader::new(&graph);
    let entities = reader.get_entities_by_type(&entity_type, enrich);

    // Step 4: return success envelope with key order: entity_type, count, entities
    // (Python dict insertion order; preserve_order serde).
    let count = entities.len();
    let entities_data: Vec<Value> = entities.iter().map(|e| e.to_dict()).collect();

    // Use IndexMap-backed json! to preserve key order: entity_type → count → entities
    let mut data = serde_json::Map::new();
    data.insert("entity_type".to_string(), serde_json::json!(entity_type));
    data.insert("count".to_string(), serde_json::json!(count));
    data.insert("entities".to_string(), serde_json::json!(entities_data));

    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(data)
    })))
}

// ---------------------------------------------------------------------------
// Route 1 — POST /create  (simulation.py:165-237)
//
// Source steps:
//   1. data = request.get_json() or {}
//   2. project_id from body; empty/missing → 400 api.requireProjectId
//   3. ProjectManager.get_project(project_id); None → 404 api.projectNotFound
//   4. graph_id = body.graph_id or project.graph_id; still empty → 400 api.graphNotBuilt
//   5. enable_twitter = data.get('enable_twitter', True)
//   6. enable_reddit  = data.get('enable_reddit',  True)
//   7. manager.create_simulation(project_id, graph_id, enable_twitter, enable_reddit)
//   8. return {"success": True, "data": state.to_dict()}
//
// Note: Python builds a fresh `SimulationManager()` per request; teri uses the shared
// `state.sim_manager` Arc (DECISION-U026-1). Both target the same FS-backed state.json files.
//
// `[≠] U025-TRACEBACK`: outer except → ApiError::server (3-key 500 shape).
// ---------------------------------------------------------------------------

/// Port of MiroFish `create_simulation` (simulation.py:165-237).
///
/// Tolerate absent/empty body like Python's `request.get_json() or {}`.
async fn create_simulation(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: Parse body (simulation.py:195) — tolerate absent/empty body → {}
    let data = body.map(|j| j.0).unwrap_or_else(|| serde_json::json!({}));

    // Step 2: project_id required (simulation.py:197-202)
    let project_id = data.get("project_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if project_id.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.requireProjectId"),
        ));
    }

    // Step 3: Project lookup (simulation.py:204-209)
    let pm = crate::models::project::ProjectManager::from_config(&state.config);
    let project = pm.get_project(&project_id).map_err(ApiError::server)?;
    let project = match project {
        None => {
            return Err(ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.projectNotFound", &[("id", &project_id)]),
            ));
        }
        Some(p) => p,
    };

    // Step 4: Resolve graph_id (simulation.py:211-215)
    // body.graph_id (if non-empty) else project.graph_id; still empty → 400
    let graph_id = data
        .get("graph_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| project.graph_id.clone().unwrap_or_default());

    if graph_id.is_empty() {
        return Err(ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.graphNotBuilt")));
    }

    // Steps 5–6: enable_twitter / enable_reddit (simulation.py:217-221) — default true
    let enable_twitter = data.get("enable_twitter").and_then(|v| v.as_bool()).unwrap_or(true);
    let enable_reddit = data.get("enable_reddit").and_then(|v| v.as_bool()).unwrap_or(true);

    // Step 7: create simulation (simulation.py:222-225)
    // DECISION-U026-1: use in-state Arc<SimulationManager>, NOT a fresh SimulationManager().
    let sim_state = state
        .sim_manager
        .create_simulation(&project_id, &graph_id, enable_twitter, enable_reddit)
        .map_err(ApiError::server)?;

    // Step 8: return success envelope (simulation.py:227-229)
    Ok(Json(serde_json::json!({
        "success": true,
        "data": sim_state.to_dict()
    })))
}

// ---------------------------------------------------------------------------
// Route 2 — GET /:simulation_id  (simulation.py:755-785)
//
// Source steps:
//   1. manager.get_simulation(simulation_id); None → 404 api.simulationNotFound
//   2. result = state.to_dict()
//   3. if state.status == SimulationStatus.READY:
//          result["run_instructions"] = manager.get_run_instructions(simulation_id)
//   4. return {"success": True, "data": result}
//
// DECISION-U026-2: step 3 now calls `.to_dict()` on the returned `RunInstructions`,
// emitting native HTTP-start guidance instead of inexpressible Python-script strings.
//
// Note: Python uses a fresh `SimulationManager()`; teri uses `state.sim_manager` Arc
// (DECISION-U026-1).  Same FS-backed state.json — no behavioral difference.
//
// `[≠] U025-TRACEBACK`: outer except → ApiError::server (3-key 500 shape).
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_simulation` (simulation.py:755-785).
async fn get_simulation(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: load simulation state (simulation.py:760-766)
    let sim_state = state.sim_manager.get_simulation(&simulation_id).map_err(ApiError::server)?;

    let sim_state = match sim_state {
        None => {
            return Err(ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.simulationNotFound", &[("id", &simulation_id)]),
            ));
        }
        Some(s) => s,
    };

    // Step 2: base result dict (simulation.py:768)
    let mut result = sim_state.to_dict();

    // Step 3: READY gate — attach run_instructions (simulation.py:771-772)
    // DECISION-U026-2: run_instructions carries NATIVE teri HTTP-start guidance.
    if sim_state.status == SimulationStatus::Ready {
        let run_instr = state
            .sim_manager
            .get_run_instructions(&simulation_id)
            .map_err(ApiError::server)?;
        // Insert into the existing dict object (to_dict() returns Value::Object).
        if let Value::Object(ref mut map) = result {
            map.insert("run_instructions".to_string(), run_instr.to_dict());
        }
    }

    // Step 4: return success envelope (simulation.py:774-777)
    Ok(Json(serde_json::json!({
        "success": true,
        "data": result
    })))
}

// ---------------------------------------------------------------------------
// Route 3 — GET /list  (simulation.py:788-814)
//
// Source steps:
//   1. project_id = request.args.get('project_id')  — optional, no default
//   2. manager.list_simulations(project_id=project_id)
//   3. return {"success": True, "data": [s.to_dict() for s in simulations], "count": len(...)}
//
// NOTE: The Python source reads ONLY `project_id` (no `?limit`). Port exactly what the
// source does; `?limit` is NOT added.
//
// Note: Python uses a fresh `SimulationManager()`; teri uses `state.sim_manager` Arc
// (DECISION-U026-1). Same FS-backed state.json — no behavioral difference.
//
// `[≠] U025-TRACEBACK`: outer except → ApiError::server (3-key 500 shape).
// ---------------------------------------------------------------------------

/// Port of MiroFish `list_simulations` (simulation.py:788-814).
async fn list_simulations(
    State(state): State<Arc<ApiState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: optional project_id filter (simulation.py:797)
    let project_id = params.get("project_id").cloned();

    // Step 2: list simulations (simulation.py:799-800)
    // DECISION-U026-1: use in-state Arc<SimulationManager>, NOT a fresh SimulationManager().
    let simulations = state
        .sim_manager
        .list_simulations(project_id.as_deref())
        .map_err(ApiError::server)?;

    // Step 3: return success envelope (simulation.py:802-806)
    // Key order: success, data, count (matches Python source).
    let data: Vec<Value> = simulations.iter().map(|s| s.to_dict()).collect();
    let count = data.len();

    Ok(Json(serde_json::json!({
        "success": true,
        "data": data,
        "count": count
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (m) — GET /history  (simulation.py:876-987)
//
// Source: history list for the home page, each simulation enriched with project
// details, run-state, file list, and report linkage.
//
// Source steps (simulation.py:911-979):
//   1. limit = request.args.get('limit', 20, type=int)
//   2. manager.list_simulations()[:limit]   — NO project_id filter (all sims)
//   3. for each sim:
//        sim_dict = sim.to_dict()                            # 17 keys
//        config = manager.get_simulation_config(sim_id)
//        if config:
//          sim_dict["simulation_requirement"] = config.get("simulation_requirement", "")
//          time_config = config.get("time_config", {})
//          sim_dict["total_simulation_hours"] = time_config.get("total_simulation_hours", 0)
//          recommended_rounds = int(tsh * 60 / max(minutes_per_round||60, 1))
//        else:
//          sim_dict["simulation_requirement"] = ""; total_simulation_hours = 0; recommended_rounds = 0
//        run_state = SimulationRunner.get_run_state(sim_id)
//        if run_state:
//          current_round = run_state.current_round
//          runner_status = run_state.runner_status.value
//          total_rounds = run_state.total_rounds if > 0 else recommended_rounds
//        else:
//          current_round = 0; runner_status = "idle"; total_rounds = recommended_rounds
//        project = ProjectManager.get_project(sim.project_id)
//        files = [{"filename": f.get("filename","未知文件")} for f in project.files[:3]] or []
//        report_id = _get_report_id_for_simulation(sim_id)   # NEWEST matching report or None
//        version = "v1.0.2"
//        created_date = sim_dict["created_at"][:10]
//   4. return {success, data:[...], count}
//
// Key order per enriched sim (Python dict insertion order; preserve_order byte-observable):
//   [17 base to_dict keys, with current_round UPDATED in place at pos 12],
//   simulation_requirement, total_simulation_hours, runner_status, total_rounds,
//   files, report_id, version, created_date.
//   (current_round is re-inserted: IndexMap keeps its original position, updates value —
//    matches Python's `dict[existing_key] = v` semantics.)
//
// `_get_report_id_for_simulation` faithfulness (simulation.py:817-873): Python collects ALL
// reports whose meta.json `simulation_id` matches, sorts by `created_at` DESC, returns the
// NEWEST `report_id` (or None). teri's `ReportManager::get_report_by_simulation` returns the
// FIRST match in fs-iteration order — NOT newest — so it would diverge when a sim has multiple
// reports. We instead use `list_reports(Some(sim_id), 1)` which sorts `created_at` DESC (same
// stable-sort as Python) and take the first → byte-faithful "newest report_id".
//
// `[~] U026-m-NEGLIMIT`: `?limit` parsed as usize (negative/non-numeric → default 20), same
//   U-025 precedent as the other limit routes. Python `type=int` would accept a negative and
//   slice `[:-n]`; that edge is non-contractual and consistent with prior sub-cycles.
// `[!] U026-m-LIVEDATA`: run-state/config/report enrichment all read real on-disk state; with
//   no live producer the values are the faithful empty-run snapshot (idle/0/recommended/None).
//   Flips to richer values automatically when producers (U-028/029/030) land — same read path.
// `[≠] U025-TRACEBACK`: outer error → ApiError::server (3-key 500 shape).
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_simulation_history` (simulation.py:876-987).
async fn get_simulation_history(
    State(state): State<Arc<ApiState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    use serde_json::Map;

    // Step 1: limit (simulation.py:912). type=int default 20; usize parse (U-025 precedent).
    let limit = params.get("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(20);

    // Step 2: list ALL simulations (no project_id filter), take [:limit] (simulation.py:914-915).
    let simulations = state.sim_manager.list_simulations(None).map_err(ApiError::server)?;

    // ProjectManager + ReportManager (constructed per Python: fresh instances, FS-backed).
    let pm = crate::models::project::ProjectManager::from_config(&state.config);
    let rm = crate::report::manager::ReportManager::new(&state.config.upload_folder);

    let mut enriched: Vec<Value> = Vec::new();

    // Step 3: enrich each sim (simulation.py:918-973). [:limit] applied via take().
    for sim in simulations.into_iter().take(limit) {
        // Base 17-key dict (simulation.py:920).
        let mut sim_dict: Map<String, Value> = match sim.to_dict() {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let sim_id = sim.simulation_id.clone();

        // ---- config block (simulation.py:923-936) ----
        let config = state.sim_manager.get_simulation_config(&sim_id).map_err(ApiError::server)?;
        let recommended_rounds: i64;
        if let Some(cfg) = config.as_ref() {
            // simulation_requirement: config.get("simulation_requirement", "")
            let req = cfg
                .get("simulation_requirement")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            sim_dict.insert("simulation_requirement".to_string(), Value::String(req));

            // time_config = config.get("time_config", {})
            let empty = Value::Object(Map::new());
            let time_config = cfg.get("time_config").unwrap_or(&empty);

            // total_simulation_hours = time_config.get("total_simulation_hours", 0) — RAW value
            let tsh_val = time_config
                .get("total_simulation_hours")
                .cloned()
                .unwrap_or(Value::Number(0.into()));
            // recommended_rounds = int(tsh * 60 / max(minutes_per_round||60, 1))
            let tsh_num = tsh_val.as_f64().unwrap_or(0.0);
            let mpr = time_config.get("minutes_per_round").and_then(|v| v.as_f64()).unwrap_or(60.0);
            recommended_rounds = (tsh_num * 60.0 / mpr.max(1.0)).trunc() as i64;

            sim_dict.insert("total_simulation_hours".to_string(), tsh_val);
        } else {
            sim_dict.insert("simulation_requirement".to_string(), Value::String(String::new()));
            sim_dict.insert("total_simulation_hours".to_string(), Value::Number(0.into()));
            recommended_rounds = 0;
        }

        // ---- run-state block (simulation.py:939-948) ----
        let run_state = state.sim_runner.get_run_state(&sim_id).await.map_err(ApiError::server)?;
        if let Some(rs) = run_state {
            // current_round UPDATES the existing key (stays at position 12).
            sim_dict.insert("current_round".to_string(), Value::Number(rs.current_round.into()));
            sim_dict.insert(
                "runner_status".to_string(),
                Value::String(rs.runner_status.as_str().to_string()),
            );
            let total_rounds =
                if rs.total_rounds > 0 { rs.total_rounds } else { recommended_rounds };
            sim_dict.insert("total_rounds".to_string(), Value::Number(total_rounds.into()));
        } else {
            sim_dict.insert("current_round".to_string(), Value::Number(0.into()));
            sim_dict.insert("runner_status".to_string(), Value::String("idle".to_string()));
            sim_dict.insert("total_rounds".to_string(), Value::Number(recommended_rounds.into()));
        }

        // ---- files block (simulation.py:951-958) ----
        // project.files[:3] → [{"filename": f.get("filename","未知文件")}]; else [].
        let project = pm.get_project(&sim.project_id).map_err(ApiError::server)?;
        let files: Vec<Value> = match project {
            Some(p) if !p.files.is_empty() => p
                .files
                .iter()
                .take(3)
                .map(|f| {
                    let filename = f.get("filename").and_then(|v| v.as_str()).unwrap_or("未知文件");
                    let mut fm = Map::with_capacity(1);
                    fm.insert("filename".to_string(), Value::String(filename.to_string()));
                    Value::Object(fm)
                })
                .collect(),
            _ => Vec::new(),
        };
        sim_dict.insert("files".to_string(), Value::Array(files));

        // ---- report_id (simulation.py:961, helper :817-873) ----
        // Faithful "newest report_id by created_at DESC", or null.
        let report_id = rm
            .list_reports(Some(&sim_id), 1)
            .first()
            .map(|r| Value::String(r.report_id.clone()))
            .unwrap_or(Value::Null);
        sim_dict.insert("report_id".to_string(), report_id);

        // ---- version (simulation.py:964) ----
        sim_dict.insert("version".to_string(), Value::String("v1.0.2".to_string()));

        // ---- created_date = created_at[:10] (simulation.py:967-971) ----
        // Python slices the first 10 chars (char-safe; ISO dates are ASCII). Empty if absent.
        let created_date = sim_dict
            .get("created_at")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(10).collect::<String>())
            .unwrap_or_default();
        sim_dict.insert("created_date".to_string(), Value::String(created_date));

        enriched.push(Value::Object(sim_dict));
    }

    // Step 4: success envelope (simulation.py:975-979). Key order: success, data, count.
    let count = enriched.len();
    Ok(Json(serde_json::json!({
        "success": true,
        "data": enriched,
        "count": count
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (d) Route 1 — POST /prepare  (simulation.py:359-639)
//
// Async LLM-driven prepare. Returns task_id immediately; the background OS thread
// (spawn_prepare_simulation, DECISION-U026-d-1-REVISED) runs the 3-stage pipeline and
// updates the task, polled via POST /prepare/status.
//
// Flow (architecture findings/u026-d-architecture.md §2):
//   1. simulation_id (400 requireSimulationId)
//   2. get_simulation None → 404 simulationNotFound
//   3. force_regenerate (default false)
//   4. if !force: check_simulation_prepared → if prepared → 200 ready/already_prepared
//   5. get_project None → 404 projectNotFound
//   6. simulation_requirement empty → 400 projectMissingRequirement
//   7. document_text = get_extracted_text or ""
//   8. entity_types / use_llm_for_profiles(true) / parallel_profile_count(5)
//   9. SYNC entity-count preview (best-effort): resolve graph → filter_defined_entities(enrich=false)
//  10. create_task("simulation_prepare", {simulation_id, project_id})
//  11. status=PREPARING + entities_count/entity_types → save
//  12. spawn worker (graph moved in; on graph-resolve failure use empty graph → worker FAILED)
//  13. 200 {success, data:{simulation_id, task_id, status:preparing, message, already_prepared:false,
//          expected_entities_count, entity_types}}
//
// `[!] U026-d-GRAPHREQ`: teri's prepare_simulation takes `graph:&KnowledgeGraph` as a REQUIRED
//   input (no live Zep). If graph resolution fails (ZEP guard / no graph_build task), the route
//   STILL creates the task + spawns with an empty graph → the worker reads 0 entities → FAILED
//   terminal (faithful to Python's zero-entities→FAILED path). The failure is observable via
//   /prepare/status as a failed task, NOT a route 500. A green end-to-end happy path requires a
//   seeded graph_build task (same producer dep (b)/(f) accept).
// `[~] U026-d-STAGE4`: copying_scripts band dead in teri (kept for total_stages=4 fidelity).
// `[≠] U026-ZEPKEY` (graph-resolve guard kept), `[≠] U025-TRACEBACK` (500 omits live traceback).
// ---------------------------------------------------------------------------

/// Port of MiroFish `prepare_simulation` route (simulation.py:359-639).
async fn prepare_simulation_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let body = body.map(|j| j.0).unwrap_or_else(|| serde_json::json!({}));

    // Step 1: simulation_id (simulation.py:408-413).
    let simulation_id = match body.get("simulation_id").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Err(ApiError::client(
                StatusCode::BAD_REQUEST,
                crate::i18n::t("api.requireSimulationId"),
            ));
        }
    };

    // Step 2: get_simulation None → 404 (simulation.py:415-422). DECISION-U026-1: in-state Arc.
    let mut sim_state =
        match state.sim_manager.get_simulation(&simulation_id).map_err(ApiError::server)? {
            Some(s) => s,
            None => {
                return Err(ApiError::client(
                    StatusCode::NOT_FOUND,
                    crate::i18n::t_args("api.simulationNotFound", &[("id", &simulation_id)]),
                ));
            }
        };

    // Step 3-4: already-prepared short-circuit (simulation.py:425-444).
    let force_regenerate = body.get("force_regenerate").and_then(|v| v.as_bool()).unwrap_or(false);
    if !force_regenerate {
        let (is_prepared, prepare_info) = check_simulation_prepared(&state.config, &simulation_id);
        if is_prepared {
            return Ok(Json(serde_json::json!({
                "success": true,
                "data": {
                    "simulation_id": simulation_id,
                    "status": "ready",
                    "message": crate::i18n::t("api.alreadyPrepared"),
                    "already_prepared": true,
                    "prepare_info": prepare_info
                }
            })));
        }
    }

    // Step 5: get_project None → 404 (simulation.py:449-454).
    let pm = crate::models::project::ProjectManager::from_config(&state.config);
    let project = match pm.get_project(&sim_state.project_id).map_err(ApiError::server)? {
        Some(p) => p,
        None => {
            return Err(ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.projectNotFound", &[("id", &sim_state.project_id)]),
            ));
        }
    };

    // Step 6: simulation_requirement empty → 400 (simulation.py:457-462).
    let simulation_requirement = project.simulation_requirement.clone().unwrap_or_default();
    if simulation_requirement.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.projectMissingRequirement"),
        ));
    }

    // Step 7: document_text (simulation.py:465).
    let document_text =
        pm.get_extracted_text(&sim_state.project_id).ok().flatten().unwrap_or_default();

    // Step 8: options (simulation.py:467-469).
    let entity_types: Option<Vec<String>> =
        body.get("entity_types").and_then(|v| v.as_array()).map(|arr| {
            arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>()
        });
    let use_llm_for_profiles =
        body.get("use_llm_for_profiles").and_then(|v| v.as_bool()).unwrap_or(true);
    let parallel_profile_count =
        body.get("parallel_profile_count").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    // Step 9: synchronous entity-count preview (best-effort, simulation.py:471-488).
    // Resolve the graph ONCE; reused as the owned input moved into the worker.
    // On resolution failure → warn + empty graph (worker reads 0 → FAILED; [!] U026-d-GRAPHREQ).
    let graph = match load_entity_reader_graph(&state, &sim_state.graph_id).await {
        Ok(g) => g,
        Err(_) => {
            tracing::warn!(
                "同步获取实体数量失败（将在后台任务中重试）: graph resolve failed for graph_id={}",
                sim_state.graph_id
            );
            crate::graph::KnowledgeGraph::new()
        }
    };
    {
        let reader = crate::services::entity_reader::KnowledgeGraphEntityReader::new(&graph);
        let preview = reader.filter_defined_entities(entity_types.as_deref(), false);
        sim_state.entities_count = preview.filtered_count;
        sim_state.entity_types = preview.entity_types.iter().cloned().collect();
    }

    // Step 10: create task (simulation.py:491-498).
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("simulation_id".to_string(), Value::String(simulation_id.clone()));
    metadata.insert("project_id".to_string(), Value::String(sim_state.project_id.clone()));
    let task_id =
        crate::task::TaskManager::global().create_task("simulation_prepare", Some(metadata));

    // Step 11: status=PREPARING + save (simulation.py:500-502). Persists entities_count/entity_types.
    sim_state.status = SimulationStatus::Preparing;
    state
        .sim_manager
        .save_simulation_state(&mut sim_state)
        .map_err(ApiError::server)?;

    // Capture response values before moving into the worker.
    let expected_entities_count = sim_state.entities_count;
    let preview_entity_types = sim_state.entity_types.clone();

    // Step 12: spawn the background prepare worker (simulation.py:504-612).
    let config_generator = crate::services::simulation_config::SimulationConfigGenerator::new(
        crate::api::build_llm(&state.config),
        state.config.llm.model.clone(),
        state.config.llm.base_url.clone(),
    );
    crate::services::simulation_manager::spawn_prepare_simulation(
        task_id.clone(),
        state.sim_manager.clone(),
        simulation_id.clone(),
        simulation_requirement,
        document_text,
        entity_types,
        use_llm_for_profiles,
        parallel_profile_count,
        crate::api::build_llm(&state.config),
        graph,
        config_generator,
    );

    // Step 13: immediate 200 response (simulation.py:614-625).
    Ok(Json(serde_json::json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "task_id": task_id,
            "status": "preparing",
            "message": crate::i18n::t("api.prepareStarted"),
            "already_prepared": false,
            "expected_entities_count": expected_entities_count,
            "entity_types": preview_entity_types
        }
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (d) Route 2 — POST /prepare/status  (simulation.py:642-752)
//
// Branch tree (architecture §3, evaluate in this order — Python control flow):
//   B1. simulation_id present + check_simulation_prepared → 200 ready/100/already_prepared:true
//   B2. task_id absent: (B2a) simulation_id present → 200 not_started/0; (B2b) neither → 400
//   B3. task_id present → get_task:
//        B3a. None: simulation_id+prepared → 200 ready/task_id/already_prepared; else 404 taskNotFound
//        B3b. Some(t): to_dict + already_prepared:false → 200
//
// The two check_simulation_prepared calls (B1 then B3a) are intentional + Python-faithful
// (the sim may finish between the two checks). `[≠] U025-TRACEBACK` on the outer 500.
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_prepare_status` route (simulation.py:642-752).
async fn prepare_status_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let body = body.map(|j| j.0).unwrap_or_else(|| serde_json::json!({}));

    let task_id = body.get("task_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    let simulation_id =
        body.get("simulation_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty());

    // B1: simulation_id present → check_simulation_prepared first (simulation.py:679-692).
    if let Some(sim_id) = simulation_id {
        let (is_prepared, prepare_info) = check_simulation_prepared(&state.config, sim_id);
        if is_prepared {
            return Ok(Json(serde_json::json!({
                "success": true,
                "data": {
                    "simulation_id": sim_id,
                    "status": "ready",
                    "progress": 100,
                    "message": crate::i18n::t("api.alreadyPrepared"),
                    "already_prepared": true,
                    "prepare_info": prepare_info
                }
            })));
        }
    }

    // B2: task_id absent (simulation.py:695-711).
    let task_id = match task_id {
        None => {
            if let Some(sim_id) = simulation_id {
                // B2a: simulation_id present but not prepared → not_started.
                return Ok(Json(serde_json::json!({
                    "success": true,
                    "data": {
                        "simulation_id": sim_id,
                        "status": "not_started",
                        "progress": 0,
                        "message": crate::i18n::t("api.notStartedPrepare"),
                        "already_prepared": false
                    }
                })));
            }
            // B2b: neither present → 400.
            return Err(ApiError::client(
                StatusCode::BAD_REQUEST,
                crate::i18n::t("api.requireTaskOrSimId"),
            ));
        }
        Some(t) => t,
    };

    // B3: task_id present → get_task (simulation.py:713-745).
    match crate::task::TaskManager::global().get_task(task_id) {
        None => {
            // B3a: task gone. If simulation_id present AND now prepared → 200 ready (2nd check).
            if let Some(sim_id) = simulation_id {
                let (is_prepared, prepare_info) = check_simulation_prepared(&state.config, sim_id);
                if is_prepared {
                    return Ok(Json(serde_json::json!({
                        "success": true,
                        "data": {
                            "simulation_id": sim_id,
                            "task_id": task_id,
                            "status": "ready",
                            "progress": 100,
                            "message": crate::i18n::t("api.taskCompletedPrepared"),
                            "already_prepared": true,
                            "prepare_info": prepare_info
                        }
                    })));
                }
            }
            Err(ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.taskNotFound", &[("id", &task_id)]),
            ))
        }
        Some(task) => {
            // B3b: task found → to_dict + already_prepared:false.
            let mut d = match task.to_dict() {
                Value::Object(m) => m,
                _ => serde_json::Map::new(),
            };
            d.insert("already_prepared".to_string(), Value::Bool(false));
            Ok(Json(serde_json::json!({ "success": true, "data": Value::Object(d) })))
        }
    }
}

// ---------------------------------------------------------------------------
// Sub-cycle (e) Route 1 — GET /:simulation_id/profiles  (simulation.py:990-1025)
//
// Source steps:
//   1. platform = request.args.get('platform', 'reddit')
//   2. manager.get_profiles(simulation_id, platform=platform)
//   3. ValueError → 404 {success:false, error}
//   4. other Exception → 500 {success:false, error, traceback}
//   5. Ok → 200 {success:true, data:{platform, count, profiles}}
//
// Key order in data: platform → count → profiles (Python dict insertion order).
//
// `[≠] U025-TRACEBACK`: outer error → ApiError::server (3-key 500 shape).
//
// Error discrimination: TeriError::Sim contains "not found" text (Python ValueError path).
// All TeriError variants use Display; the only Err from get_profiles is Sim (missing state)
// which maps 1:1 to Python's ValueError → 404.  Any other error (IO/JSON) maps → 500.
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_simulation_profiles` (simulation.py:990-1025).
async fn get_profiles(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: platform query param, default "reddit" (simulation.py:999)
    let platform = params.get("platform").map(|s| s.as_str()).unwrap_or("reddit").to_string();

    // Steps 2-4: call manager; map ValueError (TeriError::Sim) → 404, other → 500
    let profiles = match state.sim_manager.get_profiles(&simulation_id, &platform) {
        Ok(v) => v,
        Err(crate::error::TeriError::Sim(msg)) => {
            // Python: except ValueError as e: return jsonify({success:False, error:str(e)}), 404
            return Err(ApiError::client(StatusCode::NOT_FOUND, msg));
        }
        Err(e) => {
            // Python: except Exception as e: 500 with traceback
            return Err(ApiError::server(e));
        }
    };

    // Step 5: success envelope, key order: platform → count → profiles
    let count = profiles.len();
    let mut data = serde_json::Map::new();
    data.insert("platform".to_string(), serde_json::json!(platform));
    data.insert("count".to_string(), serde_json::json!(count));
    data.insert("profiles".to_string(), serde_json::json!(profiles));

    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(data)
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (e) Route 2 — GET /:simulation_id/profiles/realtime  (simulation.py:1028-1135)
//
// DIRECT FILE READ — bypasses SimulationManager entirely.
//
// Source steps:
//   1. platform = request.args.get('platform', 'reddit')
//   2. sim_dir = Config.OASIS_SIMULATION_DATA_DIR / simulation_id
//   3. sim_dir missing → 404 simulationNotFound {id}
//   4. profiles_file: reddit_profiles.json (reddit) | twitter_profiles.csv (other)
//   5. file_exists = exists(profiles_file); file_modified_at = None; profiles = []
//   6. if file_exists:
//        file_modified_at = mtime via python_isoformat_local_from
//        try { parse content (JSON array / CSV DictReader) }
//        except → log warn; profiles = []
//   7. is_generating = False; total_expected = None
//      if state.json exists: parse, status=="preparing" → is_generating; entities_count → total_expected
//      (state.json parse error → silently ignore, keep defaults)
//   8. 200: {success:true, data:{simulation_id, platform, count, total_expected, is_generating,
//                                 file_exists, file_modified_at, profiles}}
//   9. outer except → 500 traceback
//
// Key order in data: simulation_id, platform, count, total_expected, is_generating,
//                    file_exists, file_modified_at, profiles
//
// CSV: csv crate (available in Cargo.toml). Python csv.DictReader yields ordered string dicts;
// csv::Reader with headers does the same. Values are String (DictReader always yields str).
// Column order within each row matches Python (header order preserved by csv crate).
//
// `[≠] U026-MTIME`: file_modified_at uses python_isoformat_local_from (see project.rs).
// `[≠] U025-TRACEBACK`: outer error → ApiError::server.
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_simulation_profiles_realtime` (simulation.py:1028-1135).
async fn get_profiles_realtime(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: platform default "reddit"
    let platform = params.get("platform").map(|s| s.as_str()).unwrap_or("reddit").to_string();

    // Steps 2-3: sim_dir; missing → 404
    let sim_dir =
        std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&simulation_id);
    if !sim_dir.exists() {
        return Err(ApiError::client(
            StatusCode::NOT_FOUND,
            crate::i18n::t_args("api.simulationNotFound", &[("id", &simulation_id)]),
        ));
    }

    // Step 4: determine profiles file
    let profiles_file = if platform == "reddit" {
        sim_dir.join("reddit_profiles.json")
    } else {
        sim_dir.join("twitter_profiles.csv")
    };

    // Step 5: check existence
    let file_exists = profiles_file.exists();
    let mut profiles: Vec<Value> = Vec::new();
    let mut file_modified_at: Value = Value::Null;

    // Step 6: read + parse if file exists
    if file_exists {
        // mtime — [≠] U026-MTIME
        if let Ok(mtime) = std::fs::metadata(&profiles_file).and_then(|m| m.modified()) {
            file_modified_at =
                Value::String(crate::models::project::python_isoformat_local_from(mtime));
        }

        // Parse content; on any error log warn and keep profiles = []
        let parse_result: Result<Vec<Value>, Box<dyn std::error::Error>> = (|| {
            if platform == "reddit" {
                let raw = std::fs::read_to_string(&profiles_file)?;
                let parsed: Vec<Value> = serde_json::from_str(&raw)?;
                Ok(parsed)
            } else {
                // CSV: mirrors Python csv.DictReader semantics EXACTLY.
                //
                // DictReader is LENIENT (unlike csv::Reader default flexible=false):
                //   - short row (fewer fields than headers): missing trailing keys → null
                //   - long row (more fields than headers): surplus values collected into
                //     a JSON array under the key "null" (Python restkey=None, JSON-serialised
                //     by t()/jsonify as the string "null")
                //   - ragged/mid-write truncated file: each parseable row is still yielded;
                //     only a genuinely unreadable file (e.g. open error) → []
                //
                // Use flexible(true) so the csv crate does NOT hard-error on ragged rows.
                let mut rdr = csv::ReaderBuilder::new()
                    .flexible(true)
                    .has_headers(true)
                    .from_path(&profiles_file)?;
                let headers: Vec<String> = rdr.headers()?.iter().map(|s| s.to_string()).collect();
                let mut rows: Vec<Value> = Vec::new();
                for record in rdr.records() {
                    let record = record?;
                    let mut map = serde_json::Map::new();
                    let nfields = record.len();
                    let nheaders = headers.len();

                    // Zip header names with field values.
                    // Short row: iterate over available fields; remaining headers get null.
                    for (i, header) in headers.iter().enumerate() {
                        if i < nfields {
                            map.insert(header.clone(), Value::String(record[i].to_string()));
                        } else {
                            // Missing trailing field → null (DictReader default restval=None)
                            map.insert(header.clone(), Value::Null);
                        }
                    }

                    // Long row: surplus fields → array under key "null"
                    // (Python restkey=None; jsonify renders None key as "null" string)
                    if nfields > nheaders {
                        let surplus: Vec<Value> = (nheaders..nfields)
                            .map(|i| Value::String(record[i].to_string()))
                            .collect();
                        map.insert("null".to_string(), Value::Array(surplus));
                    }

                    rows.push(Value::Object(map));
                }
                Ok(rows)
            }
        })();

        match parse_result {
            Ok(v) => profiles = v,
            Err(e) => {
                // Python: logger.warning(f"读取 profiles 文件失败（可能正在写入中）: {e}")
                // profiles stays []
                tracing::warn!("Failed to read profiles file (may be in progress): {e}");
            }
        }
    }

    // Step 7: read state.json for is_generating / total_expected
    // silently ignore any read/parse errors (simulation.py:1112: `except Exception: pass`)
    let mut is_generating = false;
    let mut total_expected: Value = Value::Null;
    let state_file = sim_dir.join("state.json");
    if let Some(state_data) = std::fs::read_to_string(&state_file)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
    {
        let status = state_data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        is_generating = status == "preparing";
        // entities_count may be null; preserve null vs absent faithfully
        if let Some(ec) = state_data.get("entities_count") {
            total_expected = ec.clone();
        }
    }

    // Step 8: build response data — key order EXACTLY:
    // simulation_id, platform, count, total_expected, is_generating,
    // file_exists, file_modified_at, profiles
    let count = profiles.len();
    let mut data = serde_json::Map::new();
    data.insert("simulation_id".to_string(), Value::String(simulation_id));
    data.insert("platform".to_string(), Value::String(platform));
    data.insert("count".to_string(), serde_json::json!(count));
    data.insert("total_expected".to_string(), total_expected);
    data.insert("is_generating".to_string(), Value::Bool(is_generating));
    data.insert("file_exists".to_string(), Value::Bool(file_exists));
    data.insert("file_modified_at".to_string(), file_modified_at);
    data.insert("profiles".to_string(), Value::Array(profiles));

    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(data)
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (e) Route 3 — GET /:simulation_id/config/realtime  (simulation.py:1138-1255)
//
// DIRECT FILE READ.
//
// Source steps:
//   1. sim_dir = oasis_data_dir / simulation_id; missing → 404 simulationNotFound {id}
//   2. config_file = sim_dir / "simulation_config.json"
//   3. file_exists; file_modified_at = None; config = None
//   4. if file_exists:
//        file_modified_at = mtime; try { config = parse JSON } except → log warn; config = None
//   5. is_generating=False; generation_stage=None; config_generated=False
//      if state.json:
//        status; is_generating = status=="preparing"; config_generated = state["config_generated"] ?? False
//        if is_generating:
//          generation_stage = "generating_config" if profiles_generated else "generating_profiles"
//        elif status=="ready": generation_stage = "completed"
//        (silently ignore parse errors)
//   6. response_data key order: simulation_id, file_exists, file_modified_at, is_generating,
//                                generation_stage, config_generated, config
//   7. if config (non-null object): append summary key
//      summary: {total_agents: len(agent_configs??[]), simulation_hours: time_config??{}.total_simulation_hours,
//               initial_posts_count: len(event_config??{}.initial_posts??[]),
//               hot_topics_count: len(event_config??{}.hot_topics??[]),
//               has_twitter_config: "twitter_config" in config,
//               has_reddit_config: "reddit_config" in config,
//               generated_at: config.generated_at, llm_model: config.llm_model}
//   8. 200 {success:true, data:response_data}
//   9. outer except → 500 traceback
//
// `[≠] U026-MTIME`: mtime via python_isoformat_local_from.
// `[≠] U025-TRACEBACK`: outer error → ApiError::server.
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_simulation_config_realtime` (simulation.py:1138-1255).
async fn get_config_realtime(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: sim_dir; missing → 404
    let sim_dir =
        std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&simulation_id);
    if !sim_dir.exists() {
        return Err(ApiError::client(
            StatusCode::NOT_FOUND,
            crate::i18n::t_args("api.simulationNotFound", &[("id", &simulation_id)]),
        ));
    }

    // Step 2: config file path
    let config_file = sim_dir.join("simulation_config.json");

    // Step 3: check existence; default config=Null, file_modified_at=Null
    let file_exists = config_file.exists();
    let mut config: Value = Value::Null;
    let mut file_modified_at: Value = Value::Null;

    // Step 4: read config if file exists
    if file_exists {
        // mtime — [≠] U026-MTIME
        if let Ok(mtime) = std::fs::metadata(&config_file).and_then(|m| m.modified()) {
            file_modified_at =
                Value::String(crate::models::project::python_isoformat_local_from(mtime));
        }

        // Parse config JSON; on error → log warn, keep config = Null
        match std::fs::read_to_string(&config_file)
            .map_err(|e| e.to_string())
            .and_then(|raw| serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string()))
        {
            Ok(v) => config = v,
            Err(e) => {
                // Python: logger.warning(f"读取 config 文件失败（可能正在写入中）: {e}")
                tracing::warn!("Failed to read config file (may be in progress): {e}");
                // config stays Null
            }
        }
    }

    // Step 5: state.json for is_generating / generation_stage / config_generated
    // silently ignore any read/parse errors (simulation.py:1217: except Exception: pass)
    let mut is_generating = false;
    let mut generation_stage: Value = Value::Null;
    let mut config_generated = false;
    let state_file = sim_dir.join("state.json");
    if let Some(state_data) = std::fs::read_to_string(&state_file)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
    {
        let status = state_data.get("status").and_then(|v| v.as_str()).unwrap_or("");
        is_generating = status == "preparing";
        config_generated =
            state_data.get("config_generated").and_then(|v| v.as_bool()).unwrap_or(false);

        // Determine generation_stage (simulation.py:1210-1216)
        if is_generating {
            let profiles_generated =
                state_data.get("profiles_generated").and_then(|v| v.as_bool()).unwrap_or(false);
            generation_stage = if profiles_generated {
                Value::String("generating_config".to_string())
            } else {
                Value::String("generating_profiles".to_string())
            };
        } else if status == "ready" {
            generation_stage = Value::String("completed".to_string());
        }
        // else: generation_stage stays Null (simulation.py:1197: generation_stage = None)
    }

    // Step 6: build response_data key order EXACTLY:
    // simulation_id, file_exists, file_modified_at, is_generating,
    // generation_stage, config_generated, config
    let mut response_data = serde_json::Map::new();
    response_data.insert("simulation_id".to_string(), Value::String(simulation_id));
    response_data.insert("file_exists".to_string(), Value::Bool(file_exists));
    response_data.insert("file_modified_at".to_string(), file_modified_at);
    response_data.insert("is_generating".to_string(), Value::Bool(is_generating));
    response_data.insert("generation_stage".to_string(), generation_stage);
    response_data.insert("config_generated".to_string(), Value::Bool(config_generated));
    response_data.insert("config".to_string(), config.clone());

    // Step 7: if config is a non-null, non-empty object, append summary.
    // Python: `if config:` — an empty dict {} is falsy; only a non-empty object is truthy.
    // `config.is_object()` is true for {} too, so we must check non-empty explicitly.
    if config.as_object().is_some_and(|o| !o.is_empty()) {
        let agent_configs_len = config
            .get("agent_configs")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let simulation_hours = config
            .get("time_config")
            .and_then(|v| v.get("total_simulation_hours"))
            .cloned()
            .unwrap_or(Value::Null);

        let initial_posts_count = config
            .get("event_config")
            .and_then(|v| v.get("initial_posts"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let hot_topics_count = config
            .get("event_config")
            .and_then(|v| v.get("hot_topics"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let has_twitter_config = config.get("twitter_config").is_some();
        let has_reddit_config = config.get("reddit_config").is_some();

        let generated_at = config.get("generated_at").cloned().unwrap_or(Value::Null);
        let llm_model = config.get("llm_model").cloned().unwrap_or(Value::Null);

        // Key order mirrors Python insertion order in simulation.py:1233-1242
        let mut summary = serde_json::Map::new();
        summary.insert("total_agents".to_string(), serde_json::json!(agent_configs_len));
        summary.insert("simulation_hours".to_string(), simulation_hours);
        summary.insert("initial_posts_count".to_string(), serde_json::json!(initial_posts_count));
        summary.insert("hot_topics_count".to_string(), serde_json::json!(hot_topics_count));
        summary.insert("has_twitter_config".to_string(), Value::Bool(has_twitter_config));
        summary.insert("has_reddit_config".to_string(), Value::Bool(has_reddit_config));
        summary.insert("generated_at".to_string(), generated_at);
        summary.insert("llm_model".to_string(), llm_model);

        response_data.insert("summary".to_string(), Value::Object(summary));
    }

    // Step 8: return success envelope
    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(response_data)
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (e) Route 4 — GET /:simulation_id/config  (simulation.py:1258-1291)
//
// Source steps:
//   1. manager.get_simulation_config(simulation_id)
//   2. None → 404 {success:false, error: t('api.configNotFound')}
//   3. Some → 200 {success:true, data:config}
//   4. outer except → 500 traceback
//
// `[≠] U025-TRACEBACK`: outer error → ApiError::server.
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_simulation_config` (simulation.py:1258-1291).
async fn get_config(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: fetch config via manager (simulation.py:1272)
    let config = state
        .sim_manager
        .get_simulation_config(&simulation_id)
        .map_err(ApiError::server)?;

    // Step 2: None → 404 configNotFound (simulation.py:1274-1278)
    match config {
        None => Err(ApiError::client(StatusCode::NOT_FOUND, crate::i18n::t("api.configNotFound"))),
        // Step 3: Some → 200 {success:true, data:config}
        Some(cfg) => Ok(Json(serde_json::json!({
            "success": true,
            "data": cfg
        }))),
    }
}

// ---------------------------------------------------------------------------
// Sub-cycle (e) Route 5 — GET /:simulation_id/config/download  (simulation.py:1294-1320)
//
// Source steps:
//   1. manager._get_simulation_dir(simulation_id) (creates dir if needed)
//   2. config_path = sim_dir / "simulation_config.json"
//   3. config_path missing → 404 {success:false, error: t('api.configFileNotFound')}
//   4. file present → send_file(config_path, as_attachment=True,
//                               download_name="simulation_config.json")
//      = HTTP 200, Content-Type: application/json,
//        Content-Disposition: attachment; filename="simulation_config.json",
//        body = file bytes
//   5. outer except → 500 traceback
//
// This handler returns `Result<Response, ApiError>` (not `Json<Value>`) because the
// success path is a raw file response, not a JSON envelope.
//
// `[≠] U025-TRACEBACK`: outer error → ApiError::server.
// ---------------------------------------------------------------------------

/// Port of MiroFish `download_simulation_config` (simulation.py:1294-1320).
async fn download_config(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
) -> Result<Response, ApiError> {
    // Step 1: get_simulation_dir (pub(crate); creates dir if absent, mirrors Python)
    let sim_dir = state.sim_manager.get_simulation_dir(&simulation_id).map_err(ApiError::server)?;

    // Step 2: config path
    let config_path = sim_dir.join("simulation_config.json");

    // Step 3: missing → 404 configFileNotFound
    if !config_path.exists() {
        return Err(ApiError::client(
            StatusCode::NOT_FOUND,
            crate::i18n::t("api.configFileNotFound"),
        ));
    }

    // Step 4: read file bytes and build attachment response
    // Python: send_file(path, as_attachment=True, download_name="simulation_config.json")
    let bytes = std::fs::read(&config_path).map_err(ApiError::server)?;

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"simulation_config.json\""),
        )
        .body(axum::body::Body::from(Bytes::from(bytes)))
        .map_err(ApiError::server)?;

    Ok(resp)
}

// ---------------------------------------------------------------------------
// Sub-cycle (e2) — GET /script/:script_name/download  (simulation.py:1323-1372)
//
// Source steps:
//   1. scripts_dir = abspath(__file__/../../scripts)        — backend/scripts/
//   2. allowed_scripts = [run_twitter_simulation.py, run_reddit_simulation.py,
//                         run_parallel_simulation.py, action_logger.py]
//   3. script_name ∉ allowed → 400 {success:false, error: t('api.unknownScript',
//                                    name=script_name, allowed=allowed_scripts)}
//   4. script_path = scripts_dir / script_name
//   5. not os.path.exists(script_path) → 404 {success:false,
//                                    error: t('api.scriptFileNotFound', name=script_name)}
//   6. else send_file(script_path, as_attachment=True, download_name=script_name)
//   7. outer except → 500 traceback
//
// `[≠] U026-SCRIPTDL` (architect findings/u026-architecture.md:103/165/182; S-601):
//   MiroFish serves `backend/scripts/run_*.py` — the OASIS *subprocess* runner scripts.
//   teri's port is NATIVE in-process (DECISION-17: subprocess.Popen → tokio handles), so those
//   `run_*.py` files DO NOT EXIST and have NO native equivalent (the same architectural reason
//   `scripts_dir` was dropped from `get_run_instructions().to_dict()` in sub-cycle (c)).
//
//   Owner-decision (404 vs drop, architect flag at :165/:182): KEEP the route, return 404
//   `scriptFileNotFound` — never drop (no-downgrade law: a route in X with no equivalent in Y
//   is recorded, not silently removed) and never 200-empty (architect's explicit prohibition).
//   The full validation boundary ports VERBATIM: step 2 allowed-list, step 3 400 `unknownScript`
//   (with name + Python-`str(list)`-repr `allowed` interpolation) are byte-faithful and fully
//   testable.  Only step 6 (file bytes) is inexpressible; teri's scripts dir is conceptually
//   always empty, so a valid script name reaches exactly the 404 MiroFish itself returns when
//   `os.path.exists(script_path)` is False — same status, same `scriptFileNotFound` body shape.
// ---------------------------------------------------------------------------

/// The four runner scripts MiroFish's `backend/scripts/` exposes for download.  Kept verbatim so
/// the `unknownScript` validation contract (step 3) ports byte-for-byte.  teri does not ship these
/// (native in-process simulation — `[≠] U026-SCRIPTDL`), so a valid name resolves to 404.
const ALLOWED_SCRIPTS: [&str; 4] = [
    "run_twitter_simulation.py",
    "run_reddit_simulation.py",
    "run_parallel_simulation.py",
    "action_logger.py",
];

/// Port of MiroFish `download_simulation_script` (simulation.py:1323-1372).
async fn download_script(Path(script_name): Path<String>) -> Result<Response, ApiError> {
    // Step 3: validate against the allowed list → 400 unknownScript.
    // Python interpolates `allowed=allowed_scripts`, i.e. `str(list)` → the list's repr with
    // single-quoted items: `['run_twitter_simulation.py', 'run_reddit_simulation.py', ...]`.
    if !ALLOWED_SCRIPTS.contains(&script_name.as_str()) {
        let allowed_repr = format!(
            "[{}]",
            ALLOWED_SCRIPTS.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", ")
        );
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t_args(
                "api.unknownScript",
                &[("name", &script_name), ("allowed", &allowed_repr)],
            ),
        ));
    }

    // Steps 4-6: `[≠] U026-SCRIPTDL` — teri ships no `run_*.py` (native in-process simulation).
    // The scripts dir is conceptually always empty, so step 5's `os.path.exists` is always False:
    // a valid script name reaches the SAME 404 `scriptFileNotFound` MiroFish returns for an absent
    // file.  Never 200-empty, never drop the route.
    Err(ApiError::client(
        StatusCode::NOT_FOUND,
        crate::i18n::t_args("api.scriptFileNotFound", &[("name", &script_name)]),
    ))
}

// ---------------------------------------------------------------------------
// Sub-cycle (f) — POST /generate-profiles  (simulation.py:1377-1446)
//
// Source steps:
//   1. data = request.get_json() or {}   — tolerate absent/empty body
//   2. graph_id required → 400 api.requireGraphId
//   3. entity_types = data.get('entity_types')           — Option<Vec<String>>, may be null
//      use_llm  = data.get('use_llm', True)             — default true
//      platform = data.get('platform', 'reddit')         — default "reddit"
//   4. load_entity_reader_graph (ZEP guard + task→graph); build reader; filter_defined_entities
//   5. filtered.filtered_count == 0 → 400 api.noMatchingEntities
//   6. generate_profiles_from_entities(entities, use_llm, parallel_count=5, realtime=None)
//   7. format per platform: reddit→to_reddit_format, twitter→to_twitter_format, else→to_dict
//   8. 200 {success, data:{platform, entity_types(sorted for determinism), count, profiles}}
//   9. any unexpected error → 500 {success,error,traceback}
//
// Key order contract: platform → entity_types → count → profiles  (Python dict insertion order)
//
// `[≠] U026-ZEPKEY`: ZEP guard KEPT (from load_entity_reader_graph helper; same as sub-cycle b).
// `[≠] U025-TRACEBACK`: 500 body carries Rust backtrace string, not Python stack.
// `[~] U026-f-ENTITY_TYPES_ORDER`: Python `list(filtered.entity_types)` over a Python `set`
//   has NO order guarantee.  teri uses `HashSet<String>` — also unordered.  We sort the Vec
//   for determinism in the response and in tests (sort is a strict superset; the Python contract
//   is set-equality, not sequence-equality).  The parity verifier adjudicates.
// ---------------------------------------------------------------------------

/// Port of MiroFish `generate_profiles` (simulation.py:1377-1446).
///
/// Tolerates absent/empty body like Python's `request.get_json() or {}`.
async fn generate_profiles(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: parse body (simulation.py:1391) — tolerate missing/null body → {}
    let data = body.map(|j| j.0).unwrap_or_else(|| serde_json::json!({}));

    // Step 2: graph_id required (simulation.py:1393-1398)
    let graph_id = data.get("graph_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if graph_id.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.requireGraphId"),
        ));
    }

    // Step 3: optional params (simulation.py:1400-1402)
    // entity_types: null/absent → None (pass all entity types through)
    let entity_types: Option<Vec<String>> = data
        .get("entity_types")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect());
    // use_llm: default true
    let use_llm = data.get("use_llm").and_then(|v| v.as_bool()).unwrap_or(true);
    // platform: default "reddit"
    let platform = data.get("platform").and_then(|v| v.as_str()).unwrap_or("reddit").to_string();

    // Step 4: load graph (ZEP guard + task resolution) + build reader + filter entities
    // (simulation.py:1404-1415)
    //
    // Borrow lifetime discipline: `graph` is an owned `KnowledgeGraph`.  The reader borrows
    // `&graph` immutably, and `generate_profiles_from_entities` also borrows `Some(&graph)`
    // immutably — both are shared borrows, which is fine in Rust.  `filtered.entities` is
    // owned (Vec<EntityNode>), so it is independent of the reader's borrow once the reader
    // is dropped.  We drop the reader explicitly before the await point (it is a synchronous
    // value that implements no async; the borrow is stack-local and drops at the end of the
    // let-binding scope when we take filtered).
    let graph = load_entity_reader_graph(&state, &graph_id).await?;
    let filtered = {
        let reader = crate::services::entity_reader::KnowledgeGraphEntityReader::new(&graph);
        reader.filter_defined_entities(entity_types.as_deref(), true)
    }; // reader dropped here; `graph` still live below

    // Step 5: zero-match guard (simulation.py:1411-1415)
    if filtered.filtered_count == 0 {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.noMatchingEntities"),
        ));
    }

    // Step 6: generate profiles (simulation.py:1417-1421)
    //
    // Python: `generator = OasisProfileGenerator(); profiles = generator.generate_profiles_from_entities(entities, use_llm=use_llm)`
    // teri: construct PersonaGenerator + build_llm per-request (DECISION-U025-1: LlmClient
    // is not dyn-compatible so cannot live in ApiState; per-request construction matches
    // MiroFish's per-request `OasisProfileGenerator()`).
    //
    // `graph = Some(&graph)`: Python passes `self.graph_id` for Zep enrichment; in teri the
    // equivalent graph context is the deserialized KnowledgeGraph itself.  Reading the Rust
    // fn body (oasis_profile_export.rs:411-416), `graph` is used to call
    // `g.get_entity_by_id(id)` for context enrichment — i.e. it IS used and `Some` is correct.
    //
    // `parallel_count = 5`: matches Python's default (`parallel_count: int = 5`).
    // `realtime_output = None`: this endpoint returns profiles in the JSON response, no file.
    // `progress_callback = &mut |_,_,_| {}`: no-op; endpoint reports no progress.
    // Construct the generator and LLM per-request (DECISION-U025-1: LlmClient is not
    // dyn-compatible; same pattern as other handlers).
    //
    // Use `generate_profiles_no_cb` (the Send-compatible wrapper) because:
    //   - The endpoint returns profiles in the JSON response — no realtime file write needed.
    //   - The endpoint reports no progress — a no-op callback is correct.
    //   - `generate_profiles_from_entities(&mut dyn FnMut)` is `!Send` (dyn FnMut is ?Send),
    //     so it cannot be awaited in an axum handler future (which must be Send).
    //     `generate_profiles_no_cb` uses a concrete no-op closure (Send; empty capture)
    //     internally, making its future Send without touching the existing callers of
    //     `generate_profiles_from_entities` (simulation_manager.rs).
    let generator = crate::agent::PersonaGenerator::new();
    let llm = crate::api::build_llm(&state.config);
    let pairs = crate::services::oasis_profile_export::generate_profiles_no_cb(
        &generator,
        &llm,
        &filtered.entities,
        Some(&graph),
        use_llm,
        5,
    );

    // Step 7: format per platform (simulation.py:1423-1428)
    let profiles_data =
        crate::services::oasis_profile_export::format_profiles_for_platform(&pairs, &platform);

    // Step 8: 200 success envelope (simulation.py:1430-1438)
    //
    // Key order: platform → entity_types → count → profiles  (Python dict insertion order).
    //
    // `[~] U026-f-ENTITY_TYPES_ORDER`: sorted for response determinism; Python contract is
    // set-equality (Python `list(set)` order is arbitrary).
    let mut entity_types_vec: Vec<String> = filtered.entity_types.into_iter().collect();
    entity_types_vec.sort();
    let count = profiles_data.len();

    let mut data_obj = serde_json::Map::new();
    data_obj.insert("platform".to_string(), serde_json::json!(platform));
    data_obj.insert("entity_types".to_string(), serde_json::json!(entity_types_vec));
    data_obj.insert("count".to_string(), serde_json::json!(count));
    data_obj.insert("profiles".to_string(), serde_json::json!(profiles_data));

    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(data_obj)
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (g) helpers + handlers
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// g-helper: coerce_max_rounds (Python `int(max_rounds)` semantics)
//
// Python `int(max_rounds)` exact semantics (U026-g-MAXROUNDS-FLOAT fix):
//   - JSON integer number (e.g. 100)   → that i64 directly
//   - JSON float number (e.g. 5.7)     → truncate toward zero (int(5.7)=5, int(-2.9)=-2),
//                                         then apply ≤0 → maxRoundsPositive check.
//                                         (Python `int(float)` truncates, it does NOT round.)
//   - JSON string, strict integer only  → parse (int("5")==5, int("-3")==-3);
//                                         "5.7" → ValueError → maxRoundsInvalid
//                                         (Python `int("5.7")` raises; string path is strict.)
//   - JSON bool                         → DECISION: current code routes bool through `_ =>`
//                                         and returns maxRoundsInvalid.  Python `int(True)==1`
//                                         / `int(False)==0` would differ, but JSON bool for
//                                         max_rounds is a degenerate, non-contractual input.
//                                         We preserve the current rejection (non-contractual).
//   - null / absent                     → callers handle None before reaching here (no change).
//   - int ≤ 0 (after truncation)        → maxRoundsPositive
// ---------------------------------------------------------------------------

fn coerce_max_rounds(v: &Value) -> Result<i64, ApiError> {
    let n = match v {
        Value::Number(n) => {
            // JSON integer → direct
            if let Some(i) = n.as_i64() {
                i
            } else if let Some(f) = n.as_f64() {
                // JSON float → truncate toward zero, matching Python `int(float)` semantics.
                // Examples: int(5.7)=5, int(0.5)=0, int(-2.9)=-2
                // After truncation the ≤0 check below handles 0.5→0→maxRoundsPositive, etc.
                f.trunc() as i64
            } else {
                // u128 overflow or other exotic case → invalid
                return Err(ApiError::client(
                    StatusCode::BAD_REQUEST,
                    crate::i18n::t("api.maxRoundsInvalid"),
                ));
            }
        }
        Value::String(s) => {
            // Numeric string → strict integer parse (Python `int("5")` == 5).
            // Python `int("5.7")` raises ValueError → maxRoundsInvalid; same here.
            s.parse::<i64>().map_err(|_| {
                ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.maxRoundsInvalid"))
            })?
        }
        _ => {
            return Err(ApiError::client(
                StatusCode::BAD_REQUEST,
                crate::i18n::t("api.maxRoundsInvalid"),
            ));
        }
    };
    if n <= 0 {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.maxRoundsPositive"),
        ));
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// g-helper: check_simulation_prepared (Python `_check_simulation_prepared`, `:240-356`)
//
// Private fn — shared by /start (sub-cycle g2) and eventually /prepare (sub-cycle d).
//
// Returns `(is_prepared: bool, info: Value)`.
//
// Side effect: if `status == "preparing"` AND `config_generated` → auto-upgrades
// `state.json` on disk to `status="ready"`, updating `updated_at` to
// `python_isoformat_local()` (observable, tested). On write error: log warn, continue
// (Python `:333-334`).
// ---------------------------------------------------------------------------

pub(crate) fn check_simulation_prepared(
    config: &crate::Config,
    simulation_id: &str,
) -> (bool, Value) {
    let sim_dir = std::path::PathBuf::from(&config.oasis_simulation_data_dir).join(simulation_id);

    // Step 1: directory missing (Python :262-263)
    if !sim_dir.exists() {
        return (false, serde_json::json!({"reason": "模拟目录不存在"}));
    }

    // Step 2: required files check (Python :266-288)
    let required_files = [
        "state.json",
        "simulation_config.json",
        "reddit_profiles.json",
        "twitter_profiles.csv",
    ];
    let mut existing_files: Vec<&str> = Vec::new();
    let mut missing_files: Vec<&str> = Vec::new();
    for f in &required_files {
        if sim_dir.join(f).exists() {
            existing_files.push(f);
        } else {
            missing_files.push(f);
        }
    }
    if !missing_files.is_empty() {
        return (
            false,
            serde_json::json!({
                "reason": "缺少必要文件",
                "missing_files": missing_files,
                "existing_files": existing_files
            }),
        );
    }

    // Step 3: read + parse state.json (Python :291-296)
    let state_file = sim_dir.join("state.json");
    let raw = match std::fs::read_to_string(&state_file) {
        Ok(r) => r,
        Err(e) => {
            return (false, serde_json::json!({"reason": format!("读取状态文件失败: {e}")}));
        }
    };
    let mut state_data: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return (false, serde_json::json!({"reason": format!("读取状态文件失败: {e}")}));
        }
    };

    // Step 4: extract status + config_generated (Python :297-300)
    let status = state_data.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let config_generated =
        state_data.get("config_generated").and_then(|v| v.as_bool()).unwrap_or(false);

    // Step 5: prepared statuses (Python :311)
    let prepared_statuses = ["ready", "preparing", "running", "completed", "stopped", "failed"];
    if prepared_statuses.contains(&status.as_str()) && config_generated {
        // profiles_count (Python :316-321)
        let profiles_path = sim_dir.join("reddit_profiles.json");
        let profiles_count: u64 = std::fs::read_to_string(&profiles_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .and_then(|v| v.as_array().map(|a| a.len() as u64))
            .unwrap_or(0);

        // AUTO-UPGRADE: preparing → ready (Python :323-334, OBSERVABLE side effect)
        let mut effective_status = status.clone();
        if status == "preparing" {
            let now = python_isoformat_local();
            if let Some(obj) = state_data.as_object_mut() {
                obj.insert("status".to_string(), serde_json::json!("ready"));
                obj.insert("updated_at".to_string(), serde_json::json!(now));
            }
            match serde_json::to_string_pretty(&state_data) {
                Ok(json_str) => {
                    if let Err(e) = std::fs::write(&state_file, json_str.as_bytes()) {
                        // Python :333-334: catch, log warn, continue
                        tracing::warn!("自动更新状态失败: {e}");
                    } else {
                        effective_status = "ready".to_string();
                        tracing::info!("自动更新模拟状态: {} preparing -> ready", simulation_id);
                    }
                }
                Err(e) => {
                    tracing::warn!("自动更新状态失败: {e}");
                }
            }
        }

        // Build info dict, key order matching Python `:337-346`
        // entities_count, entity_types: from state_data (|| 0 / || [])
        let entities_count =
            state_data.get("entities_count").cloned().unwrap_or(serde_json::json!(0));
        let entity_types = state_data.get("entity_types").cloned().unwrap_or(serde_json::json!([]));
        let created_at = state_data.get("created_at").cloned().unwrap_or(Value::Null);
        let updated_at = state_data.get("updated_at").cloned().unwrap_or(Value::Null);

        // Use ordered Map to preserve key order (Python dict insertion order)
        let mut info = serde_json::Map::new();
        info.insert("status".to_string(), serde_json::json!(effective_status));
        info.insert("entities_count".to_string(), entities_count);
        info.insert("profiles_count".to_string(), serde_json::json!(profiles_count));
        info.insert("entity_types".to_string(), entity_types);
        info.insert("config_generated".to_string(), serde_json::json!(config_generated));
        info.insert("created_at".to_string(), created_at);
        info.insert("updated_at".to_string(), updated_at);
        info.insert("existing_files".to_string(), serde_json::json!(existing_files));

        (true, Value::Object(info))
    } else {
        // Not in prepared statuses or config_generated=false (Python :348-353)
        (
            false,
            serde_json::json!({
                "reason": format!(
                    "状态不在已准备列表中或config_generated为false: status={status}, config_generated={config_generated}"
                ),
                "status": status,
                "config_generated": config_generated
            }),
        )
    }
}

// ---------------------------------------------------------------------------
// g1 — POST /stop  (Python :1644-1700)
//
// Source steps:
//   1. data = request.get_json() or {}
//   2. simulation_id required → 400 requireSimulationId
//   3. runner.stop_simulation(id)  — ValueError(TeriError::Sim) → 400, Exception → 500
//   4. manager.get_simulation(id); if Some → status=Paused + save_simulation_state
//   5. return {success:true, data:run_state.to_dict()}
//
// Error mapping: TeriError::Sim → 400 (mirrors Python ValueError); all other errors → 500.
// `[≠] U025-TRACEBACK`: outer except → ApiError::server (3-key 500 shape).
// ---------------------------------------------------------------------------

/// Map a runner `Result` to `ApiError`: `TeriError::Sim` → 400, all else → 500.
///
/// Mirrors Python's `ValueError` → 400 / `Exception` → 500 two-tier error contract in
/// `/start` and `/stop` (simulation.py:1629-1641, :1688-1700).
fn map_runner_err(err: crate::error::TeriError) -> ApiError {
    match err {
        // Python `TimeoutError` → HTTP 504. Matched BEFORE the `Sim`/catch-all arms so a
        // timeout is never folded into a 400/500 (resolves `[≠] U026-k/l-TIMEOUT504`).
        crate::error::TeriError::Timeout(msg) => ApiError::client(StatusCode::GATEWAY_TIMEOUT, msg),
        crate::error::TeriError::Sim(msg) => ApiError::client(StatusCode::BAD_REQUEST, msg),
        other => ApiError::server(other),
    }
}

/// Error mapper for the interview routes — wraps a `TeriError::Timeout` in the route-specific
/// i18n key (Python `t('api.interviewTimeout', error=str(e))` → 504,
/// `simulation.py:2256-2260` / `:2394-2398` / `:2497-2501`), deferring every other error class
/// to [`map_runner_err`].
fn map_interview_err(err: crate::error::TeriError, timeout_key: &str) -> ApiError {
    match err {
        crate::error::TeriError::Timeout(msg) => ApiError::client(
            StatusCode::GATEWAY_TIMEOUT,
            crate::i18n::t_args(timeout_key, &[("error", &msg)]),
        ),
        other => map_runner_err(other),
    }
}

/// Port of MiroFish `stop_simulation` (simulation.py:1644-1700).
///
/// Body or {}; `simulation_id` required → 400 `requireSimulationId`.
/// `runner.stop_simulation(id)` — ValueError→400, Exception→500.
/// If simulation state found → status=Paused + save.
/// 200 `{success:true, data:run_state.to_dict()}`.
async fn stop_simulation(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: body or {} (Python :1665)
    let data = body.map(|Json(v)| v).unwrap_or_else(|| serde_json::json!({}));

    // Step 2: simulation_id required (Python :1667-1672)
    let id = data
        .get("simulation_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requireSimulationId"))
        })?;

    // Step 3: runner.stop_simulation(id) — ValueError→400, Exception→500 (Python :1674)
    let run_state = state.sim_runner.stop_simulation(id).await.map_err(map_runner_err)?;

    // Step 4: update simulation status → Paused + save (Python :1676-1681)
    if let Some(mut sim) = state.sim_manager.get_simulation(id).map_err(ApiError::server)? {
        sim.status = SimulationStatus::Paused;
        state.sim_manager.save_simulation_state(&mut sim).map_err(ApiError::server)?;
    }

    // Step 5: 200 response (Python :1683-1686)
    Ok(Json(serde_json::json!({
        "success": true,
        "data": run_state.to_dict()
    })))
}

// ---------------------------------------------------------------------------
// g2 — POST /start  (Python :1451-1641)
//
// Ports the FULL boundary + state-machine + helpers.
// ONE architect-sanctioned gap at the `RunInputs` construction point (GAP-U026-RUNINPUTS-BUILDER).
// Every 400/404/state-machine path is fully ported and testable now.
//
// Source steps (abbreviated):
//   1. body or {}; simulation_id required → 400 requireSimulationId
//   2. platform default "parallel"; max_rounds optional; enable_graph_memory_update default false;
//      force default false
//   3. platform ∉ {twitter,reddit,parallel} → 400 invalidPlatform
//   4. manager.get_simulation(id) → None → 404 simulationNotFound
//   5. force_restarted = false
//   6. if state.status != READY: check_simulation_prepared(id)
//      - is_prepared: Running+live-run: force→stop(warn-swallow) else→400 simRunningForceHint
//                    force→cleanup_simulation_logs+force_restarted=true
//                    status→Ready+save
//      - not prepared: 400 simNotReady{status}
//   7. graph_id resolution (when enable_graph_memory_update)
//      sim.graph_id || ProjectManager::get_project(project_id).graph_id → none→400 graphIdRequiredForMemory
//   8. [!] GAP-U026-RUNINPUTS-BUILDER — return 500 honest error
//      (no fabricated run_state, no MockLlm, no fake success)
// ---------------------------------------------------------------------------

/// Assemble the [`RunInputs`] for a **single-platform** run (U-028 c3b-ii) — the builder that
/// closes the engine + pool halves of `GAP-U026-RUNINPUTS-BUILDER`.
///
/// - `engine` = [`SimConfig::from_simulation_config`] (c1) → [`SimEngine`], with the time-based
///   activation policy (c3a) and the `actions.jsonl` producer (c3b-i) attached. The producer is
///   what the landed monitor (`spawn_monitor_task`) tails to mark the run COMPLETED.
/// - `pool` = [`load_agent_pool`](crate::services::oasis_profile_export::load_agent_pool) (c2).
/// - `graph` = the entity-reader graph when memory is enabled, else an empty graph (the engine
///   does not yet consume it — `SimEngine::run`'s `_graph` is reserved); `graph_for_updater` is
///   the `Arc<Mutex<_>>` the graph-memory updater writes into (U-021).
/// - `llm` = [`build_llm`](crate::api::build_llm).
///
/// **`platform="parallel"` (U-030 cycle B):** attaches a DUAL-logger producer
/// ([`PlatformLoggerSet::parallel`]) over the unioned twitter+reddit pool, so the unified
/// `SimEngine::run` fans boundary records to both loggers and routes each action to its platform
/// file — emitting `twitter/` AND `reddit/actions.jsonl` and satisfying the monitor's dual-platform
/// completion gate (S-615). Single-platform `twitter`/`reddit` attach a one-entry set (U-028 c3b-ii).
///
/// **Eager-vs-lazy seam (`[≠]` inherited from U-022 `RunInputs`):** Python's `/start` returns the
/// running state immediately and the *subprocess* later reads profiles/config; teri assembles the
/// pool + engine synchronously here, so a missing profile/config surfaces at `/start` rather than
/// mid-run. Unreachable in practice — the caller only reaches this after the READY-state gate
/// (`check_simulation_prepared`), which implies `/prepare` already wrote both artifacts.
async fn build_run_inputs(
    state: &Arc<ApiState>,
    simulation_id: &str,
    platform: &str,
    max_rounds: Option<i64>,
    enable_graph_memory_update: bool,
    graph_id: Option<&str>,
) -> Result<
    (
        RunInputs<crate::llm::OpenAiAdapter>,
        Option<Arc<tokio::sync::Mutex<KnowledgeGraph>>>,
    ),
    ApiError,
> {
    use crate::agent::Platform;
    use crate::sim::action_logger::PlatformActionLogger;
    use crate::sim::activation::TimeActivationPolicy;
    use crate::sim::{PlatformLoggerSet, RunProducer, SimConfig, SimEngine};

    // sim_dir (creates if absent, mirrors Python `_get_simulation_dir`).
    let sim_dir = state.sim_manager.get_simulation_dir(simulation_id).map_err(ApiError::server)?;

    // The prepared config artifact (READY ⟹ present). Match the runner's missing-config message
    // so the (unreachable-here) error path is consistent.
    let config = state
        .sim_manager
        .get_simulation_config(simulation_id)
        .map_err(ApiError::server)?
        .ok_or_else(|| {
            ApiError::client(
                StatusCode::BAD_REQUEST,
                "模拟配置不存在，请先调用 /prepare 接口".to_string(),
            )
        })?;

    // engine: config→ticks (c1) + activation gate (c3a) + actions.jsonl producer (c3b-i).
    // parallelism defaults to teri's convention (8); OASIS used a semaphore of 30 (architect §2).
    let mut engine = SimEngine::new(SimConfig::from_simulation_config(&config, max_rounds, 8));
    engine.with_activation(Arc::new(TimeActivationPolicy::from_config(&config, None)));
    // The actions.jsonl producer (U-028 c3b-i / U-030). `make_logger` builds a per-platform
    // `PlatformActionLogger` writing `{sim_dir}/{platform}/actions.jsonl`.
    let make_logger = |p: &str| -> Result<Arc<PlatformActionLogger>, ApiError> {
        Ok(Arc::new(
            PlatformActionLogger::new(p, &sim_dir)
                .map_err(|e| ApiError::server(format!("action logger init failed: {e}")))?,
        ))
    };
    // - "parallel" (U-030 cycle B): a DUAL-logger set (twitter + reddit). The unioned pool's agents
    //   carry their own `social.platform`, so `SimEngine::run` fans boundary records to both loggers
    //   and routes each action to its platform file — emitting twitter/ AND reddit/actions.jsonl, so
    //   the monitor's dual-platform completion gate (S-615) can fire.
    // - "twitter"/"reddit" (U-028 c3b-ii): a single-platform logger set — byte-identical to before.
    let loggers = if platform == "parallel" {
        PlatformLoggerSet::parallel(make_logger("twitter")?, make_logger("reddit")?)
    } else {
        let platform_enum = if platform == "reddit" { Platform::Reddit } else { Platform::Twitter };
        PlatformLoggerSet::single(platform_enum, make_logger(platform)?)
    };
    // Workstream C: the social-world substrate, one `SocialWorld` per platform present in the run.
    // Seeds round-0 posts, feeds recent posts back into prompts, applies committed social actions,
    // and (with `sqlite`) materializes `{sim_dir}/{platform}_simulation.db` so `/posts` +
    // `/comments` return real data. The platforms are exactly those of the logger set, so the
    // social DB files line up with the producer's per-platform action logs.
    let social_platforms: Vec<Platform> = if platform == "parallel" {
        vec![Platform::Twitter, Platform::Reddit]
    } else {
        vec![if platform == "reddit" { Platform::Reddit } else { Platform::Twitter }]
    };
    engine.with_producer(RunProducer { loggers, config: config.clone() });
    engine.with_social(
        crate::sim::social_world::SocialWorldSet::new(social_platforms, &sim_dir)
            .map_err(ApiError::server)?,
    );

    // pool: profile files → AgentPool (c2).
    let pool = crate::services::oasis_profile_export::load_agent_pool(&sim_dir, platform)
        .map_err(ApiError::server)?;

    // llm: always OpenAiAdapter (the concrete monomorphization, DECISION-U025-1).
    let llm = Arc::new(crate::api::build_llm(&state.config));

    // Dual-LLM boost (U-030 S-934): ONLY for a parallel run, and ONLY when `LLM_BOOST_API_KEY` is
    // configured — reddit agents then run against the boost client, twitter against `llm`
    // (`create_model(use_boost=True/False)` per coroutine). Single-platform runs and an unconfigured
    // boost → `None` → every agent uses `llm` (byte-identical to before).
    let boost_llm = if platform == "parallel" {
        crate::api::build_boost_llm(&state.config).map(Arc::new)
    } else {
        None
    };

    // graph + the updater's shared write-handle. The engine reads `graph` (currently a no-op);
    // the updater (U-021) writes the `Arc<Mutex<_>>` clone.
    let (graph, graph_for_updater) = if enable_graph_memory_update {
        let gid = graph_id.ok_or_else(|| {
            ApiError::client(
                StatusCode::BAD_REQUEST,
                crate::i18n::t("api.graphIdRequiredForMemory"),
            )
        })?;
        let g = load_entity_reader_graph(state, gid).await?;
        let updater_handle = Arc::new(tokio::sync::Mutex::new(g.clone()));
        (g, Some(updater_handle))
    } else {
        (KnowledgeGraph::new(), None)
    };

    Ok((RunInputs { engine, pool, graph, llm, boost_llm }, graph_for_updater))
}

/// Port of MiroFish `start_simulation` (simulation.py:1451-1641).
///
/// Full boundary port. The `RunInputs` construction gap (GAP-U026-RUNINPUTS-BUILDER) is CLOSED for
/// ALL platforms via [`build_run_inputs`]: single-platform `twitter`/`reddit` (U-028 c3b-ii) and
/// `parallel` dual-sink (U-030 cycle B — dual-logger producer over the unioned pool, monitor
/// dual-platform gate S-615).
async fn start_simulation(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: body or {} (Python :1493)
    let data = body.map(|Json(v)| v).unwrap_or_else(|| serde_json::json!({}));

    // Step 2a: simulation_id required (Python :1495-1500)
    let id = data
        .get("simulation_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requireSimulationId"))
        })?;

    // Step 2b: platform default "parallel" (Python :1502)
    let platform = data.get("platform").and_then(Value::as_str).unwrap_or("parallel");

    // Step 2c: max_rounds optional — Python `int(max_rounds)` coercion (Python :1508-1520)
    // None/Null → None; int/numeric-string → Ok(i64); ≤0 → maxRoundsPositive; non-numeric → maxRoundsInvalid
    let max_rounds: Option<i64> = match data.get("max_rounds") {
        None | Some(Value::Null) => None,
        Some(v) => Some(coerce_max_rounds(v)?),
    };

    // Step 2d: flags (Python :1504-1505)
    let enable_graph_memory_update =
        data.get("enable_graph_memory_update").and_then(Value::as_bool).unwrap_or(false);
    let force = data.get("force").and_then(Value::as_bool).unwrap_or(false);

    // Step 3: platform validation (Python :1522-1526)
    if !matches!(platform, "twitter" | "reddit" | "parallel") {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t_args("api.invalidPlatform", &[("platform", &platform)]),
        ));
    }

    // Step 4: get_simulation → None → 404 (Python :1529-1536)
    let mut sim =
        state.sim_manager.get_simulation(id).map_err(ApiError::server)?.ok_or_else(|| {
            ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.simulationNotFound", &[("id", &id)]),
            )
        })?;

    // Step 5: force_restarted = false (Python :1538)
    let mut force_restarted = false;

    // Step 6: state-machine — if status != READY (Python :1541-1582)
    if sim.status != SimulationStatus::Ready {
        let (is_prepared, _info) = check_simulation_prepared(&state.config, id);

        if is_prepared {
            // If Running, check whether the runner process is truly running (Python :1547-1563)
            if sim.status == SimulationStatus::Running
                && let Some(rs) =
                    state.sim_runner.get_run_state(id).await.map_err(ApiError::server)?
                && rs.runner_status == RunnerStatus::Running
            {
                if force {
                    // Force-stop; warn on error, swallow (Python :1554-1558)
                    if let Err(e) = state.sim_runner.stop_simulation(id).await {
                        tracing::warn!("停止模拟时出现警告: {e}");
                    }
                } else {
                    return Err(ApiError::client(
                        StatusCode::BAD_REQUEST,
                        crate::i18n::t("api.simRunningForceHint"),
                    ));
                }
            }

            // Force → cleanup logs + mark force_restarted (Python :1565-1571)
            if force {
                let r = state.sim_runner.cleanup_simulation_logs(id).await;
                if !r.success {
                    tracing::warn!("清理日志时出现警告: {:?}", r.errors);
                }
                force_restarted = true;
            }

            // Reset status to Ready + save (Python :1574-1576)
            sim.status = SimulationStatus::Ready;
            state.sim_manager.save_simulation_state(&mut sim).map_err(ApiError::server)?;
        } else {
            // Not prepared → 400 simNotReady{status} (Python :1578-1582)
            return Err(ApiError::client(
                StatusCode::BAD_REQUEST,
                crate::i18n::t_args("api.simNotReady", &[("status", &sim.status.as_str())]),
            ));
        }
    }

    // Step 7: graph_id resolution when enable_graph_memory_update (Python :1584-1601)
    let graph_id: Option<String> = if enable_graph_memory_update {
        // sim.graph_id is a plain String (not Option<String>)
        let gid: Option<String> = if !sim.graph_id.is_empty() {
            Some(sim.graph_id.clone())
        } else {
            // Try project.graph_id (Python :1590-1593)
            crate::models::project::ProjectManager::from_config(&state.config)
                .get_project(&sim.project_id)
                .ok()
                .flatten()
                .and_then(|p| p.graph_id)
                .filter(|s| !s.is_empty())
        };
        let gid = gid.ok_or_else(|| {
            ApiError::client(
                StatusCode::BAD_REQUEST,
                crate::i18n::t("api.graphIdRequiredForMemory"),
            )
        })?;
        Some(gid)
    } else {
        None
    };

    // Step 8 (Python :1604-1627): build RunInputs + start the run + assemble the 200.
    //
    // GAP-U026-RUNINPUTS-BUILDER CLOSED for ALL platforms: single-platform twitter/reddit (U-028
    // c3b-ii) and parallel dual-sink (U-030 cycle B). `build_run_inputs` assembles the engine
    // (config→ticks + activation + per-platform actions.jsonl producer), pool (profile reader),
    // graph, and llm; for "parallel" it attaches a dual-logger producer over the unioned pool so the
    // unified run emits twitter/ AND reddit/actions.jsonl. The landed monitor tails the producer's
    // `actions.jsonl` per platform → `simulation_end` → (for parallel, BOTH platforms via the
    // dual-platform gate S-615) → COMPLETED.
    let (inputs, graph_for_updater) = build_run_inputs(
        &state,
        id,
        platform,
        max_rounds,
        enable_graph_memory_update,
        graph_id.as_deref(),
    )
    .await?;

    // Drive the run (Python :1604-1610). On error (e.g. already-running ValueError analog) →
    // map_runner_err (Sim→400), matching Python's `except ValueError → 400`.
    let run_state = state
        .sim_runner
        .start_simulation(
            id,
            platform,
            max_rounds,
            enable_graph_memory_update,
            graph_id.as_deref(),
            inputs,
            graph_for_updater,
        )
        .await
        .map_err(map_runner_err)?;

    // Mark the simulation RUNNING + save (Python :1613-1614).
    sim.status = SimulationStatus::Running;
    state.sim_manager.save_simulation_state(&mut sim).map_err(ApiError::server)?;

    // Assemble the 200 response (Python :1616-1627): run_state.to_dict() + conditional fields.
    let mut response_data = run_state.to_dict();
    if let Some(mr) = max_rounds {
        // Python `if max_rounds:` — truthy; max_rounds here is always Some(>0) (≤0 already 400'd).
        response_data.insert("max_rounds_applied".to_string(), Value::from(mr));
    }
    response_data.insert(
        "graph_memory_update_enabled".to_string(),
        Value::from(enable_graph_memory_update),
    );
    response_data.insert("force_restarted".to_string(), Value::from(force_restarted));
    if enable_graph_memory_update && let Some(gid) = &graph_id {
        response_data.insert("graph_id".to_string(), Value::from(gid.as_str()));
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(response_data)
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (h) Route 1 — GET /:simulation_id/run-status  (simulation.py:1705-1760)
//
// Poll-based one-shot snapshot ("用于前端轮询" — for frontend polling).  NOT streaming
// (architect findings/u026-architecture.md §"No route in U-026 streams").
//
// Source steps:
//   1. run_state = SimulationRunner.get_run_state(id)
//   2. not run_state → 200 {success:true, data: <8-key idle stub>}
//      (runner_status:"idle", current_round/total_rounds/progress_percent/
//       twitter_actions_count/reddit_actions_count/total_actions_count all 0 — int 0,
//       NOT the full to_dict shape; NOT 404)
//   3. else → 200 {success:true, data: run_state.to_dict()}
//   4. except → 500 traceback
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_run_status` (simulation.py:1705-1760).
async fn run_status(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Step 1: get_run_state (cache-then-file); except → 500.
    let run_state =
        state.sim_runner.get_run_state(&simulation_id).await.map_err(ApiError::server)?;

    match run_state {
        // Step 2: None → 200 with the 8-key idle stub (NOT 404, NOT full to_dict).
        // progress_percent is the Python literal int 0 here (the full to_dict emits an f64).
        None => Ok(Json(serde_json::json!({
            "success": true,
            "data": {
                "simulation_id": simulation_id,
                "runner_status": "idle",
                "current_round": 0,
                "total_rounds": 0,
                "progress_percent": 0,
                "twitter_actions_count": 0,
                "reddit_actions_count": 0,
                "total_actions_count": 0,
            }
        }))),
        // Step 3: Some → 200 with full to_dict snapshot.
        Some(rs) => Ok(Json(serde_json::json!({
            "success": true,
            "data": Value::Object(rs.to_dict())
        }))),
    }
}

// ---------------------------------------------------------------------------
// Sub-cycle (h) Route 2 — GET /:simulation_id/run-status/detail  (simulation.py:1763-1850)
//
// One-shot detailed snapshot with embedded action lists (reads the U-047 actions.jsonl tail).
// Query param: `platform` (twitter|reddit, optional) — filters all_actions + recent_actions.
//
// `[!] U026-h-ACTIONS-PRODUCER-PENDING`: the action lists come from `get_all_actions`, which
// reads `{sim_dir}/{platform}/actions.jsonl`.  teri's SimEngine does not yet WRITE that log
// (producer lands with U-028/029/030 platform runners), so on a started run the lists are
// currently empty — a FAITHFUL no-op on a missing file (Python's reader also yields [] when the
// file is absent), NOT a dropped feature.  The full route assembly + filter logic is ported and
// proven now; the lists populate when the producer lands.
//
// Source assembly (simulation.py:1857-1878):
//   result = run_state.to_dict()
//   result["all_actions"]     = [a.to_dict() for a in get_all_actions(id, platform_filter)]
//   result["twitter_actions"] = get_all_actions(id, "twitter") if not pf or pf=="twitter" else []
//   result["reddit_actions"]  = get_all_actions(id, "reddit")  if not pf or pf=="reddit"  else []
//   result["rounds_count"]    = len(run_state.rounds)
//   result["recent_actions"]  = get_all_actions(id, platform_filter, round_num=current_round)
//                               if current_round > 0 else []
// None run_state → 200 {simulation_id, runner_status:"idle", all_actions:[],
//                       twitter_actions:[], reddit_actions:[]}  (5-key idle stub)
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_run_status_detail` (simulation.py:1763-1850).
async fn run_status_detail(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    // Python `request.args.get('platform')`: None when absent, "" when `?platform=` (falsy).
    let platform_filter: Option<&str> = params.get("platform").map(String::as_str);
    // Python `not platform_filter` is True for both None and "" (empty string is falsy).
    let filter_falsy = platform_filter.is_none_or(str::is_empty);

    // Step 1: get_run_state; except → 500.
    let run_state =
        state.sim_runner.get_run_state(&simulation_id).await.map_err(ApiError::server)?;

    let Some(rs) = run_state else {
        // None → 200 with the 5-key idle stub.
        return Ok(Json(serde_json::json!({
            "success": true,
            "data": {
                "simulation_id": simulation_id,
                "runner_status": "idle",
                "all_actions": [],
                "twitter_actions": [],
                "reddit_actions": []
            }
        })));
    };

    // get_all_actions reads the actions.jsonl tail (U-047); platform_filter passed verbatim.
    // An empty-string filter is FALSY (Python `not platform`) so get_all_actions("") reads BOTH
    // platforms and skips the record filter — handled inside get_all_actions/read_actions_from_file.
    let all_actions = state
        .sim_runner
        .get_all_actions(&simulation_id, platform_filter, None, None)
        .map_err(ApiError::server)?;
    // twitter/reddit lists are gated by Python's falsy `not pf or pf == "<platform>"`.
    let twitter_actions = if filter_falsy || platform_filter == Some("twitter") {
        state
            .sim_runner
            .get_all_actions(&simulation_id, Some("twitter"), None, None)
            .map_err(ApiError::server)?
    } else {
        Vec::new()
    };
    let reddit_actions = if filter_falsy || platform_filter == Some("reddit") {
        state
            .sim_runner
            .get_all_actions(&simulation_id, Some("reddit"), None, None)
            .map_err(ApiError::server)?
    } else {
        Vec::new()
    };
    // recent_actions = current round's tail (only when a round has started).
    let current_round = rs.current_round;
    let recent_actions = if current_round > 0 {
        state
            .sim_runner
            .get_all_actions(&simulation_id, platform_filter, None, Some(current_round))
            .map_err(ApiError::server)?
    } else {
        Vec::new()
    };

    // Assemble: base to_dict() + the five extra keys (Python mutates the same dict in order).
    let mut result = rs.to_dict();
    let to_objs = |v: Vec<crate::services::simulation_runner::AgentAction>| {
        Value::Array(v.into_iter().map(|a| Value::Object(a.to_dict())).collect())
    };
    result.insert("all_actions".into(), to_objs(all_actions));
    result.insert("twitter_actions".into(), to_objs(twitter_actions));
    result.insert("reddit_actions".into(), to_objs(reddit_actions));
    result.insert("rounds_count".into(), Value::Number((rs.rounds.len() as i64).into()));
    result.insert("recent_actions".into(), to_objs(recent_actions));

    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(result)
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (i) — world-state read routes: actions / timeline / agent-stats
//   (simulation.py:1864-1980). Primitives ported + parity-verified in U-022(d).
//
// `[!] U026-i-PRODUCER-PENDING` (informational, NOT a port bug): all three read the
// actions.jsonl tail (via SimulationRunner readers).  teri's SimEngine does not yet WRITE
// that log (producer lands U-028/029/030), so they return the FAITHFUL empty contract — a
// sim that produced no actions yields count 0 / empty lists.  Identical to Python on an
// absent log.  Routes port + verify against THAT contract now.
//
// Flask `type=int` graceful-fallback (same convention as U-025 graph `?limit`): an absent
// OR unparseable int param falls back to its default (NOT a 400).
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_simulation_actions` (simulation.py:1864-1912).
/// GET /:simulation_id/actions — paginated agent-action history.
async fn get_simulation_actions(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    // Flask: limit default 100, offset default 0 (bad/absent → default).
    let limit = params.get("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(100);
    let offset = params.get("offset").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    // platform: None when absent, Some("") when `?platform=` (falsy — handled in get_all_actions).
    let platform = params.get("platform").map(String::as_str);
    // agent_id / round_num: type=int, NO default → None on absent or unparseable.
    let agent_id = params.get("agent_id").and_then(|s| s.parse::<i64>().ok());
    let round_num = params.get("round_num").and_then(|s| s.parse::<i64>().ok());

    let actions = state
        .sim_runner
        .get_actions(&simulation_id, limit, offset, platform, agent_id, round_num)
        .map_err(ApiError::server)?;

    Ok(Json(serde_json::json!({
        "success": true,
        "data": {
            "count": actions.len(),
            "actions": actions.iter().map(|a| Value::Object(a.to_dict())).collect::<Vec<_>>()
        }
    })))
}

/// Port of MiroFish `get_simulation_timeline` (simulation.py:1918-1956).
/// GET /:simulation_id/timeline — per-round summary timeline.
async fn get_simulation_timeline(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    // Flask: start_round default 0; end_round type=int NO default → None.
    let start_round = params.get("start_round").and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    let end_round = params.get("end_round").and_then(|s| s.parse::<i64>().ok());

    let timeline = state
        .sim_runner
        .get_timeline(&simulation_id, start_round, end_round)
        .map_err(ApiError::server)?;

    Ok(Json(serde_json::json!({
        "success": true,
        "data": {
            "rounds_count": timeline.len(),
            "timeline": timeline.iter().map(TimelineEntry::to_value).collect::<Vec<_>>()
        }
    })))
}

/// Port of MiroFish `get_agent_stats` (simulation.py:1959-1980).
/// GET /:simulation_id/agent-stats — per-agent activity statistics.
async fn get_simulation_agent_stats(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let stats = state.sim_runner.get_agent_stats(&simulation_id).map_err(ApiError::server)?;

    Ok(Json(serde_json::json!({
        "success": true,
        "data": {
            "agents_count": stats.len(),
            "stats": stats.iter().map(AgentStats::to_value).collect::<Vec<_>>()
        }
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (j) — social-DB read routes: posts / comments  (simulation.py:1987-2120).
//
// Both read `{sim_dir}/{platform}_simulation.db` (the OASIS per-platform SQLite produced by a
// running social simulation).  teri locates it under the SAME single simulation dir every other
// route uses (`oasis_simulation_data_dir/<id>`, where cleanup_simulation_logs deletes these very
// `*_simulation.db` files) — NO dir-creation side effect (plain join, mirroring Python's
// read-only `os.path.join`).
//
// `[!] GAP-U026-SOCIALDB` (architect findings/u026-architecture.md:123-128,162,187; same producer
// frontier as GAP-SOCIAL-WORLDSTATE): the DB is written by the social-sim PRODUCER (U-028 twitter /
// U-029 reddit / U-030 parallel), all unported, AND the SQLite read needs the `sqlite` cargo
// feature (Cargo.toml:110, OFF by default).  So today the DB NEVER exists → both routes 200-return
// the missing-DB empty contract, which is the FAITHFUL current behavior (a sim that never ran has
// no DB).  That branch is ported + verifiable NOW.  The populated `SELECT` branch is implemented
// behind `#[cfg(feature = "sqlite")]` (rusqlite, mirroring `get_interview_history_from_db`) so it
// is correct + test-verifiable against a hand-built DB and ready when the producer lands; the
// default (no-sqlite) build returns an HONEST 500 if a DB somehow exists (never a silent empty).
// ---------------------------------------------------------------------------

/// Locate `{sim_dir}/{file}` without creating the dir (read-only, mirrors Python `os.path.join`).
fn social_db_path(state: &ApiState, simulation_id: &str, file: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(&state.config.oasis_simulation_data_dir)
        .join(simulation_id)
        .join(file)
}

/// Port of MiroFish `get_simulation_posts` (simulation.py:1987-2056).
/// GET /:simulation_id/posts — posts from the platform's SQLite DB.
async fn get_simulation_posts(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let platform = params.get("platform").map(String::as_str).unwrap_or("reddit");
    let limit = params.get("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(50);
    let offset = params.get("offset").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);

    let db_path = social_db_path(&state, &simulation_id, &format!("{platform}_simulation.db"));

    // Missing DB → 200 with the 4-key empty contract + dbNotExist message (the faithful current
    // behavior: no producer → no DB).
    if !db_path.exists() {
        return Ok(Json(serde_json::json!({
            "success": true,
            "data": {
                "platform": platform,
                "count": 0,
                "posts": [],
                "message": crate::i18n::t("api.dbNotExist")
            }
        })));
    }

    // DB exists → populated branch (deferred GAP-U026-SOCIALDB).
    read_posts_response(&db_path, platform, limit, offset)
}

/// Port of MiroFish `get_simulation_comments` (simulation.py:2061-2120). Reddit-only.
/// GET /:simulation_id/comments — comments from reddit_simulation.db, optional `post_id` filter.
async fn get_simulation_comments(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let post_id = params.get("post_id").map(String::as_str);
    let limit = params.get("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(50);
    let offset = params.get("offset").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);

    let db_path = social_db_path(&state, &simulation_id, "reddit_simulation.db");

    // Missing DB → 200 with the 2-key empty contract.
    if !db_path.exists() {
        return Ok(Json(serde_json::json!({
            "success": true,
            "data": { "count": 0, "comments": [] }
        })));
    }

    read_comments_response(&db_path, post_id, limit, offset)
}

// --- Populated-branch readers (feature-gated) -------------------------------
// `#[cfg(feature = "sqlite")]`: the real rusqlite SELECT path.  `#[cfg(not(...))]`: an honest 500
// when a DB exists but teri was built without `sqlite` — never a silent empty (no-downgrade).

/// Build a JSON object from a full SQLite row, keyed by column name (Python `dict(sqlite3.Row)`).
#[cfg(feature = "sqlite")]
fn sqlite_row_to_object(
    row: &rusqlite::Row<'_>,
    columns: &[String],
) -> rusqlite::Result<serde_json::Map<String, Value>> {
    use rusqlite::types::ValueRef;
    let mut m = serde_json::Map::new();
    for (i, name) in columns.iter().enumerate() {
        let v = match row.get_ref(i)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(n) => Value::Number(n.into()),
            ValueRef::Real(f) => {
                serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
            }
            ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(b) => Value::String(String::from_utf8_lossy(b).into_owned()),
        };
        m.insert(name.clone(), v);
    }
    Ok(m)
}

#[cfg(feature = "sqlite")]
fn read_posts_response(
    db_path: &std::path::Path,
    platform: &str,
    limit: usize,
    offset: usize,
) -> Result<Json<Value>, ApiError> {
    use rusqlite::Connection;
    // Outer try: connection failure → 500 (Python outer except). Inner OperationalError (e.g. no
    // `post` table) → posts=[], total=0 (Python's `except sqlite3.OperationalError`).
    let conn = Connection::open(db_path).map_err(ApiError::server)?;
    let queried: rusqlite::Result<(Vec<serde_json::Map<String, Value>>, i64)> = (|| {
        let mut stmt =
            conn.prepare("SELECT * FROM post ORDER BY created_at DESC LIMIT ? OFFSET ?")?;
        let cols: Vec<String> = stmt.column_names().iter().map(|s| (*s).to_string()).collect();
        let rows = stmt.query_map(rusqlite::params![limit as i64, offset as i64], |r| {
            sqlite_row_to_object(r, &cols)
        })?;
        let mut posts = Vec::new();
        for r in rows {
            posts.push(r?);
        }
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM post", [], |r| r.get(0))?;
        Ok((posts, total))
    })();
    let (posts, total) = queried.unwrap_or_else(|_| (Vec::new(), 0));
    Ok(Json(serde_json::json!({
        "success": true,
        "data": { "platform": platform, "total": total, "count": posts.len(), "posts": posts }
    })))
}

#[cfg(not(feature = "sqlite"))]
fn read_posts_response(
    _db_path: &std::path::Path,
    platform: &str,
    _limit: usize,
    _offset: usize,
) -> Result<Json<Value>, ApiError> {
    Err(ApiError::server(format!(
        "{platform}_simulation.db exists but teri was built without the `sqlite` feature \
         (GAP-U026-SOCIALDB — enable the sqlite feature + the U-028/029/030 producer)"
    )))
}

#[cfg(feature = "sqlite")]
fn read_comments_response(
    db_path: &std::path::Path,
    post_id: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<Json<Value>, ApiError> {
    use rusqlite::Connection;
    let conn = Connection::open(db_path).map_err(ApiError::server)?;
    let queried: rusqlite::Result<Vec<serde_json::Map<String, Value>>> = (|| {
        let (sql, owned_pid) = match post_id {
            Some(pid) => (
                "SELECT * FROM comment WHERE post_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
                Some(pid.to_string()),
            ),
            None => ("SELECT * FROM comment ORDER BY created_at DESC LIMIT ? OFFSET ?", None),
        };
        let mut stmt = conn.prepare(sql)?;
        let cols: Vec<String> = stmt.column_names().iter().map(|s| (*s).to_string()).collect();
        let map_row = |r: &rusqlite::Row<'_>| sqlite_row_to_object(r, &cols);
        let rows = match &owned_pid {
            Some(pid) => {
                stmt.query_map(rusqlite::params![pid, limit as i64, offset as i64], map_row)?
            }
            None => stmt.query_map(rusqlite::params![limit as i64, offset as i64], map_row)?,
        };
        let mut comments = Vec::new();
        for r in rows {
            comments.push(r?);
        }
        Ok(comments)
    })();
    let comments = queried.unwrap_or_default();
    Ok(Json(serde_json::json!({
        "success": true,
        "data": { "count": comments.len(), "comments": comments }
    })))
}

#[cfg(not(feature = "sqlite"))]
fn read_comments_response(
    _db_path: &std::path::Path,
    _post_id: Option<&str>,
    _limit: usize,
    _offset: usize,
) -> Result<Json<Value>, ApiError> {
    Err(ApiError::server(
        "reddit_simulation.db exists but teri was built without the `sqlite` feature \
         (GAP-U026-SOCIALDB — enable the sqlite feature + the U-028/029/030 producer)"
            .to_string(),
    ))
}

// ---------------------------------------------------------------------------
// Sub-cycle (k) — interview routes (simulation.py:2142-2570).
//
// Four POST routes (body-driven, no path params): /interview, /interview/batch, /interview/all,
// /interview/history.  The first three drive the live IPC env (interview an agent / batch /
// all); the fourth reads interview history from the per-platform SQLite DB.
//
// `[!] U026-k-IPC-PRODUCER-PENDING`: routes 1-3 require a LIVE env — `check_env_alive` is true
// only when a run is registered with a running IPC server (started by the U-028/029/030 social-sim
// producer, all unported).  Until then `check_env_alive` is false → every request 200-validates
// then 400s with `envNotRunning` — the FAITHFUL no-running-sim contract, fully ported + tested
// now.  The interview-call success path (optimize prompt → IPC send → shape result) is ported but
// only reachable with a live env (code-inspection verified, runtime producer-pending).
//
// `U026-k-TIMEOUT504` — RESOLVED (U-028 cycle 1): Python maps `TimeoutError` → HTTP 504
// (`interviewTimeout` / `batchInterviewTimeout` / `globalInterviewTimeout`).  teri now has a
// `TeriError::Timeout` variant — the IPC `send_command` elapsed branch produces it
// (`simulation_ipc.rs`), and each interview route maps it via `map_interview_err(e, <key>)` →
// 504 with the route-specific i18n key.  The 504 is now FAITHFUL, not a downgraded 400/500.
// (End-to-end through a live IPC env remains producer-pending — reachable when U-028/029/030
// register a run — but the Timeout→504 mapping is unit-verified now.)
//
// `/interview/history` is in the `[!] GAP-U026-SOCIALDB` family (same as posts/comments): reads
// `{platform}_simulation.db` behind `#[cfg(feature="sqlite")]`; no-DB → faithful empty, DB-exists
// without the feature → honest 500.
// ---------------------------------------------------------------------------

/// Interview prompt prefix (`simulation.py:23`).  Prepended so the agent replies with text instead
/// of invoking tools.  The CJK literal is OBSERVABLE in the prompt sent to the agent — preserve
/// it byte-for-byte.
const INTERVIEW_PROMPT_PREFIX: &str =
    "结合你的人设、所有的过往记忆与行动，不调用任何工具直接用文本回复我：";

/// Port of `optimize_interview_prompt` (`simulation.py:28-43`).  Pure string: empty → unchanged;
/// already-prefixed → unchanged (no double prefix); else prepend the prefix.
fn optimize_interview_prompt(prompt: &str) -> String {
    if prompt.is_empty() {
        return String::new();
    }
    if prompt.starts_with(INTERVIEW_PROMPT_PREFIX) {
        return prompt.to_string();
    }
    format!("{INTERVIEW_PROMPT_PREFIX}{prompt}")
}

/// Parse a `timeout` field (JSON number, seconds) with a default; non-finite/negative → default
/// (so `Duration::from_secs_f64` can never panic). Python `data.get('timeout', default)`.
fn parse_timeout(data: &Value, default_secs: f64) -> Duration {
    let secs = data
        .get("timeout")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(default_secs);
    Duration::from_secs_f64(secs)
}

/// Shape an `IPCResponse` into Python's interview result dict (mirrors the
/// `SimulationRunner.interview_*` classmethod tails). Key order: `success`, then the per-route
/// extra keys (agent_id+prompt OR interviews_count), then `result` (when completed) or `error`,
/// then `timestamp`.
fn shape_interview_result(resp: &IPCResponse, extra: Vec<(&'static str, Value)>) -> Value {
    let completed = resp.status == CommandStatus::Completed;
    let mut m = serde_json::Map::new();
    m.insert("success".into(), Value::Bool(completed));
    for (k, v) in extra {
        m.insert(k.to_string(), v);
    }
    if completed {
        m.insert("result".into(), resp.result.clone().map(Value::Object).unwrap_or(Value::Null));
    } else {
        m.insert("error".into(), resp.error.clone().map(Value::String).unwrap_or(Value::Null));
    }
    m.insert("timestamp".into(), Value::String(resp.timestamp.clone()));
    Value::Object(m)
}

/// Validate an optional platform field: present + non-empty + not in {twitter,reddit} → Err(key).
/// Mirrors Python `if platform and platform not in (...)`. Empty string is falsy → skipped.
fn validate_interview_platform(platform: Option<&str>, err_key: &str) -> Result<(), ApiError> {
    if let Some(p) = platform
        && !p.is_empty()
        && !matches!(p, "twitter" | "reddit")
    {
        return Err(ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t(err_key)));
    }
    Ok(())
}

/// Count agents eligible for a global interview = agent_configs entries carrying an `agent_id`
/// (mirrors Python `interview_all_agents`' built-list length for the `interviews_count` field).
fn count_interview_agents(state: &ApiState, simulation_id: &str) -> usize {
    let config_path = social_db_path(state, simulation_id, "simulation_config.json");
    let Ok(content) = std::fs::read_to_string(&config_path) else { return 0 };
    let Ok(config) = serde_json::from_str::<Value>(&content) else { return 0 };
    config
        .get("agent_configs")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|a| a.get("agent_id").and_then(Value::as_i64).is_some())
                .count()
        })
        .unwrap_or(0)
}

/// Port of MiroFish `interview_agent` route (simulation.py:2142-2270). POST /interview.
async fn interview_agent_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = body.map(|Json(v)| v).unwrap_or_else(|| serde_json::json!({}));

    let simulation_id = data
        .get("simulation_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requireSimulationId"))
        })?;
    // Python `if agent_id is None` → 400 (agent_id 0 is valid).
    let agent_id = data.get("agent_id").and_then(Value::as_i64).ok_or_else(|| {
        ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requireAgentId"))
    })?;
    // Python `if not prompt` → empty/missing → 400.
    let prompt = data
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requirePrompt"))
        })?;
    let platform = data.get("platform").and_then(Value::as_str);
    validate_interview_platform(platform, "api.invalidInterviewPlatform")?;
    let timeout = parse_timeout(&data, 60.0);

    // env-alive gate (Python check_env_alive false → 400 envNotRunning; teri Err(no-run) → false).
    if !state.sim_runner.check_env_alive(simulation_id).await.unwrap_or(false) {
        return Err(ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.envNotRunning")));
    }

    // --- IPC-PRODUCER-PENDING success path ---
    let optimized = optimize_interview_prompt(prompt);
    let resp = state
        .sim_runner
        .interview_agent(simulation_id, agent_id, &optimized, platform, timeout)
        .await
        .map_err(|e| map_interview_err(e, "api.interviewTimeout"))?;
    let result = shape_interview_result(
        &resp,
        vec![("agent_id", serde_json::json!(agent_id)), ("prompt", Value::String(optimized))],
    );
    let success = result.get("success").and_then(Value::as_bool).unwrap_or(false);
    Ok(Json(serde_json::json!({ "success": success, "data": result })))
}

/// Port of MiroFish `interview_agents_batch` route (simulation.py:2271-2408). POST /interview/batch.
async fn interview_batch_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = body.map(|Json(v)| v).unwrap_or_else(|| serde_json::json!({}));

    let simulation_id = data
        .get("simulation_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requireSimulationId"))
        })?;
    // Python `if not interviews or not isinstance(interviews, list)` → 400.
    let interviews = data
        .get("interviews")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requireInterviews"))
        })?;
    let platform = data.get("platform").and_then(Value::as_str);
    validate_interview_platform(platform, "api.invalidInterviewPlatform")?;

    // Per-item validation (Python loop, index is 1-based).
    for (i, interview) in interviews.iter().enumerate() {
        let idx = (i + 1).to_string();
        if interview.get("agent_id").is_none() {
            return Err(ApiError::client(
                StatusCode::BAD_REQUEST,
                crate::i18n::t_args("api.interviewListMissingAgentId", &[("index", &idx)]),
            ));
        }
        if interview.get("prompt").is_none() {
            return Err(ApiError::client(
                StatusCode::BAD_REQUEST,
                crate::i18n::t_args("api.interviewListMissingPrompt", &[("index", &idx)]),
            ));
        }
        let item_platform = interview.get("platform").and_then(Value::as_str);
        if let Some(p) = item_platform
            && !p.is_empty()
            && !matches!(p, "twitter" | "reddit")
        {
            return Err(ApiError::client(
                StatusCode::BAD_REQUEST,
                crate::i18n::t_args("api.interviewListInvalidPlatform", &[("index", &idx)]),
            ));
        }
    }

    let timeout = parse_timeout(&data, 120.0);

    if !state.sim_runner.check_env_alive(simulation_id).await.unwrap_or(false) {
        return Err(ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.envNotRunning")));
    }

    // --- IPC-PRODUCER-PENDING success path ---
    // Optimize each item's prompt (Python copies the item + replaces prompt).
    let optimized: Vec<Value> = interviews
        .iter()
        .map(|item| {
            let mut obj = item.as_object().cloned().unwrap_or_default();
            let p = obj.get("prompt").and_then(Value::as_str).unwrap_or("");
            obj.insert("prompt".into(), Value::String(optimize_interview_prompt(p)));
            Value::Object(obj)
        })
        .collect();
    let count = optimized.len();
    let resp = state
        .sim_runner
        .interview_agents_batch(simulation_id, optimized, platform, timeout)
        .await
        .map_err(|e| map_interview_err(e, "api.batchInterviewTimeout"))?;
    let result =
        shape_interview_result(&resp, vec![("interviews_count", serde_json::json!(count))]);
    let success = result.get("success").and_then(Value::as_bool).unwrap_or(false);
    Ok(Json(serde_json::json!({ "success": success, "data": result })))
}

/// Port of MiroFish `interview_all_agents` route (simulation.py:2409-2511). POST /interview/all.
async fn interview_all_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = body.map(|Json(v)| v).unwrap_or_else(|| serde_json::json!({}));

    let simulation_id = data
        .get("simulation_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requireSimulationId"))
        })?;
    let prompt = data
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requirePrompt"))
        })?;
    let platform = data.get("platform").and_then(Value::as_str);
    validate_interview_platform(platform, "api.invalidInterviewPlatform")?;
    let timeout = parse_timeout(&data, 180.0);

    if !state.sim_runner.check_env_alive(simulation_id).await.unwrap_or(false) {
        return Err(ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.envNotRunning")));
    }

    // --- IPC-PRODUCER-PENDING success path ---
    let optimized = optimize_interview_prompt(prompt);
    let resp = state
        .sim_runner
        .interview_all_agents(simulation_id, &optimized, platform, timeout)
        .await
        .map_err(|e| map_interview_err(e, "api.globalInterviewTimeout"))?;
    // interviews_count = the built-list length the primitive used (agent_configs w/ agent_id).
    let count = count_interview_agents(&state, simulation_id);
    let result =
        shape_interview_result(&resp, vec![("interviews_count", serde_json::json!(count))]);
    let success = result.get("success").and_then(Value::as_bool).unwrap_or(false);
    Ok(Json(serde_json::json!({ "success": success, "data": result })))
}

/// Port of MiroFish `get_interview_history` route (simulation.py:2512-2570). POST /interview/history.
async fn interview_history_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = body.map(|Json(v)| v).unwrap_or_else(|| serde_json::json!({}));

    let simulation_id = data
        .get("simulation_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requireSimulationId"))
        })?;
    let platform = data.get("platform").and_then(Value::as_str);
    let agent_id = data.get("agent_id").and_then(Value::as_i64);
    let limit = data.get("limit").and_then(Value::as_u64).map(|n| n as usize).unwrap_or(100);

    interview_history_response(&state, simulation_id, platform, agent_id, limit)
}

#[cfg(feature = "sqlite")]
fn interview_history_response(
    state: &ApiState,
    simulation_id: &str,
    platform: Option<&str>,
    agent_id: Option<i64>,
    limit: usize,
) -> Result<Json<Value>, ApiError> {
    let history = state
        .sim_runner
        .get_interview_history(simulation_id, platform, agent_id, limit)
        .map_err(ApiError::server)?;
    Ok(Json(serde_json::json!({
        "success": true,
        "data": { "count": history.len(), "history": history }
    })))
}

#[cfg(not(feature = "sqlite"))]
fn interview_history_response(
    state: &ApiState,
    simulation_id: &str,
    platform: Option<&str>,
    _agent_id: Option<i64>,
    _limit: usize,
) -> Result<Json<Value>, ApiError> {
    // Without the sqlite feature the interview DBs can't be read. If a relevant DB exists, surface
    // an HONEST 500 (GAP-U026-SOCIALDB) — never a silent empty; else the faithful no-DB empty.
    let platforms: Vec<&str> = match platform {
        Some("twitter") | Some("reddit") => vec![platform.unwrap()],
        _ => vec!["twitter", "reddit"],
    };
    for p in platforms {
        if social_db_path(state, simulation_id, &format!("{p}_simulation.db")).exists() {
            return Err(ApiError::server(
                "interview-history DB exists but teri was built without the `sqlite` feature \
                 (GAP-U026-SOCIALDB — enable the sqlite feature + the U-028/029/030 producer)"
                    .to_string(),
            ));
        }
    }
    Ok(Json(serde_json::json!({
        "success": true,
        "data": { "count": 0, "history": [] }
    })))
}

// ---------------------------------------------------------------------------
// Sub-cycle (l) — env-status / close-env  (simulation.py:2585-2716).
//
// `POST /env-status` is a PURE read (check_env_alive + get_env_status_detail file read) — always
// 200, fully portable + tested TODAY (no env → env_alive:false, both-available:false,
// message:envNotRunningShort).
//
// `POST /close-env` drives the live IPC env (graceful close) — `[!] U026-l-IPC-PRODUCER-PENDING`:
// close_simulation_env errors `Simulation not found` (→400) until a run with a live IPC server is
// registered (U-028/029/030).  Validation + the no-env 400 are tested now; the success path
// (IPC close → shape → status=Completed) is ported, code-inspection-verified, producer-pending.
//
// `U026-l-TIMEOUT` — RESOLVED (U-028 cycle 1): Python's close_simulation_env catches
// `TimeoutError` and returns a GRACEFUL **200** `{success:true, message:"环境关闭命令已发送（等待
// 响应超时，环境可能正在关闭）"}` (a close-timeout is treated as "probably closing").  teri's IPC
// `send_command` now produces `TeriError::Timeout` on the elapsed branch, and `close_env_route`
// catches it BEFORE `map_runner_err` and returns the same 2-key graceful 200 (the CJK literal,
// verbatim).  The hard-400 divergence is gone.  (End-to-end through a live IPC env remains
// producer-pending — reachable when U-028/029/030 register a run.)
// ---------------------------------------------------------------------------

/// Port of MiroFish `get_env_status` route (simulation.py:2585-2647). POST /env-status.
async fn env_status_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = body.map(|Json(v)| v).unwrap_or_else(|| serde_json::json!({}));

    let simulation_id = data
        .get("simulation_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requireSimulationId"))
        })?;

    // env_alive: Python check_env_alive (False when no env); teri Err(no-run) → false.
    let env_alive = state.sim_runner.check_env_alive(simulation_id).await.unwrap_or(false);
    // get_env_status_detail reads env_status.json (default when absent). Python catches read errors
    // and returns the default too → unwrap_or_default() yields the false/false defaults.
    let env_status = state.sim_runner.get_env_status_detail(simulation_id).unwrap_or_default();
    let message = if env_alive {
        crate::i18n::t("api.envRunning")
    } else {
        crate::i18n::t("api.envNotRunningShort")
    };

    Ok(Json(serde_json::json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "env_alive": env_alive,
            "twitter_available": env_status.get("twitter_available").and_then(Value::as_bool).unwrap_or(false),
            "reddit_available": env_status.get("reddit_available").and_then(Value::as_bool).unwrap_or(false),
            "message": message
        }
    })))
}

/// Port of MiroFish `close_simulation_env` route (simulation.py:2649-2716). POST /close-env.
async fn close_env_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = body.map(|Json(v)| v).unwrap_or_else(|| serde_json::json!({}));

    let simulation_id = data
        .get("simulation_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ApiError::client(StatusCode::BAD_REQUEST, crate::i18n::t("api.requireSimulationId"))
        })?;
    let timeout = parse_timeout(&data, 30.0);

    // --- IPC-PRODUCER-PENDING: no env → Err(no-run) → map_runner_err → 400 (Python ValueError). ---
    // `None` marks the GRACEFUL-TIMEOUT outcome: Python's `close_simulation_env` primitive catches
    // `TimeoutError` and returns a graceful dict (`simulation_runner.py:1651-1656`), then the ROUTE
    // continues unconditionally. teri surfaces the timeout as `TeriError::Timeout` at the route and
    // maps it to `None` here — crucially WITHOUT early-returning, so the status-update block below
    // still runs (matching Python's unconditional `status=COMPLETED; save` at `:2691-2696`).
    let resp: Option<IPCResponse> =
        match state.sim_runner.close_simulation_env(simulation_id, timeout).await {
            Ok(r) => Some(r),
            Err(crate::error::TeriError::Timeout(_)) => None,
            Err(e) => return Err(map_runner_err(e)),
        };

    // Status update runs for BOTH outcomes (Python `:2691-2696` is unconditional — it executes
    // whether the primitive returned the normal dict OR swallowed a TimeoutError).
    if let Some(mut sim) =
        state.sim_manager.get_simulation(simulation_id).map_err(ApiError::server)?
    {
        sim.status = SimulationStatus::Completed;
        state.sim_manager.save_simulation_state(&mut sim).map_err(ApiError::server)?;
    }

    // Graceful-timeout body: a distinct 2-key `{success:true, message}` dict (the CJK literal,
    // verbatim) — NOT the 4-key normal-close shape. Produced after the status update above.
    let resp = match resp {
        Some(r) => r,
        None => {
            return Ok(Json(serde_json::json!({
                "success": true,
                "data": {
                    "success": true,
                    "message": "环境关闭命令已发送（等待响应超时，环境可能正在关闭）"
                }
            })));
        }
    };

    // Shape the "close command sent" result dict (Python close_simulation_env tail). The message
    // is a hardcoded CJK literal in the Python primitive (NOT an i18n key) — preserved verbatim.
    // `[≠] U026-l-ALREADYCLOSED`: teri's close_simulation_env always sends (no already-closed early
    // return), so the Python 2-key {success,message:"环境已经关闭"} branch is not produced; this
    // path is producer-pending regardless.
    let completed = resp.status == CommandStatus::Completed;
    let mut result = serde_json::Map::new();
    result.insert("success".into(), Value::Bool(completed));
    result.insert("message".into(), Value::String("环境关闭命令已发送".to_string()));
    result.insert("result".into(), resp.result.clone().map(Value::Object).unwrap_or(Value::Null));
    result.insert("timestamp".into(), Value::String(resp.timestamp.clone()));

    Ok(Json(serde_json::json!({ "success": completed, "data": Value::Object(result) })))
}

// ---------------------------------------------------------------------------
// Tests — sub-cycle (c)
//
// Mirror graph.rs's test style: `ApiState::new(Config::build_test())`, drive via the
// full `create_app` router using `tower::ServiceExt::oneshot`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn test_state() -> (Arc<ApiState>, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let mut config = crate::Config::build_test();
        config.upload_folder = tmp.path().to_string_lossy().to_string();
        config.oasis_simulation_data_dir =
            tmp.path().join("simulations").to_string_lossy().to_string();
        (Arc::new(ApiState::new(config)), tmp)
    }

    fn test_app() -> (axum::Router, tempfile::TempDir) {
        let (state, tmp) = test_state();
        let app = crate::server::create_app(state);
        (app, tmp)
    }

    /// Seed a project with a graph_id so create_simulation can succeed.
    fn seed_project_with_graph(state: &Arc<ApiState>) -> (String, String) {
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("Test Project").expect("seed project");
        p.graph_id = Some("graph-abc".to_string());
        pm.save_project(&mut p).expect("seed save");
        (p.project_id.clone(), "graph-abc".to_string())
    }

    // -----------------------------------------------------------------------
    // ROUTE-ORDER test — `[!] U026-ROUTE-ORDER`
    //
    // GET /api/simulation/list must resolve to list_simulations, NOT to
    // get_simulation("list").  Axum 0.7 static-before-capture rule.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn route_order_list_not_matched_as_simulation_id() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/api/simulation/list").body(Body::empty()).unwrap())
            .await
            .unwrap();

        // Must be 200, not 404 (which would happen if axum matched "list" as a simulation_id)
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "ROUTE-ORDER: /list must not be matched as get_simulation('list')"
        );

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], true);
        assert!(json["data"].is_array(), "list envelope must have data array: {json}");
        assert!(json["count"].is_number(), "list envelope must have count: {json}");
    }

    // -----------------------------------------------------------------------
    // create_simulation — happy path (200 + state shape)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn create_simulation_happy_path() {
        let (state, _tmp) = test_state();
        let (project_id, _graph_id) = seed_project_with_graph(&state);

        let app = crate::server::create_app(state);
        let body = serde_json::json!({"project_id": project_id}).to_string();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/create")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "create must return 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["project_id"], project_id);
        assert_eq!(data["status"], "created");
        assert!(
            data["simulation_id"].as_str().unwrap_or("").starts_with("sim_"),
            "simulation_id must start with 'sim_'"
        );
        // enable_twitter/enable_reddit default to true
        assert_eq!(data["enable_twitter"], true);
        assert_eq!(data["enable_reddit"], true);
    }

    // -----------------------------------------------------------------------
    // create_simulation — missing project_id → 400
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn create_simulation_missing_project_id_400() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/create")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json["error"].as_str().is_some(), "error field must be present");
    }

    // -----------------------------------------------------------------------
    // create_simulation — project not found → 404
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn create_simulation_project_not_found_404() {
        let (app, _tmp) = test_app();
        let body = serde_json::json!({"project_id": "nonexistent-proj"}).to_string();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/create")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        let error = json["error"].as_str().unwrap_or("");
        assert!(
            error.contains("nonexistent-proj"),
            "404 error must mention the project id: {error}"
        );
    }

    // -----------------------------------------------------------------------
    // create_simulation — graph not built → 400
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn create_simulation_graph_not_built_400() {
        let (state, _tmp) = test_state();
        // Create a project WITHOUT a graph_id
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let p = pm.create_project("No Graph Project").expect("seed");
        let project_id = p.project_id.clone();

        let app = crate::server::create_app(state);
        let body = serde_json::json!({"project_id": project_id}).to_string();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/create")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
    }

    // -----------------------------------------------------------------------
    // get_simulation — happy path (200, state shape)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_simulation_happy_path() {
        let (state, _tmp) = test_state();
        let (project_id, _graph_id) = seed_project_with_graph(&state);

        // Create a simulation directly via the shared manager
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "get_simulation must return 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["simulation_id"], sim_id);
        assert_eq!(data["project_id"], project_id);
        assert_eq!(data["status"], "created");
        // Non-READY sim must NOT have run_instructions
        assert!(
            data.get("run_instructions").is_none(),
            "non-READY sim must not have run_instructions in data"
        );
    }

    // -----------------------------------------------------------------------
    // get_simulation — not found → 404
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_simulation_not_found_404() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        let error = json["error"].as_str().unwrap_or("");
        assert!(
            error.contains("sim_nonexistent"),
            "404 error must mention the simulation id: {error}"
        );
    }

    // -----------------------------------------------------------------------
    // get_simulation — READY status → run_instructions present (DECISION-U026-2)
    //
    // Constructs a simulation, patches its state.json to status=ready,
    // then verifies the READY gate fires and run_instructions is embedded.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_simulation_ready_has_run_instructions() {
        use crate::services::simulation_manager::SimulationStatus;

        let (state, _tmp) = test_state();
        let (project_id, _) = seed_project_with_graph(&state);

        // Create a simulation
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        // Patch state.json to set status = "ready"
        let sim_dir = state.config.oasis_simulation_data_dir.clone();
        let state_file = std::path::PathBuf::from(&sim_dir).join(&sim_id).join("state.json");
        let raw = std::fs::read_to_string(&state_file).expect("read state.json");
        let mut obj: serde_json::Value = serde_json::from_str(&raw).expect("parse state.json");
        obj.as_object_mut().unwrap().insert(
            "status".to_string(),
            serde_json::Value::String(SimulationStatus::Ready.as_str().to_string()),
        );
        std::fs::write(&state_file, serde_json::to_string_pretty(&obj).unwrap()).unwrap();
        // Evict from cache so the patched file is re-read
        state.sim_manager.evict_cache_for_test(&sim_id);

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "READY get must return 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["status"], "ready");

        // run_instructions must be present for READY sim
        let ri = data.get("run_instructions").expect("READY sim must have run_instructions");
        assert!(ri.get("simulation_dir").is_some(), "run_instructions must have simulation_dir");
        assert!(ri.get("config_file").is_some(), "run_instructions must have config_file");
        assert!(ri.get("commands").is_some(), "run_instructions must have commands");
        assert!(ri.get("instructions").is_some(), "run_instructions must have instructions");
        assert!(ri.get("substrate_note").is_some(), "run_instructions must have substrate_note");
        // scripts_dir must NOT appear ([≠] scripts_dir)
        assert!(
            ri.get("scripts_dir").is_none(),
            "[≠] scripts_dir must not appear in run_instructions"
        );

        // commands must have twitter/reddit/parallel, each referencing /start
        let cmds = ri.get("commands").unwrap();
        for platform in ["twitter", "reddit", "parallel"] {
            let cmd = cmds.get(platform).and_then(|v| v.as_str()).unwrap_or("");
            assert!(!cmd.is_empty(), "commands.{platform} must be non-empty");
            assert!(cmd.contains("/start"), "commands.{platform} must reference /start: {cmd}");
            assert!(cmd.contains(platform), "commands.{platform} must name its platform: {cmd}");
            // Route SHAPE regression guard (parity-gate FAIL fix): must point at the real
            // body-id route `POST /api/simulation/start`, NOT a nonexistent `/<id>/start`.
            assert!(
                cmd.contains("POST /api/simulation/start"),
                "commands.{platform} must use the body-id start route exactly: {cmd}"
            );
            assert!(
                !cmd.contains("/start") || !cmd.contains(&format!("simulation/{sim_id}/start")),
                "commands.{platform} must NOT use the id-in-path /<id>/start route: {cmd}"
            );
        }

        // instructions must mention SimEngine
        let instructions = ri.get("instructions").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            instructions.contains("SimEngine"),
            "instructions must mention SimEngine: {instructions}"
        );
    }

    // -----------------------------------------------------------------------
    // list_simulations — empty → 200 with count 0
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_simulations_empty() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/api/simulation/list").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["success"], true);
        assert_eq!(json["count"], 0);
        assert_eq!(json["data"].as_array().unwrap().len(), 0);
    }

    // -----------------------------------------------------------------------
    // list_simulations — ?project_id filter
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_simulations_project_id_filter() {
        let (state, _tmp) = test_state();
        let (project_id_a, _) = seed_project_with_graph(&state);

        // Seed a second project
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut pb = pm.create_project("Project B").expect("seed B");
        pb.graph_id = Some("graph-b".to_string());
        pm.save_project(&mut pb).expect("save B");
        let project_id_b = pb.project_id.clone();

        // Create one simulation for each project
        state
            .sim_manager
            .create_simulation(&project_id_a, "graph-abc", true, true)
            .unwrap();
        state
            .sim_manager
            .create_simulation(&project_id_b, "graph-b", true, true)
            .unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/list?project_id={project_id_a}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["success"], true);
        assert_eq!(json["count"], 1, "filter must return only project A's simulation");
        let arr = json["data"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["project_id"], project_id_a);
    }

    // -----------------------------------------------------------------------
    // list_simulations — count key matches data length
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_simulations_count_matches_data_len() {
        let (state, _tmp) = test_state();
        let (project_id, _) = seed_project_with_graph(&state);

        state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .unwrap();
        state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(Request::builder().uri("/api/simulation/list").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        let count = json["count"].as_u64().unwrap();
        let data_len = json["data"].as_array().unwrap().len() as u64;
        assert_eq!(count, data_len, "count must equal data.len()");
        assert_eq!(count, 2, "must see 2 simulations");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (a) gate (retained)
    // -----------------------------------------------------------------------

    #[test]
    fn simulation_router_skeleton_builds_from_state() {
        let config = crate::Config::build_test();
        let state = Arc::new(ApiState::new(config));
        let _router = simulation_router(state.clone());
        assert!(Arc::strong_count(&state.sim_manager) >= 1);
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (b) helpers
    // -----------------------------------------------------------------------

    /// Seed a completed task with a real embedded KnowledgeGraph (2 entities, 1 edge).
    /// The task_id IS the graph_id for entity-read routes.
    /// Returns (graph_id, alice_uuid, entity_type_str).
    fn seed_entity_graph_task() -> (String, String, String) {
        use crate::graph::{Entity, EntityKind, KnowledgeGraph, Relation, RelationKind};

        let alice_id = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let bob_id = uuid::Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();

        let mut graph = KnowledgeGraph::new();
        let alice = Entity { id: alice_id, name: "Alice".to_string(), kind: EntityKind::Person };
        let bob = Entity { id: bob_id, name: "Bob".to_string(), kind: EntityKind::Person };
        let alice_idx = graph.add_entity(alice).expect("add alice");
        let bob_idx = graph.add_entity(bob).expect("add bob");
        graph.add_relation(
            alice_idx,
            bob_idx,
            Relation::new(RelationKind::RelatedTo, 0.8).expect("rel"),
        );

        let graph_json_str = graph.serialize_to_json().expect("serialize graph");
        let graph_json: serde_json::Value =
            serde_json::from_str(&graph_json_str).expect("parse graph json");

        let result = serde_json::json!({
            "graph_name":       "EntityTestGraph",
            "graph_info":       {"node_count": 2, "edge_count": 1, "entity_types": []},
            "chunks_processed": 1,
            "graph":            graph_json
        });

        let tm = crate::task::TaskManager::global();
        let task_id = tm.create_task("graph_build", None);
        tm.complete_task(&task_id, result);

        // EntityKind::Person.to_string() is the type label (the Display token for Person)
        let entity_type = EntityKind::Person.to_string();
        (task_id, alice_id.to_string(), entity_type)
    }

    /// Build an app with zep_api_key = None so the ZEP guard fires.
    fn test_app_no_zep_sim() -> (axum::Router, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let mut config = crate::Config::build_test();
        config.upload_folder = tmp.path().to_string_lossy().to_string();
        config.zep_api_key = None;
        let state = Arc::new(ApiState::new(config));
        (crate::server::create_app(state), tmp)
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (b) — Route 1: GET /entities/:graph_id
    // -----------------------------------------------------------------------

    /// GET /entities/:graph_id — happy path → 200 + FilteredEntities shape
    #[tokio::test]
    async fn get_graph_entities_happy_200_filtered_entities_shape() {
        let (graph_id, _alice_id, _entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "happy path must 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert!(data["entities"].is_array(), "data must have entities array");
        assert!(data["entity_types"].is_array(), "data must have entity_types array");
        assert!(data["total_count"].is_number(), "data must have total_count");
        assert!(data["filtered_count"].is_number(), "data must have filtered_count");
        // 2 entities of kind Person — both should pass filter
        assert_eq!(data["filtered_count"], 2, "both person entities must be returned");
    }

    /// GET /entities/:graph_id?entity_types=person — type filter narrows to matching
    #[tokio::test]
    async fn get_graph_entities_entity_types_filter() {
        let (graph_id, _alice_id, entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}?entity_types={entity_type}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["filtered_count"], 2, "filter must keep both persons");
    }

    /// GET /entities/:graph_id?entity_types=,  (whitespace/empty CSV) → None → all entities
    #[tokio::test]
    async fn get_graph_entities_empty_entity_types_csv_treated_as_none() {
        let (graph_id, _alice_id, _entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}?entity_types=,+,"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        // All-whitespace/empty CSV → None → filter passes through all
        assert_eq!(json["data"]["filtered_count"], 2);
    }

    /// GET /entities/:graph_id?enrich=false — enrich=false is respected
    #[tokio::test]
    async fn get_graph_entities_enrich_false() {
        let (graph_id, _alice_id, _entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}?enrich=false"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["filtered_count"], 2);
    }

    /// GET /entities/:graph_id — ZEP guard empty → 500 api.zepApiKeyMissing
    #[tokio::test]
    async fn get_graph_entities_zep_guard_empty_500() {
        let (graph_id, _alice_id, _entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app_no_zep_sim();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR, "ZEP-guard must 500");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("error").is_some(), "must have error key");
        // ZEP guard uses ApiError::client → 2-key body (no traceback)
        assert!(json.get("traceback").is_none(), "ZEP guard must not emit traceback");
    }

    /// GET /entities/unknown-graph-id — graph not found → 500 server
    #[tokio::test]
    async fn get_graph_entities_graph_not_found_500() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/entities/no-such-task-id-xyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR, "missing graph must 500");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("error").is_some());
        // Server 500 has 3-key body (success, error, traceback)
        assert!(json.get("traceback").is_some(), "server 500 must have traceback");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (b) — Route 2: GET /entities/:graph_id/:entity_uuid
    // -----------------------------------------------------------------------

    /// GET /entities/:graph_id/:entity_uuid — found → 200 + EntityNode shape
    #[tokio::test]
    async fn get_entity_detail_found_200() {
        let (graph_id, alice_id, _entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}/{alice_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "found entity must 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["name"], "Alice", "data.name must be Alice");
        assert!(data["uuid"].is_string(), "data must have uuid");
        assert!(data["labels"].is_array(), "data must have labels array");
        assert!(data["related_edges"].is_array(), "data must have related_edges");
        assert!(data["related_nodes"].is_array(), "data must have related_nodes");
    }

    /// GET /entities/:graph_id/:entity_uuid — not found → 404 entityNotFound
    #[tokio::test]
    async fn get_entity_detail_not_found_404() {
        let (graph_id, _alice_id, _entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let missing_uuid = "cccccccc-cccc-cccc-cccc-cccccccccccc";
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}/{missing_uuid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "missing entity must 404");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        let error = json["error"].as_str().unwrap_or("");
        assert!(error.contains(missing_uuid), "404 error must contain entity uuid: {error}");
    }

    /// GET /entities/:graph_id/:entity_uuid — ZEP guard → 500
    #[tokio::test]
    async fn get_entity_detail_zep_guard_500() {
        let (graph_id, alice_id, _entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app_no_zep_sim();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}/{alice_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "ZEP guard must not emit traceback");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (b) — Route 3: GET /entities/:graph_id/by-type/:entity_type
    // -----------------------------------------------------------------------

    /// GET /entities/:graph_id/by-type/:entity_type — happy path → 200 + correct shape
    #[tokio::test]
    async fn get_entities_by_type_happy_200() {
        let (graph_id, _alice_id, entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}/by-type/{entity_type}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "by-type happy path must 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["success"], true);
        let data = &json["data"];
        // Key presence
        assert_eq!(data["entity_type"], entity_type, "entity_type must match path param");
        assert!(data["count"].is_number(), "data must have count");
        assert!(data["entities"].is_array(), "data must have entities array");
        // count == entities.len()
        let count = data["count"].as_u64().unwrap();
        let len = data["entities"].as_array().unwrap().len() as u64;
        assert_eq!(count, len, "count must equal entities.len()");
        assert_eq!(count, 2, "both Person entities must appear");
    }

    /// GET /entities/:graph_id/by-type/:entity_type — count == entities.len() invariant
    #[tokio::test]
    async fn get_entities_by_type_count_equals_len() {
        let (graph_id, _alice_id, entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}/by-type/{entity_type}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = &json["data"];
        let count = data["count"].as_u64().unwrap();
        let len = data["entities"].as_array().unwrap().len() as u64;
        assert_eq!(count, len, "count MUST equal entities.len()");
    }

    /// GET /entities/:graph_id/by-type/nonexistent — no matches → count 0, entities []
    #[tokio::test]
    async fn get_entities_by_type_no_match_count_0() {
        let (graph_id, _alice_id, _entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}/by-type/Organization"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["count"], 0);
        assert_eq!(json["data"]["entities"].as_array().unwrap().len(), 0);
    }

    /// GET /entities/:graph_id/by-type/:type — ZEP guard → 500
    #[tokio::test]
    async fn get_entities_by_type_zep_guard_500() {
        let (graph_id, _alice_id, entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app_no_zep_sim();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}/by-type/{entity_type}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none());
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (b) — Routing-collision test
    //
    // `[!] U026-ROUTE-ORDER-ENTITIES`: GET /entities/:graph_id/by-type/:entity_type
    // must NOT be captured by GET /entities/:graph_id/:entity_uuid.
    // Axum 0.7 distinguishes these routes by segment count:
    //   /entities/:g/:u          → 3 total segments (entities, g, u)
    //   /entities/:g/by-type/:t  → 4 total segments (entities, g, by-type, t)
    // A request to /entities/G/by-type/X (4 segs) MUST resolve to get_entities_by_type,
    // NOT get_entity_detail with entity_uuid="by-type".
    // -----------------------------------------------------------------------

    /// Routing: /entities/:graph_id/by-type/X must NOT match :entity_uuid="by-type"
    #[tokio::test]
    async fn route_by_type_not_captured_as_entity_uuid() {
        let (graph_id, _alice_id, entity_type) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/entities/{graph_id}/by-type/{entity_type}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "by-type route must respond 200, not 404");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);

        // Verify it went to get_entities_by_type, not get_entity_detail.
        // get_entities_by_type returns {entity_type, count, entities} inside data.
        // get_entity_detail returns a single entity node {uuid, name, labels, ...}.
        // If routing was wrong, data would have {uuid, name} not {entity_type, count, entities}.
        let data = &json["data"];
        assert!(
            data.get("entity_type").is_some(),
            "response must have entity_type key (by-type handler), not entity detail fields: {data}"
        );
        assert!(
            data.get("count").is_some(),
            "response must have count key (by-type handler): {data}"
        );
        assert!(
            data.get("entities").is_some(),
            "response must have entities key (by-type handler): {data}"
        );
        // Must NOT have entity-detail shape
        assert!(
            data.get("uuid").is_none(),
            "response must NOT have uuid key (would indicate wrong handler): {data}"
        );
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (b) — ADVERSARIAL enrich-parse fidelity (parity-verifier added)
    //
    // Python: `request.args.get('enrich','true').lower() == 'true'`
    //   → ONLY the literal "true" (case-insensitive) is True; "1", "yes" → False.
    // This REFUTES a generic bool-parse: ?enrich=1 MUST be False, ?enrich=TRUE MUST be True.
    // Observable effect: enrich populates related_edges (seed graph has Alice→Bob edge).
    // -----------------------------------------------------------------------

    /// Helper: total related_edges across all returned entities for a given enrich query.
    async fn related_edge_count_for(query: &str) -> usize {
        let (graph_id, _alice, _t) = seed_entity_graph_task();
        let (app, _tmp) = test_app();
        let uri = format!("/api/simulation/entities/{graph_id}{query}");
        let resp = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        json["data"]["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["related_edges"].as_array().map(|a| a.len()).unwrap_or(0))
            .sum()
    }

    /// ?enrich=1 MUST be treated as FALSE (compare-to-"true", NOT generic bool parse).
    #[tokio::test]
    async fn enrich_one_is_false_no_edges() {
        let enriched = related_edge_count_for("?enrich=1").await;
        assert_eq!(
            enriched, 0,
            "?enrich=1 must be FALSE → no related_edges (Python .lower()=='true')"
        );
    }

    /// ?enrich=yes MUST be treated as FALSE.
    #[tokio::test]
    async fn enrich_yes_is_false_no_edges() {
        let enriched = related_edge_count_for("?enrich=yes").await;
        assert_eq!(enriched, 0, "?enrich=yes must be FALSE → no related_edges");
    }

    /// ADVERSARIAL: Route 2 with an UNKNOWN graph_id.
    /// Python `get_entity_detail` → `get_entity_with_context` catches ALL exceptions → None → 404.
    /// Rust `load_entity_reader_graph` task-not-found → 500. This probe RECORDS the actual status
    /// so the parity verdict can classify it (divergence vs defensible [≠]).
    #[tokio::test]
    async fn probe_route2_unknown_graph_status() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/entities/no-such-graph-zzz/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Record observed status for the parity ledger (no hard assert — this is a probe).
        eprintln!("PROBE route2 unknown-graph status = {}", resp.status());
        assert!(
            resp.status() == StatusCode::INTERNAL_SERVER_ERROR
                || resp.status() == StatusCode::NOT_FOUND,
            "route2 unknown-graph returned unexpected status: {}",
            resp.status()
        );
    }

    /// ?enrich=TRUE (uppercase) MUST be treated as TRUE (case-insensitive compare).
    #[tokio::test]
    async fn enrich_uppercase_true_is_true_has_edges() {
        let enriched = related_edge_count_for("?enrich=TRUE").await;
        assert!(enriched > 0, "?enrich=TRUE must be TRUE → related_edges populated");
        // And default (absent) must also be TRUE (Python default 'true').
        let default_enriched = related_edge_count_for("").await;
        assert!(default_enriched > 0, "absent enrich defaults TRUE → related_edges populated");
        // And explicit ?enrich=false must be FALSE.
        let off = related_edge_count_for("?enrich=false").await;
        assert_eq!(off, 0, "?enrich=false must be FALSE → no related_edges");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (e) test helpers
    // -----------------------------------------------------------------------

    /// Seed a simulation and return (state, sim_id, tmp) for sub-cycle (e) tests.
    fn seed_sim(state: &Arc<ApiState>) -> (String,) {
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("E Test Project").expect("seed project");
        p.graph_id = Some("graph-e".to_string());
        pm.save_project(&mut p).expect("save project");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "graph-e", true, true)
            .expect("create sim");
        (sim.simulation_id.clone(),)
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (e) Route 1 — GET /:simulation_id/profiles
    // -----------------------------------------------------------------------

    /// GET /:id/profiles — happy path: platform=reddit, no profiles file yet → empty array
    #[tokio::test]
    async fn get_profiles_happy_empty() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/profiles"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "profiles must return 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["platform"], "reddit");
        assert_eq!(data["count"], 0);
        assert!(data["profiles"].as_array().unwrap().is_empty());

        // Key order: platform → count → profiles
        let keys: Vec<&str> = data.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        assert_eq!(
            keys,
            ["platform", "count", "profiles"],
            "key order must be platform,count,profiles"
        );
    }

    /// GET /:id/profiles — nonexistent simulation_id → 404
    #[tokio::test]
    async fn get_profiles_not_found_404() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_nonexistent/profiles")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json["error"].as_str().is_some(), "error field must be present");
        // 404 is client error → 2-key body (no traceback)
        assert!(json.get("traceback").is_none(), "404 must not have traceback");
    }

    /// GET /:id/profiles — with profiles file containing JSON array → count/profiles match
    #[tokio::test]
    async fn get_profiles_with_data() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);

        // Write a reddit_profiles.json into the sim dir
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);
        let profiles_data = serde_json::json!([{"id":1,"name":"Alice"},{"id":2,"name":"Bob"}]);
        std::fs::write(sim_dir.join("reddit_profiles.json"), profiles_data.to_string()).unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/profiles"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["count"], 2);
        assert_eq!(json["data"]["profiles"].as_array().unwrap().len(), 2);
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (e) Route 2 — GET /:simulation_id/profiles/realtime
    // -----------------------------------------------------------------------

    /// GET /:id/profiles/realtime — nonexistent sim_dir → 404
    #[tokio::test]
    async fn get_profiles_realtime_not_found_404() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_no_dir/profiles/realtime")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "404 must not have traceback");
    }

    /// GET /:id/profiles/realtime — file_exists=false → metadata shape correct
    #[tokio::test]
    async fn get_profiles_realtime_file_not_present_shape() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/profiles/realtime"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["simulation_id"], sim_id);
        assert_eq!(data["platform"], "reddit");
        assert_eq!(data["file_exists"], false);
        assert!(
            data["file_modified_at"].is_null(),
            "file_modified_at must be null when file absent"
        );
        assert_eq!(data["is_generating"], false);
        // total_expected mirrors state.json entities_count (0 after create, not null)
        assert!(
            data["total_expected"].is_number() || data["total_expected"].is_null(),
            "total_expected must be a number or null"
        );
        assert_eq!(data["count"], 0);
        assert!(data["profiles"].as_array().unwrap().is_empty());

        // Key order assertion: simulation_id, platform, count, total_expected, is_generating,
        //                      file_exists, file_modified_at, profiles
        let keys: Vec<&str> = data.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        assert_eq!(
            keys,
            [
                "simulation_id",
                "platform",
                "count",
                "total_expected",
                "is_generating",
                "file_exists",
                "file_modified_at",
                "profiles"
            ],
            "realtime profiles key order must match Python source"
        );
    }

    /// GET /:id/profiles/realtime — file_exists=true → file_modified_at is non-null string,
    ///   is_generating reads from state.json
    #[tokio::test]
    async fn get_profiles_realtime_file_exists_mtime_and_state() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);

        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);

        // Write profiles file
        std::fs::write(
            sim_dir.join("reddit_profiles.json"),
            serde_json::json!([{"name":"Alice"}]).to_string(),
        )
        .unwrap();

        // Write state.json with status=preparing and entities_count=50
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::json!({"status":"preparing","entities_count":50}).to_string(),
        )
        .unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/profiles/realtime"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["file_exists"], true);
        // file_modified_at must be a non-null ISO string
        assert!(
            data["file_modified_at"].is_string(),
            "file_modified_at must be a string when file exists"
        );
        let mtime_str = data["file_modified_at"].as_str().unwrap();
        assert!(
            mtime_str.starts_with("20"),
            "file_modified_at must look like ISO datetime: {mtime_str}"
        );
        assert_eq!(data["is_generating"], true);
        assert_eq!(data["total_expected"], 50);
        assert_eq!(data["count"], 1);
    }

    /// GET /:id/profiles/realtime — is_generating transitions driven by state.json
    #[tokio::test]
    async fn get_profiles_realtime_is_generating_false_when_ready() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);

        // state.json with status=ready
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::json!({"status":"ready","entities_count":10}).to_string(),
        )
        .unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/profiles/realtime"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["is_generating"], false);
        assert_eq!(json["data"]["total_expected"], 10);
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (e) Route 3 — GET /:simulation_id/config/realtime
    // -----------------------------------------------------------------------

    /// GET /:id/config/realtime — nonexistent sim_dir → 404
    #[tokio::test]
    async fn get_config_realtime_not_found_404() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_no_dir/config/realtime")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "404 must not have traceback");
    }

    /// GET /:id/config/realtime — file absent → file_exists=false, shape correct, key order
    #[tokio::test]
    async fn get_config_realtime_file_absent_shape_and_key_order() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/config/realtime"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["simulation_id"], sim_id);
        assert_eq!(data["file_exists"], false);
        assert!(data["file_modified_at"].is_null());
        assert_eq!(data["is_generating"], false);
        assert!(data["generation_stage"].is_null());
        assert_eq!(data["config_generated"], false);
        assert!(data["config"].is_null());
        // summary must NOT be present when config is null
        assert!(data.get("summary").is_none(), "summary must be absent when config=null");

        // Key order: simulation_id, file_exists, file_modified_at, is_generating,
        //            generation_stage, config_generated, config
        let keys: Vec<&str> = data.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        assert_eq!(
            keys,
            [
                "simulation_id",
                "file_exists",
                "file_modified_at",
                "is_generating",
                "generation_stage",
                "config_generated",
                "config"
            ],
            "config/realtime key order must match Python source"
        );
    }

    /// GET /:id/config/realtime — file present, state.json generating_profiles stage
    #[tokio::test]
    async fn get_config_realtime_generation_stage_generating_profiles() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);

        // state.json: preparing + profiles_generated=false → "generating_profiles"
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::json!({
                "status": "preparing",
                "profiles_generated": false,
                "config_generated": false
            })
            .to_string(),
        )
        .unwrap();

        // Write a valid config file
        let cfg = serde_json::json!({
            "agent_configs": [{"id":1},{"id":2}],
            "time_config": {"total_simulation_hours": 24},
            "event_config": {"initial_posts": [{"text":"hi"}], "hot_topics": ["ai","rust"]},
            "twitter_config": {},
            "generated_at": "2025-12-04T12:00:00",
            "llm_model": "gpt-4"
        });
        std::fs::write(sim_dir.join("simulation_config.json"), cfg.to_string()).unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/config/realtime"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["is_generating"], true);
        assert_eq!(data["generation_stage"], "generating_profiles");
        assert_eq!(data["config_generated"], false);
        assert_eq!(data["file_exists"], true);
        // file_modified_at must be non-null (mtime test — [≠] U026-MTIME)
        assert!(
            data["file_modified_at"].is_string(),
            "file_modified_at must be set when file present"
        );

        // summary must be present because config is a non-null object
        let summary = data.get("summary").expect("summary must be present when config is non-null");
        assert_eq!(summary["total_agents"], 2);
        assert_eq!(summary["simulation_hours"], 24);
        assert_eq!(summary["initial_posts_count"], 1);
        assert_eq!(summary["hot_topics_count"], 2);
        assert_eq!(summary["has_twitter_config"], true);
        assert_eq!(summary["has_reddit_config"], false);
        assert_eq!(summary["generated_at"], "2025-12-04T12:00:00");
        assert_eq!(summary["llm_model"], "gpt-4");

        // Summary key order: total_agents, simulation_hours, initial_posts_count, hot_topics_count,
        //                    has_twitter_config, has_reddit_config, generated_at, llm_model
        let summary_keys: Vec<&str> =
            summary.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        assert_eq!(
            summary_keys,
            [
                "total_agents",
                "simulation_hours",
                "initial_posts_count",
                "hot_topics_count",
                "has_twitter_config",
                "has_reddit_config",
                "generated_at",
                "llm_model"
            ],
            "summary key order must match Python source"
        );
    }

    /// GET /:id/config/realtime — generation_stage transitions
    #[tokio::test]
    async fn get_config_realtime_generation_stage_transitions() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);

        // Test generating_config stage: preparing + profiles_generated=true
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::json!({
                "status": "preparing",
                "profiles_generated": true,
                "config_generated": false
            })
            .to_string(),
        )
        .unwrap();

        let app = crate::server::create_app(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/config/realtime"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["generation_stage"], "generating_config");

        // Test completed stage: status=ready
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::json!({"status": "ready", "config_generated": true}).to_string(),
        )
        .unwrap();

        let app2 = crate::server::create_app(Arc::clone(&state));
        let resp2 = app2
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/config/realtime"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes2 = axum::body::to_bytes(resp2.into_body(), usize::MAX).await.unwrap();
        let json2: serde_json::Value = serde_json::from_slice(&bytes2).unwrap();
        assert_eq!(json2["data"]["generation_stage"], "completed");
        assert_eq!(json2["data"]["config_generated"], true);
        assert_eq!(json2["data"]["is_generating"], false);
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (e) Route 4 — GET /:simulation_id/config
    // -----------------------------------------------------------------------

    /// GET /:id/config — config file absent → 404 configNotFound
    #[tokio::test]
    async fn get_config_not_found_404() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/config"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json["error"].as_str().is_some());
        assert!(json.get("traceback").is_none(), "404 must not have traceback");
    }

    /// GET /:id/config — config file present → 200 {success:true, data:config}
    #[tokio::test]
    async fn get_config_happy_200() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);

        // Write simulation_config.json
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);
        let cfg = serde_json::json!({"llm_model": "gpt-4", "agent_configs": []});
        std::fs::write(sim_dir.join("simulation_config.json"), cfg.to_string()).unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/config"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["llm_model"], "gpt-4");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (e) Route 5 — GET /:simulation_id/config/download
    // -----------------------------------------------------------------------

    /// GET /:id/config/download — config file absent → 404 configFileNotFound
    #[tokio::test]
    async fn download_config_not_found_404() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/config/download"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "404 must not have traceback");
    }

    /// GET /:id/config/download — config present → 200, Content-Disposition attachment,
    ///   Content-Type application/json, body = file bytes
    #[tokio::test]
    async fn download_config_happy_200_attachment() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);

        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);
        let cfg_content = r#"{"llm_model":"gpt-4","agent_configs":[]}"#;
        std::fs::write(sim_dir.join("simulation_config.json"), cfg_content).unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/config/download"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "download must return 200");

        // Check headers
        let content_type = resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            content_type.contains("application/json"),
            "Content-Type must be application/json, got: {content_type}"
        );

        let content_disp = resp
            .headers()
            .get(axum::http::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            content_disp.contains("attachment"),
            "Content-Disposition must contain 'attachment': {content_disp}"
        );
        assert!(
            content_disp.contains("simulation_config.json"),
            "Content-Disposition must contain filename: {content_disp}"
        );

        // Check body bytes match file content
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            body_bytes.as_ref(),
            cfg_content.as_bytes(),
            "body must equal file content exactly"
        );
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (e2) — GET /script/:script_name/download
    //   [≠] U026-SCRIPTDL: teri ships no run_*.py (native in-process simulation).
    //   Validation boundary ports verbatim (400 unknownScript); a valid name → 404.
    // -----------------------------------------------------------------------

    /// GET /script/<name>/download — name NOT in the allowed list → 400 unknownScript.
    /// The error interpolates `name` and the Python-`str(list)`-repr `allowed` list.
    #[tokio::test]
    async fn download_script_unknown_name_400() {
        let (state, _tmp) = test_state();
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/script/not_a_real_script.py/download")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "400 must not have traceback");
        let err = json["error"].as_str().unwrap();
        assert!(err.contains("not_a_real_script.py"), "error must name the bad script: {err}");
        // Python `str(allowed_scripts)` repr is single-quoted; at least one allowed entry shows.
        assert!(
            err.contains("run_twitter_simulation.py"),
            "error must list the allowed scripts: {err}"
        );
    }

    /// GET /script/<name>/download — VALID name, but teri ships no run_*.py → 404
    /// scriptFileNotFound ([≠] U026-SCRIPTDL: native in-process, scripts dir always empty).
    /// Never 200, never empty body, never dropped.
    #[tokio::test]
    async fn download_script_valid_name_404_no_native_script() {
        let (state, _tmp) = test_state();
        let app = crate::server::create_app(state);
        for name in [
            "run_twitter_simulation.py",
            "run_reddit_simulation.py",
            "run_parallel_simulation.py",
            "action_logger.py",
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/simulation/script/{name}/download"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "valid script {name} must 404 (no native run_*.py)"
            );
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(json["success"], false);
            assert!(json.get("traceback").is_none(), "404 must not have traceback");
            assert!(
                json["error"].as_str().unwrap().contains(name),
                "scriptFileNotFound must name the script"
            );
        }
    }

    /// Route disambiguation: `/script/<name>/download` (static seg0) must resolve to
    /// `download_script`, NOT collide with the `/:simulation_id/config/download` capture route.
    #[tokio::test]
    async fn download_script_route_distinct_from_config_download() {
        let (state, _tmp) = test_state();
        let app = crate::server::create_app(state);
        // A name shaped like a script reaches download_script → 400 (unknown), proving it did
        // NOT route into config/download (which would 404 with configFileNotFound, no "allowed").
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/script/bogus.py/download")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            json["error"].as_str().unwrap().contains("run_twitter_simulation.py"),
            "must be download_script's unknownScript error, not config/download's 404"
        );
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (h) — GET /:id/run-status  and  /:id/run-status/detail
    //   Poll snapshots. None run_state → idle stub (200, NOT 404). Detail action
    //   lists read the U-047 tail (empty until U-028/029/030 producers wire it).
    // -----------------------------------------------------------------------

    /// Write a `run_state.json` so `get_run_state` loads a non-idle state from disk.
    fn seed_run_state(state: &Arc<ApiState>, sim_id: &str, json: serde_json::Value) {
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(sim_id);
        std::fs::create_dir_all(&sim_dir).unwrap();
        std::fs::write(sim_dir.join("run_state.json"), serde_json::to_vec(&json).unwrap()).unwrap();
    }

    /// GET /:id/run-status — no run_state → 200 with the 8-key idle stub (NOT 404).
    #[tokio::test]
    async fn run_status_idle_stub_when_no_run_state() {
        let (state, _tmp) = test_state();
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_x/run-status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "idle run-status must be 200, not 404");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["runner_status"], "idle");
        assert_eq!(data["simulation_id"], "sim_x");
        assert_eq!(data["current_round"], 0);
        assert_eq!(data["total_actions_count"], 0);
        // progress_percent is the Python int literal 0 in the idle stub (full to_dict emits f64).
        assert!(
            data["progress_percent"].is_i64() || data["progress_percent"].is_u64(),
            "idle progress_percent must be integer 0, got {}",
            data["progress_percent"]
        );
        assert_eq!(data.as_object().unwrap().len(), 8, "idle stub is exactly 8 keys");
    }

    /// GET /:id/run-status — run_state present → 200 with full to_dict (total_actions_count computed).
    #[tokio::test]
    async fn run_status_returns_to_dict_when_state_present() {
        let (state, _tmp) = test_state();
        seed_run_state(
            &state,
            "sim_run",
            serde_json::json!({
                "runner_status": "running",
                "current_round": 5,
                "total_rounds": 144,
                "twitter_actions_count": 150,
                "reddit_actions_count": 200,
                "twitter_running": true,
                "reddit_running": true,
                "updated_at": "2025-12-01T10:30:00"
            }),
        );
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_run/run-status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = &json["data"];
        assert_eq!(data["runner_status"], "running");
        assert_eq!(data["current_round"], 5);
        assert_eq!(data["total_actions_count"], 350, "computed twitter+reddit");
        // The full to_dict carries process_pid (the idle stub does not) and is larger than 8 keys.
        assert!(data.get("process_pid").is_some(), "full to_dict has process_pid");
        assert!(data.as_object().unwrap().len() > 8, "full to_dict > idle stub");
    }

    /// GET /:id/run-status/detail — no run_state → 200 with the 5-key idle stub.
    #[tokio::test]
    async fn run_status_detail_idle_stub_when_no_run_state() {
        let (state, _tmp) = test_state();
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_x/run-status/detail")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = &json["data"];
        assert_eq!(data["runner_status"], "idle");
        assert_eq!(data["all_actions"], serde_json::json!([]));
        assert_eq!(data["twitter_actions"], serde_json::json!([]));
        assert_eq!(data["reddit_actions"], serde_json::json!([]));
        assert_eq!(data.as_object().unwrap().len(), 5, "detail idle stub is exactly 5 keys");
    }

    /// GET /:id/run-status/detail — run_state present, no actions.jsonl → base to_dict + the five
    /// extra keys, all action lists empty (faithful no-op on missing tail; producer-pending).
    #[tokio::test]
    async fn run_status_detail_assembles_keys_with_empty_tail() {
        let (state, _tmp) = test_state();
        seed_run_state(
            &state,
            "sim_d",
            serde_json::json!({"runner_status":"running","current_round":0,"total_rounds":10}),
        );
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_d/run-status/detail")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = &json["data"];
        assert_eq!(data["runner_status"], "running", "base to_dict merged");
        assert_eq!(data["all_actions"], serde_json::json!([]));
        assert_eq!(data["twitter_actions"], serde_json::json!([]));
        assert_eq!(data["reddit_actions"], serde_json::json!([]));
        assert_eq!(data["recent_actions"], serde_json::json!([]), "current_round 0 → no recent");
        assert_eq!(data["rounds_count"], 0);
    }

    /// GET /:id/run-status/detail — actions.jsonl present → lists populate from the tail; the
    /// `?platform=` filter gates twitter/reddit lists per Python's falsy semantics.
    #[tokio::test]
    async fn run_status_detail_reads_actions_tail_and_filters_platform() {
        let (state, _tmp) = test_state();
        seed_run_state(
            &state,
            "sim_act",
            serde_json::json!({"runner_status":"running","current_round":1,"total_rounds":10}),
        );
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join("sim_act");
        std::fs::create_dir_all(sim_dir.join("twitter")).unwrap();
        std::fs::create_dir_all(sim_dir.join("reddit")).unwrap();
        std::fs::write(
            sim_dir.join("twitter").join("actions.jsonl"),
            b"{\"round\":1,\"timestamp\":\"2025-12-01T10:00:00\",\"agent_id\":3,\"agent_name\":\"T\",\"action_type\":\"CREATE_POST\"}\n",
        )
        .unwrap();
        std::fs::write(
            sim_dir.join("reddit").join("actions.jsonl"),
            b"{\"round\":1,\"timestamp\":\"2025-12-01T10:01:00\",\"agent_id\":4,\"agent_name\":\"R\",\"action_type\":\"CREATE_COMMENT\"}\n",
        )
        .unwrap();
        let app = crate::server::create_app(state);

        // No filter: all_actions = both; twitter_actions = twitter; reddit_actions = reddit;
        // recent_actions = current round (1) = both.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_act/run-status/detail")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = &json["data"];
        assert_eq!(data["all_actions"].as_array().unwrap().len(), 2);
        assert_eq!(data["twitter_actions"].as_array().unwrap().len(), 1);
        assert_eq!(data["reddit_actions"].as_array().unwrap().len(), 1);
        assert_eq!(data["recent_actions"].as_array().unwrap().len(), 2);
        assert_eq!(data["twitter_actions"][0]["platform"], "twitter");
        assert_eq!(data["reddit_actions"][0]["platform"], "reddit");

        // ?platform=twitter: all_actions filtered to twitter; reddit_actions gated to [].
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_act/run-status/detail?platform=twitter")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = &json["data"];
        assert_eq!(data["all_actions"].as_array().unwrap().len(), 1, "filtered to twitter");
        assert_eq!(data["twitter_actions"].as_array().unwrap().len(), 1);
        assert_eq!(data["reddit_actions"].as_array().unwrap().len(), 0, "reddit gated off");
    }

    /// Parity regression (gate-caught): empty-string `?platform=` is FALSY in Python
    /// (`not platform` is True), so `get_all_actions("")` reads BOTH platform files and skips the
    /// record-level filter. all_actions/recent_actions must read both — NOT be filtered to empty.
    #[tokio::test]
    async fn run_status_detail_empty_platform_query_reads_both() {
        let (state, _tmp) = test_state();
        seed_run_state(
            &state,
            "sim_e",
            serde_json::json!({"runner_status":"running","current_round":1,"total_rounds":10}),
        );
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join("sim_e");
        std::fs::create_dir_all(sim_dir.join("twitter")).unwrap();
        std::fs::create_dir_all(sim_dir.join("reddit")).unwrap();
        std::fs::write(
            sim_dir.join("twitter").join("actions.jsonl"),
            b"{\"round\":1,\"timestamp\":\"2025-12-01T10:00:00\",\"agent_id\":3,\"agent_name\":\"T\",\"action_type\":\"CREATE_POST\"}\n",
        )
        .unwrap();
        std::fs::write(
            sim_dir.join("reddit").join("actions.jsonl"),
            b"{\"round\":1,\"timestamp\":\"2025-12-01T10:01:00\",\"agent_id\":4,\"agent_name\":\"R\",\"action_type\":\"CREATE_COMMENT\"}\n",
        )
        .unwrap();
        let app = crate::server::create_app(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_e/run-status/detail?platform=")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = &json["data"];
        // Empty filter is falsy → both files read, no record filter applied.
        assert_eq!(data["all_actions"].as_array().unwrap().len(), 2, "empty filter reads both");
        assert_eq!(data["recent_actions"].as_array().unwrap().len(), 2, "empty filter recent=both");
        // twitter/reddit lists also populate (`not platform` is True for both gates).
        assert_eq!(data["twitter_actions"].as_array().unwrap().len(), 1);
        assert_eq!(data["reddit_actions"].as_array().unwrap().len(), 1);
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (i) — GET /:id/actions, /:id/timeline, /:id/agent-stats
    //   Read the U-047 actions.jsonl tail. Empty contract until producers land;
    //   read path proven with real jsonl fixtures.
    // -----------------------------------------------------------------------

    /// Seed a twitter actions.jsonl with the given raw lines (each a JSON object).
    fn seed_actions(state: &Arc<ApiState>, sim_id: &str, platform: &str, lines: &[&str]) {
        let dir = std::path::PathBuf::from(&state.config.oasis_simulation_data_dir)
            .join(sim_id)
            .join(platform);
        std::fs::create_dir_all(&dir).unwrap();
        let body = lines.join("\n") + "\n";
        std::fs::write(dir.join("actions.jsonl"), body.as_bytes()).unwrap();
    }

    /// GET /:id/actions — no log → 200 {count:0, actions:[]} (faithful empty contract).
    #[tokio::test]
    async fn get_actions_empty_contract() {
        let (state, _tmp) = test_state();
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_a/actions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["count"], 0);
        assert_eq!(json["data"]["actions"], serde_json::json!([]));
    }

    /// GET /:id/actions — populated log → pagination via limit/offset; bad `?limit=` falls back
    /// to the default (Flask type=int graceful fallback, NOT 400).
    #[tokio::test]
    async fn get_actions_reads_tail_with_pagination_and_int_fallback() {
        let (state, _tmp) = test_state();
        seed_actions(
            &state,
            "sim_b",
            "twitter",
            &[
                r#"{"round":1,"timestamp":"2025-12-01T10:00:00","agent_id":1,"agent_name":"A","action_type":"X"}"#,
                r#"{"round":1,"timestamp":"2025-12-01T10:01:00","agent_id":2,"agent_name":"B","action_type":"Y"}"#,
                r#"{"round":2,"timestamp":"2025-12-01T10:02:00","agent_id":1,"agent_name":"A","action_type":"Z"}"#,
            ],
        );
        let app = crate::server::create_app(state);

        // limit=2 → 2 newest (sorted desc).
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_b/actions?limit=2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["count"], 2);
        assert_eq!(json["data"]["actions"].as_array().unwrap().len(), 2);

        // Bad ?limit=abc → falls back to default 100 → all 3 (NOT a 400).
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_b/actions?limit=abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "bad int must fall back, not 400");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["count"], 3);
    }

    /// GET /:id/actions — agent_id filter narrows results.
    #[tokio::test]
    async fn get_actions_agent_id_filter() {
        let (state, _tmp) = test_state();
        seed_actions(
            &state,
            "sim_f",
            "twitter",
            &[
                r#"{"round":1,"timestamp":"2025-12-01T10:00:00","agent_id":1,"agent_name":"A","action_type":"X"}"#,
                r#"{"round":1,"timestamp":"2025-12-01T10:01:00","agent_id":2,"agent_name":"B","action_type":"Y"}"#,
            ],
        );
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_f/actions?agent_id=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["count"], 1, "filtered to agent 1");
        assert_eq!(json["data"]["actions"][0]["agent_id"], 1);
    }

    /// GET /:id/timeline — empty → {rounds_count:0, timeline:[]}; populated → per-round entries.
    #[tokio::test]
    async fn get_timeline_empty_and_populated() {
        let (state, _tmp) = test_state();
        // Empty
        {
            let app = crate::server::create_app(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/simulation/sim_c/timeline")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(json["data"]["rounds_count"], 0);
            assert_eq!(json["data"]["timeline"], serde_json::json!([]));
        }
        // Populated: 2 actions in round 1, 1 in round 2 → 2 timeline entries.
        seed_actions(
            &state,
            "sim_c",
            "twitter",
            &[
                r#"{"round":1,"timestamp":"2025-12-01T10:00:00","agent_id":1,"agent_name":"A","action_type":"X"}"#,
                r#"{"round":1,"timestamp":"2025-12-01T10:01:00","agent_id":2,"agent_name":"B","action_type":"Y"}"#,
                r#"{"round":2,"timestamp":"2025-12-01T10:02:00","agent_id":1,"agent_name":"A","action_type":"Z"}"#,
            ],
        );
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_c/timeline")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["rounds_count"], 2);
        let timeline = json["data"]["timeline"].as_array().unwrap();
        assert_eq!(timeline.len(), 2);
        // Full 9-key set + EXACT Python order (preserve_order makes this byte-observable).
        let keys: Vec<&String> = timeline[0].as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            vec![
                "round_num",
                "twitter_actions",
                "reddit_actions",
                "total_actions",
                "active_agents_count",
                "active_agents",
                "action_types",
                "first_action_time",
                "last_action_time"
            ],
            "timeline entry must be the full 9-key Python shape in order"
        );
        // DESC-iteration semantics: round 1 has 10:00 + 10:01; first-seen (newest) = 10:01,
        // last (oldest) = 10:00. Names are intentionally inverted vs chronology (matches Python).
        assert_eq!(timeline[0]["round_num"], 1);
        assert_eq!(timeline[0]["first_action_time"], "2025-12-01T10:01:00");
        assert_eq!(timeline[0]["last_action_time"], "2025-12-01T10:00:00");
    }

    /// GET /:id/agent-stats — empty → {agents_count:0, stats:[]}; populated → per-agent stats.
    #[tokio::test]
    async fn get_agent_stats_empty_and_populated() {
        let (state, _tmp) = test_state();
        {
            let app = crate::server::create_app(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/simulation/sim_d2/agent-stats")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(json["data"]["agents_count"], 0);
            assert_eq!(json["data"]["stats"], serde_json::json!([]));
        }
        // Populated: agent 1 (2 actions), agent 2 (1 action) → 2 stats.
        seed_actions(
            &state,
            "sim_d2",
            "twitter",
            &[
                r#"{"round":1,"timestamp":"2025-12-01T10:00:00","agent_id":1,"agent_name":"A","action_type":"X"}"#,
                r#"{"round":2,"timestamp":"2025-12-01T10:02:00","agent_id":1,"agent_name":"A","action_type":"Z"}"#,
                r#"{"round":1,"timestamp":"2025-12-01T10:01:00","agent_id":2,"agent_name":"B","action_type":"Y"}"#,
            ],
        );
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_d2/agent-stats")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["agents_count"], 2);
        let stats = json["data"]["stats"].as_array().unwrap();
        assert_eq!(stats.len(), 2);
        // Full 8-key set + EXACT Python order: action_types BEFORE the two timestamps.
        let keys: Vec<&String> = stats[0].as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            vec![
                "agent_id",
                "agent_name",
                "total_actions",
                "twitter_actions",
                "reddit_actions",
                "action_types",
                "first_action_time",
                "last_action_time"
            ],
            "agent-stats entry must be the full 8-key Python shape in order"
        );
        // Sorted by total_actions DESC → agent 1 (2 actions) first. DESC-iteration timestamps:
        // agent 1's first-seen (newest) = 10:02, last (oldest) = 10:00.
        assert_eq!(stats[0]["agent_id"], 1);
        assert_eq!(stats[0]["total_actions"], 2);
        assert_eq!(stats[0]["first_action_time"], "2025-12-01T10:02:00");
        assert_eq!(stats[0]["last_action_time"], "2025-12-01T10:00:00");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (j) — GET /:id/posts, /:id/comments  (social-DB reads)
    //   Missing DB → 200 empty contract (faithful now). Populated SELECT behind
    //   `sqlite` feature; no-sqlite-but-DB-exists → honest 500 ([!] GAP-U026-SOCIALDB).
    // -----------------------------------------------------------------------

    /// GET /:id/posts — no DB → 200 {platform, count:0, posts:[], message:dbNotExist}.
    /// Default platform "reddit"; explicit ?platform is echoed.
    #[tokio::test]
    async fn get_posts_missing_db_empty_contract() {
        let (state, _tmp) = test_state();
        let app = crate::server::create_app(state);
        // default platform = reddit
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_np/posts")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["platform"], "reddit");
        assert_eq!(data["count"], 0);
        assert_eq!(data["posts"], serde_json::json!([]));
        assert!(data.get("message").is_some(), "missing-DB carries dbNotExist message");
        // no `total` key on the empty branch (only on populated)
        assert!(data.get("total").is_none());
        assert_eq!(data.as_object().unwrap().len(), 4, "empty posts = 4 keys");

        // explicit ?platform=twitter is echoed
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_np/posts?platform=twitter")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["platform"], "twitter");
    }

    /// GET /:id/comments — no DB → 200 {count:0, comments:[]} (2-key).
    #[tokio::test]
    async fn get_comments_missing_db_empty_contract() {
        let (state, _tmp) = test_state();
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_nc/comments")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = &json["data"];
        assert_eq!(data["count"], 0);
        assert_eq!(data["comments"], serde_json::json!([]));
        assert_eq!(data.as_object().unwrap().len(), 2, "empty comments = 2 keys");
    }

    /// No-sqlite build: a DB that DOES exist must produce an HONEST 500 (GAP-U026-SOCIALDB),
    /// never a silent empty (no-downgrade). Only meaningful when built WITHOUT the sqlite feature.
    #[cfg(not(feature = "sqlite"))]
    #[tokio::test]
    async fn get_posts_db_exists_without_sqlite_feature_honest_500() {
        let (state, _tmp) = test_state();
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join("sim_db");
        std::fs::create_dir_all(&sim_dir).unwrap();
        std::fs::write(sim_dir.join("reddit_simulation.db"), b"not-empty").unwrap();
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_db/posts")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB present + no sqlite feature must be an honest 500, not a silent empty"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json["error"].as_str().unwrap().contains("GAP-U026-SOCIALDB"));
    }

    /// sqlite build: real SELECT over a hand-built `post` table — ordering, count, total, pagination.
    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn get_posts_populated_from_sqlite() {
        let (state, _tmp) = test_state();
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join("sim_pp");
        std::fs::create_dir_all(&sim_dir).unwrap();
        {
            let conn = rusqlite::Connection::open(sim_dir.join("reddit_simulation.db")).unwrap();
            conn.execute_batch(
                "CREATE TABLE post (post_id INTEGER, content TEXT, created_at TEXT);
                 INSERT INTO post VALUES (1,'hello','2025-12-01T10:00:00');
                 INSERT INTO post VALUES (2,'world','2025-12-01T10:01:00');",
            )
            .unwrap();
        }
        let app = crate::server::create_app(state);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_pp/posts")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let data = &json["data"];
        assert_eq!(data["platform"], "reddit");
        assert_eq!(data["total"], 2);
        assert_eq!(data["count"], 2);
        // ORDER BY created_at DESC → post 2 first; row keyed by column name (Python dict(row)).
        assert_eq!(data["posts"][0]["post_id"], 2);
        assert_eq!(data["posts"][0]["content"], "world");

        // ?limit=1 → count 1, total still 2 (COUNT(*) is unpaginated).
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_pp/posts?limit=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["count"], 1);
        assert_eq!(json["data"]["total"], 2);
    }

    /// sqlite build: real SELECT over a `comment` table + optional post_id filter.
    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn get_comments_populated_from_sqlite_with_post_id_filter() {
        let (state, _tmp) = test_state();
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join("sim_cc");
        std::fs::create_dir_all(&sim_dir).unwrap();
        {
            let conn = rusqlite::Connection::open(sim_dir.join("reddit_simulation.db")).unwrap();
            conn.execute_batch(
                "CREATE TABLE comment (comment_id INTEGER, post_id INTEGER, content TEXT, created_at TEXT);
                 INSERT INTO comment VALUES (1,10,'a','2025-12-01T10:00:00');
                 INSERT INTO comment VALUES (2,10,'b','2025-12-01T10:01:00');
                 INSERT INTO comment VALUES (3,20,'c','2025-12-01T10:02:00');",
            )
            .unwrap();
        }
        let app = crate::server::create_app(state);

        // No filter → all 3.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_cc/comments")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["count"], 3);

        // post_id=10 → 2 comments.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/simulation/sim_cc/comments?post_id=10")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["data"]["count"], 2, "filtered to post_id 10");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (k) — interview routes (/interview, /interview/{batch,all,history})
    //   Validation + env-gate fully tested (env never alive now → 400 envNotRunning,
    //   the faithful IPC-PRODUCER-PENDING contract). History no-DB empty + sqlite reads.
    // -----------------------------------------------------------------------

    fn post_json(uri: &str, body: serde_json::Value) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn body_json(resp: axum::http::Response<Body>) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// optimize_interview_prompt: empty→empty; non-prefixed→prefixed; already-prefixed→unchanged.
    #[test]
    fn optimize_interview_prompt_behaviors() {
        assert_eq!(optimize_interview_prompt(""), "");
        let p = optimize_interview_prompt("你好");
        assert!(p.starts_with(INTERVIEW_PROMPT_PREFIX), "prefix prepended");
        assert!(p.ends_with("你好"));
        // No double prefix.
        assert_eq!(optimize_interview_prompt(&p), p);
    }

    // --- /interview ---

    #[tokio::test]
    async fn interview_missing_simulation_id_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json("/api/simulation/interview", serde_json::json!({})))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn interview_missing_agent_id_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview",
                serde_json::json!({"simulation_id":"s"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// agent_id 0 is VALID (Python `if agent_id is None`); with a prompt missing it must fall
    /// through to requirePrompt, NOT requireAgentId.
    #[tokio::test]
    async fn interview_agent_id_zero_is_valid_then_requires_prompt() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview",
                serde_json::json!({"simulation_id":"s","agent_id":0}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        // The error is requirePrompt (agent_id 0 passed), not requireAgentId.
        assert_eq!(json["error"], crate::i18n::t("api.requirePrompt"));
    }

    #[tokio::test]
    async fn interview_bad_platform_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview",
                serde_json::json!({"simulation_id":"s","agent_id":0,"prompt":"q","platform":"x"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert_eq!(json["error"], crate::i18n::t("api.invalidInterviewPlatform"));
    }

    /// Valid request, but no running env → 400 envNotRunning (the faithful producer-pending path).
    #[tokio::test]
    async fn interview_valid_but_no_env_400_env_not_running() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview",
                serde_json::json!({"simulation_id":"s","agent_id":0,"prompt":"q"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert_eq!(json["error"], crate::i18n::t("api.envNotRunning"));
    }

    // --- /interview/batch ---

    #[tokio::test]
    async fn interview_batch_missing_interviews_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview/batch",
                serde_json::json!({"simulation_id":"s"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert_eq!(json["error"], crate::i18n::t("api.requireInterviews"));
    }

    #[tokio::test]
    async fn interview_batch_item_missing_agent_id_400_with_index() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview/batch",
                serde_json::json!({"simulation_id":"s","interviews":[{"prompt":"q"}]}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        // 1-based index in the message (interviewListMissingAgentId index=1).
        assert_eq!(
            json["error"],
            crate::i18n::t_args("api.interviewListMissingAgentId", &[("index", &"1")])
        );
    }

    #[tokio::test]
    async fn interview_batch_item_missing_prompt_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview/batch",
                serde_json::json!({"simulation_id":"s","interviews":[{"agent_id":0}]}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn interview_batch_valid_but_no_env_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview/batch",
                serde_json::json!({"simulation_id":"s","interviews":[{"agent_id":0,"prompt":"q"}]}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert_eq!(json["error"], crate::i18n::t("api.envNotRunning"));
    }

    // --- /interview/all ---

    #[tokio::test]
    async fn interview_all_missing_prompt_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview/all",
                serde_json::json!({"simulation_id":"s"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert_eq!(json["error"], crate::i18n::t("api.requirePrompt"));
    }

    #[tokio::test]
    async fn interview_all_valid_but_no_env_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview/all",
                serde_json::json!({"simulation_id":"s","prompt":"q"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert_eq!(json["error"], crate::i18n::t("api.envNotRunning"));
    }

    // --- /interview/history ---

    #[tokio::test]
    async fn interview_history_missing_simulation_id_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json("/api/simulation/interview/history", serde_json::json!({})))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// No DB → 200 {count:0, history:[]} (faithful; the same in sqlite and non-sqlite builds).
    #[tokio::test]
    async fn interview_history_no_db_empty() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview/history",
                serde_json::json!({"simulation_id":"s_no_db"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["count"], 0);
        assert_eq!(json["data"]["history"], serde_json::json!([]));
    }

    /// No-sqlite build: an interview DB that EXISTS → honest 500 (GAP-U026-SOCIALDB), not silent empty.
    #[cfg(not(feature = "sqlite"))]
    #[tokio::test]
    async fn interview_history_db_exists_without_sqlite_honest_500() {
        let (state, _tmp) = test_state();
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join("s_db");
        std::fs::create_dir_all(&sim_dir).unwrap();
        std::fs::write(sim_dir.join("reddit_simulation.db"), b"x").unwrap();
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview/history",
                serde_json::json!({"simulation_id":"s_db"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = body_json(resp).await;
        assert!(json["error"].as_str().unwrap().contains("GAP-U026-SOCIALDB"));
    }

    /// sqlite build: real interview-history read from a `trace` table.
    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn interview_history_populated_from_sqlite() {
        let (state, _tmp) = test_state();
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join("s_hist");
        std::fs::create_dir_all(&sim_dir).unwrap();
        {
            let conn = rusqlite::Connection::open(sim_dir.join("reddit_simulation.db")).unwrap();
            conn.execute_batch(
                "CREATE TABLE trace (user_id INTEGER, action TEXT, info TEXT, created_at TEXT);
                 INSERT INTO trace VALUES (0,'interview','{\"prompt\":\"q\",\"response\":\"r\"}','2025-12-01T10:00:00');
                 INSERT INTO trace VALUES (1,'post','{}','2025-12-01T10:01:00');",
            )
            .unwrap();
        }
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(post_json(
                "/api/simulation/interview/history",
                serde_json::json!({"simulation_id":"s_hist","platform":"reddit"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        // Only the action='interview' row is returned.
        assert_eq!(json["data"]["count"], 1);
        assert_eq!(json["data"]["history"][0]["agent_id"], 0);
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (l) — env-status (pure read) / close-env (IPC)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn env_status_missing_simulation_id_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json("/api/simulation/env-status", serde_json::json!({})))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// No env → 200 with env_alive:false, both-available:false, envNotRunningShort message.
    #[tokio::test]
    async fn env_status_no_env_200_default_shape() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/env-status",
                serde_json::json!({"simulation_id":"s_env"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["simulation_id"], "s_env");
        assert_eq!(data["env_alive"], false);
        assert_eq!(data["twitter_available"], false);
        assert_eq!(data["reddit_available"], false);
        assert_eq!(data["message"], crate::i18n::t("api.envNotRunningShort"));
        assert_eq!(data.as_object().unwrap().len(), 5, "env-status data = 5 keys");
    }

    /// env-status reads env_status.json: a file with twitter_available:true surfaces in the response.
    #[tokio::test]
    async fn env_status_reads_env_status_json() {
        let (state, _tmp) = test_state();
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join("s_ef");
        std::fs::create_dir_all(&sim_dir).unwrap();
        std::fs::write(
            sim_dir.join("env_status.json"),
            br#"{"status":"running","twitter_available":true,"reddit_available":false,"timestamp":"2025-12-01T10:00:00"}"#,
        )
        .unwrap();
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(post_json(
                "/api/simulation/env-status",
                serde_json::json!({"simulation_id":"s_ef"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["data"]["twitter_available"], true, "read from env_status.json");
        assert_eq!(json["data"]["reddit_available"], false);
        // env_alive is still false (no live IPC env), independent of the file fields.
        assert_eq!(json["data"]["env_alive"], false);
    }

    #[tokio::test]
    async fn close_env_missing_simulation_id_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json("/api/simulation/close-env", serde_json::json!({})))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert_eq!(json["error"], crate::i18n::t("api.requireSimulationId"));
    }

    /// No registered run → close_simulation_env errors "not found" → 400 (Python ValueError).
    /// The faithful IPC-PRODUCER-PENDING contract.
    #[tokio::test]
    async fn close_env_no_env_400() {
        let (app, _t) = test_app();
        let resp = app
            .oneshot(post_json(
                "/api/simulation/close-env",
                serde_json::json!({"simulation_id":"s_noenv"}),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "400 must not have traceback");
    }

    // -----------------------------------------------------------------------
    // FIX-1 regression: CSV DictReader semantics (flexible parsing)
    // -----------------------------------------------------------------------

    /// CSV short row: truncated file mid-write still yields earlier valid rows.
    /// A file that ends abruptly (last row has fewer columns than the header)
    /// must NOT drop all rows — earlier complete rows must still be returned,
    /// and the short final row must be returned with null-padded missing keys.
    #[tokio::test]
    async fn get_profiles_realtime_csv_truncated_file_yields_valid_rows() {
        let (state, _tmp) = test_state();

        // We need a twitter-platform simulation. Seed a project and create the sim dir manually.
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("CSV Truncated Test").expect("seed project");
        p.graph_id = Some("graph-csv-trunc".to_string());
        pm.save_project(&mut p).expect("save project");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "graph-csv-trunc", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);

        // Write a CSV that is "truncated mid-write": second row has fewer fields than the header.
        // Header: id,name,score  → 3 fields
        // Row 1: 0,Alice,99      → complete (3 fields)
        // Row 2: 1,Bob           → short (2 fields — truncated mid-write)
        let csv_content = "id,name,score\n0,Alice,99\n1,Bob\n";
        std::fs::write(sim_dir.join("twitter_profiles.csv"), csv_content).unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/profiles/realtime?platform=twitter"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true, "truncated CSV must not 500: {json}");

        let data = &json["data"];
        let profiles = data["profiles"].as_array().expect("profiles must be an array");

        // Both rows must be returned (not dropped)
        assert_eq!(profiles.len(), 2, "truncated CSV must yield 2 rows, not {}", profiles.len());

        // Row 1 (complete): all fields present as strings
        assert_eq!(profiles[0]["id"], "0");
        assert_eq!(profiles[0]["name"], "Alice");
        assert_eq!(profiles[0]["score"], "99");

        // Row 2 (short): existing fields as strings, missing trailing key → null
        assert_eq!(profiles[1]["id"], "1");
        assert_eq!(profiles[1]["name"], "Bob");
        assert!(
            profiles[1]["score"].is_null(),
            "missing trailing field must be null, got: {}",
            profiles[1]["score"]
        );
    }

    /// CSV short row explicit: header + "0,A,a,b,s\n1,Bob\n" → 2 row objects, NOT [].
    /// Row 1 is full; row 2 has only one field with the remaining 4 headers null.
    #[tokio::test]
    async fn get_profiles_realtime_csv_short_row_null_padding() {
        let (state, _tmp) = test_state();

        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("CSV Short Row Test").expect("seed project");
        p.graph_id = Some("graph-csv-short".to_string());
        pm.save_project(&mut p).expect("save project");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "graph-csv-short", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);

        // Exactly the test case from the verifier: "0,A,a,b,s\n1,Bob\n" with 5-column header
        // Header: 0,A,a,b,s  → 5 columns
        // Row 1: 0,A,a,b,s   → 5 fields (complete)
        // Row 2: 1,Bob        → 2 fields (3 trailing headers must be null)
        let csv_content = "0,A,a,b,s\n0,A,a,b,s\n1,Bob\n";
        std::fs::write(sim_dir.join("twitter_profiles.csv"), csv_content).unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/profiles/realtime?platform=twitter"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);

        let profiles = json["data"]["profiles"].as_array().expect("must be array");
        // Must return 2 rows, NOT []
        assert_eq!(profiles.len(), 2, "must return 2 rows (short row must not be dropped)");

        // Row 1 (full): all 5 fields
        assert_eq!(profiles[0]["0"], "0");
        assert_eq!(profiles[0]["A"], "A");
        assert_eq!(profiles[0]["a"], "a");
        assert_eq!(profiles[0]["b"], "b");
        assert_eq!(profiles[0]["s"], "s");

        // Row 2 (short): first 2 fields set, remaining 3 headers null
        assert_eq!(profiles[1]["0"], "1");
        assert_eq!(profiles[1]["A"], "Bob");
        assert!(profiles[1]["a"].is_null(), "3rd header must be null for short row");
        assert!(profiles[1]["b"].is_null(), "4th header must be null for short row");
        assert!(profiles[1]["s"].is_null(), "5th header must be null for short row");
    }

    /// CSV long row: surplus fields collected under key "null" as a JSON array.
    #[tokio::test]
    async fn get_profiles_realtime_csv_long_row_surplus_under_null_key() {
        let (state, _tmp) = test_state();

        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("CSV Long Row Test").expect("seed project");
        p.graph_id = Some("graph-csv-long".to_string());
        pm.save_project(&mut p).expect("save project");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "graph-csv-long", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);

        // Header has 2 columns; row has 4 fields → 2 surplus values
        let csv_content = "name,age\nAlice,30,extra1,extra2\n";
        std::fs::write(sim_dir.join("twitter_profiles.csv"), csv_content).unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/profiles/realtime?platform=twitter"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);

        let profiles = json["data"]["profiles"].as_array().expect("must be array");
        assert_eq!(profiles.len(), 1);

        let row = &profiles[0];
        assert_eq!(row["name"], "Alice");
        assert_eq!(row["age"], "30");

        // Surplus fields must be under the "null" key as a JSON array of strings
        let surplus = row["null"].as_array().expect("surplus must be under key 'null' as array");
        assert_eq!(surplus.len(), 2, "must have 2 surplus values");
        assert_eq!(surplus[0], "extra1");
        assert_eq!(surplus[1], "extra2");
    }

    // -----------------------------------------------------------------------
    // FIX-2 regression: config/realtime summary gate — empty object is falsy
    // -----------------------------------------------------------------------

    /// GET /:id/config/realtime — simulation_config.json containing {} (empty object)
    /// must NOT have a "summary" key in the response (Python `if {}:` is False).
    #[tokio::test]
    async fn get_config_realtime_empty_object_config_no_summary() {
        let (state, _tmp) = test_state();
        let (sim_id,) = seed_sim(&state);
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);

        // Write simulation_config.json with an empty object
        std::fs::write(sim_dir.join("simulation_config.json"), "{}").unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/simulation/{sim_id}/config/realtime"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);

        let data = &json["data"];
        // config should be the empty object itself
        assert!(data["config"].is_object(), "config must be the parsed {{}} object");

        // summary must NOT be present — Python `if {}:` is False
        assert!(
            data.get("summary").is_none(),
            "summary must be absent when config is empty object {{}}, got: {data}"
        );
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (f) — POST /generate-profiles
    // -----------------------------------------------------------------------

    /// Seed a task with a small KnowledgeGraph (2 Person entities) for generate-profiles tests.
    /// Returns (graph_id, entity_type_label).
    fn seed_generate_profiles_graph() -> (String, String) {
        use crate::graph::{Entity, EntityKind, KnowledgeGraph, Relation, RelationKind};

        let alice_id = uuid::Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap();
        let bob_id = uuid::Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").unwrap();

        let mut graph = KnowledgeGraph::new();
        let alice = Entity { id: alice_id, name: "Alice".to_string(), kind: EntityKind::Person };
        let bob = Entity { id: bob_id, name: "Bob".to_string(), kind: EntityKind::Person };
        let ai = graph.add_entity(alice).expect("add alice");
        let bi = graph.add_entity(bob).expect("add bob");
        graph.add_relation(ai, bi, Relation::new(RelationKind::RelatedTo, 0.8).expect("rel"));

        let graph_json_str = graph.serialize_to_json().expect("serialize");
        let graph_json: serde_json::Value = serde_json::from_str(&graph_json_str).expect("parse");
        let result = serde_json::json!({
            "graph_name": "GenProfilesTestGraph",
            "graph_info": {"node_count": 2, "edge_count": 1, "entity_types": []},
            "chunks_processed": 1,
            "graph": graph_json,
        });

        let tm = crate::task::TaskManager::global();
        let task_id = tm.create_task("graph_build", None);
        tm.complete_task(&task_id, result);

        let entity_type = EntityKind::Person.to_string();
        (task_id, entity_type)
    }

    /// POST /generate-profiles — missing graph_id → 400 requireGraphId
    #[tokio::test]
    async fn generate_profiles_missing_graph_id_400() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/generate-profiles")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "missing graph_id must 400");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        let err = json["error"].as_str().unwrap_or("");
        assert!(!err.is_empty(), "error field must be non-empty");
    }

    /// POST /generate-profiles — absent body tolerated (returns 400 requireGraphId, not 422)
    #[tokio::test]
    async fn generate_profiles_absent_body_tolerated() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/generate-profiles")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Empty body → data = {} → no graph_id → 400, not 422/500
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "absent body must tolerate and produce 400 (no graph_id)"
        );
    }

    /// POST /generate-profiles — entity_types filter that matches nothing → 400 noMatchingEntities
    ///
    /// Uses `use_llm=false` so the generator uses rule-based generation (no live LLM needed).
    /// The entity_types filter "NonExistentType" produces filtered_count=0 on a Person-only graph.
    #[tokio::test]
    async fn generate_profiles_zero_match_entity_types_400() {
        let (graph_id, _entity_type) = seed_generate_profiles_graph();
        let (app, _tmp) = test_app();

        let body = serde_json::json!({
            "graph_id": graph_id,
            "entity_types": ["NonExistentType"],
            "use_llm": false,
        })
        .to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/generate-profiles")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "zero-match must 400");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        let err = json["error"].as_str().unwrap_or("");
        assert!(!err.is_empty(), "noMatchingEntities error must be non-empty");
    }

    /// POST /generate-profiles — happy path, platform=reddit, use_llm=false
    ///
    /// Checks: 200, key order (platform→entity_types→count→profiles), count==profiles.len(),
    /// platform field matches, each profile has at least the OASIS Reddit fields.
    ///
    /// `use_llm=false` is used so tests run without a live LLM (rule-based generation).
    /// This matches the Python contract: `use_llm` is a parameter and `False` is valid.
    ///
    /// `multi_thread` flavor required: the handler uses `tokio::task::block_in_place` (which
    /// needs a multi-threaded Tokio runtime) to call the `!Send` generator future from a
    /// `Send` axum handler.
    #[tokio::test(flavor = "multi_thread")]
    async fn generate_profiles_reddit_happy_200() {
        let (graph_id, _entity_type) = seed_generate_profiles_graph();
        let (app, _tmp) = test_app();

        let body = serde_json::json!({
            "graph_id": graph_id,
            "use_llm": false,
            "platform": "reddit",
        })
        .to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/generate-profiles")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "reddit happy path must 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);

        let data = &json["data"];

        // Key order: platform → entity_types → count → profiles
        let keys: Vec<&str> = data.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        assert_eq!(
            keys,
            ["platform", "entity_types", "count", "profiles"],
            "key order must be platform,entity_types,count,profiles"
        );

        // platform field
        assert_eq!(data["platform"], "reddit");

        // entity_types: set-equality — must contain the Person entity type label
        let et = data["entity_types"].as_array().expect("entity_types must be an array");
        assert!(!et.is_empty(), "entity_types must be non-empty for a Person graph");

        // count == profiles.len()
        let profiles = data["profiles"].as_array().expect("profiles must be array");
        let count = data["count"].as_u64().expect("count must be a number");
        assert_eq!(count as usize, profiles.len(), "count must equal profiles.len()");

        // 2 entities seeded → 2 profiles
        assert_eq!(profiles.len(), 2, "must generate one profile per entity");

        // Each profile must have the Reddit OASIS required fields
        for p in profiles {
            assert!(p["user_id"].is_number(), "profile must have user_id");
            assert!(p["username"].is_string(), "profile must have username (no underscore)");
            assert!(p["name"].is_string(), "profile must have name");
            assert!(p["bio"].is_string(), "profile must have bio");
            assert!(p["persona"].is_string(), "profile must have persona");
            assert!(p["karma"].is_number(), "reddit profile must have karma");
            assert!(p["created_at"].is_string(), "profile must have created_at");
        }
    }

    /// POST /generate-profiles — platform=twitter produces twitter-format profiles (has friend_count, no karma)
    #[tokio::test(flavor = "multi_thread")]
    async fn generate_profiles_twitter_format() {
        let (graph_id, _entity_type) = seed_generate_profiles_graph();
        let (app, _tmp) = test_app();

        let body = serde_json::json!({
            "graph_id": graph_id,
            "use_llm": false,
            "platform": "twitter",
        })
        .to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/generate-profiles")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "twitter path must 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);

        let data = &json["data"];
        assert_eq!(data["platform"], "twitter");

        let profiles = data["profiles"].as_array().expect("profiles must be array");
        assert_eq!(profiles.len(), 2, "2 entities → 2 twitter profiles");

        // Twitter format: must have friend_count; must NOT have karma (Reddit-only)
        for p in profiles {
            assert!(p["friend_count"].is_number(), "twitter profile must have friend_count");
            assert!(p["follower_count"].is_number(), "twitter profile must have follower_count");
            assert!(p["statuses_count"].is_number(), "twitter profile must have statuses_count");
            assert!(
                p.get("karma").is_none() || p["karma"].is_null(),
                "twitter profile must NOT have karma (Reddit-only)"
            );
        }
    }

    /// POST /generate-profiles — platform=other uses to_dict() format (has karma + user_name)
    #[tokio::test(flavor = "multi_thread")]
    async fn generate_profiles_other_platform_to_dict() {
        let (graph_id, _entity_type) = seed_generate_profiles_graph();
        let (app, _tmp) = test_app();

        let body = serde_json::json!({
            "graph_id": graph_id,
            "use_llm": false,
            "platform": "fediverse",   // any non-reddit/twitter value
        })
        .to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/generate-profiles")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "other platform must 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);

        let data = &json["data"];
        assert_eq!(data["platform"], "fediverse");

        let profiles = data["profiles"].as_array().expect("profiles must be array");
        assert_eq!(profiles.len(), 2, "2 entities → 2 to_dict profiles");

        // to_dict format: uses user_name (with underscore), includes ALL fields
        for p in profiles {
            // to_dict uses "user_name" (underscore), not "username" (platform formats)
            assert!(p["user_name"].is_string(), "to_dict must use 'user_name' key");
            // to_dict includes source_entity_type
            assert!(
                !p["source_entity_type"].is_null() || p["source_entity_type"].is_string(),
                "to_dict must have source_entity_type"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (g) helpers
    // -----------------------------------------------------------------------

    /// Seed a simulation with all 4 required files + state.json at the given status.
    /// Returns `(state, sim_id, _tmp)`.
    fn seed_prepared_sim(
        sim_status: &str,
        config_generated: bool,
    ) -> (Arc<ApiState>, String, tempfile::TempDir) {
        let (state, tmp) = test_state();
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("G Test Project").expect("seed project");
        p.graph_id = Some("graph-g".to_string());
        pm.save_project(&mut p).expect("save");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "graph-g", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        // Write required files
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);
        std::fs::create_dir_all(&sim_dir).expect("mkdir sim_dir");

        // state.json
        let now = crate::models::project::python_isoformat_local();
        let state_json = serde_json::json!({
            "status": sim_status,
            "config_generated": config_generated,
            "entities_count": 5,
            "entity_types": ["Person"],
            "created_at": now,
            "updated_at": now
        });
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::to_string_pretty(&state_json).unwrap(),
        )
        .expect("write state.json");

        // simulation_config.json
        std::fs::write(sim_dir.join("simulation_config.json"), b"{}").expect("write config");

        // reddit_profiles.json — a JSON array with 3 entries
        std::fs::write(
            sim_dir.join("reddit_profiles.json"),
            b"[{\"name\":\"alice\"},{\"name\":\"bob\"},{\"name\":\"carol\"}]",
        )
        .expect("write reddit_profiles");

        // twitter_profiles.csv — minimal CSV
        std::fs::write(sim_dir.join("twitter_profiles.csv"), b"username\nalice\nbob\n")
            .expect("write twitter_profiles");

        // Update in-manager state to match the desired status (evict cache so it re-reads)
        state.sim_manager.evict_cache_for_test(&sim_id);

        // Patch the simulation_manager's cached SimulationState status if needed
        // (we want the manager to return a state with the right status)
        if sim_status != "created" {
            // Re-read via get_simulation + patch + save (uses the manager's save which updates
            // the cache — but the state.json we wrote above has the right sim_status, so we
            // need to also update the manager's in-memory status if it cached "created").
            // Simplest: just evict again; the manager will re-read our patched state.json.
            state.sim_manager.evict_cache_for_test(&sim_id);
        }

        (state, sim_id, tmp)
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (g) — check_simulation_prepared
    // -----------------------------------------------------------------------

    /// check_simulation_prepared: missing sim_dir → (false, reason)
    #[test]
    fn check_prepared_missing_dir() {
        let (_, tmp) = test_state();
        let mut config = crate::Config::build_test();
        config.oasis_simulation_data_dir = tmp.path().join("sims").to_string_lossy().to_string();
        let (ok, info) = crate::api::simulation::check_simulation_prepared(&config, "sim_missing");
        assert!(!ok);
        assert!(info["reason"].as_str().unwrap().contains("不存在"));
    }

    /// check_simulation_prepared: missing required files → (false, missing_files)
    #[test]
    fn check_prepared_missing_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = crate::Config::build_test();
        config.oasis_simulation_data_dir = tmp.path().to_string_lossy().to_string();
        let sim_dir = tmp.path().join("sim_x");
        std::fs::create_dir_all(&sim_dir).unwrap();
        // Only write state.json; the other 3 are absent
        std::fs::write(sim_dir.join("state.json"), b"{}").unwrap();

        let (ok, info) = crate::api::simulation::check_simulation_prepared(&config, "sim_x");
        assert!(!ok, "must be false when required files are missing");
        let missing = info["missing_files"].as_array().expect("missing_files must be array");
        assert!(!missing.is_empty(), "must report missing files");
    }

    /// check_simulation_prepared: status="preparing" + config_generated=true + all files
    /// → auto-upgrades state.json to "ready" on disk (KEY observable side effect).
    #[test]
    fn check_prepared_preparing_auto_upgrades_to_ready() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = crate::Config::build_test();
        config.oasis_simulation_data_dir = tmp.path().to_string_lossy().to_string();
        let sim_dir = tmp.path().join("sim_prep");
        std::fs::create_dir_all(&sim_dir).unwrap();

        let initial_updated_at = "2025-01-01T00:00:00.000000";
        let state_json = serde_json::json!({
            "status": "preparing",
            "config_generated": true,
            "entities_count": 2,
            "entity_types": ["Person"],
            "created_at": initial_updated_at,
            "updated_at": initial_updated_at
        });
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::to_string_pretty(&state_json).unwrap(),
        )
        .unwrap();
        std::fs::write(sim_dir.join("simulation_config.json"), b"{}").unwrap();
        std::fs::write(
            sim_dir.join("reddit_profiles.json"),
            b"[{\"name\":\"alice\"},{\"name\":\"bob\"}]",
        )
        .unwrap();
        std::fs::write(sim_dir.join("twitter_profiles.csv"), b"username\nalice\n").unwrap();

        let (ok, info) = crate::api::simulation::check_simulation_prepared(&config, "sim_prep");

        // Must return true
        assert!(ok, "preparing+config_generated must be considered prepared");
        // Returned info must show status=ready (after auto-upgrade)
        assert_eq!(info["status"].as_str().unwrap(), "ready", "info.status must be ready");

        // *** OBSERVABLE SIDE EFFECT: state.json on disk must now read status="ready" ***
        let on_disk_raw = std::fs::read_to_string(sim_dir.join("state.json")).unwrap();
        let on_disk: serde_json::Value = serde_json::from_str(&on_disk_raw).unwrap();
        assert_eq!(
            on_disk["status"].as_str().unwrap(),
            "ready",
            "state.json must have status=ready after auto-upgrade"
        );
        // updated_at must have changed (was initial_updated_at)
        let new_updated_at = on_disk["updated_at"].as_str().unwrap();
        assert_ne!(
            new_updated_at, initial_updated_at,
            "updated_at must be refreshed by auto-upgrade"
        );
    }

    /// check_simulation_prepared: status="failed" + config_generated=true → (true, info)
    /// (no auto-upgrade triggered — only "preparing" triggers it)
    #[test]
    fn check_prepared_failed_config_generated_is_prepared() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = crate::Config::build_test();
        config.oasis_simulation_data_dir = tmp.path().to_string_lossy().to_string();
        let sim_dir = tmp.path().join("sim_fail");
        std::fs::create_dir_all(&sim_dir).unwrap();

        let state_json = serde_json::json!({
            "status": "failed",
            "config_generated": true,
            "entities_count": 0,
            "entity_types": [],
            "created_at": "2025-01-01T00:00:00",
            "updated_at": "2025-01-01T00:00:00"
        });
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::to_string_pretty(&state_json).unwrap(),
        )
        .unwrap();
        std::fs::write(sim_dir.join("simulation_config.json"), b"{}").unwrap();
        std::fs::write(sim_dir.join("reddit_profiles.json"), b"[]").unwrap();
        std::fs::write(sim_dir.join("twitter_profiles.csv"), b"username\n").unwrap();

        let (ok, info) = crate::api::simulation::check_simulation_prepared(&config, "sim_fail");
        assert!(ok, "failed+config_generated must be prepared");
        assert_eq!(info["status"].as_str().unwrap(), "failed");
    }

    /// check_simulation_prepared: config_generated=false → (false, ...)
    #[test]
    fn check_prepared_config_not_generated_is_not_prepared() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut config = crate::Config::build_test();
        config.oasis_simulation_data_dir = tmp.path().to_string_lossy().to_string();
        let sim_dir = tmp.path().join("sim_nocfg");
        std::fs::create_dir_all(&sim_dir).unwrap();

        let state_json = serde_json::json!({
            "status": "ready",
            "config_generated": false,
            "entities_count": 0,
            "entity_types": [],
            "created_at": "2025-01-01T00:00:00",
            "updated_at": "2025-01-01T00:00:00"
        });
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::to_string_pretty(&state_json).unwrap(),
        )
        .unwrap();
        std::fs::write(sim_dir.join("simulation_config.json"), b"{}").unwrap();
        std::fs::write(sim_dir.join("reddit_profiles.json"), b"[]").unwrap();
        std::fs::write(sim_dir.join("twitter_profiles.csv"), b"username\n").unwrap();

        let (ok, _info) = crate::api::simulation::check_simulation_prepared(&config, "sim_nocfg");
        assert!(!ok, "config_generated=false must not be prepared");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (g) — cleanup_simulation_logs
    // -----------------------------------------------------------------------

    /// cleanup_simulation_logs: creates the scoped files, calls cleanup, verifies they are gone.
    /// Config/profile files must NOT be deleted.
    #[tokio::test]
    async fn cleanup_simulation_logs_deletes_scoped_files_only() {
        let (state, _tmp) = test_state();
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("Cleanup Test").expect("seed project");
        p.graph_id = Some("graph-cl".to_string());
        pm.save_project(&mut p).expect("save");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "graph-cl", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);
        std::fs::create_dir_all(&sim_dir).unwrap();
        std::fs::create_dir_all(sim_dir.join("twitter")).unwrap();
        std::fs::create_dir_all(sim_dir.join("reddit")).unwrap();

        // Create the scoped files
        let scoped_files = [
            "run_state.json",
            "simulation.log",
            "stdout.log",
            "stderr.log",
            "env_status.json",
        ];
        for f in &scoped_files {
            std::fs::write(sim_dir.join(f), b"data").unwrap();
        }
        std::fs::write(sim_dir.join("twitter").join("actions.jsonl"), b"{}").unwrap();
        std::fs::write(sim_dir.join("reddit").join("actions.jsonl"), b"{}").unwrap();

        // Create config/profile files that MUST NOT be deleted
        std::fs::write(sim_dir.join("simulation_config.json"), b"{}").unwrap();
        std::fs::write(sim_dir.join("reddit_profiles.json"), b"[]").unwrap();
        std::fs::write(sim_dir.join("twitter_profiles.csv"), b"username\n").unwrap();
        std::fs::write(sim_dir.join("state.json"), b"{}").unwrap();

        // Run cleanup
        let result = state.sim_runner.cleanup_simulation_logs(&sim_id).await;
        assert!(result.success, "cleanup must succeed: {:?}", result.errors);

        // Scoped files must be gone
        for f in &scoped_files {
            assert!(!sim_dir.join(f).exists(), "scoped file must be deleted: {f}");
        }
        assert!(!sim_dir.join("twitter").join("actions.jsonl").exists());
        assert!(!sim_dir.join("reddit").join("actions.jsonl").exists());

        // Config/profile files must remain
        assert!(sim_dir.join("simulation_config.json").exists(), "config must survive cleanup");
        assert!(sim_dir.join("reddit_profiles.json").exists(), "reddit profiles must survive");
        assert!(sim_dir.join("twitter_profiles.csv").exists(), "twitter profiles must survive");
        assert!(sim_dir.join("state.json").exists(), "state.json must survive cleanup");
    }

    /// cleanup_simulation_logs: missing sim_dir → success=true (Python :1129-1130)
    #[tokio::test]
    async fn cleanup_simulation_logs_missing_dir_is_success() {
        let (state, _tmp) = test_state();
        let result = state.sim_runner.cleanup_simulation_logs("sim_nonexistent_cl").await;
        assert!(result.success, "missing sim_dir must return success=true");
        assert!(result.message.is_some(), "must have a message");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (g1) — POST /stop
    // -----------------------------------------------------------------------

    /// POST /stop — missing simulation_id → 400 requireSimulationId
    #[tokio::test]
    async fn stop_simulation_missing_id_400() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/stop")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "missing id must be 400");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json["error"].as_str().is_some(), "must have error");
        assert!(json.get("traceback").is_none(), "400 must not have traceback");
    }

    /// POST /stop — empty body → 400 requireSimulationId (Python `or {}`)
    #[tokio::test]
    async fn stop_simulation_empty_body_400() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
    }

    /// POST /stop — simulation exists but not running → 400 (TeriError::Sim → ValueError → 400)
    /// This exercises the map_runner_err 400 path without needing a live run.
    #[tokio::test]
    async fn stop_simulation_not_running_400() {
        let (state, _tmp) = test_state();
        let (project_id, _) = seed_project_with_graph(&state);
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        let app = crate::server::create_app(state);
        let body = serde_json::json!({"simulation_id": sim_id}).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/stop")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        // No run_state.json → TeriError::Sim("模拟不存在") → map_runner_err → 400
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "not-running sim stop must be 400");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "ValueError path must not have traceback");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (g2) — POST /start
    // -----------------------------------------------------------------------

    /// POST /start — missing simulation_id → 400 requireSimulationId
    #[tokio::test]
    async fn start_simulation_missing_id_400() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "400 must not have traceback");
    }

    /// POST /start — invalid platform → 400 invalidPlatform
    #[tokio::test]
    async fn start_simulation_bad_platform_400() {
        let (app, _tmp) = test_app();
        let body =
            serde_json::json!({"simulation_id": "sim_x", "platform": "fediverse"}).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        let error = json["error"].as_str().unwrap_or("");
        assert!(error.contains("fediverse"), "error must mention the bad platform: {error}");
    }

    /// POST /start — max_rounds=0 → 400 maxRoundsPositive
    #[tokio::test]
    async fn start_simulation_max_rounds_zero_400() {
        let (app, _tmp) = test_app();
        let body = serde_json::json!({"simulation_id": "sim_x", "max_rounds": 0}).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
    }

    /// POST /start — max_rounds=-5 → 400 maxRoundsPositive
    #[tokio::test]
    async fn start_simulation_max_rounds_negative_400() {
        let (app, _tmp) = test_app();
        let body = serde_json::json!({"simulation_id": "sim_x", "max_rounds": -5}).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
    }

    /// POST /start — max_rounds="abc" (non-numeric string) → 400 maxRoundsInvalid
    #[tokio::test]
    async fn start_simulation_max_rounds_non_numeric_400() {
        let (app, _tmp) = test_app();
        let body = serde_json::json!({"simulation_id": "sim_x", "max_rounds": "abc"}).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
    }

    /// POST /start — max_rounds="5" (numeric string) must be accepted (Python int("5")==5)
    /// — validation passes; reaches 404 not-found (sim_x doesn't exist)
    #[tokio::test]
    async fn start_simulation_max_rounds_numeric_string_accepted() {
        let (app, _tmp) = test_app();
        let body =
            serde_json::json!({"simulation_id": "sim_nonexistent", "max_rounds": "5"}).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        // max_rounds="5" is valid (≥1) — must NOT get 400 maxRoundsInvalid.
        // Should get 404 (sim not found) because sim_nonexistent doesn't exist.
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "numeric string max_rounds must pass validation and reach 404"
        );
    }

    // -----------------------------------------------------------------------
    // U026-g-MAXROUNDS-FLOAT: float truncation tests (Python int(float) semantics)
    // -----------------------------------------------------------------------

    /// POST /start — max_rounds=5.7 (JSON float) → truncated to 5 → passes validation
    /// (reaches 404 not-found because sim_nonexistent doesn't exist, not a 400)
    #[tokio::test]
    async fn start_simulation_max_rounds_float_truncated_accepted() {
        let (app, _tmp) = test_app();
        // Use a raw JSON string so serde_json preserves the float (5.7) exactly.
        let body = r#"{"simulation_id": "sim_nonexistent", "max_rounds": 5.7}"#;
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        // max_rounds=5.7 truncates to 5 (≥1) → must NOT be 400 maxRoundsInvalid.
        // Reaches 404 because the simulation doesn't exist.
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "float max_rounds 5.7 must truncate to 5 and pass validation (reach 404)"
        );
    }

    /// POST /start — max_rounds=0.5 (JSON float) → truncated to 0 → 400 maxRoundsPositive
    #[tokio::test]
    async fn start_simulation_max_rounds_float_zero_truncated_400() {
        let (app, _tmp) = test_app();
        let body = r#"{"simulation_id": "sim_x", "max_rounds": 0.5}"#;
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        // 0.5 truncates to 0 → ≤0 → maxRoundsPositive (400)
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
    }

    /// POST /start — max_rounds=-2.9 (JSON float) → truncated to -2 → 400 maxRoundsPositive
    #[tokio::test]
    async fn start_simulation_max_rounds_float_negative_truncated_400() {
        let (app, _tmp) = test_app();
        let body = r#"{"simulation_id": "sim_x", "max_rounds": -2.9}"#;
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        // -2.9 truncates toward zero to -2 → ≤0 → maxRoundsPositive (400)
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
    }

    /// POST /start — max_rounds="5.7" (JSON string) → 400 maxRoundsInvalid
    /// (Python int("5.7") raises ValueError; string path is strict-integer only)
    #[tokio::test]
    async fn start_simulation_max_rounds_float_string_invalid_400() {
        let (app, _tmp) = test_app();
        let body = serde_json::json!({"simulation_id": "sim_x", "max_rounds": "5.7"}).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        // "5.7" as a string → strict integer parse fails → 400 maxRoundsInvalid
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
    }

    /// POST /start — unknown simulation_id → 404 simulationNotFound
    #[tokio::test]
    async fn start_simulation_not_found_404() {
        let (app, _tmp) = test_app();
        let body = serde_json::json!({"simulation_id": "sim_unknown_xyz"}).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        let error = json["error"].as_str().unwrap_or("");
        assert!(error.contains("sim_unknown_xyz"), "404 error must mention the sim id: {error}");
    }

    /// POST /start — sim exists but not prepared (status="created", config_generated=false)
    /// → 400 simNotReady
    #[tokio::test]
    async fn start_simulation_not_ready_400() {
        let (state, _tmp) = test_state();
        let (project_id, _) = seed_project_with_graph(&state);
        // Sim is created but has no prepared files → check_simulation_prepared returns false
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        let app = crate::server::create_app(state);
        let body = serde_json::json!({"simulation_id": sim_id}).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "not-prepared sim must be 400");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "400 must not have traceback");
    }

    /// POST /start — enable_graph_memory_update=true without graph_id on project
    /// → 400 graphIdRequiredForMemory.
    ///
    /// Uses a sim whose project has NO graph_id and the sim's own graph_id is also absent.
    #[tokio::test]
    async fn start_simulation_graph_id_required_for_memory_400() {
        // We need a sim that is status=Ready (passes the state-machine gate) so we reach the
        // graph_id resolution step. Seed with all prepared files.
        let (state, _sim_id_prepared, _tmp) = seed_prepared_sim("ready", true);

        // Override the project: strip graph_id from it so the fallback lookup also fails.
        // Also strip graph_id from the SimulationState itself (it defaults to the project's).
        // The sim's graph_id field comes from the project at create time; we need to directly
        // set up a sim with empty graph_id.
        //
        // Easiest: create a project with NO graph_id, then create a simulation for it.
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        // Create p2 just to confirm the pattern; it won't be used directly
        let _p2 = pm.create_project("No Graph Project 2").expect("seed");
        // Do not set graph_id on p2

        // Create a sim for p2 (graph_id="" passed through, then empty graph_id on state)
        // The manager requires a graph_id; pass an empty string... actually it validates.
        // We'll instead use a hack: patch the SimulationState's graph_id after creation.
        // The simplest test: create sim with a fake graph_id, then patch the state.json
        // and sim state to have empty graph_id, and set status=ready + config_generated=true.
        let mut p2m = pm.create_project("No Graph Proj Memory").expect("seed2");
        p2m.graph_id = Some("temp-g".to_string()); // needed to pass create_simulation
        pm.save_project(&mut p2m).expect("save");
        let sim2 = state
            .sim_manager
            .create_simulation(&p2m.project_id, "temp-g", true, true)
            .expect("create sim2");
        let sim2_id = sim2.simulation_id.clone();

        // Patch state.json: set status=ready, config_generated=true, clear graph_id
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim2_id);
        std::fs::create_dir_all(&sim_dir).unwrap();
        let state_json = serde_json::json!({
            "status": "ready",
            "config_generated": true,
            "graph_id": "",           // empty → will try project fallback
            "entities_count": 0,
            "entity_types": [],
            "created_at": "2025-01-01T00:00:00",
            "updated_at": "2025-01-01T00:00:00"
        });
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::to_string_pretty(&state_json).unwrap(),
        )
        .unwrap();
        // Required files for check_simulation_prepared (though for ready sim they won't be checked)
        std::fs::write(sim_dir.join("simulation_config.json"), b"{}").unwrap();
        std::fs::write(sim_dir.join("reddit_profiles.json"), b"[]").unwrap();
        std::fs::write(sim_dir.join("twitter_profiles.csv"), b"username\n").unwrap();

        // Strip graph_id from the project too (make it None)
        let mut p2m_reload = pm.get_project(&p2m.project_id).expect("get").expect("project exists");
        p2m_reload.graph_id = None;
        pm.save_project(&mut p2m_reload).expect("save again");

        state.sim_manager.evict_cache_for_test(&sim2_id);

        let app = crate::server::create_app(state);
        let body = serde_json::json!({
            "simulation_id": sim2_id,
            "enable_graph_memory_update": true
        })
        .to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "missing graph_id must be 400");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "400 must not have traceback");
    }

    /// POST /start — a fully prepared **parallel** sim (config + BOTH profile files) → **200**.
    ///
    /// The gap-closure proof for U-030 cycle B: `platform="parallel"` (the default) is no longer an
    /// honest-500. `build_run_inputs` attaches a DUAL-logger producer over the unioned twitter+reddit
    /// pool; `start_simulation` spawns the run + monitor and returns 200 with the Python response
    /// shape. (The runner-level e2e — both `twitter/`+`reddit/actions.jsonl` written → monitor
    /// dual-gate S-615 → COMPLETED — is proven by `parallel_producer_run_reaches_completed` in
    /// `services::simulation_runner`.)
    #[tokio::test(flavor = "multi_thread")]
    async fn start_simulation_parallel_prepared_returns_200() {
        let (state, _tmp) = test_state();
        let (project_id, _) = seed_project_with_graph(&state);
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        // Prepare on disk: READY state + config + BOTH profile files (load_agent_pool("parallel")
        // requires twitter_profiles.csv AND reddit_profiles.json).
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);
        std::fs::create_dir_all(&sim_dir).unwrap();
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "ready",
                "config_generated": true,
                "entities_count": 0,
                "entity_types": [],
                "created_at": "2025-01-01T00:00:00",
                "updated_at": "2025-01-01T00:00:00"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            sim_dir.join("simulation_config.json"),
            serde_json::to_string(&serde_json::json!({
                "time_config": { "total_simulation_hours": 1, "minutes_per_round": 30 }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            sim_dir.join("twitter_profiles.csv"),
            "user_id,name,username,user_char,description\n0,Alice,alice,curious,a bio\n",
        )
        .unwrap();
        std::fs::write(
            sim_dir.join("reddit_profiles.json"),
            serde_json::to_string(&serde_json::json!([{
                "user_id": 1, "username": "bob", "name": "Bob", "bio": "b bio",
                "persona": "grumpy", "karma": 1000, "created_at": "2025-01-01"
            }]))
            .unwrap(),
        )
        .unwrap();
        state.sim_manager.evict_cache_for_test(&sim_id);

        let app = crate::server::create_app(state);
        // Default platform is "parallel"; pass it explicitly for clarity.
        let body =
            serde_json::json!({ "simulation_id": sim_id, "platform": "parallel" }).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Parallel is now wired → 200 (NOT the old honest-500).
        assert_eq!(resp.status(), StatusCode::OK, "prepared parallel sim must start → 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        assert!(json.get("traceback").is_none(), "200 must not carry a traceback");
        let data = &json["data"];
        assert_eq!(data["simulation_id"], sim_id);
        assert_eq!(data["total_rounds"], 2, "1h/30min → 2 rounds");
        assert_eq!(data["graph_memory_update_enabled"], false);
        assert_eq!(data["force_restarted"], false);
        assert!(data.get("max_rounds_applied").is_none());
        assert!(data.get("graph_id").is_none());
    }

    /// POST /start — a fully prepared **twitter** sim (config + profiles on disk) → **200**.
    ///
    /// This is the gap-closure proof for U-028 c3b-ii: `build_run_inputs` assembles the engine
    /// (config→ticks + activation + actions.jsonl producer) + pool (profile reader) + graph + llm,
    /// `start_simulation` spawns the run + monitor, and the handler returns 200 with the Python
    /// response shape (`run_state.to_dict()` + `graph_memory_update_enabled` + `force_restarted`).
    #[tokio::test(flavor = "multi_thread")]
    async fn start_simulation_twitter_prepared_returns_200() {
        let (state, _tmp) = test_state();
        let (project_id, _) = seed_project_with_graph(&state);
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        // Prepare the sim on disk: READY state + a real config (1h/30min → 2 rounds) + a twitter
        // profile so load_agent_pool builds a non-empty pool.
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);
        std::fs::create_dir_all(&sim_dir).unwrap();
        std::fs::write(
            sim_dir.join("state.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "ready",
                "config_generated": true,
                "entities_count": 0,
                "entity_types": [],
                "created_at": "2025-01-01T00:00:00",
                "updated_at": "2025-01-01T00:00:00"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            sim_dir.join("simulation_config.json"),
            serde_json::to_string(&serde_json::json!({
                "time_config": { "total_simulation_hours": 1, "minutes_per_round": 30 }
            }))
            .unwrap(),
        )
        .unwrap();
        // twitter_profiles.csv: header + one row (5 cols: user_id,name,username,user_char,description)
        std::fs::write(
            sim_dir.join("twitter_profiles.csv"),
            "user_id,name,username,user_char,description\n0,Alice,alice,curious,a bio\n",
        )
        .unwrap();
        state.sim_manager.evict_cache_for_test(&sim_id);

        let app = crate::server::create_app(state);
        let body =
            serde_json::json!({ "simulation_id": sim_id, "platform": "twitter" }).to_string();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/simulation/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK, "prepared twitter sim must start → 200");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
        let data = &json["data"];
        // run_state.to_dict() shape + the conditional fields (Python :1616-1622).
        assert_eq!(data["simulation_id"], sim_id);
        assert_eq!(data["total_rounds"], 2, "1h/30min → 2 rounds");
        assert_eq!(data["graph_memory_update_enabled"], false);
        assert_eq!(data["force_restarted"], false);
        // max_rounds absent → no max_rounds_applied; memory disabled → no graph_id.
        assert!(data.get("max_rounds_applied").is_none());
        assert!(data.get("graph_id").is_none());
    }

    /// POST /generate-profiles — entity_types set-equality: response entity_types is a subset
    /// of the graph's entity types when a matching filter is applied.
    #[tokio::test(flavor = "multi_thread")]
    async fn generate_profiles_entity_types_set_equality() {
        let (graph_id, entity_type) = seed_generate_profiles_graph();
        let (app, _tmp) = test_app();

        let body = serde_json::json!({
            "graph_id": graph_id,
            "use_llm": false,
            "platform": "reddit",
            "entity_types": [entity_type.clone()],
        })
        .to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/simulation/generate-profiles")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);

        let data = &json["data"];
        let et_arr = data["entity_types"].as_array().expect("entity_types must be array");
        // Must include the entity_type we filtered on (set-equality check)
        let et_strings: Vec<&str> = et_arr.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            et_strings.contains(&entity_type.as_str()),
            "entity_types must contain the filtered type '{}', got {:?}",
            entity_type,
            et_strings
        );
    }

    // =======================================================================
    // Sub-cycle (m) — GET /history  (simulation.py:876-987)
    // =======================================================================

    async fn get_history(app: axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let resp = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    /// No simulations → 200 {success:true, data:[], count:0}.
    #[tokio::test]
    async fn history_empty_no_sims() {
        let (app, _tmp) = test_app();
        let (status, json) = get_history(app, "/api/simulation/history").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["success"], true);
        assert_eq!(json["count"], 0);
        assert_eq!(json["data"].as_array().unwrap().len(), 0);
    }

    /// Single sim, no config / no run_state / no report → faithful empty-run defaults.
    #[tokio::test]
    async fn history_single_sim_defaults() {
        let (state, _tmp) = test_state();
        let (project_id, _g) = seed_project_with_graph(&state);
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let created_at = sim.created_at.clone();

        let app = crate::server::create_app(state);
        let (status, json) = get_history(app, "/api/simulation/history").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["count"], 1);
        let s = &json["data"][0];

        // Enrichment defaults (no config, no run_state, no project files, no report).
        assert_eq!(s["simulation_requirement"], "");
        assert_eq!(s["total_simulation_hours"], 0);
        assert_eq!(s["runner_status"], "idle");
        assert_eq!(s["total_rounds"], 0);
        assert_eq!(s["current_round"], 0);
        assert!(s["files"].as_array().unwrap().is_empty(), "files must be []");
        assert_eq!(s["report_id"], serde_json::Value::Null);
        assert_eq!(s["version"], "v1.0.2");
        // created_date = created_at[:10]
        let expected_date: String = created_at.chars().take(10).collect();
        assert_eq!(s["created_date"], expected_date);
        // base to_dict key carried through
        assert_eq!(s["status"], "created");
    }

    /// Config present with time_config → simulation_requirement + total_simulation_hours +
    /// recommended_rounds become total_rounds (no run_state → uses recommended).
    #[tokio::test]
    async fn history_with_config_recommended_rounds() {
        let (state, _tmp) = test_state();
        let (project_id, _g) = seed_project_with_graph(&state);
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        // Write a simulation_config.json with time_config (24h / 60min-per-round → 24 rounds).
        let sim_dir = state.sim_manager.get_simulation_dir(&sim_id).expect("dir");
        let cfg = serde_json::json!({
            "simulation_requirement": "如果武汉大学发布公告会怎样",
            "time_config": { "total_simulation_hours": 24, "minutes_per_round": 60 }
        });
        std::fs::write(sim_dir.join("simulation_config.json"), cfg.to_string()).unwrap();

        let app = crate::server::create_app(state);
        let (status, json) = get_history(app, "/api/simulation/history").await;
        assert_eq!(status, StatusCode::OK);
        let s = &json["data"][0];
        assert_eq!(s["simulation_requirement"], "如果武汉大学发布公告会怎样");
        assert_eq!(s["total_simulation_hours"], 24);
        // recommended_rounds = int(24 * 60 / max(60,1)) = 24; no run_state → total_rounds = 24
        assert_eq!(s["total_rounds"], 24);
        assert_eq!(s["runner_status"], "idle");
    }

    /// recommended_rounds uses int() truncation and max(minutes_per_round,1).
    /// 5h, 90min/round → 5*60/90 = 3.33.. → int() = 3.
    #[tokio::test]
    async fn history_recommended_rounds_truncates() {
        let (state, _tmp) = test_state();
        let (project_id, _g) = seed_project_with_graph(&state);
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let sim_dir = state.sim_manager.get_simulation_dir(&sim.simulation_id).expect("dir");
        let cfg = serde_json::json!({
            "time_config": { "total_simulation_hours": 5, "minutes_per_round": 90 }
        });
        std::fs::write(sim_dir.join("simulation_config.json"), cfg.to_string()).unwrap();

        let app = crate::server::create_app(state);
        let (_status, json) = get_history(app, "/api/simulation/history").await;
        assert_eq!(json["data"][0]["total_rounds"], 3);
        // total_simulation_hours echoes the raw config value
        assert_eq!(json["data"][0]["total_simulation_hours"], 5);
    }

    /// Key order is byte-observable (preserve_order). Verify the exact 25-key insertion order,
    /// including current_round UPDATED IN PLACE at position 12 (not appended).
    #[tokio::test]
    async fn history_key_order_preserved() {
        let (state, _tmp) = test_state();
        let (project_id, _g) = seed_project_with_graph(&state);
        state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");

        let app = crate::server::create_app(state);
        let (_status, json) = get_history(app, "/api/simulation/history").await;
        let s = json["data"][0].as_object().expect("object");
        let keys: Vec<&str> = s.keys().map(|k| k.as_str()).collect();
        let expected = vec![
            "simulation_id",
            "project_id",
            "graph_id",
            "enable_twitter",
            "enable_reddit",
            "status",
            "entities_count",
            "profiles_count",
            "entity_types",
            "config_generated",
            "config_reasoning",
            "current_round", // UPDATED in place — stays at pos 12, not re-appended
            "twitter_status",
            "reddit_status",
            "created_at",
            "updated_at",
            "error",
            "simulation_requirement",
            "total_simulation_hours",
            "runner_status",
            "total_rounds",
            "files",
            "report_id",
            "version",
            "created_date",
        ];
        assert_eq!(keys, expected, "history enriched key order must match Python dict order");
    }

    /// ?limit caps the number of returned sims ([:limit] slice).
    #[tokio::test]
    async fn history_limit_caps_results() {
        let (state, _tmp) = test_state();
        let (project_id, _g) = seed_project_with_graph(&state);
        for _ in 0..3 {
            state
                .sim_manager
                .create_simulation(&project_id, "graph-abc", true, true)
                .expect("create sim");
        }

        let app = crate::server::create_app(state);
        let (status, json) = get_history(app, "/api/simulation/history?limit=2").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["count"], 2, "limit=2 must cap to 2");
        assert_eq!(json["data"].as_array().unwrap().len(), 2);
    }

    /// Non-numeric ?limit falls back to default 20 ([~] U026-m-NEGLIMIT / U-025 precedent).
    #[tokio::test]
    async fn history_bad_limit_falls_back_to_default() {
        let (state, _tmp) = test_state();
        let (project_id, _g) = seed_project_with_graph(&state);
        state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");

        let app = crate::server::create_app(state);
        let (status, json) = get_history(app, "/api/simulation/history?limit=abc").await;
        assert_eq!(status, StatusCode::OK);
        // default 20 ≥ 1 sim → all returned
        assert_eq!(json["count"], 1);
    }

    /// project.files[:3] → [{"filename": ...}] with the 未知文件 default for a missing key.
    #[tokio::test]
    async fn history_project_files_capped_and_defaulted() {
        let (state, _tmp) = test_state();
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("Files Project").expect("seed project");
        p.graph_id = Some("graph-files".to_string());
        // 4 files: 3 with filename, 1 without (→ default). Only first 3 are kept.
        p.files = vec![
            serde_json::json!({"filename": "a.txt"}),
            serde_json::json!({"filename": "b.txt"}),
            serde_json::json!({"no_name": true}),
            serde_json::json!({"filename": "d.txt"}),
        ];
        pm.save_project(&mut p).expect("save");
        state
            .sim_manager
            .create_simulation(&p.project_id, "graph-files", true, true)
            .expect("create sim");

        let app = crate::server::create_app(state);
        let (_status, json) = get_history(app, "/api/simulation/history").await;
        let files = json["data"][0]["files"].as_array().expect("files array");
        assert_eq!(files.len(), 3, "files must be capped at 3");
        assert_eq!(files[0]["filename"], "a.txt");
        assert_eq!(files[1]["filename"], "b.txt");
        // third file has no filename key → 未知文件 default
        assert_eq!(files[2]["filename"], "未知文件");
    }

    // =======================================================================
    // Sub-cycle (d) — POST /prepare + POST /prepare/status  (simulation.py:359-752)
    // =======================================================================

    async fn prep_post(
        app: axum::Router,
        uri: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    // ---- /prepare -----------------------------------------------------------

    #[tokio::test]
    async fn prepare_missing_simulation_id_400() {
        let (app, _tmp) = test_app();
        let (status, json) = prep_post(app, "/api/simulation/prepare", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none(), "400 must not have traceback");
    }

    #[tokio::test]
    async fn prepare_sim_not_found_404() {
        let (app, _tmp) = test_app();
        let (status, json) = prep_post(
            app,
            "/api/simulation/prepare",
            serde_json::json!({"simulation_id": "sim_doesnotexist"}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn prepare_already_prepared_short_circuit() {
        // A prepared sim (status=ready + config_generated + required files) short-circuits to
        // 200 ready/already_prepared:true WITHOUT spawning a prepare task.
        let (state, sim_id, _tmp) = seed_prepared_sim("ready", true);
        let app = crate::server::create_app(state);
        let (status, json) =
            prep_post(app, "/api/simulation/prepare", serde_json::json!({"simulation_id": sim_id}))
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["status"], "ready");
        assert_eq!(json["data"]["already_prepared"], true);
        assert!(json["data"].get("prepare_info").is_some(), "must carry prepare_info");
        // No task_id in the already-prepared short-circuit (distinct from the spawn path).
        assert!(json["data"].get("task_id").is_none());
    }

    #[tokio::test]
    async fn prepare_project_missing_requirement_400() {
        // Sim in 'created' status (not prepared) whose project has NO simulation_requirement.
        let (state, _tmp) = test_state();
        let (project_id, _g) = seed_project_with_graph(&state); // no requirement set
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let app = crate::server::create_app(state);
        let (status, json) = prep_post(
            app,
            "/api/simulation/prepare",
            serde_json::json!({"simulation_id": sim.simulation_id}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn prepare_happy_returns_preparing_with_task() {
        // Project WITH a requirement → reaches spawn → immediate 200 preparing + task_id.
        // (Graph 'graph-x' has no graph_build task → empty graph → preview 0; the background
        //  worker FAILS harmlessly. We assert only the immediate response contract.)
        let (state, _tmp) = test_state();
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("Prep Project").expect("seed project");
        p.graph_id = Some("graph-x".to_string());
        p.simulation_requirement = Some("如果武汉大学发布公告会怎样".to_string());
        pm.save_project(&mut p).expect("save");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "graph-x", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        let app = crate::server::create_app(state);
        let (status, json) =
            prep_post(app, "/api/simulation/prepare", serde_json::json!({"simulation_id": sim_id}))
                .await;
        assert_eq!(status, StatusCode::OK, "happy prepare must return 200: {json}");
        assert_eq!(json["success"], true);
        let data = &json["data"];
        assert_eq!(data["simulation_id"], sim_id);
        assert_eq!(data["status"], "preparing");
        assert_eq!(data["already_prepared"], false);
        assert!(data["task_id"].as_str().is_some_and(|s| !s.is_empty()), "must return a task_id");
        assert!(data["expected_entities_count"].is_number());
        assert!(data["entity_types"].is_array());
    }

    // ---- /prepare/status ----------------------------------------------------

    #[tokio::test]
    async fn prepare_status_neither_id_400() {
        let (app, _tmp) = test_app();
        let (status, json) =
            prep_post(app, "/api/simulation/prepare/status", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn prepare_status_sim_not_prepared_not_started() {
        // simulation_id present, not prepared, no task_id → not_started/0.
        let (state, _tmp) = test_state();
        let (project_id, _g) = seed_project_with_graph(&state);
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let app = crate::server::create_app(state);
        let (status, json) = prep_post(
            app,
            "/api/simulation/prepare/status",
            serde_json::json!({"simulation_id": sim.simulation_id}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["status"], "not_started");
        assert_eq!(json["data"]["progress"], 0);
        assert_eq!(json["data"]["already_prepared"], false);
    }

    #[tokio::test]
    async fn prepare_status_sim_prepared_ready() {
        let (state, sim_id, _tmp) = seed_prepared_sim("ready", true);
        let app = crate::server::create_app(state);
        let (status, json) = prep_post(
            app,
            "/api/simulation/prepare/status",
            serde_json::json!({"simulation_id": sim_id}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["status"], "ready");
        assert_eq!(json["data"]["progress"], 100);
        assert_eq!(json["data"]["already_prepared"], true);
    }

    #[tokio::test]
    async fn prepare_status_task_found_returns_to_dict() {
        let (app, _tmp) = test_app();
        let task_id = crate::task::TaskManager::global().create_task("simulation_prepare", None);
        let (status, json) = prep_post(
            app,
            "/api/simulation/prepare/status",
            serde_json::json!({"task_id": task_id}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["task_id"], task_id);
        assert_eq!(json["data"]["already_prepared"], false);
        // to_dict carries the status field (pending for a fresh task).
        assert!(json["data"].get("status").is_some());
    }

    #[tokio::test]
    async fn prepare_status_task_gone_no_sim_404() {
        let (app, _tmp) = test_app();
        let (status, json) = prep_post(
            app,
            "/api/simulation/prepare/status",
            serde_json::json!({"task_id": "task-does-not-exist-xyz"}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
    }

    /// B1 precedence: when simulation_id is PREPARED, the B1 short-circuit returns ready
    /// FIRST — before task_id is even looked at — so the response carries NO task_id (the
    /// B1 shape, distinct from B3a's task_id-echo). This is the Python control-flow order
    /// (L679-692 runs before the task_id branch). The B3a task_id-echo path is only reachable
    /// when B1 says not-prepared yet the sim becomes prepared between the two checks (a
    /// non-deterministic race; covered by inspection per architecture §3).
    #[tokio::test]
    async fn prepare_status_b1_precedes_task_id_when_prepared() {
        let (state, sim_id, _tmp) = seed_prepared_sim("ready", true);
        let app = crate::server::create_app(state);
        let (status, json) = prep_post(
            app,
            "/api/simulation/prepare/status",
            serde_json::json!({"task_id": "task-gone-abc", "simulation_id": sim_id}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["status"], "ready");
        assert_eq!(json["data"]["progress"], 100);
        assert_eq!(json["data"]["already_prepared"], true);
        // B1 shape carries prepare_info and NO task_id (unlike B3a).
        assert!(json["data"].get("prepare_info").is_some());
        assert!(json["data"].get("task_id").is_none(), "B1 precedes task_id → no task_id field");
    }

    // -----------------------------------------------------------------------
    // U-028 (c1): TeriError::Timeout → faithful HTTP status mapping.
    //   - map_runner_err:  Timeout → 504 (Python TimeoutError → 504) raw msg.
    //   - map_interview_err: Timeout → 504 wrapped in the route-specific i18n key
    //     (Python `t('api.interviewTimeout', error=str(e))`).
    // (ApiError's private fields are visible here: this module is a descendant of `api`.)
    // -----------------------------------------------------------------------

    #[test]
    fn map_runner_err_timeout_maps_to_504() {
        let err =
            map_runner_err(crate::error::TeriError::Timeout("等待命令响应超时 (60.0秒)".into()));
        assert_eq!(err.status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(err.body["success"], false);
        // Raw message preserved (the generic mapper does not i18n-wrap).
        assert_eq!(err.body["error"], "等待命令响应超时 (60.0秒)");
    }

    #[test]
    fn map_runner_err_sim_still_400_timeout_arm_does_not_capture_sim() {
        let err = map_runner_err(crate::error::TeriError::Sim("bad".into()));
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        // A non-timeout, non-sim error stays a 500 (catch-all).
        let err = map_runner_err(crate::error::TeriError::Api("boom".into()));
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn map_interview_err_timeout_wraps_in_i18n_key_504() {
        let inner = "等待命令响应超时 (60.0秒)";
        // en: "Interview response timed out: {error}"
        let err = crate::i18n::with_locale("en".to_string(), async {
            map_interview_err(
                crate::error::TeriError::Timeout(inner.into()),
                "api.interviewTimeout",
            )
        })
        .await;
        assert_eq!(err.status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(err.body["error"], format!("Interview response timed out: {inner}"));

        // zh batch key: "等待批量Interview响应超时: {error}"
        let err = crate::i18n::with_locale("zh".to_string(), async {
            map_interview_err(
                crate::error::TeriError::Timeout(inner.into()),
                "api.batchInterviewTimeout",
            )
        })
        .await;
        assert_eq!(err.status, StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(err.body["error"], format!("等待批量Interview响应超时: {inner}"));
    }

    #[tokio::test]
    async fn map_interview_err_defers_non_timeout_to_map_runner_err() {
        // A ValueError-class (Sim) error still maps to 400 even through the interview mapper.
        let err = crate::i18n::with_locale("en".to_string(), async {
            map_interview_err(
                crate::error::TeriError::Sim("agent不存在".into()),
                "api.interviewTimeout",
            )
        })
        .await;
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        // The i18n key is NOT applied to a non-timeout error (raw msg preserved).
        assert_eq!(err.body["error"], "agent不存在");
    }
}
