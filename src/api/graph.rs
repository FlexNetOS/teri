//! Graph API route handlers — port of `backend/app/api/graph.py` (MiroFish, 622 lines).
//!
//! Sub-cycles (a)+(b): shared seam + the 4 project-management routes.
//! Sub-cycles (c)–(f) will add the remaining 6 routes to `graph_router` by adding
//! `.route(...)` lines below the existing ones.
//!
//! # Route → handler map (this file, routes 1–4 of 10)
//!
//! | # | Python route (graph.py)              | teri handler    | Status     |
//! |---|--------------------------------------|-----------------|------------|
//! | 1 | `GET  /project/<id>`           (:36) | `get_project`   | ported (b) |
//! | 2 | `GET  /project/list`           (:55) | `list_projects` | ported (b) |
//! | 3 | `DELETE /project/<id>`         (:70) | `delete_project`| ported (b) |
//! | 4 | `POST /project/<id>/reset`     (:89) | `reset_project` | ported (b) |
//! | 5 | `POST /ontology/generate`     (:122) | `generate_ontology` | PENDING (c) |
//! | 6 | `POST /build`                 (:260) | `build_graph`   | PENDING (d) |
//! | 7 | `GET  /task/<id>`             (:534) | `get_task`      | PENDING (e) |
//! | 8 | `GET  /tasks`                 (:553) | `list_tasks`    | PENDING (e) |
//! | 9 | `GET  /data/<graph_id>`       (:569) | `get_graph_data`| PENDING (f) |
//! | 10| `DELETE /delete/<graph_id>`   (:597) | `delete_graph`  | PENDING (f) |
//!
//! # `[≠]` / `[!]` flags inherited from architecture doc
//!
//! - `[≠] U025-TRACEBACK`: `traceback` key in 500 body carries Rust string, not Python stack.
//!   The 3-key shape `{success, error, traceback}` is preserved — only the value differs.
//! - `[!] U025-ROUTE-ORDER`: `/project/list` and `/project/:project_id` must NOT overlap-panic
//!   in axum 0.7.  Axum 0.7 ranks static segments above captures on the same router, so the
//!   static `/project/list` wins over `:project_id` for the literal "list" segment.
//!   See test `route_order_list_not_matched_as_project_id`.

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
use crate::models::project::{ProjectManager, ProjectStatus};

// ---------------------------------------------------------------------------
// Router factory — wires ONLY the 4 project routes (sub-cycle b).
// Sub-cycles c–f add `.route(...)` lines here as they land.
//
// `[!] U025-ROUTE-ORDER`: The route `/project/list` is registered BEFORE `/project/:project_id`.
// In axum 0.7, static path segments take priority over capture segments when both are registered
// on the same Router, so `GET /project/list` resolves to `list_projects` — NOT `get_project`.
// The test `route_order_list_not_matched_as_project_id` asserts this invariant.
// ---------------------------------------------------------------------------

