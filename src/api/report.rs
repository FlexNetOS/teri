//! U-027 — Report HTTP API (`report.py` → `/api/report`).
//!
//! Port-fresh ROUTING layer over the U-024 `reuse-Y` substrate (`ReportAgent`,
//! `ReportManager`, `ReportLogger`, `ReportConsoleLogger`, `ReportTools` — all
//! already parity-verified). U-027 writes ZERO new producer logic; it wires HTTP
//! handlers onto U-024's verified symbols. ARCHITECTED → findings/u027-architecture.md.
//!
//! ## Route → handler map (18 handlers / 17 paths, sub-cycles a–f)
//!
//! | Sub-cycle | Routes | Substrate |
//! |-----------|--------|-----------|
//! | a  | `GET /:id`, `/by-simulation/:sim`, `/list`, `/:id/progress`, `/check/:sim`, `DELETE /:id` | `ReportManager` reads |
//! | b  | `GET /:id/agent-log[/stream]`, `/console-log[/stream]` | `ReportManager` log reads (JSON, NOT SSE) |
//! | c  | `GET /:id/download`, `/:id/sections`, `/:id/section/:idx` | `ReportManager` + GAP-A/B pub wrappers |
//! | d  | `POST /tools/search`, `/tools/statistics` | `ReportTools` (graph facade) |
//! | e  | `POST /chat` | `ReportAgent::chat` |
//! | f  | `POST /generate`, `/generate/status` | async `generate_report` (OS-thread spawn) |
//!
//! All handlers return the U-025 envelope `Result<Json<Value>, ApiError>`
//! (`[≠] U025-TRACEBACK`). `ReportManager` is instance-based in teri
//! (`ReportManager::new(&config.upload_folder)`); Python uses classmethods over the
//! same `uploads/reports` dir — no behavioral difference (FS-backed, stateless).
//!
//! `[!] U027-ROUTE-ORDER`: seg-0 statics (`/list`, `/generate`, `/chat`, `/tools/*`,
//!   `/by-simulation/:s`, `/check/:s`) registered BEFORE the `/:report_id` capture
//!   (axum 0.7 static-before-capture; same convention as simulation_router).

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

use crate::api::simulation::load_entity_reader_graph;
use crate::api::{ApiError, ApiState};
use crate::report::ReportStatus;
use crate::report::manager::ReportManager;
use crate::services::zep_tools::ReportTools;

/// Build the `/report` sub-router. Mirrors `simulation_router` (DECISION-U026-1 state).
///
/// The router GROWS per sub-cycle (same incremental pattern as `simulation_router`):
/// sub-cycle (a) registers the 6 pure-read routes below; (b)–(f) append their routes
/// in their own commits. Unported routes are simply absent (404), never stubbed.
pub fn report_router(state: Arc<ApiState>) -> Router {
    Router::new()
        // ── Sub-cycle (a): pure read routes ──
        // seg-0 STATIC `/list` FIRST (axum 0.7 ranks static > the `/:report_id` capture).
        .route("/list", get(list_reports_route))
        // seg-0 static `by-simulation`/`check` — distinct from `/:report_id/...` by full path.
        .route("/by-simulation/:simulation_id", get(get_report_by_sim_route))
        .route("/check/:simulation_id", get(check_report_status_route))
        // capture root: /:report_id GET + DELETE (same path, two methods).
        .route("/:report_id", get(get_report_route).delete(delete_report_route))
        .route("/:report_id/progress", get(get_progress_route))
        // ── Sub-cycle (b): log-read routes (one-shot JSON, NOT SSE — source IS JSON) ──
        // 2-seg /agent-log vs 3-seg /agent-log/stream: distinct static tails, full-path match.
        .route("/:report_id/agent-log", get(get_agent_log_route))
        .route("/:report_id/agent-log/stream", get(get_agent_log_stream_route))
        .route("/:report_id/console-log", get(get_console_log_route))
        .route("/:report_id/console-log/stream", get(get_console_log_stream_route))
        // ── Sub-cycle (c): sections + download ──
        .route("/:report_id/download", get(download_report_route))
        .route("/:report_id/sections", get(get_sections_route))
        // /section/:idx — captured as String + parsed manually so a non-integer 404s
        // (Flask <int:> no-match status), not 400 (axum typed-path parse failure).
        .route("/:report_id/section/:section_index", get(get_single_section_route))
        // ── Sub-cycle (d): tools debug routes (POST, body graph_id) ──
        // seg-0 static "tools" — distinct from the `/:report_id` capture by full path.
        .route("/tools/search", post(tools_search_route))
        .route("/tools/statistics", post(tools_statistics_route))
        .with_state(state)
}

/// `ReportManager` rooted at `{upload_folder}/reports` (Python's `uploads/reports`).
fn report_manager(state: &ApiState) -> ReportManager {
    ReportManager::new(&state.config.upload_folder)
}

// ===========================================================================
// Sub-cycle (a) — pure read routes
// ===========================================================================

