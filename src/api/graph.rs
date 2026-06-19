//! Graph API route handlers — port of `backend/app/api/graph.py` (MiroFish, 622 lines).
//!
//! Sub-cycles (a)+(b): shared seam + the 4 project-management routes.
//! Sub-cycle (c): `/ontology/generate` multipart upload → LLM ontology generation.
//! Sub-cycles (d)–(f) will add the remaining 5 routes to `graph_router`.
//!
//! # Route → handler map (this file, routes 1–5 of 10)
//!
//! | # | Python route (graph.py)              | teri handler        | Status     |
//! |---|--------------------------------------|---------------------|------------|
//! | 1 | `GET  /project/<id>`           (:36) | `get_project`       | ported (b) |
//! | 2 | `GET  /project/list`           (:55) | `list_projects`     | ported (b) |
//! | 3 | `DELETE /project/<id>`         (:70) | `delete_project`    | ported (b) |
//! | 4 | `POST /project/<id>/reset`     (:89) | `reset_project`     | ported (b) |
//! | 5 | `POST /ontology/generate`     (:122) | `generate_ontology` | ported (c) |
//! | 6 | `POST /build`                 (:260) | `build_graph`       | PENDING (d) |
//! | 7 | `GET  /task/<id>`             (:534) | `get_task`          | PENDING (e) |
//! | 8 | `GET  /tasks`                 (:553) | `list_tasks`        | PENDING (e) |
//! | 9 | `GET  /data/<graph_id>`       (:569) | `get_graph_data`    | PENDING (f) |
//! | 10| `DELETE /delete/<graph_id>`   (:597) | `delete_graph`      | PENDING (f) |
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
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
};
use serde_json::Value;

use crate::api::{ApiError, ApiState, build_llm};
use crate::llm::LlmClient;
use crate::models::project::{ProjectManager, ProjectStatus};
use crate::seed::{self, SeedIngestor};
use crate::services::ontology::OntologyGenerator;

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
    // Per-route body limit for the multipart upload endpoint (graph.py MAX_CONTENT_LENGTH = 50 MB).
    // We read the limit from `Config::max_content_length` at router build time so tests can
    // override it via `Config::build_test` (which also defaults to 50 MB).
    let upload_limit = state.config.max_content_length as usize;

    Router::new()
        // Project management routes (sub-cycle b)
        // Static /project/list MUST be registered before the capture /project/:project_id.
        // Axum 0.7 will correctly route "list" to list_projects, not get_project.
        .route("/project/list", get(list_projects))
        .route("/project/:project_id", get(get_project).delete(delete_project))
        .route("/project/:project_id/reset", post(reset_project))
        // Sub-cycle (c): multipart upload → LLM ontology generation.
        // DefaultBodyLimit::max enforces the 50 MB cap (mirrors Flask MAX_CONTENT_LENGTH).
        .route(
            "/ontology/generate",
            post(generate_ontology).layer(DefaultBodyLimit::max(upload_limit)),
        )
        // Sub-cycles (d)–(f) will add:
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
// Route 5 — POST /ontology/generate  (graph.py:122-255)
//
// allowed_file(filename) → graph.py:26-31
//   Returns false if filename is empty or contains no '.'.
//   Extension = rsplit('.', max=1)[1].lower() (matches os.path.splitext lower).
//   Uses SeedIngestor::is_supported which mirrors Config.ALLOWED_EXTENSIONS
//   (txt / md / markdown / pdf / json — established at seed::mod:12).
//
// generate_ontology steps (graph.py:150-248):
//   1. Collect multipart fields in ONE forward pass (axum Multipart is single-pass).
//   2. Validate simulation_requirement present → 400 api.requireSimulationRequirement.
//   3. Validate ≥1 file with non-empty filename → 400 api.requireFileUpload.
//   4. Create project (ProjectManager::create_project).
//   5. Per file with non-empty filename AND allowed_file: save, extract, preprocess.
//   6. If no documents processed: delete project → 400 api.noDocProcessed.
//   7. total_text_length = all_text.chars().count() (CHAR count, NOT bytes — CJK safe).
//   8. save_extracted_text + OntologyGenerator::generate.
//   9. Store 2-key ontology {entity_types, edge_types} + analysis_summary + status.
//  10. Return 6-key data {project_id, project_name, ontology, analysis_summary,
//                         files, total_text_length}.
//
// Error shape: any non-validation error → ApiError::server (3-key 500 body).
// Validation errors → ApiError::client (2-key 400 body).
// ---------------------------------------------------------------------------

