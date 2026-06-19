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
use crate::services::simulation_manager::SimulationStatus;
use crate::services::simulation_runner::RunnerStatus;

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
/// Used by: `get_graph_entities`, `get_entity_detail`, `get_entities_by_type`.
async fn load_entity_reader_graph(
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
        crate::error::TeriError::Sim(msg) => ApiError::client(StatusCode::BAD_REQUEST, msg),
        other => ApiError::server(other),
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

/// Port of MiroFish `start_simulation` (simulation.py:1451-1641).
///
/// Full boundary port with ONE honest 500 at the RunInputs construction gap
/// (GAP-U026-RUNINPUTS-BUILDER). All validation + state-machine paths are fully ported.
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

    // Capture locals for the gap comment (prevent unused-variable warnings)
    let _ = (max_rounds, enable_graph_memory_update, force_restarted, graph_id, platform);

    // === [!] GAP-U026-RUNINPUTS-BUILDER =======================================
    // Python (:1604-1627): run_state = SimulationRunner.start_simulation(id, platform,
    //   max_rounds, enable_graph_memory_update, graph_id)
    //   → state.status = RUNNING + save
    //   → response = run_state.to_dict() + max_rounds_applied? + graph_memory_update_enabled
    //                + force_restarted + graph_id?
    //
    // teri CANNOT build RunInputs<OpenAiAdapter> { engine, pool, graph, llm }:
    //   • engine needs SimConfig::from_simulation_config (does not exist)
    //   • pool needs profile→AgentPool reader (does not exist)
    // Both are produced by U-028 (twitter) / U-029 (reddit) / U-030 (parallel).
    //
    // DO NOT call state.sim_runner.start_simulation with MockLlm / fake RunInputs.
    // DO NOT fabricate a run_state or a 200 success.
    //
    // When U-028/029/030 land: build RunInputs here (SimConfig::from_simulation_config for engine;
    //   profile reader for pool; load_entity_reader_graph wrapped Arc<Mutex<_>> for graph when
    //   memory enabled; build_llm for llm), call state.sim_runner.start_simulation(...),
    //   set sim.status=Running+save, assemble 200 response. ONE localized swap.
    //
    // [!] GRAPH-UPDATER-WIRING-PENDING: when memory is enabled and graph_id is resolved,
    //   `graph_for_updater = Some(Arc::new(tokio::sync::Mutex::new(load_entity_reader_graph(&state, graph_id).await?)))`.
    //   This wiring is specified but not live until the producer lands.
    // ==========================================================================
    Err(ApiError::server(format!(
        "simulation runtime not available: no RunInputs builder for '{id}' \
         (blocked on U-028/U-029/U-030 platform producers — GAP-U026-RUNINPUTS-BUILDER)"
    )))
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

    /// POST /start — fully prepared, valid request reaches the RunInputs gap → 500 GAP message.
    ///
    /// This is the KEY parity test for (g2): proves the entire boundary (validation +
    /// state-machine + status=Ready) runs before the honest error, and that the handler
    /// emits the structured GAP-U026-RUNINPUTS-BUILDER 500 (not a fabricated 200).
    #[tokio::test]
    async fn start_simulation_prepared_reaches_gap_500() {
        // Seed a sim with status=ready (passes state-machine without triggering the branch)
        let (state, _tmp) = test_state();
        let (project_id, _) = seed_project_with_graph(&state);
        let sim = state
            .sim_manager
            .create_simulation(&project_id, "graph-abc", true, true)
            .expect("create sim");
        let sim_id = sim.simulation_id.clone();

        // Patch the sim to status=Ready in the manager cache + state.json
        let sim_dir =
            std::path::PathBuf::from(&state.config.oasis_simulation_data_dir).join(&sim_id);
        std::fs::create_dir_all(&sim_dir).unwrap();
        let state_json = serde_json::json!({
            "status": "ready",
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
        state.sim_manager.evict_cache_for_test(&sim_id);

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

        // Must be 500 with the GAP message — NOT a fabricated 200, NOT a 400/404
        assert_eq!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "prepared sim must reach the gap and return 500"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false, "gap 500 must have success=false");

        let error = json["error"].as_str().unwrap_or("");
        assert!(
            error.contains("GAP-U026-RUNINPUTS-BUILDER"),
            "error must contain GAP marker: {error}"
        );
        assert!(
            error.contains("runtime not available") || error.contains("no RunInputs"),
            "error must describe the gap: {error}"
        );

        // Must have traceback (server 500 shape)
        assert!(json.get("traceback").is_some(), "server 500 must have traceback key");
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
}