// ---------------------------------------------------------------------------
// GET /:report_id  (report.py:277-316)
//   get_report(id) None → 404 reportNotFound; Some → 200 {success, data:to_dict}
// ---------------------------------------------------------------------------
async fn get_report_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    match report_manager(&state).get_report(&report_id) {
        None => Err(ApiError::client(
            StatusCode::NOT_FOUND,
            crate::i18n::t_args("api.reportNotFound", &[("id", &report_id)]),
        )),
        Some(report) => Ok(Json(serde_json::json!({
            "success": true,
            "data": Value::Object(report.to_dict())
        }))),
    }
}

// ---------------------------------------------------------------------------
// GET /by-simulation/:simulation_id  (report.py:319-355)
//   None → 404 {success:false, error:noReportForSim, has_report:false}
//   Some → 200 {success:true, data:to_dict, has_report:true}
//
// `has_report` is a TOP-LEVEL envelope key (sibling of success/data), not inside data.
// 404 path uses a manual body (ApiError can't carry the extra has_report key) — built
// via client_with so the 404 carries {success:false, error, has_report:false}.
// ---------------------------------------------------------------------------
async fn get_report_by_sim_route(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    match report_manager(&state).get_report_by_simulation(&simulation_id) {
        None => {
            let mut extra = serde_json::Map::new();
            extra.insert("has_report".to_string(), Value::Bool(false));
            Err(ApiError::client_with(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.noReportForSim", &[("id", &simulation_id)]),
                extra,
            ))
        }
        Some(report) => Ok(Json(serde_json::json!({
            "success": true,
            "data": Value::Object(report.to_dict()),
            "has_report": true
        }))),
    }
}

// ---------------------------------------------------------------------------
// GET /list  (report.py:358-395)
//   ?simulation_id (optional filter), ?limit (type=int default 50)
//   → 200 {success, data:[to_dict], count}
// ---------------------------------------------------------------------------
async fn list_reports_route(
    State(state): State<Arc<ApiState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let simulation_id = params.get("simulation_id").map(|s| s.as_str());
    // type=int default 50; usize-parse (neg/non-numeric → 50, U-025 precedent).
    let limit = params.get("limit").and_then(|s| s.parse::<usize>().ok()).unwrap_or(50);

    let reports = report_manager(&state).list_reports(simulation_id, limit);
    let data: Vec<Value> = reports.iter().map(|r| Value::Object(r.to_dict())).collect();
    let count = data.len();

    Ok(Json(serde_json::json!({
        "success": true,
        "data": data,
        "count": count
    })))
}

// ---------------------------------------------------------------------------
// DELETE /:report_id  (report.py:444-467)
//   delete_report(id) false → 404 reportNotFound; true → 200 {success, message:reportDeleted}
// ---------------------------------------------------------------------------
async fn delete_report_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if report_manager(&state).delete_report(&report_id) {
        Ok(Json(serde_json::json!({
            "success": true,
            "message": crate::i18n::t_args("api.reportDeleted", &[("id", &report_id)])
        })))
    } else {
        Err(ApiError::client(
            StatusCode::NOT_FOUND,
            crate::i18n::t_args("api.reportNotFound", &[("id", &report_id)]),
        ))
    }
}

// ---------------------------------------------------------------------------
// GET /:report_id/progress  (report.py:569-607)
//   get_progress(id) None → 404 reportProgressNotAvail; Some → 200 {success, data:progress}
// ---------------------------------------------------------------------------
async fn get_progress_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    match report_manager(&state).get_progress(&report_id) {
        None => Err(ApiError::client(
            StatusCode::NOT_FOUND,
            crate::i18n::t_args("api.reportProgressNotAvail", &[("id", &report_id)]),
        )),
        Some(progress) => Ok(Json(serde_json::json!({
            "success": true,
            "data": Value::Object(progress)
        }))),
    }
}

// ---------------------------------------------------------------------------
// GET /check/:simulation_id  (report.py:707-753)
//   Always 200. data = {simulation_id, has_report, report_status, report_id,
//   interview_unlocked}. report_status/report_id null when no report.
//   interview_unlocked = has_report && status == Completed.
// ---------------------------------------------------------------------------
async fn check_report_status_route(
    State(state): State<Arc<ApiState>>,
    Path(simulation_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let report = report_manager(&state).get_report_by_simulation(&simulation_id);
    let has_report = report.is_some();
    let report_status: Value = match &report {
        Some(r) => serde_json::to_value(&r.status)
            .ok()
            .and_then(|v| v.as_str().map(|s| Value::String(s.to_string())))
            .unwrap_or(Value::Null),
        None => Value::Null,
    };
    let report_id: Value = report
        .as_ref()
        .map(|r| Value::String(r.report_id.clone()))
        .unwrap_or(Value::Null);
    let interview_unlocked =
        report.as_ref().map(|r| r.status == ReportStatus::Completed).unwrap_or(false);

    Ok(Json(serde_json::json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "has_report": has_report,
            "report_status": report_status,
            "report_id": report_id,
            "interview_unlocked": interview_unlocked
        }
    })))
}