/// Build the axum sub-router for `/api/graph/*` (U-025).
///
/// The caller nests this under `/api/graph` in `create_app`:
/// ```text
/// let api_router = Router::new().nest("/graph", graph_router(state.clone()));
/// ```
///
/// Sub-cycles (c)–(f) add additional `.route(...)` calls to this factory.
/// U-026/U-027 add `.nest("/simulation", …)` / `.nest("/report", …)` to `api_router` in
/// `server.rs`, NOT to this function.
pub fn graph_router(state: Arc<ApiState>) -> Router {
    Router::new()
        // Project management routes (sub-cycle b)
        // Static /project/list MUST be registered before the capture /project/:project_id.
        // Axum 0.7 will correctly route "list" to list_projects, not get_project.
        .route("/project/list", get(list_projects))
        .route("/project/:project_id", get(get_project).delete(delete_project))
        .route("/project/:project_id/reset", post(reset_project))
        // Sub-cycles (c)–(f) will add:
        // .route("/ontology/generate", post(generate_ontology))
        // .route("/build", post(build_graph))
        // .route("/task/:task_id", get(get_task))
        // .route("/tasks", get(list_tasks))
        // .route("/data/:graph_id", get(get_graph_data))
        // .route("/delete/:graph_id", delete(delete_graph))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Route 1 — GET /project/:project_id  (graph.py:36-52)
//
// Source:
//   project = ProjectManager.get_project(project_id)
//   if not project:
//       return jsonify({"success": False, "error": t('api.projectNotFound', id=...)})}, 404
//   return jsonify({"success": True, "data": project.to_dict()})
//
// Note: Python has NO try/except here — any exception (IOError, JSON parse) propagates as 500.
// Rust faithfully maps: Err(e) → ApiError::server(e).
// ---------------------------------------------------------------------------

async fn get_project(
    State(state): State<Arc<ApiState>>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let pm = ProjectManager::from_config(&state.config);
    let project = pm.get_project(&project_id).map_err(ApiError::server)?;

    match project {
        None => Err(ApiError::client(
            StatusCode::NOT_FOUND,
            crate::i18n::t_args("api.projectNotFound", &[("id", &project_id)]),
        )),
        Some(p) => Ok(Json(serde_json::json!({
            "success": true,
            "data": p.to_dict()
        }))),
    }
}

// ---------------------------------------------------------------------------
// Route 2 — GET /project/list  (graph.py:55-67)
//
// Source:
//   limit = request.args.get('limit', 50, type=int)
//   projects = ProjectManager.list_projects(limit=limit)
//   return jsonify({"success": True, "data": [...], "count": len(projects)})
//
// Flask's `type=int` semantics: returns the DEFAULT value (50) when the param
// is ABSENT or when parsing fails (e.g. ?limit=abc → 50, NOT 400).
// We replicate: parse optional "limit" string → fallback to 50 on absent/bad.
// ---------------------------------------------------------------------------

async fn list_projects(
    State(state): State<Arc<ApiState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    // Replicate Flask request.args.get('limit', 50, type=int):
    // absent → 50; present but not a valid integer → 50 (NOT a 400 error).
    let limit: usize = params.get("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(50);

    let pm = ProjectManager::from_config(&state.config);
    let projects = pm.list_projects(limit).map_err(ApiError::server)?;

    let data: Vec<Value> = projects.iter().map(|p| p.to_dict()).collect();
    let count = data.len();

    Ok(Json(serde_json::json!({
        "success": true,
        "data": data,
        "count": count
    })))
}

// ---------------------------------------------------------------------------
// Route 3 — DELETE /project/:project_id  (graph.py:70-86)
//
// Source:
//   success = ProjectManager.delete_project(project_id)
//   if not success:
//       return jsonify({"success": False, "error": t('api.projectDeleteFailed', id=...)}), 404
//   return jsonify({"success": True, "message": t('api.projectDeleted', id=...)})
// ---------------------------------------------------------------------------

async fn delete_project(
    State(state): State<Arc<ApiState>>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let pm = ProjectManager::from_config(&state.config);
    let deleted = pm.delete_project(&project_id).map_err(ApiError::server)?;

    if !deleted {
        return Err(ApiError::client(
            StatusCode::NOT_FOUND,
            crate::i18n::t_args("api.projectDeleteFailed", &[("id", &project_id)]),
        ));
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": crate::i18n::t_args("api.projectDeleted", &[("id", &project_id)])
    })))
}

// ---------------------------------------------------------------------------
// Route 4 — POST /project/:project_id/reset  (graph.py:89-117)
//
// Source:
//   project = ProjectManager.get_project(project_id)
//   if not project:
//       return jsonify({...projectNotFound...}), 404
//   # Reset to ontology_generated if ontology exists, else created
//   if project.ontology:
//       project.status = ProjectStatus.ONTOLOGY_GENERATED
//   else:
//       project.status = ProjectStatus.CREATED
//   project.graph_id = None
//   project.graph_build_task_id = None
//   project.error = None
//   ProjectManager.save_project(project)
//   return jsonify({"success": True,
//                   "message": t('api.projectReset', id=...),
//                   "data": project.to_dict()})
//
// Key order in success body: success, message, data — matches graph.py:113-117.
// ---------------------------------------------------------------------------