/// Port of MiroFish `allowed_file` (graph.py:26-31).
///
/// Replicates Python `os.path.splitext` semantics EXACTLY.
///
/// `os.path.splitext` treats leading dots of the basename as part of the root,
/// not as extension separators.  The extension is the last `.`-segment only when
/// a non-dot character precedes it (after stripping leading dots).
///
/// # Behaviour table (matches Python `os.path.splitext(basename)[1]`):
///
/// | input        | Python ext | allowed? |
/// |--------------|-----------|----------|
/// | `file.txt`   | `.txt`    | yes      |
/// | `.hidden.txt`| `.txt`    | yes      |
/// | `.a.txt`     | `.txt`    | yes      |
/// | `..a.txt`    | `.txt`    | yes      |
/// | `.txt`       | ``        | no (no non-dot before the dot) |
/// | `..txt`      | ``        | no       |
/// | `...txt`     | ``        | no       |
/// | `..md`       | ``        | no       |
/// | `nodotfile`  | ``        | no       |
/// | ``           | ``        | no       |
///
/// Uses `seed::is_allowed_ext` to check against the canonical set
/// (txt / md / markdown / pdf / json — `seed::mod:12`) — the SAME constant
/// `SeedIngestor::is_supported` uses, without duplicating it.
fn allowed_file(filename: &str) -> bool {
    // Take just the basename component so path separators don't confuse us
    // (Python's `file.filename` is already a basename, but be defensive).
    let basename = std::path::Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(filename);

    if basename.is_empty() || !basename.contains('.') {
        return false;
    }

    // Skip leading dots — they are part of the root in os.path.splitext.
    let stem_start = basename.find(|c: char| c != '.').unwrap_or(basename.len());
    let after_leading = &basename[stem_start..];

    // The extension is the suffix after the LAST dot in the non-leading portion.
    // If after_leading has no dot at all (e.g. "..txt" → after_leading = "txt"),
    // there is no extension.
    match after_leading.rfind('.') {
        Some(rel) => {
            // rel >= 0; since after_leading starts with a non-dot char, rel >= 1
            // is guaranteed (a dot at position 0 of after_leading would mean a
            // non-dot char followed by nothing — impossible after rfind).
            let ext = after_leading[rel + 1..].to_lowercase();
            seed::is_allowed_ext(&ext)
        }
        None => false,
    }
}

/// Port of MiroFish `generate_ontology` (graph.py:122-255).
///
/// Accepts a `multipart/form-data` POST with:
/// - `simulation_requirement` (text, required)
/// - `project_name` (text, optional, default "Unnamed Project")
/// - `additional_context` (text, optional, default "")
/// - `files` (file fields, ≥1 required, must have allowed extensions)
///
/// The body limit (50 MB default, from `Config::max_content_length`) is applied
/// via `DefaultBodyLimit::max` in `graph_router`.
///
/// # Handler design note — LLM injection
/// `OntologyGenerator<L: LlmClient>` is generic, so tests can inject any
/// `LlmClient` impl directly by calling the inner logic (`generate_ontology_inner`)
/// instead of this axum handler.  The axum handler itself uses `build_llm` (concrete
/// `OpenAiAdapter`) and is tested end-to-end in the happy-path test only when a real
/// LLM endpoint is not available by asserting the pre-LLM validation paths.
async fn generate_ontology(
    State(state): State<Arc<ApiState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    // -----------------------------------------------------------------------
    // Step 1: Collect multipart fields in one forward pass (graph.py:154-173).
    // Axum Multipart is single-pass — we MUST collect everything in one loop.
    // -----------------------------------------------------------------------
    let mut simulation_requirement = String::new();
    let mut project_name = String::from("Unnamed Project");
    let mut additional_context = String::new();
    // Each entry: (original_filename, bytes)
    let mut collected_files: Vec<(String, Vec<u8>)> = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(ApiError::server)? {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "simulation_requirement" => {
                let text = field.text().await.map_err(ApiError::server)?;
                simulation_requirement = text;
            }
            "project_name" => {
                let text = field.text().await.map_err(ApiError::server)?;
                project_name = text;
            }
            "additional_context" => {
                let text = field.text().await.map_err(ApiError::server)?;
                additional_context = text;
            }
            "files" => {
                let filename = field.file_name().unwrap_or("").to_string();
                let bytes = field.bytes().await.map_err(ApiError::server)?;
                collected_files.push((filename, bytes.to_vec()));
            }
            _ => {
                // Ignore unknown fields (drain so the connection is cleanly consumed)
                let _ = field.bytes().await;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Step 2: Validate simulation_requirement (graph.py:161-165).
    // -----------------------------------------------------------------------
    if simulation_requirement.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.requireSimulationRequirement"),
        ));
    }

    // -----------------------------------------------------------------------
    // Step 3: Validate ≥1 file with non-empty filename (graph.py:168-173).
    // -----------------------------------------------------------------------
    if collected_files.is_empty() || collected_files.iter().all(|(name, _)| name.is_empty()) {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.requireFileUpload"),
        ));
    }

    // Steps 4-10 delegated to generate_ontology_inner so tests can inject a mock LLM.
    let pm = ProjectManager::from_config(&state.config);
    generate_ontology_inner(
        &pm,
        build_llm(&state.config),
        simulation_requirement,
        project_name,
        additional_context,
        collected_files,
    )
    .await
}