// ===========================================================================
// Sub-cycle (b) — log read routes
//
// Decision (ii) (findings/u027-architecture.md §3.ii): the `/stream` routes are
// NOT SSE — the source (report.py:817-852, 899-934) returns a one-shot JSON
// `{logs, count}` full-dump. The "stream" name means "get the whole stream at
// once"; incremental tailing is the `from_line` param on the NON-`/stream` routes.
// So all four port as ordinary JSON handlers. `[~] U027-SSE-SEAM-DORMANT`: the
// U-024 sink.rs SseSink seam stays reserved (no source route maps to live SSE).
// ===========================================================================

// ---------------------------------------------------------------------------
// GET /:report_id/agent-log  (report.py:758-814)
//   ?from_line (type=int default 0) → get_agent_log(id, from_line) → {success, data:map}
//   where map = {logs, total_lines, from_line, has_more}.
// ---------------------------------------------------------------------------
async fn get_agent_log_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let from_line = params.get("from_line").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let log_data = report_manager(&state).get_agent_log(&report_id, from_line);
    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(log_data)
    })))
}

// ---------------------------------------------------------------------------
// GET /:report_id/agent-log/stream  (report.py:817-848)
//   get_agent_log_stream(id) → {success, data:{logs, count}}  (one-shot full dump)
// ---------------------------------------------------------------------------
async fn get_agent_log_stream_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let logs = report_manager(&state).get_agent_log_stream(&report_id);
    let count = logs.len();
    Ok(Json(serde_json::json!({
        "success": true,
        "data": { "logs": logs, "count": count }
    })))
}

// ---------------------------------------------------------------------------
// GET /:report_id/console-log  (report.py:853-896)
//   ?from_line → get_console_log(id, from_line) → {success, data:map}
// ---------------------------------------------------------------------------
async fn get_console_log_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let from_line = params.get("from_line").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let log_data = report_manager(&state).get_console_log(&report_id, from_line);
    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(log_data)
    })))
}

// ---------------------------------------------------------------------------
// GET /:report_id/console-log/stream  (report.py:899-930)
//   get_console_log_stream(id) → {success, data:{logs, count}}
// ---------------------------------------------------------------------------
async fn get_console_log_stream_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let logs = report_manager(&state).get_console_log_stream(&report_id);
    let count = logs.len();
    Ok(Json(serde_json::json!({
        "success": true,
        "data": { "logs": logs, "count": count }
    })))
}

// ===========================================================================
// Sub-cycle (c) — sections + download
// ===========================================================================

// ---------------------------------------------------------------------------
// GET /:report_id/download  (report.py:398-441)
//   get_report None → 404 reportNotFound; else serve the markdown as a
//   `text/markdown` attachment (download_name={report_id}.md).
//   Source serves the on-disk full_report.md if present, else a temp file from
//   report.markdown_content — both yield the SAME bytes (save_report writes
//   full_report.md = markdown_content). teri reads the on-disk md (GAP-A) and
//   falls back to markdown_content. This handler returns `Response`, NOT `Json`.
// ---------------------------------------------------------------------------
async fn download_report_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
) -> Result<Response, ApiError> {
    let mgr = report_manager(&state);
    let report = match mgr.get_report(&report_id) {
        None => {
            return Err(ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.reportNotFound", &[("id", &report_id)]),
            ));
        }
        Some(r) => r,
    };

    // On-disk full_report.md if present (Python md_path branch), else markdown_content
    // (Python temp-file branch). Same bytes either way.
    let content = mgr.read_report_markdown(&report_id).unwrap_or(report.markdown_content);

    let disposition = format!("attachment; filename=\"{report_id}.md\"");
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("text/markdown; charset=utf-8"))
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&disposition).map_err(ApiError::server)?,
        )
        .body(axum::body::Body::from(Bytes::from(content.into_bytes())))
        .map_err(ApiError::server)?;
    Ok(resp)
}

// ---------------------------------------------------------------------------
// GET /:report_id/sections  (report.py:610-650)
//   get_generated_sections(id) + get_report (is_complete = report && Completed)
//   → 200 {success, data:{report_id, sections, total_sections, is_complete}}
// ---------------------------------------------------------------------------
async fn get_sections_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let mgr = report_manager(&state);
    let sections = mgr.get_generated_sections(&report_id);
    let total_sections = sections.len();
    let is_complete = mgr
        .get_report(&report_id)
        .map(|r| r.status == ReportStatus::Completed)
        .unwrap_or(false);

    let sections_arr: Vec<Value> = sections.into_iter().map(Value::Object).collect();
    Ok(Json(serde_json::json!({
        "success": true,
        "data": {
            "report_id": report_id,
            "sections": sections_arr,
            "total_sections": total_sections,
            "is_complete": is_complete
        }
    })))
}