async fn reset_project(
    State(state): State<Arc<ApiState>>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let pm = ProjectManager::from_config(&state.config);
    let project_opt = pm.get_project(&project_id).map_err(ApiError::server)?;

    let mut project = match project_opt {
        None => {
            return Err(ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.projectNotFound", &[("id", &project_id)]),
            ));
        }
        Some(p) => p,
    };

    // Reset status: ontology present → OntologyGenerated, else → Created (graph.py:103-106)
    project.status = if project.ontology.is_some() {
        ProjectStatus::OntologyGenerated
    } else {
        ProjectStatus::Created
    };

    // Clear build state (graph.py:108-110)
    project.graph_id = None;
    project.graph_build_task_id = None;
    project.error = None;

    pm.save_project(&mut project).map_err(ApiError::server)?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": crate::i18n::t_args("api.projectReset", &[("id", &project_id)]),
        "data": project.to_dict()
    })))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    // -----------------------------------------------------------------------
    // Helper: build a test state pointing at a temp upload_folder.
    // -----------------------------------------------------------------------

    fn test_state() -> (Arc<ApiState>, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let mut config = crate::Config::build_test();
        config.upload_folder = tmp.path().to_string_lossy().to_string();
        (Arc::new(ApiState::new(config)), tmp)
    }

    /// Build a full `create_app` router wired to a temp upload_folder,
    /// so `/api/graph/*` routes are accessible.
    fn test_app() -> (Router, tempfile::TempDir) {
        let (state, tmp) = test_state();
        let app = crate::server::create_app(state);
        (app, tmp)
    }

    /// Seed a project in the given state and return its project_id.
    fn seed_project(state: &Arc<ApiState>, name: &str) -> String {
        let pm = ProjectManager::from_config(&state.config);
        let p = pm.create_project(name).expect("seed create");
        p.project_id
    }

    /// Seed a project with an ontology set (for reset tests).
    fn seed_project_with_ontology(state: &Arc<ApiState>, name: &str) -> String {
        let pm = ProjectManager::from_config(&state.config);
        let mut p = pm.create_project(name).expect("seed create");
        p.ontology = Some(serde_json::json!({"entities": []}));
        pm.save_project(&mut p).expect("seed save");
        p.project_id
    }

    // -----------------------------------------------------------------------
    // ROUTE-ORDER test — `[!] U025-ROUTE-ORDER`
    //
    // GET /api/graph/project/list must resolve to list_projects, NOT to
    // get_project("list").  Axum 0.7 static-before-capture resolution ensures
    // this; the test proves it by checking for the list envelope response.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn route_order_list_not_matched_as_project_id() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/api/graph/project/list").body(Body::empty()).unwrap())
            .await
            .unwrap();

        // Must be 200, not 404 (which would happen if axum matched "list" as a project_id)
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "ROUTE-ORDER: /project/list must not be matched as get_project('list')"
        );

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], true);
        // The list endpoint returns a "data" array and "count", not a "data" object
        assert!(json["data"].is_array(), "list envelope must have data array: {json}");
        assert!(json["count"].is_number(), "list envelope must have count: {json}");
    }

    // -----------------------------------------------------------------------
    // get_project — 200 (seeded) and 404 (missing)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_project_200_data_matches_to_dict() {
        let (state, _tmp) = test_state();
        let project_id = seed_project(&state, "Test Project");

        // Fetch the seeded project via the full app
        let app = crate::server::create_app(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/graph/project/{project_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["project_id"], project_id);
        assert_eq!(json["data"]["name"], "Test Project");
        assert_eq!(json["data"]["status"], "created");
    }

    #[tokio::test]
    async fn get_project_404_missing() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/graph/project/nonexistent-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], false);
        // error message contains the project id
        let error = json["error"].as_str().unwrap();
        assert!(
            error.contains("nonexistent-id"),
            "404 error must mention the project id: {error}"
        );
    }

    // -----------------------------------------------------------------------
    // list_projects — default limit, explicit ?limit=N, bad ?limit=abc
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn list_projects_empty_default_limit() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/api/graph/project/list").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], true);
        assert_eq!(json["count"], 0);
        assert_eq!(json["data"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_projects_explicit_limit() {
        let (state, _tmp) = test_state();
        // Seed 3 projects
        seed_project(&state, "A");
        seed_project(&state, "B");
        seed_project(&state, "C");

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/graph/project/list?limit=2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], true);
        // limit=2 → at most 2 results
        assert!(json["count"].as_u64().unwrap() <= 2, "limit=2 must cap results at 2: {json}");
    }

    #[tokio::test]
    async fn list_projects_bad_limit_falls_back_to_50() {
        // Flask semantics: bad type=int value → falls back to default (50), NOT a 400
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/graph/project/list?limit=abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Must be 200 (not 400/422) — Flask ignores bad int, uses default 50
        assert_eq!(resp.status(), StatusCode::OK, "bad ?limit=abc must fall back to 50, not 400");
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], true, "body must be success envelope: {json}");
    }

    #[tokio::test]
    async fn list_projects_count_matches_data_len() {
        let (state, _tmp) = test_state();
        seed_project(&state, "X");
        seed_project(&state, "Y");

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(Request::builder().uri("/api/graph/project/list").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let count = json["count"].as_u64().unwrap() as usize;
        let data_len = json["data"].as_array().unwrap().len();
        assert_eq!(count, data_len, "count field must equal data array length");
    }

    // -----------------------------------------------------------------------
    // delete_project — 200 (deleted) and 404 (not found)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn delete_project_200_message() {
        let (state, _tmp) = test_state();
        let project_id = seed_project(&state, "ToDelete");

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/graph/project/{project_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], true);
        assert!(json.get("message").is_some(), "delete 200 must have message key: {json}");
    }

    #[tokio::test]
    async fn delete_project_404_not_found() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/graph/project/does-not-exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], false);
        assert!(json.get("error").is_some(), "delete 404 must have error key: {json}");
    }

    // -----------------------------------------------------------------------
    // reset_project — status machine, field clearing, data+message, 404
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn reset_project_with_ontology_sets_ontology_generated() {
        let (state, _tmp) = test_state();
        let project_id = seed_project_with_ontology(&state, "WithOntology");

        // Set graph_id and graph_build_task_id so we can verify they're cleared
        let pm = ProjectManager::from_config(&state.config);
        let mut p = pm.get_project(&project_id).unwrap().unwrap();
        p.graph_id = Some("some-graph-id".to_string());
        p.graph_build_task_id = Some("some-task-id".to_string());
        p.error = Some("some-error".to_string());
        p.status = crate::models::project::ProjectStatus::GraphCompleted;
        pm.save_project(&mut p).unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/graph/project/{project_id}/reset"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], true);
        assert!(json.get("message").is_some(), "reset must have message key");
        assert!(json.get("data").is_some(), "reset must have data key");

        // Status must be ontology_generated (ontology was set)
        assert_eq!(
            json["data"]["status"], "ontology_generated",
            "project with ontology must reset to ontology_generated: {json}"
        );
        // graph_id / graph_build_task_id / error must be cleared
        assert!(json["data"]["graph_id"].is_null(), "graph_id must be null after reset: {json}");
        assert!(
            json["data"]["graph_build_task_id"].is_null(),
            "graph_build_task_id must be null after reset: {json}"
        );
        assert!(json["data"]["error"].is_null(), "error must be null after reset: {json}");
    }

    #[tokio::test]
    async fn reset_project_without_ontology_sets_created() {
        let (state, _tmp) = test_state();
        let project_id = seed_project(&state, "NoOntology");

        // Force status to something other than Created to prove the reset sets it back
        let pm = ProjectManager::from_config(&state.config);
        let mut p = pm.get_project(&project_id).unwrap().unwrap();
        p.status = crate::models::project::ProjectStatus::GraphCompleted;
        pm.save_project(&mut p).unwrap();

        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/graph/project/{project_id}/reset"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], true);
        assert_eq!(
            json["data"]["status"], "created",
            "project without ontology must reset to created: {json}"
        );
    }

    #[tokio::test]
    async fn reset_project_404_missing() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/graph/project/no-such-project/reset")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["success"], false);
        assert!(json.get("error").is_some(), "reset 404 must have error key: {json}");
    }

    // -----------------------------------------------------------------------
    // ApiError IntoResponse — verify exact HTTP status codes and body shapes
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn api_error_client_body_shape() {
        use axum::response::IntoResponse;

        // Build a client error and verify it produces the exact 2-key shape
        // {success: false, error: "<msg>"} with the correct status code.
        let err = ApiError::client(StatusCode::BAD_REQUEST, "something bad");
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], false);
        assert_eq!(json["error"], "something bad");
        // 2-key shape: must NOT have traceback
        assert!(json.get("traceback").is_none(), "client error must not have traceback key");
    }

    #[tokio::test]
    async fn api_error_client_status_code_404() {
        // Drive through a real handler: get_project on missing project → 404
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(
                Request::builder().uri("/api/graph/project/bad-id").body(Body::empty()).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "client error must yield 404");
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], false);
        assert!(json.get("error").is_some());
        assert!(json.get("traceback").is_none(), "client error must NOT have traceback key");
    }

    #[tokio::test]
    async fn api_error_server_body_has_3_keys() {
        // ApiError::server always produces {success, error, traceback}
        let err = ApiError::server("internal failure");
        // Check body has traceback key using IntoResponse
        use axum::response::IntoResponse;
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], false);
        assert_eq!(json["error"], "internal failure");
        assert!(
            json.get("traceback").is_some(),
            "server error must have traceback key (3-key shape)"
        );
    }

    // -----------------------------------------------------------------------
    // /health still works — U-002/U-003 non-regression
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn health_still_returns_200_after_graph_router_wired() {
        let (app, _tmp) = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["service"], "teri");
    }
}