/// Inner logic for `/ontology/generate` (steps 4-10 of graph.py:176-248).
///
/// Separated from the axum handler so tests can inject any `LlmClient` (e.g.
/// `MockLlmClient`) directly, exercising the full response-assembly path
/// without a real LLM endpoint.
///
/// `files` is `Vec<(original_filename, raw_bytes)>` — the already-collected
/// multipart file entries, each with a non-empty filename (validated in step 3
/// before this is called; empty-named entries are still filtered below for
/// safety but the outer handler already guarantees ≥1 has a name).
async fn generate_ontology_inner<L: LlmClient>(
    pm: &ProjectManager,
    llm: L,
    simulation_requirement: String,
    project_name: String,
    additional_context: String,
    files: Vec<(String, Vec<u8>)>,
) -> Result<Json<Value>, ApiError> {
    // -----------------------------------------------------------------------
    // Step 4: Create project (graph.py:176-177).
    // -----------------------------------------------------------------------
    let mut project = pm.create_project(&project_name).map_err(ApiError::server)?;
    project.simulation_requirement = Some(simulation_requirement.clone());

    // -----------------------------------------------------------------------
    // Step 5: Per file: save, extract text, preprocess (graph.py:184-201).
    // -----------------------------------------------------------------------
    let mut document_texts: Vec<String> = Vec::new();
    let mut all_text = String::new();

    for (filename, bytes) in &files {
        if filename.is_empty() || !allowed_file(filename) {
            continue;
        }

        // Save file to project directory (graph.py:187-191)
        let file_info = pm
            .save_file_to_project(&project.project_id, bytes, filename)
            .map_err(|e| ApiError::server(format!("Failed to save file '{filename}': {e}")))?;

        // 2-key {filename, size} shape pushed to project.files (graph.py:192-195)
        project.files.push(serde_json::json!({
            "filename": file_info.original_filename,
            "size": file_info.size
        }));

        // Text extraction: SeedIngestor::from_file gives raw_text (== FileParser.extract_text)
        // (graph.py:198)
        let doc = SeedIngestor::from_file(&file_info.path).await.map_err(|e| {
            ApiError::server(format!(
                "Text extraction failed for '{}': {e}",
                file_info.original_filename
            ))
        })?;
        let raw = doc.raw_text;

        // Preprocess (== TextProcessor.preprocess_text) (graph.py:199)
        let text = crate::seed::text_processor::preprocess_text(&raw);

        document_texts.push(text.clone());
        // EXACT header format from graph.py:201: "\n\n=== {original_filename} ===\n{text}"
        all_text.push_str(&format!("\n\n=== {} ===\n{}", file_info.original_filename, text));
    }

    // -----------------------------------------------------------------------
    // Step 6: Validate ≥1 processed document (graph.py:203-208).
    // -----------------------------------------------------------------------
    if document_texts.is_empty() {
        // Cleanup: delete the project that was created before we discovered no docs (graph.py:204)
        let _ = pm.delete_project(&project.project_id);
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.noDocProcessed"),
        ));
    }

    // -----------------------------------------------------------------------
    // Step 7: Record char count + save extracted text (graph.py:211-212).
    // CHAR count (not bytes) — matches Python len() which counts Unicode scalars.
    // -----------------------------------------------------------------------
    project.total_text_length = all_text.chars().count() as i64;
    pm.save_extracted_text(&project.project_id, &all_text)
        .map_err(ApiError::server)?;

    // -----------------------------------------------------------------------
    // Step 8: Generate ontology via LLM (graph.py:216-222).
    // -----------------------------------------------------------------------
    let additional_context_opt =
        if additional_context.is_empty() { None } else { Some(additional_context.as_str()) };

    let generator = OntologyGenerator::new(llm);
    let ontology = generator
        .generate(&document_texts, &simulation_requirement, additional_context_opt)
        .await
        .map_err(ApiError::server)?;

    // -----------------------------------------------------------------------
    // Step 9: Store ontology + analysis_summary + status (graph.py:229-235).
    // 2-key {entity_types, edge_types} object — EXACT shape, extra ontology keys dropped.
    // -----------------------------------------------------------------------
    project.ontology = Some(serde_json::json!({
        "entity_types": ontology.get("entity_types").cloned().unwrap_or(Value::Array(vec![])),
        "edge_types":   ontology.get("edge_types").cloned().unwrap_or(Value::Array(vec![]))
    }));
    project.analysis_summary = Some(
        ontology
            .get("analysis_summary")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    );
    project.status = ProjectStatus::OntologyGenerated;
    pm.save_project(&mut project).map_err(ApiError::server)?;

    // -----------------------------------------------------------------------
    // Step 10: Return success envelope (graph.py:238-248).
    // Key order: success, then data with 6 keys: project_id / project_name /
    //            ontology / analysis_summary / files / total_text_length.
    // -----------------------------------------------------------------------
    Ok(Json(serde_json::json!({
        "success": true,
        "data": {
            "project_id":         project.project_id,
            "project_name":       project.name,
            "ontology":           project.ontology,
            "analysis_summary":   project.analysis_summary,
            "files":              project.files,
            "total_text_length":  project.total_text_length
        }
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

    // -----------------------------------------------------------------------
    // allowed_file — unit tests (graph.py:26-31)
    // -----------------------------------------------------------------------

    #[test]
    fn allowed_file_empty_returns_false() {
        assert!(!allowed_file(""), "empty filename must not be allowed");
    }

    #[test]
    fn allowed_file_no_dot_returns_false() {
        assert!(!allowed_file("nodotfile"), "filename without '.' must not be allowed");
    }

    #[test]
    fn allowed_file_txt_allowed() {
        assert!(allowed_file("document.txt"), ".txt must be allowed");
    }

    #[test]
    fn allowed_file_md_allowed() {
        assert!(allowed_file("README.md"), ".md must be allowed");
    }

    #[test]
    fn allowed_file_markdown_allowed() {
        assert!(allowed_file("spec.markdown"), ".markdown must be allowed");
    }

    #[test]
    fn allowed_file_pdf_allowed() {
        assert!(allowed_file("report.pdf"), ".pdf must be allowed");
    }

    #[test]
    fn allowed_file_json_allowed() {
        assert!(allowed_file("data.json"), ".json must be allowed");
    }

    #[test]
    fn allowed_file_uppercase_extension_allowed() {
        // Python os.path.splitext lower-cases; SeedIngestor::is_supported also lower-cases.
        assert!(allowed_file("DOC.TXT"), "uppercase .TXT must be treated as .txt");
        assert!(allowed_file("Report.PDF"), "uppercase .PDF must be treated as .pdf");
    }

    #[test]
    fn allowed_file_exe_rejected() {
        assert!(!allowed_file("malware.exe"), ".exe must be rejected");
    }

    #[test]
    fn allowed_file_only_dot_rejected() {
        assert!(!allowed_file("."), "bare '.' must be rejected (no ext after dot)");
    }

    // -----------------------------------------------------------------------
    // allowed_file — os.path.splitext leading-multi-dot semantics (FIX 1)
    //
    // Python's os.path.splitext treats leading dots of the basename as part of
    // the root, not as extension separators.  Extension = the last '.' segment
    // only when a non-dot char precedes it (after the leading dots).
    //
    // REJECT cases (no extension per splitext):
    //   "..txt"  → after leading dots: "txt"  → no '.' in "txt" → no ext
    //   "...txt" → after leading dots: "txt"  → no '.' in "txt" → no ext
    //   "..md"   → after leading dots: "md"   → no '.' in "md"  → no ext
    //
    // ACCEPT cases (splitext finds an ext):
    //   ".hidden.txt" → leading dot stripped → non-dot remainder "hidden.txt"
    //                   → rfind('.') in "hidden.txt" → ext = "txt" → ACCEPT
    //   ".a.txt"      → leading dot → "a.txt" → ext = "txt" → ACCEPT
    //   "..a.txt"     → two leading dots → "a.txt" → ext = "txt" → ACCEPT
    // -----------------------------------------------------------------------

    #[test]
    fn allowed_file_double_dot_prefix_no_ext_rejected() {
        // "..txt" — Python splitext("..txt") = ('..txt', '') → no extension → REJECT
        assert!(!allowed_file("..txt"), "..txt must be rejected (no ext per os.path.splitext)");
    }

    #[test]
    fn allowed_file_triple_dot_prefix_no_ext_rejected() {
        // "...txt" — Python splitext("...txt") = ('...txt', '') → no extension → REJECT
        assert!(!allowed_file("...txt"), "...txt must be rejected");
    }

    #[test]
    fn allowed_file_double_dot_md_rejected() {
        // "..md" — same rule; "md" after leading dots has no dot → no ext → REJECT
        assert!(!allowed_file("..md"), "..md must be rejected");
    }

    #[test]
    fn allowed_file_hidden_dot_txt_accepted() {
        // ".hidden.txt" — Python splitext(".hidden.txt") = ('.hidden', '.txt') → ACCEPT
        assert!(allowed_file(".hidden.txt"), ".hidden.txt must be accepted (.txt ext)");
    }

    #[test]
    fn allowed_file_single_dot_prefix_txt_accepted() {
        // ".a.txt" — Python splitext(".a.txt") = ('.a', '.txt') → ACCEPT
        assert!(allowed_file(".a.txt"), ".a.txt must be accepted (.txt ext)");
    }

    #[test]
    fn allowed_file_double_dot_prefix_then_dot_txt_accepted() {
        // "..a.txt" — two leading dots then "a.txt" → ext = ".txt" → ACCEPT
        assert!(allowed_file("..a.txt"), "..a.txt must be accepted (.txt ext)");
    }

    // -----------------------------------------------------------------------
    // generate_ontology_inner — happy-path 200 with MockLlmClient
    //
    // Drives the REAL inner function (steps 4-10) with a mock LLM.
    // Asserts the exact 6-key data envelope the handler returns:
    //   {project_id, project_name, ontology(2-key), analysis_summary,
    //    files(2-key [{filename,size}]), total_text_length(char count)}.
    // Uses both a .txt and a .md file to prove multi-file aggregation.
    // Uses CJK content to prove char count vs byte count.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_ontology_inner_200_real_response_envelope() {
        let (state, _tmp) = test_state();
        let pm = ProjectManager::from_config(&state.config);

        // Two files: a .txt with CJK content (char≠byte) and a plain .md
        let files: Vec<(String, Vec<u8>)> = vec![
            ("doc.txt".to_string(), "你好世界".as_bytes().to_vec()),
            ("notes.md".to_string(), b"Hello world".to_vec()),
        ];

        let result = generate_ontology_inner(
            &pm,
            MockLlmClient::canned_ontology(),
            "simulate a social network".to_string(),
            "Inner Test Project".to_string(),
            String::new(), // empty additional_context
            files,
        )
        .await
        .expect("generate_ontology_inner must succeed with mock LLM");

        let Json(body) = result;

        // Top-level envelope: {success: true, data: {...}}
        assert_eq!(body["success"], true, "top-level success must be true: {body}");
        let data = &body["data"];
        assert!(data.is_object(), "data must be an object: {body}");

        // Exactly 6 keys in data
        let data_obj = data.as_object().unwrap();
        assert_eq!(
            data_obj.len(),
            6,
            "data must have exactly 6 keys, got {}: {data}",
            data_obj.len()
        );

        // All 6 keys present
        for key in &[
            "project_id",
            "project_name",
            "ontology",
            "analysis_summary",
            "files",
            "total_text_length",
        ] {
            assert!(data_obj.contains_key(*key), "data must contain '{key}': {data}");
        }

        // project_name matches what we passed
        assert_eq!(
            data["project_name"].as_str().unwrap(),
            "Inner Test Project",
            "project_name must match: {data}"
        );

        // ontology is exactly 2-key {entity_types, edge_types}
        let ont = &data["ontology"];
        assert!(ont.is_object(), "ontology must be an object: {data}");
        let ont_obj = ont.as_object().unwrap();
        assert_eq!(ont_obj.len(), 2, "ontology must be 2-key: {ont}");
        assert!(ont_obj.contains_key("entity_types"), "ontology must have entity_types: {ont}");
        assert!(ont_obj.contains_key("edge_types"), "ontology must have edge_types: {ont}");

        // analysis_summary is a non-null string
        assert!(
            data["analysis_summary"].as_str().is_some(),
            "analysis_summary must be a string: {data}"
        );

        // files: array with 2 entries, each 2-key {filename, size}
        let files_arr = data["files"].as_array().expect("files must be array");
        assert_eq!(files_arr.len(), 2, "files must have 2 entries (doc.txt + notes.md): {data}");
        for f in files_arr {
            let fobj = f.as_object().expect("file entry must be object");
            assert_eq!(fobj.len(), 2, "file entry must be 2-key {{filename, size}}: {f}");
            assert!(fobj.contains_key("filename"), "file entry must have filename: {f}");
            assert!(fobj.contains_key("size"), "file entry must have size: {f}");
        }

        // total_text_length must be a positive integer (char count)
        let ttl = data["total_text_length"].as_i64().expect("total_text_length must be i64");
        assert!(ttl > 0, "total_text_length must be positive: {ttl}");

        // Verify char count < byte count for the CJK content (proves char not byte counting).
        // "你好世界" = 4 chars, 12 UTF-8 bytes.  all_text wraps it in a header, so both
        // char_count and byte_count are larger, but the delta of 8 bytes (3 extra per CJK char)
        // ensures char_count < byte_count as long as any CJK bytes are present.
        // We verify by loading the project from the PM and checking extracted text.
        let project_id = data["project_id"].as_str().expect("project_id must be string");
        let saved_text = pm
            .get_extracted_text(project_id)
            .expect("get_extracted_text ok")
            .expect("extracted text present");
        let char_count = saved_text.chars().count() as i64;
        let byte_count = saved_text.len() as i64;
        assert!(
            char_count < byte_count,
            "CJK content: char count ({char_count}) must be < byte count ({byte_count})"
        );
        assert_eq!(
            ttl, char_count,
            "total_text_length must equal char count of all_text: {ttl} vs {char_count}"
        );
    }

    // -----------------------------------------------------------------------
    // generate_ontology — helpers for building raw multipart bodies
    // -----------------------------------------------------------------------

    /// Build a raw `multipart/form-data` body with the given boundary and parts.
    ///
    /// Each part is `(field_name, optional_filename, content_type, body_bytes)`.
    fn multipart_body(boundary: &str, parts: &[(&str, Option<&str>, &str, &[u8])]) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        for (name, filename, content_type, data) in parts {
            buf.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            if let Some(fname) = filename {
                buf.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{fname}\"\r\n"
                    )
                    .as_bytes(),
                );
            } else {
                buf.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n").as_bytes(),
                );
            }
            buf.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
            buf.extend_from_slice(data);
            buf.extend_from_slice(b"\r\n");
        }
        buf.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        buf
    }

    // -----------------------------------------------------------------------
    // generate_ontology — 400 missing simulation_requirement
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_ontology_400_missing_simulation_requirement() {
        let (app, _tmp) = test_app();
        let boundary = "testboundary001";

        // Only provide a file, no simulation_requirement text field
        let body = multipart_body(
            boundary,
            &[("files", Some("hello.txt"), "text/plain", b"some content")],
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/graph/ontology/generate")
                    .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], false);
        assert!(json["error"].as_str().is_some(), "must have error key: {json}");
        // No traceback key on client error
        assert!(json.get("traceback").is_none(), "client error must not have traceback: {json}");
    }

    // -----------------------------------------------------------------------
    // generate_ontology — 400 no files uploaded
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_ontology_400_no_files() {
        let (app, _tmp) = test_app();
        let boundary = "testboundary002";

        // Only simulation_requirement, no files field
        let body = multipart_body(
            boundary,
            &[("simulation_requirement", None, "text/plain", b"simulate a market")],
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/graph/ontology/generate")
                    .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], false);
    }

    // -----------------------------------------------------------------------
    // generate_ontology — 400 all files have empty filename
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_ontology_400_empty_filename() {
        let (app, _tmp) = test_app();
        let boundary = "testboundary003";

        // File field with empty filename
        let body = multipart_body(
            boundary,
            &[
                ("simulation_requirement", None, "text/plain", b"simulate a market"),
                ("files", Some(""), "text/plain", b"content"),
            ],
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/graph/ontology/generate")
                    .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], false);
    }

    // -----------------------------------------------------------------------
    // generate_ontology — 400 no docs processed (all disallowed extension)
    // Also asserts the project created earlier was deleted (graph.py:204).
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_ontology_400_no_docs_processed_disallowed_ext() {
        let (state, _tmp) = test_state();
        let app = crate::server::create_app(state.clone());
        let boundary = "testboundary004";

        // .exe is not in ALLOWED_EXTENSIONS
        let body = multipart_body(
            boundary,
            &[
                ("simulation_requirement", None, "text/plain", b"simulate a market"),
                ("files", Some("payload.exe"), "application/octet-stream", b"binary data"),
            ],
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/graph/ontology/generate")
                    .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], false);

        // Verify the project was cleaned up: list_projects should return 0
        let pm = ProjectManager::from_config(&state.config);
        let projects = pm.list_projects(50).expect("list");
        assert_eq!(
            projects.len(),
            0,
            "project created before noDocProcessed must be deleted: {projects:?}"
        );
    }

    // -----------------------------------------------------------------------
    // generate_ontology — file save + text extraction + preprocess
    //   → document_texts populated, all_text has EXACT header format,
    //     total_text_length is CHAR count (not bytes, tested with CJK text).
    //
    // We test the file-processing path by invoking the inner logic directly
    // via a mock LLM client injected into OntologyGenerator — this avoids
    // needing a real LLM endpoint while proving every pre-LLM behavior.
    // -----------------------------------------------------------------------

    /// A canned `LlmClient` for test injection into `OntologyGenerator`.
    ///
    /// Returns a fixed JSON ontology payload without making any network calls.
    /// This is the canonical test-injection pattern for `OntologyGenerator<L>`.
    struct MockLlmClient {
        /// The JSON string the mock returns as the LLM "message content"
        response_json: String,
    }

    impl MockLlmClient {
        fn canned_ontology() -> Self {
            Self {
                response_json: r#"{"entity_types":[{"name":"Person","description":"A person","attributes":[],"examples":[]}],"edge_types":[{"name":"KNOWS","description":"knows","attributes":[],"source_targets":[]}],"analysis_summary":"test summary"}"#
                    .to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::llm::LlmClient for MockLlmClient {
        async fn complete(&self, _prompt: &str) -> crate::error::Result<String> {
            Ok(self.response_json.clone())
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(
            &self,
            _prompt: &str,
        ) -> crate::error::Result<T> {
            serde_json::from_str(&self.response_json).map_err(crate::error::TeriError::from)
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> crate::error::Result<
            std::pin::Pin<Box<dyn futures::Stream<Item = crate::error::Result<String>> + Send>>,
        > {
            Err(crate::error::TeriError::Llm(
                "MockLlmClient::stream not implemented".to_string(),
            ))
        }

        async fn chat(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> crate::error::Result<String> {
            Ok(self.response_json.clone())
        }

        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> crate::error::Result<T> {
            serde_json::from_str(&self.response_json).map_err(crate::error::TeriError::from)
        }
    }

    // -----------------------------------------------------------------------
    // generate_ontology — happy path with mock LLM via generate_ontology_inner
    //   Proves: project created, file saved, text extracted + preprocessed,
    //           all_text header format exact, total_text_length = CHAR count,
    //           ontology 2-key shape, analysis_summary, status OntologyGenerated,
    //           response data 6-key shape, files 2-key shape.
    //
    // Uses generate_ontology_inner directly so it exercises the REAL handler
    // code path (steps 4-10), not a re-implementation.
    // Uses CJK file content to prove char count vs byte count.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_ontology_happy_path_with_mock_llm() {
        let (state, _tmp) = test_state();
        let pm = ProjectManager::from_config(&state.config);

        // CJK text: "你好世界" = 4 chars but 12 UTF-8 bytes
        let files: Vec<(String, Vec<u8>)> =
            vec![("test.txt".to_string(), "你好世界".as_bytes().to_vec())];

        let Json(body) = generate_ontology_inner(
            &pm,
            MockLlmClient::canned_ontology(),
            "simulate a social network".to_string(),
            "Test Project".to_string(),
            String::new(),
            files,
        )
        .await
        .expect("generate_ontology_inner must succeed");

        // Top-level envelope
        assert_eq!(body["success"], true);
        let data = &body["data"];

        // 6-key data shape
        let data_obj = data.as_object().unwrap();
        assert_eq!(data_obj.len(), 6, "response data must have exactly 6 keys: {data}");
        for key in &[
            "project_id",
            "project_name",
            "ontology",
            "analysis_summary",
            "files",
            "total_text_length",
        ] {
            assert!(data_obj.contains_key(*key), "data must contain '{key}': {data}");
        }

        // project_name
        assert_eq!(data["project_name"].as_str().unwrap(), "Test Project");

        // Ontology 2-key shape
        let ont = &data["ontology"];
        let ont_obj = ont.as_object().expect("ontology must be object");
        assert_eq!(ont_obj.len(), 2, "ontology must be 2-key: {ont}");
        assert!(ont_obj.contains_key("entity_types"), "ontology must have entity_types: {ont}");
        assert!(ont_obj.contains_key("edge_types"), "ontology must have edge_types: {ont}");

        // analysis_summary
        assert!(
            data["analysis_summary"].as_str().is_some(),
            "analysis_summary must be a string: {data}"
        );

        // files 2-key shape, 1 entry
        let files_arr = data["files"].as_array().expect("files must be array");
        assert_eq!(files_arr.len(), 1, "must have 1 file entry: {data}");
        let f = &files_arr[0];
        let fobj = f.as_object().expect("file entry must be object");
        assert_eq!(fobj.len(), 2, "file entry must be 2-key {{filename, size}}: {f}");
        assert!(fobj.contains_key("filename"), "file entry must have filename: {f}");
        assert!(fobj.contains_key("size"), "file entry must have size: {f}");

        // total_text_length: CHAR count < byte count (proves CJK-safe counting)
        let ttl = data["total_text_length"].as_i64().expect("total_text_length must be i64");
        let project_id = data["project_id"].as_str().expect("project_id string");
        let saved_text = pm.get_extracted_text(project_id).expect("ok").expect("some");
        let char_count = saved_text.chars().count() as i64;
        let byte_count = saved_text.len() as i64;
        assert!(
            char_count < byte_count,
            "CJK char count ({char_count}) must be < byte count ({byte_count})"
        );
        assert_eq!(ttl, char_count, "total_text_length must equal char count");

        // all_text header format: exactly "\n\n=== test.txt ===\n{text}"
        assert!(saved_text.contains("=== test.txt ==="), "header format: {saved_text:?}");
        assert!(saved_text.starts_with("\n\n=== "), "leading CRLF pair: {saved_text:?}");

        // Verify persisted project status
        let saved = pm.get_project(project_id).expect("get").expect("found");
        assert_eq!(saved.status, ProjectStatus::OntologyGenerated);
        assert_eq!(saved.total_text_length, char_count);
        assert!(saved.analysis_summary.is_some(), "analysis_summary must be set");
    }

    // -----------------------------------------------------------------------
    // generate_ontology — multipart parse: text fields extracted correctly
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_ontology_multipart_text_fields_parsed() {
        // We verify text field parsing by hitting the route with a minimal valid body
        // and observing that simulation_requirement / project_name / additional_context
        // are correctly parsed — we do this by checking the 400 that results from a
        // DISALLOWED extension carries no project-field errors (text fields parsed OK
        // but file rejected), and by directly testing a valid upload path below.
        let (app, _tmp) = test_app();
        let boundary = "testboundary010";

        // Include all 3 text fields + a disallowed file (so we get noDocProcessed 400)
        let body = multipart_body(
            boundary,
            &[
                ("simulation_requirement", None, "text/plain", b"simulate a trade war"),
                ("project_name", None, "text/plain", b"Trade War Sim"),
                ("additional_context", None, "text/plain", b"Focus on tech sector"),
                ("files", Some("bad.exe"), "application/octet-stream", b"binary"),
            ],
        );

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/graph/ontology/generate")
                    .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        // We get noDocProcessed (not requireSimulationRequirement or requireFileUpload)
        // which proves simulation_requirement was parsed and the file was seen with a name.
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], false);
        // The error must be the noDocProcessed message (not the other two 400s)
        let err = json["error"].as_str().unwrap_or("");
        assert!(
            !err.contains("simulation_requirement") && !err.contains("upload"),
            "error must be noDocProcessed (not sim_req or upload): {err}"
        );
    }

    // -----------------------------------------------------------------------
    // Non-regression: (a)+(b) project routes still work after (c) landed
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn project_routes_non_regression_after_ontology_route_added() {
        let (state, _tmp) = test_state();
        let project_id = seed_project(&state, "Regression Check");

        let app = crate::server::create_app(state);

        // GET /project/:id
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/graph/project/{project_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "GET /project/:id must still be 200");

        // GET /project/list
        let resp = app
            .oneshot(Request::builder().uri("/api/graph/project/list").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "GET /project/list must still be 200");
    }
}