// ---------------------------------------------------------------------------
// GET /:report_id/section/:section_index  (report.py:661-702)
//   Flask `<int:section_index>`: a NON-integer segment doesn't match the route →
//   Werkzeug default 404. axum's typed `Path<usize>` would instead 400 (parse
//   failure), a STATUS divergence. So we capture the segment as a String and
//   parse it manually → non-integer yields a faithful 404 (matching Flask's
//   <int:> no-match status). `[≠] U027-c-SECTIONIDX-404BODY`: Flask returns its
//   default HTML 404 (no JSON errorhandler in __init__.py); teri returns the JSON
//   sectionNotFound 404 — the STATUS (404) is contract-faithful, the body shape is
//   a framework-default-error-page artifact (non-contractual; a non-integer index
//   is never sent by the frontend, which constructs `/section/{int}`).
//   Valid-but-missing index → 404 sectionNotFound{index:02d}; present →
//   200 {success, data:{filename, section_index, content}}.
// ---------------------------------------------------------------------------
async fn get_single_section_route(
    State(state): State<Arc<ApiState>>,
    Path((report_id, section_index_raw)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    // Flask <int:> non-match → 404 (route doesn't match).
    let section_index: usize = match section_index_raw.parse() {
        Ok(n) => n,
        Err(_) => {
            return Err(ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.sectionNotFound", &[("index", &section_index_raw)]),
            ));
        }
    };

    match report_manager(&state).get_single_section(&report_id, section_index) {
        None => {
            // Python: t('api.sectionNotFound', index=f"{section_index:02d}")
            let idx2 = format!("{section_index:02}");
            Err(ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.sectionNotFound", &[("index", &idx2)]),
            ))
        }
        Some((filename, content)) => Ok(Json(serde_json::json!({
            "success": true,
            "data": {
                "filename": filename,
                "section_index": section_index,
                "content": content
            }
        }))),
    }
}

// ===========================================================================
// Sub-cycle (d) — tools debug routes  (report.py:935-1020)
//
// Both POST. Body-supplied `graph_id` → `load_entity_reader_graph` (ZEP guard +
// task→graph, REUSED from simulation.rs, not duplicated) → `ReportTools::new(&graph,
// &llm)` → `search_graph`/`get_graph_statistics` → `to_dict`. First handlers to
// exercise the `ReportTools` borrow-facade + `build_llm`. The graph resolves to an
// OWNED `KnowledgeGraph` (after the only `.await`), then `ReportTools` borrows it +
// the per-request `llm` and the sync search/stats calls return owned Maps — NO borrow
// is held across an await, so the handler futures stay `Send`.
//
// Flags:
//   `[≠] U026-ZEPKEY` (inherited via load_entity_reader_graph): ZEP guard KEPT —
//      empty config.zep_api_key → 500 zepApiKeyMissing (matches source + U-025/026).
//   `[!] U027-GRAPHREQ`: a RESOLVED-but-empty graph returns an EMPTY result set
//      (facts/edges/nodes []), faithful to source on an empty graph — the data is
//      producer-supplied (a built graph_build task). Runs end-to-end today.
//   `[≠] U026-R2-ABSENTGRAPH` (inherited from sub-cycle b entities): an UNRESOLVABLE
//      graph_id (no graph_build task) → 500 (teri cannot build a reader over a
//      nonexistent LOCAL graph) vs Python's blanket-except → empty. STATUS differs
//      ONLY for the secondary absent-graph case; the PRIMARY contract (resolved graph
//      → result.to_dict) is faithful. Same substrate-forced input-domain narrowing
//      already adjudicated for the entities routes (DECISION-U026 R2).
//   `[≠] U025-TRACEBACK`: 500 body carries a Rust backtrace string, not a Python stack.
// ===========================================================================

/// Tolerate absent/empty body like Python's `request.get_json() or {}`.
fn parse_tools_body(body: Option<Json<Value>>) -> Value {
    body.map(|j| j.0).unwrap_or_else(|| serde_json::json!({}))
}

// ---------------------------------------------------------------------------
// POST /tools/search  (report.py:935-980)
//   data = get_json() or {}; graph_id = data.graph_id; query = data.query;
//   limit = data.get('limit', 10).
//   not graph_id or not query → 400 api.requireGraphIdAndQuery.
//   else ZepToolsService().search_graph(graph_id, query, limit) → {success, data:to_dict}.
//
// teri MAP: ZepToolsService() → load_entity_reader_graph + ReportTools::new(&graph,&llm).
// Source omits `scope` → Python default "edges"; teri passes Some("edges") to match.
// ---------------------------------------------------------------------------
async fn tools_search_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = parse_tools_body(body);
    let graph_id = data.get("graph_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let query = data.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string();
    // Python `data.get('limit', 10)`: absent/non-int → 10.
    let limit = data.get("limit").and_then(|v| v.as_i64()).unwrap_or(10);

    // Python `not graph_id or not query` — empty string / absent are both falsy → 400.
    if graph_id.is_empty() || query.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.requireGraphIdAndQuery"),
        ));
    }

    // ZEP guard + graph resolution (REUSED helper). Only await; yields an owned graph.
    let graph = load_entity_reader_graph(&state, &graph_id).await?;
    let llm = crate::api::build_llm(&state.config);
    let tools = ReportTools::new(&graph, &llm);
    // scope omitted in source → Python default "edges".
    let result = tools.search_graph(&graph_id, &query, limit, Some("edges"));

    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(result.to_dict())
    })))
}

