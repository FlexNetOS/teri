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
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
};
use serde_json::Value;

use crate::api::{ApiError, ApiState};
use crate::services::simulation_manager::SimulationStatus;

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
        // Sub-cycle (c): simulation create/get/list routes.
        // IMPORTANT: /list (static) BEFORE /:simulation_id (capture) — axum 0.7 route order.
        .route("/create", post(create_simulation))
        .route("/list", get(list_simulations))
        .route("/:simulation_id", get(get_simulation))
        .with_state(state)
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
}