// ---------------------------------------------------------------------------
// POST /tools/statistics  (report.py:983-1020)
//   data = get_json() or {}; graph_id = data.graph_id.
//   not graph_id → 400 api.requireGraphId.
//   else ZepToolsService().get_graph_statistics(graph_id) → {success, data:result}.
//   (result is already a plain dict — {graph_id,total_nodes,total_edges,entity_types,
//   relation_types} — so data is the Map directly, NOT a .to_dict() wrapper.)
// ---------------------------------------------------------------------------
async fn tools_statistics_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = parse_tools_body(body);
    let graph_id = data.get("graph_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

    if graph_id.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.requireGraphId"),
        ));
    }

    let graph = load_entity_reader_graph(&state, &graph_id).await?;
    let llm = crate::api::build_llm(&state.config);
    let tools = ReportTools::new(&graph, &llm);
    let stats = tools.get_graph_statistics(&graph_id);

    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(stats)
    })))
}

// Sub-cycles (e)–(f) append their routes + handlers in their own commits:
//   (e) chat; (f) generate+generate/status.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{Report, ReportOutline, ReportSection, ReportStatus};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state() -> (Arc<ApiState>, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let mut config = crate::Config::build_test();
        config.upload_folder = tmp.path().to_string_lossy().to_string();
        (Arc::new(ApiState::new(config)), tmp)
    }

    fn make_report(id: &str, sim: &str, status: ReportStatus) -> Report {
        Report {
            report_id: id.to_string(),
            simulation_id: sim.to_string(),
            graph_id: "g1".to_string(),
            simulation_requirement: "req".to_string(),
            status,
            outline: Some(ReportOutline {
                title: "T".to_string(),
                summary: "S".to_string(),
                sections: vec![ReportSection { title: "Sec1".to_string(), content: String::new() }],
            }),
            markdown_content: "# T\n\nBody.".to_string(),
            created_at: "2024-01-01T00:00:00".to_string(),
            completed_at: "2024-01-01T01:00:00".to_string(),
            error: None,
        }
    }

    fn seed(state: &Arc<ApiState>, report: &Report) {
        report_manager(state).save_report(report).expect("save_report");
    }

    async fn req(app: axum::Router, method: &str, uri: &str) -> (StatusCode, serde_json::Value) {
        let resp = app
            .oneshot(Request::builder().method(method).uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    // ---- GET /:report_id ----------------------------------------------------

    #[tokio::test]
    async fn get_report_not_found_404() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
        assert!(json.get("traceback").is_none());
    }

    #[tokio::test]
    async fn get_report_found_200() {
        let (state, _t) = test_state();
        seed(&state, &make_report("report_a", "sim1", ReportStatus::Completed));
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_a").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["report_id"], "report_a");
        assert_eq!(json["data"]["status"], "completed");
    }

    // ---- GET /by-simulation/:simulation_id ---------------------------------

    #[tokio::test]
    async fn by_simulation_not_found_404_has_report_false() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/by-simulation/sim_x").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
        // has_report is a TOP-LEVEL key (sibling of success/error), not inside data.
        assert_eq!(json["has_report"], false);
    }

    #[tokio::test]
    async fn by_simulation_found_200_has_report_true() {
        let (state, _t) = test_state();
        seed(&state, &make_report("report_b", "sim_y", ReportStatus::Completed));
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/by-simulation/sim_y").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["has_report"], true);
        assert_eq!(json["data"]["simulation_id"], "sim_y");
    }

    // ---- GET /list ----------------------------------------------------------

    #[tokio::test]
    async fn list_empty() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/list").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["count"], 0);
        assert!(json["data"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_filters_by_simulation_and_caps_limit() {
        let (state, _t) = test_state();
        seed(&state, &make_report("report_1", "simA", ReportStatus::Completed));
        seed(&state, &make_report("report_2", "simA", ReportStatus::Completed));
        seed(&state, &make_report("report_3", "simB", ReportStatus::Completed));
        let app = crate::server::create_app(state.clone());
        // all
        let (_s, all) = req(app, "GET", "/api/report/list").await;
        assert_eq!(all["count"], 3);
        // filter by simulation_id
        let app2 = crate::server::create_app(state.clone());
        let (_s, by_a) = req(app2, "GET", "/api/report/list?simulation_id=simA").await;
        assert_eq!(by_a["count"], 2);
        // limit
        let app3 = crate::server::create_app(state);
        let (_s, lim) = req(app3, "GET", "/api/report/list?limit=1").await;
        assert_eq!(lim["count"], 1);
    }

    /// ROUTE-ORDER: /list must resolve to list_reports, NOT get_report("list").
    #[tokio::test]
    async fn route_order_list_not_report_id() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/list").await;
        assert_eq!(status, StatusCode::OK, "/list must not match /:report_id");
        assert!(json["data"].is_array());
    }

    // ---- DELETE /:report_id -------------------------------------------------

    #[tokio::test]
    async fn delete_not_found_404() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "DELETE", "/api/report/report_nope").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn delete_found_200() {
        let (state, _t) = test_state();
        seed(&state, &make_report("report_del", "sim1", ReportStatus::Completed));
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "DELETE", "/api/report/report_del").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["success"], true);
        assert!(json["message"].is_string());
    }

    // ---- GET /:report_id/progress ------------------------------------------

    #[tokio::test]
    async fn progress_not_found_404() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_x/progress").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn progress_found_200() {
        let (state, _t) = test_state();
        // Write progress.json directly to the report folder.
        let rm = report_manager(&state);
        let folder = rm.ensure_report_folder("report_p").expect("folder");
        std::fs::write(
            folder.join("progress.json"),
            serde_json::json!({"status":"generating","progress":45,"message":"m"}).to_string(),
        )
        .unwrap();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_p/progress").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["status"], "generating");
        assert_eq!(json["data"]["progress"], 45);
    }

    // ---- GET /check/:simulation_id -----------------------------------------

    #[tokio::test]
    async fn check_no_report() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/check/sim_none").await;
        assert_eq!(status, StatusCode::OK);
        let d = &json["data"];
        assert_eq!(d["simulation_id"], "sim_none");
        assert_eq!(d["has_report"], false);
        assert_eq!(d["report_status"], serde_json::Value::Null);
        assert_eq!(d["report_id"], serde_json::Value::Null);
        assert_eq!(d["interview_unlocked"], false);
    }

    #[tokio::test]
    async fn check_completed_report_unlocks_interview() {
        let (state, _t) = test_state();
        seed(&state, &make_report("report_c", "sim_done", ReportStatus::Completed));
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/check/sim_done").await;
        assert_eq!(status, StatusCode::OK);
        let d = &json["data"];
        assert_eq!(d["has_report"], true);
        assert_eq!(d["report_status"], "completed");
        assert_eq!(d["report_id"], "report_c");
        assert_eq!(d["interview_unlocked"], true);
    }

    #[tokio::test]
    async fn check_non_completed_report_locks_interview() {
        let (state, _t) = test_state();
        seed(&state, &make_report("report_g", "sim_gen", ReportStatus::Generating));
        let app = crate::server::create_app(state);
        let (_status, json) = req(app, "GET", "/api/report/check/sim_gen").await;
        let d = &json["data"];
        assert_eq!(d["has_report"], true);
        assert_eq!(d["report_status"], "generating");
        assert_eq!(d["interview_unlocked"], false, "non-completed must NOT unlock interview");
    }

    // =======================================================================
    // Sub-cycle (b) — log read routes
    // =======================================================================

    fn seed_agent_log(state: &Arc<ApiState>, report_id: &str, lines: &[serde_json::Value]) {
        let folder = report_manager(state).ensure_report_folder(report_id).expect("folder");
        let body: String =
            lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n") + "\n";
        std::fs::write(folder.join("agent_log.jsonl"), body).unwrap();
    }

    fn seed_console_log(state: &Arc<ApiState>, report_id: &str, lines: &[&str]) {
        let folder = report_manager(state).ensure_report_folder(report_id).expect("folder");
        std::fs::write(folder.join("console_log.txt"), lines.join("\n") + "\n").unwrap();
    }

    // ---- agent-log ----------------------------------------------------------

    #[tokio::test]
    async fn agent_log_missing_file_empty_shape() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_x/agent-log").await;
        assert_eq!(status, StatusCode::OK);
        let d = &json["data"];
        assert_eq!(d["logs"].as_array().unwrap().len(), 0);
        assert_eq!(d["total_lines"], 0);
        assert_eq!(d["from_line"], 0);
        assert_eq!(d["has_more"], false);
    }

    #[tokio::test]
    async fn agent_log_with_entries_and_from_line() {
        let (state, _t) = test_state();
        seed_agent_log(
            &state,
            "report_al",
            &[
                serde_json::json!({"action": "report_start", "section_index": 0}),
                serde_json::json!({"action": "tool_call", "section_index": 1}),
                serde_json::json!({"action": "report_complete", "section_index": 2}),
            ],
        );
        let app = crate::server::create_app(state.clone());
        let (status, json) = req(app, "GET", "/api/report/report_al/agent-log").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["total_lines"], 3);
        assert_eq!(json["data"]["logs"].as_array().unwrap().len(), 3);
        assert_eq!(json["data"]["logs"][0]["action"], "report_start");

        // from_line=1 → skip the first line.
        let app2 = crate::server::create_app(state);
        let (_s, j2) = req(app2, "GET", "/api/report/report_al/agent-log?from_line=1").await;
        assert_eq!(j2["data"]["from_line"], 1);
        assert_eq!(j2["data"]["logs"].as_array().unwrap().len(), 2);
        assert_eq!(j2["data"]["logs"][0]["action"], "tool_call");
    }

    #[tokio::test]
    async fn agent_log_stream_logs_and_count() {
        let (state, _t) = test_state();
        seed_agent_log(
            &state,
            "report_als",
            &[serde_json::json!({"action": "a"}), serde_json::json!({"action": "b"})],
        );
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_als/agent-log/stream").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["count"], 2);
        assert_eq!(json["data"]["logs"].as_array().unwrap().len(), 2);
    }

    /// ROUTE-ORDER: /agent-log (2-seg) vs /agent-log/stream (3-seg) resolve distinctly.
    #[tokio::test]
    async fn agent_log_stream_vs_nonstream_distinct() {
        let (state, _t) = test_state();
        seed_agent_log(&state, "report_d", &[serde_json::json!({"x": 1})]);
        // /agent-log returns the {logs,total_lines,from_line,has_more} shape...
        let app = crate::server::create_app(state.clone());
        let (_s, j1) = req(app, "GET", "/api/report/report_d/agent-log").await;
        assert!(j1["data"].get("total_lines").is_some(), "non-stream has total_lines");
        // ...while /agent-log/stream returns {logs,count}.
        let app2 = crate::server::create_app(state);
        let (_s, j2) = req(app2, "GET", "/api/report/report_d/agent-log/stream").await;
        assert!(j2["data"].get("count").is_some(), "stream has count");
        assert!(j2["data"].get("total_lines").is_none(), "stream has NO total_lines");
    }

    // ---- console-log --------------------------------------------------------

    #[tokio::test]
    async fn console_log_missing_file_empty_shape() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_x/console-log").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["logs"].as_array().unwrap().len(), 0);
        assert_eq!(json["data"]["total_lines"], 0);
    }

    #[tokio::test]
    async fn console_log_with_lines_and_from_line() {
        let (state, _t) = test_state();
        seed_console_log(
            &state,
            "report_cl",
            &["[19:46:14] INFO: a", "[19:46:15] INFO: b", "[19:46:16] WARNING: c"],
        );
        let app = crate::server::create_app(state.clone());
        let (status, json) = req(app, "GET", "/api/report/report_cl/console-log").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["total_lines"], 3);
        assert_eq!(json["data"]["logs"].as_array().unwrap().len(), 3);

        let app2 = crate::server::create_app(state);
        let (_s, j2) = req(app2, "GET", "/api/report/report_cl/console-log?from_line=2").await;
        assert_eq!(j2["data"]["from_line"], 2);
        assert_eq!(j2["data"]["logs"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn console_log_stream_logs_and_count() {
        let (state, _t) = test_state();
        seed_console_log(&state, "report_cls", &["line1", "line2", "line3"]);
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_cls/console-log/stream").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["count"], 3);
        assert_eq!(json["data"]["logs"].as_array().unwrap().len(), 3);
    }

    // =======================================================================
    // Sub-cycle (c) — sections + download
    // =======================================================================

    fn seed_section(state: &Arc<ApiState>, report_id: &str, idx: usize, content: &str) {
        let folder = report_manager(state).ensure_report_folder(report_id).expect("folder");
        std::fs::write(folder.join(format!("section_{idx:02}.md")), content).unwrap();
    }

    // ---- download (Response, not Json) -------------------------------------

    #[tokio::test]
    async fn download_not_found_404() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_none/download").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn download_found_serves_markdown_attachment() {
        let (state, _t) = test_state();
        seed(&state, &make_report("report_dl", "sim1", ReportStatus::Completed));
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/report/report_dl/download")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap().to_str().unwrap().to_string();
        let cd = resp.headers().get("content-disposition").unwrap().to_str().unwrap().to_string();
        assert!(ct.starts_with("text/markdown"), "content-type: {ct}");
        assert_eq!(cd, "attachment; filename=\"report_dl.md\"");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        // markdown_content of make_report is "# T\n\nBody."
        assert_eq!(body, "# T\n\nBody.");
    }

    // ---- sections -----------------------------------------------------------

    #[tokio::test]
    async fn sections_empty_no_files() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_x/sections").await;
        assert_eq!(status, StatusCode::OK);
        let d = &json["data"];
        assert_eq!(d["report_id"], "report_x");
        assert_eq!(d["total_sections"], 0);
        assert!(d["sections"].as_array().unwrap().is_empty());
        assert_eq!(d["is_complete"], false, "no report → not complete");
    }

    #[tokio::test]
    async fn sections_with_files_and_is_complete() {
        let (state, _t) = test_state();
        seed(&state, &make_report("report_sec", "sim1", ReportStatus::Completed));
        seed_section(&state, "report_sec", 1, "## Sec1\n\nA");
        seed_section(&state, "report_sec", 2, "## Sec2\n\nB");
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_sec/sections").await;
        assert_eq!(status, StatusCode::OK);
        let d = &json["data"];
        assert_eq!(d["total_sections"], 2);
        let secs = d["sections"].as_array().unwrap();
        assert_eq!(secs.len(), 2);
        // sorted by filename → section_01 first
        assert_eq!(secs[0]["filename"], "section_01.md");
        assert_eq!(secs[0]["section_index"], 1);
        assert_eq!(secs[0]["content"], "## Sec1\n\nA");
        assert_eq!(d["is_complete"], true, "Completed report → is_complete");
    }

    #[tokio::test]
    async fn sections_generating_report_not_complete() {
        let (state, _t) = test_state();
        seed(&state, &make_report("report_gen", "sim1", ReportStatus::Generating));
        seed_section(&state, "report_gen", 1, "x");
        let app = crate::server::create_app(state);
        let (_s, json) = req(app, "GET", "/api/report/report_gen/sections").await;
        assert_eq!(json["data"]["total_sections"], 1);
        assert_eq!(json["data"]["is_complete"], false);
    }

    // ---- single section -----------------------------------------------------

    #[tokio::test]
    async fn single_section_found() {
        let (state, _t) = test_state();
        seed_section(&state, "report_ss", 1, "## Exec Summary\n\nBody");
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_ss/section/1").await;
        assert_eq!(status, StatusCode::OK);
        let d = &json["data"];
        assert_eq!(d["filename"], "section_01.md");
        assert_eq!(d["section_index"], 1);
        assert_eq!(d["content"], "## Exec Summary\n\nBody");
    }

    #[tokio::test]
    async fn single_section_not_found_404() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = req(app, "GET", "/api/report/report_x/section/5").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
        // sectionNotFound carries the 2-digit zero-padded index "05".
        assert!(json["error"].as_str().unwrap().contains("05"), "error: {}", json["error"]);
    }

    /// Flask `<int:section_index>` — a non-integer segment must 404 (axum usize parse).
    #[tokio::test]
    async fn single_section_non_integer_404() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/report/report_x/section/abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "non-integer section index must 404 (Flask <int:>)"
        );
    }

    // =======================================================================
    // Sub-cycle (d) — tools debug routes (POST, body graph_id)
    // =======================================================================

    /// POST a JSON body and decode the (status, json) response.
    async fn post_json(
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

    /// Seed a completed `graph_build` task whose result embeds a real KnowledgeGraph
    /// (2 entities Alice/Bob + 1 edge). The task_id IS the graph_id. Mirrors the
    /// simulation.rs entities-route test fixture.
    fn seed_graph_task() -> String {
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
            "graph_name":       "ReportToolsTestGraph",
            "graph_info":       {"node_count": 2, "edge_count": 1, "entity_types": []},
            "chunks_processed": 1,
            "graph":            graph_json
        });

        let tm = crate::task::TaskManager::global();
        let task_id = tm.create_task("graph_build", None);
        tm.complete_task(&task_id, result);
        task_id
    }

    /// Build an app whose `zep_api_key` is None so the inherited ZEP guard fires.
    fn test_app_no_zep() -> (axum::Router, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let mut config = crate::Config::build_test();
        config.upload_folder = tmp.path().to_string_lossy().to_string();
        config.zep_api_key = None;
        (crate::server::create_app(Arc::new(ApiState::new(config))), tmp)
    }

    // ---- /tools/search ------------------------------------------------------

    #[tokio::test]
    async fn tools_search_missing_graph_id_400() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) =
            post_json(app, "/api/report/tools/search", serde_json::json!({"query": "x"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn tools_search_missing_query_400() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) =
            post_json(app, "/api/report/tools/search", serde_json::json!({"graph_id": "g"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn tools_search_resolved_graph_200_shape() {
        let (state, _t) = test_state();
        let graph_id = seed_graph_task();
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/tools/search",
            serde_json::json!({"graph_id": graph_id, "query": "Alice", "limit": 5}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["success"], true);
        let d = &json["data"];
        // SearchResult::to_dict 5-key contract.
        assert!(d["facts"].is_array());
        assert!(d["edges"].is_array());
        assert!(d["nodes"].is_array());
        assert_eq!(d["query"], "Alice");
        assert!(d["total_count"].is_number());
    }

    /// `[≠] U026-ZEPKEY` inherited: empty zep_api_key → 500 zepApiKeyMissing (validation
    /// passes first since both graph_id+query are present, THEN the ZEP guard fires).
    #[tokio::test]
    async fn tools_search_zep_guard_500() {
        let (app, _t) = test_app_no_zep();
        let (status, json) = post_json(
            app,
            "/api/report/tools/search",
            serde_json::json!({"graph_id": "g", "query": "q"}),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(json["success"], false);
    }

    // ---- /tools/statistics --------------------------------------------------

    #[tokio::test]
    async fn tools_statistics_missing_graph_id_400() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) =
            post_json(app, "/api/report/tools/statistics", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn tools_statistics_resolved_graph_200_shape() {
        let (state, _t) = test_state();
        let graph_id = seed_graph_task();
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/tools/statistics",
            serde_json::json!({"graph_id": graph_id}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["success"], true);
        let d = &json["data"];
        // get_graph_statistics 5-key contract; graph_id echoed; 2 nodes / 1 edge.
        assert_eq!(d["graph_id"], graph_id);
        assert_eq!(d["total_nodes"], 2);
        assert_eq!(d["total_edges"], 1);
        assert!(d["entity_types"].is_object());
        assert!(d["relation_types"].is_object());
    }

    /// `[≠] U026-ZEPKEY` inherited: statistics ZEP guard → 500.
    #[tokio::test]
    async fn tools_statistics_zep_guard_500() {
        let (app, _t) = test_app_no_zep();
        let (status, json) =
            post_json(app, "/api/report/tools/statistics", serde_json::json!({"graph_id": "g"}))
                .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(json["success"], false);
    }
}
