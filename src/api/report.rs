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
    response::{
        Json, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use serde_json::Value;
use std::time::Duration;

use crate::api::simulation::load_entity_reader_graph;
use crate::api::{ApiError, ApiState};
use crate::graph::KnowledgeGraph;
use crate::llm::{ChatMessage, ProviderAdapter};
use crate::report::ReportStatus;
use crate::report::manager::ReportManager;
use crate::report::sink::{ReportEvent, ReportSink};
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
        // ── Sub-cycle (b'): live SSE log tails (teri UPGRADE beyond source; additive) ──
        // 3-seg /agent-log/sse vs /agent-log/stream: distinct static tails, no route conflict.
        // These are genuine `text/event-stream` tails (not one-shot) — see the SSE-SEAM note.
        .route("/:report_id/agent-log/sse", get(get_agent_log_sse_route))
        .route("/:report_id/console-log/sse", get(get_console_log_sse_route))
        // ── Sub-cycle (b''): live report-generation event feed (teri UPGRADE; additive) ──
        // 2-seg /:report_id/events — the progress+section push feed the sink seam reserved.
        .route("/:report_id/events", get(get_report_events_sse_route))
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
        // ── Sub-cycle (e): chat (POST, seg-0 static) ──
        .route("/chat", post(chat_route))
        // ── Sub-cycle (f): async generate keystone (POST, seg-0 statics) ──
        // /generate (1-seg) + /generate/status (2-seg) — distinct by full path.
        .route("/generate", post(generate_report_route))
        .route("/generate/status", post(generate_status_route))
        .with_state(state)
}

/// `ReportManager` rooted at `{upload_folder}/reports` (Python's `uploads/reports`).
fn report_manager(state: &ApiState) -> ReportManager {
    ReportManager::new(&state.config.upload_folder)
}

/// Workstream B (U4): build the embedding-search lens for the report ReACT tools from shared
/// state. `None` when the vector store is unavailable — the tools then use keyword search
/// (no-downgrade, keyless-safe).
fn graph_search_lens(state: &ApiState) -> Option<crate::services::zep_tools::GraphSearchLens> {
    state
        .graph_vectors
        .as_ref()
        .map(|store| crate::services::zep_tools::GraphSearchLens {
            embedder: state.embedder.clone(),
            store: store.clone(),
        })
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
// So all four `/stream` + non-`/stream` routes port as ordinary JSON handlers —
// PARITY PRESERVED, untouched.
//
// `[x] U027-SSE-SEAM-ACTIVE` (teri UPGRADE, additive — sub-cycle b'): the genuine
// live `text/event-stream` surface the source never had now lands as SIBLING routes
// `/{agent,console}-log/sse`. They reuse the SAME `from_line` tail seam (the manager's
// `total_lines` cursor) but, instead of one-shot, poll the on-disk log every
// `LOG_SSE_POLL` and push each NEW line as an SSE `log` event until the report reaches
// a terminal stage (`completed`/`failed` in `progress.json`, written live by the
// generation worker via `update_progress`), then emit a final `done` event and close.
// The parity JSON routes above are NOT modified (no-downgrade): a client choosing the
// one-shot contract keeps it; a client wanting live tailing opts into `/sse`.
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
// Sub-cycle (b') — live SSE log tails (teri UPGRADE, additive)
//
// `/{agent,console}-log/sse` stream the report's log file LIVE as `text/event-stream`,
// reusing the same `from_line` cursor the one-shot routes expose. Engineered to be a
// strict superset, never a downgrade: the parity JSON routes are untouched.
//
// Frame contract (per SSE event):
//   event: log    data: <one log record>   — a NEW line since the client's cursor.
//                                             agent-log → the JSON entry; console-log → the raw text line.
//   event: done   data: {"status","total_lines"} — report reached a terminal stage; stream closes.
//   event: error  data: {"error"}          — report id never materialized (closed after a short grace).
//
// Tailing: poll the on-disk log every `LOG_SSE_POLL`; advance the cursor by the manager's
// `total_lines` (PHYSICAL line count) so skipped/unparseable agent-log lines don't desync it.
// Terminal: `progress.json` `status` (written live by the generation worker via
// `update_progress`) hitting `completed`/`failed`; fall back to the persisted report meta.
// Bounds: `UNKNOWN_GRACE` polls of zero evidence → `error` + close (typo'd id can't hang);
// `MAX_SSE_POLLS` ceiling → `done{status:"timeout"}` + close (a wedged report can't stream forever).
// ===========================================================================

/// Poll cadence for the live log tail. 500ms mirrors the project's file-watch debounce
/// feel — responsive without busy-spinning the disk.
const LOG_SSE_POLL: Duration = Duration::from_millis(500);

/// Upper bound on tail polls before the stream self-closes with a `timeout` done-event.
/// 1200 × 500ms = 10 min — generous for a real report, but never an unbounded connection.
const MAX_SSE_POLLS: u32 = 1200;

/// Consecutive zero-evidence polls (no progress.json, no report meta, empty log) tolerated
/// before declaring the id unknown. 6 × 500ms = ~3s — absorbs the detached worker being a
/// hair behind the `/generate` response without hanging a genuinely bad id.
const UNKNOWN_GRACE: u32 = 6;

/// Which report log a live SSE tail is following.
#[derive(Clone, Copy)]
enum LogKind {
    Agent,
    Console,
}

impl LogKind {
    /// Read log records at/after `from_line`, returning the new-line SSE events and the
    /// PHYSICAL line count (the cursor to resume from — robust to skipped agent-log lines).
    fn read_events(
        self,
        mgr: &ReportManager,
        report_id: &str,
        from_line: usize,
    ) -> (Vec<Event>, usize) {
        match self {
            LogKind::Agent => {
                let m = mgr.get_agent_log(report_id, from_line);
                let total = m.get("total_lines").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let events = m
                    .get("logs")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|entry| {
                                // json_data only fails on non-serializable values; a parsed
                                // Value always serializes, but fall back to a string frame.
                                Event::default().event("log").json_data(entry).unwrap_or_else(
                                    |_| Event::default().event("log").data(entry.to_string()),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (events, total)
            }
            LogKind::Console => {
                let m = mgr.get_console_log(report_id, from_line);
                let total = m.get("total_lines").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let events = m
                    .get("logs")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|line| Event::default().event("log").data(line))
                            .collect()
                    })
                    .unwrap_or_default();
                (events, total)
            }
        }
    }
}

/// Live status of a report: the in-flight `progress.json` `status` (written by the
/// generation worker) preferred over the persisted report meta. `None` ⇒ no evidence yet.
fn live_report_status(mgr: &ReportManager, report_id: &str) -> Option<String> {
    if let Some(s) = mgr
        .get_progress(report_id)
        .and_then(|p| p.get("status").and_then(|v| v.as_str()).map(|s| s.to_string()))
    {
        return Some(s);
    }
    mgr.get_report(report_id).and_then(|r| {
        serde_json::to_value(&r.status)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    })
}

/// Build the SSE response for a live log tail. Shared by the agent + console routes.
fn log_sse_response(
    state: Arc<ApiState>,
    report_id: String,
    kind: LogKind,
    from_line: usize,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let stream = async_stream::stream! {
        let mgr = report_manager(&state);
        let mut cursor = from_line;
        let mut polls: u32 = 0;
        let mut unknown_polls: u32 = 0;

        loop {
            // Emit any lines that appeared since the cursor.
            let (events, total) = kind.read_events(&mgr, &report_id, cursor);
            for ev in events {
                yield Ok(ev);
            }
            cursor = cursor.max(total);

            let status = live_report_status(&mgr, &report_id);
            let exists = status.is_some() || total > 0;

            // Terminal stage → flush done-frame and close.
            if let Some(s) = status.as_deref().filter(|s| *s == "completed" || *s == "failed") {
                let done = Event::default()
                    .event("done")
                    .json_data(serde_json::json!({ "status": s, "total_lines": cursor }))
                    .unwrap_or_else(|_| Event::default().event("done").data(s));
                yield Ok(done);
                break;
            }

            // Unknown id (no evidence after a short grace) → error-frame and close.
            if exists {
                unknown_polls = 0;
            } else {
                unknown_polls += 1;
                if unknown_polls >= UNKNOWN_GRACE {
                    let err = Event::default()
                        .event("error")
                        .json_data(serde_json::json!({ "error": "reportNotFound" }))
                        .unwrap_or_else(|_| Event::default().event("error").data("reportNotFound"));
                    yield Ok(err);
                    break;
                }
            }

            // Safety ceiling so a wedged report cannot stream forever.
            polls += 1;
            if polls >= MAX_SSE_POLLS {
                let done = Event::default()
                    .event("done")
                    .json_data(serde_json::json!({ "status": "timeout", "total_lines": cursor }))
                    .unwrap_or_else(|_| Event::default().event("done").data("timeout"));
                yield Ok(done);
                break;
            }

            tokio::time::sleep(LOG_SSE_POLL).await;
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// GET /:report_id/agent-log/sse  — live `text/event-stream` tail of `agent_log.jsonl`.
///   ?from_line (default 0) → resume cursor for a reconnecting client.
async fn get_agent_log_sse_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let from_line = params.get("from_line").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    log_sse_response(state, report_id, LogKind::Agent, from_line)
}

/// GET /:report_id/console-log/sse  — live `text/event-stream` tail of `console_log.txt`.
///   ?from_line (default 0) → resume cursor for a reconnecting client.
async fn get_console_log_sse_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let from_line = params.get("from_line").and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    log_sse_response(state, report_id, LogKind::Console, from_line)
}

// ===========================================================================
// Sub-cycle (b'') — live report-generation event feed (teri UPGRADE, additive)
//
// `/:report_id/events` is the push feed `report/sink.rs` reserved as the `SseSink`
// seam (`[~] U027-SSE-SEAM` for report PROGRESS). Rather than wire an in-memory
// `broadcast<ReportEvent>` registry through the detached `!Send` generation worker
// (extra shared-state lifecycle + a race window + only observable mid-LLM-run), this
// tails the SAME artifacts the worker already writes LIVE to disk and that the
// frontend already polls: `progress.json` (each `update_progress` stage transition)
// and `section_NN.md` (each section the ReACT loop completes, written via `save_section`).
//
// This is the deliberate "engineered-enough" call: 500ms granularity is imperceptible
// for a human watching a report build, the feed delivers the seam's full value (live
// progress + each section's markdown the moment it lands), it needs ZERO ApiState
// surgery, and — decisively — it is verifiable end-to-end at the HTTP surface without a
// live LLM (write the artifacts, curl, watch). The in-memory `ReportEvent` broadcast
// stays reserved for a future sub-500ms-latency need; the disk is the source of truth.
//
// Frame contract (per SSE event):
//   event: progress  data: the progress.json map  — emitted on first sight + every change.
//   event: section   data: {filename, section_index, content}  — once per NEW section file.
//   event: done      data: {status, sections}  — terminal stage reached; stream closes.
//   event: error     data: {error}  — report id never materialized (closed after a grace).
//
// Terminal/bounds reuse the log-tail seam exactly (`live_report_status`, `UNKNOWN_GRACE`,
// `MAX_SSE_POLLS`, `LOG_SSE_POLL`) — one streaming discipline for both feeds.
// ===========================================================================

/// Build the SSE response for the live report-generation event feed (progress + sections).
fn report_events_response(
    state: Arc<ApiState>,
    report_id: String,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let stream = async_stream::stream! {
        let mgr = report_manager(&state);
        // Signature of the last-emitted progress (status, progress, message, current_section)
        // — EXCLUDES updated_at so a re-write with no semantic change doesn't spam the client.
        let mut last_progress_sig: Option<String> = None;
        let mut sections_emitted: usize = 0;
        let mut polls: u32 = 0;
        let mut unknown_polls: u32 = 0;

        loop {
            // 1) progress.json change → `progress` event.
            let progress = mgr.get_progress(&report_id);
            if let Some(p) = &progress {
                let sig = format!(
                    "{}|{}|{}|{}",
                    p.get("status").and_then(|v| v.as_str()).unwrap_or(""),
                    p.get("progress").map(|v| v.to_string()).unwrap_or_default(),
                    p.get("message").and_then(|v| v.as_str()).unwrap_or(""),
                    p.get("current_section").and_then(|v| v.as_str()).unwrap_or(""),
                );
                if last_progress_sig.as_deref() != Some(sig.as_str()) {
                    let ev = Event::default()
                        .event("progress")
                        .json_data(Value::Object(p.clone()))
                        .unwrap_or_else(|_| Event::default().event("progress").data(sig.clone()));
                    yield Ok(ev);
                    last_progress_sig = Some(sig);
                }
            }

            // 2) New section files → one `section` event each, in filename order.
            let sections = mgr.get_generated_sections(&report_id);
            while sections_emitted < sections.len() {
                let sec = &sections[sections_emitted];
                let ev = Event::default()
                    .event("section")
                    .json_data(Value::Object(sec.clone()))
                    .unwrap_or_else(|_| Event::default().event("section").data(""));
                yield Ok(ev);
                sections_emitted += 1;
            }

            // 3) Terminal stage → done-frame and close.
            let status = live_report_status(&mgr, &report_id);
            if let Some(s) = status.as_deref().filter(|s| *s == "completed" || *s == "failed") {
                let done = Event::default()
                    .event("done")
                    .json_data(serde_json::json!({ "status": s, "sections": sections_emitted }))
                    .unwrap_or_else(|_| Event::default().event("done").data(s));
                yield Ok(done);
                break;
            }

            // 4) Unknown id (no progress, no sections, no report meta) after a grace → error.
            let exists = progress.is_some() || !sections.is_empty() || status.is_some();
            if exists {
                unknown_polls = 0;
            } else {
                unknown_polls += 1;
                if unknown_polls >= UNKNOWN_GRACE {
                    let err = Event::default()
                        .event("error")
                        .json_data(serde_json::json!({ "error": "reportNotFound" }))
                        .unwrap_or_else(|_| Event::default().event("error").data("reportNotFound"));
                    yield Ok(err);
                    break;
                }
            }

            // 5) Safety ceiling so a wedged report cannot stream forever.
            polls += 1;
            if polls >= MAX_SSE_POLLS {
                let done = Event::default()
                    .event("done")
                    .json_data(serde_json::json!({ "status": "timeout", "sections": sections_emitted }))
                    .unwrap_or_else(|_| Event::default().event("done").data("timeout"));
                yield Ok(done);
                break;
            }

            tokio::time::sleep(LOG_SSE_POLL).await;
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// GET /:report_id/events  — live `text/event-stream` feed of report-generation progress
/// and each section's markdown the moment it is written. Closes on the terminal stage.
async fn get_report_events_sse_route(
    State(state): State<Arc<ApiState>>,
    Path(report_id): Path<String>,
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    report_events_response(state, report_id)
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
    // Workstream B (U4): embedding-cosine search with keyword fallback (no-downgrade). When no
    // vector store is available, this is identical to the keyword `search_graph`. scope omitted
    // in source → Python default "edges".
    let result = match &state.graph_vectors {
        Some(store) => {
            crate::services::graph_backend::semantic_search(
                &tools,
                &graph_id,
                &query,
                limit,
                Some("edges"),
                &state.embedder,
                store,
            )
            .await
        }
        None => tools.search_graph(&graph_id, &query, limit, Some("edges")),
    };

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

// ===========================================================================
// Sub-cycle (e) — chat route  (report.py:472-564)
//
// The heaviest non-async-background handler: a full resolution chain ending in a
// ReACT LLM conversation. `ReportAgent::chat` is a PLAIN `async fn` (NO RefCell held
// across an await — unlike `generate_report`), so it ports as an ordinary Send axum
// handler (no OS-thread needed). It borrows `&tools`/`&llm`/`&manager` across its LLM
// awaits, all shared refs over Sync types → the handler future stays Send.
//
// Resolution chain (report.py:518-551):
//   sim_manager.get_simulation(simulation_id) → None → 404 simulationNotFound{id}
//   ProjectManager.get_project(state.project_id) → None → 404 projectNotFound{id}
//   graph_id = state.graph_id or project.graph_id → empty → 400 missingGraphId
//   simulation_requirement = project.simulation_requirement or ""
//   ReportAgent::new_react(graph_id, simulation_id, requirement)
//   agent.chat(&tools, &llm, &manager, message, &history) → ChatResponse::to_dict
//
// Flags:
//   `[≠] U026-ZEPKEY` (inherited via load_entity_reader_graph): empty zep key → 500.
//   `[!] U027-GRAPHREQ` / `[!] U027-e-LLM-GATED`: the 200 success path drives the LLM
//      (ReACT loop) over a resolved graph — it runs end-to-end with a live LLM (chat
//      gracefully handles an empty report via the "（暂无报告）" placeholder, mod:2172).
//      The chat SUBSTRATE is already U-024 parity-verified with mock adapters; this
//      handler only WIRES the verified `ReportAgent::chat`. The tests below exercise the
//      ENTIRE pre-LLM contract surface (both 400s, both 404s, the graph_id fallback both
//      ways, and the ZEP-guard 500); the LLM round-trip itself is not unit-tested here
//      (no live/mock HTTP LLM in these route tests — same producer-gating convention as
//      the (g)/(k) success paths).
//   `[~] U027-e-CHATROLE-NARROW`: see `parse_chat_history`.
//   `[≠] U025-TRACEBACK`: 500 body carries a Rust backtrace string.
// ===========================================================================

/// Parse the optional `chat_history` JSON array into `Vec<ChatMessage>`.
///
/// Python appends each `{role, content}` dict to the LLM messages verbatim
/// (`report_agent.py:1808` `for h in chat_history[-10:]: messages.append(h)`); teri's
/// `ReportAgent::chat` does the SAME `[-10:]` windowing internally over typed
/// `ChatMessage`s, so the handler parses the FULL array here and lets `chat` window it.
///
/// `[~] U027-e-CHATROLE-NARROW`: Python passes arbitrary role strings straight to the
/// LLM API; teri's `ChatRole` is a closed enum {system,user,assistant}. We map
/// "system"/"assistant" to their roles and EVERYTHING ELSE (incl "user", absent,
/// unknown) to `user`. The frontend only ever sends user/assistant (the documented
/// contract, report.py:484-485), so the narrowing is non-contractual. A non-array
/// `chat_history` (never sent) → empty history (Python would iterate a string char-wise;
/// unreachable under the contract).
fn parse_chat_history(raw: Option<&Value>) -> Vec<ChatMessage> {
    let Some(Value::Array(arr)) = raw else {
        return Vec::new();
    };
    arr.iter()
        .map(|entry| {
            let role = entry.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let content = entry.get("content").and_then(|v| v.as_str()).unwrap_or("");
            match role {
                "system" => ChatMessage::system(content),
                "assistant" => ChatMessage::assistant(content),
                _ => ChatMessage::user(content),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// POST /chat  (report.py:472-564)
// ---------------------------------------------------------------------------
async fn chat_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = parse_tools_body(body);

    // Step 1-2: required fields (report.py:502-516) — Python falsy: absent/"" → 400.
    let simulation_id =
        data.get("simulation_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if simulation_id.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.requireSimulationId"),
        ));
    }
    let message = data.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if message.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.requireMessage"),
        ));
    }

    // Step 3: optional chat_history (report.py:504).
    let chat_history = parse_chat_history(data.get("chat_history"));

    // Step 4: resolve simulation (report.py:519-526).
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

    // Step 5: resolve project (report.py:528-533).
    let pm = crate::models::project::ProjectManager::from_config(&state.config);
    let project = pm.get_project(&sim_state.project_id).map_err(ApiError::server)?;
    let project = match project {
        None => {
            return Err(ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.projectNotFound", &[("id", &sim_state.project_id)]),
            ));
        }
        Some(p) => p,
    };

    // Step 6: graph_id = state.graph_id or project.graph_id (report.py:535-540).
    // Python `a or b`: state.graph_id (non-empty) wins, else project.graph_id, else 400.
    let graph_id = if !sim_state.graph_id.is_empty() {
        sim_state.graph_id.clone()
    } else {
        project.graph_id.clone().unwrap_or_default()
    };
    if graph_id.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.missingGraphId"),
        ));
    }

    // Python `project.simulation_requirement or ""` (report.py:542). None/Some("") → "".
    let simulation_requirement = project.simulation_requirement.clone().unwrap_or_default();

    // Step 7: build agent + tools + llm + manager, run the ReACT chat (report.py:545-551).
    // load_entity_reader_graph applies the ZEP guard (inherited [≠] U026-ZEPKEY).
    let graph = load_entity_reader_graph(&state, &graph_id).await?;
    let llm = crate::api::build_llm(&state.config);
    // U-024: thread the live SimulationRunner so `interview_agents` can reach the
    // batch-interview IPC seam (the SAME runner instance holds the live sims).
    let tools = ReportTools::with_runner(&graph, &llm, Some(&state.sim_runner))
        .with_search_lens(graph_search_lens(&state));
    let manager = report_manager(&state);
    let agent =
        crate::report::ReportAgent::new_react(&graph_id, &simulation_id, &simulation_requirement);

    let result = agent.chat(&tools, &llm, &manager, &message, &chat_history).await;

    Ok(Json(serde_json::json!({
        "success": true,
        "data": Value::Object(result.to_dict())
    })))
}

// ===========================================================================
// Sub-cycle (f) — async generate keystone  (report.py:25-272)
//
// The HARDEST sub-cycle: an async background report-generation task. Two routes:
//   POST /generate        — validate + resolve, create report_id + task eagerly,
//                           spawn the generation worker, return {report_id,task_id,
//                           status:"generating"} IMMEDIATELY (report.py:25-200).
//   POST /generate/status — poll the task (simulation_id completed short-circuit
//                           then task_id lookup) (report.py:203-272).
//
// ## Decision (i) — OS-thread + current-thread runtime (NOT tokio::spawn)
//
// `ReportAgent::generate_report` (mod:1635) is `!Send`: it wraps the `&mut dyn
// ReportSink` in a `RefCell` (mod:1672) and borrows it through `Fn` closures held
// LIVE across the section-generation `.await`s. `RefCell` is `!Sync` → a future
// holding that borrow across `.await` is `!Send` → it cannot be awaited inside a
// `tokio::spawn` worker (which requires `+ Send`). This is the SAME property that
// forced DECISION-U026-d-1-REVISED for `prepare_simulation`. So we reuse the
// `spawn_prepare_simulation` template VERBATIM (simulation_manager.rs:1836): a
// dedicated `std::thread::spawn` running a `current_thread` tokio runtime drives the
// `!Send` future on one thread → ZERO signature changes to the U-024-verified
// `generate_report` (blast radius 0). This is also the faithful port of Python's
// `threading.Thread(target=run_generate, daemon=True)` (report.py:179).
//
// The progress callback (Python `progress_callback(stage,progress,msg)` →
// `task_manager.update_task`, report.py:146-151) maps to `TaskUpdateSink` — a
// `ReportSink` that fans each `ReportEvent` to `TaskManager::update_task`.
//
// ## `[≠] U027-f-GRAPHRESOLVE-EAGER` (consistent with the prepare-route precedent)
// Python resolves the graph LAZILY inside the worker thread (ReportAgent builds
// ZepToolsService there). teri resolves it in the ROUTE via `load_entity_reader_graph`
// (ZEP guard + task→graph) and passes the owned `KnowledgeGraph` into the worker —
// EXACTLY as the U-026(d) prepare route passes an owned graph to
// `spawn_prepare_simulation`. So a graph-resolution failure (empty zep key / no
// graph_build task) returns a synchronous 500 here rather than Python's "generating"
// + background failed-task. The PRIMARY contract (valid graph → generating + bg
// generation) is faithful; on graph error the report does not generate either way
// (frontend observes it via the error vs a failed `/generate/status`). Same
// substrate-forced eager-resolution already accepted for prepare.
//
// `[!] U027-f-LLM-GATED`: the worker drives the LLM (full report pipeline); the 200
// "generating" response returns BEFORE the LLM runs, so the route is tested without a
// live LLM (the detached worker fails its task on a missing LLM — no test impact).
// `[≠] U026-ZEPKEY` / `[≠] U025-TRACEBACK` inherited. `/generate/status`'s outer 500
// has NO traceback in source (report.py:269-272) but teri's status handler has no
// reachable server-error path (get_report_by_simulation/get_task are infallible), so
// the divergence is unreachable.
// ===========================================================================

/// A `ReportSink` that forwards each progress event to `TaskManager::update_task`.
///
/// Port of Python's `progress_callback(stage, progress, message)` →
/// `task_manager.update_task(task_id, progress=progress, message=f"[{stage}] {message}")`
/// (report.py:146-151). The stage is rendered as its lowercase status string (matching
/// what Python's agent passes as `stage`), prefixed in brackets onto the message.
struct TaskUpdateSink {
    task_id: String,
}

impl ReportSink for TaskUpdateSink {
    fn event(&mut self, ev: &ReportEvent) {
        let message = format!("[{}] {}", ev.stage.to_status_str(), ev.message);
        crate::task::TaskManager::global().update_task(
            &self.task_id,
            None,
            Some(ev.progress as i64),
            Some(message),
            None,
            None,
            None,
        );
    }
}

/// Spawn the report-generation worker on a dedicated OS thread + current-thread tokio
/// runtime (Decision (i)). Mirrors `spawn_prepare_simulation` verbatim.
#[allow(clippy::too_many_arguments)]
fn spawn_report_generation(
    task_id: String,
    graph_id: String,
    simulation_id: String,
    simulation_requirement: String,
    report_id: String,
    llm: ProviderAdapter,
    graph: KnowledgeGraph,
    upload_folder: String,
    // U-024: owned `Arc` clone of the live runner, moved into the detached worker
    // thread (a borrow cannot cross the `'static` thread boundary).
    runner: Option<
        std::sync::Arc<crate::services::simulation_runner::SimulationRunner<ProviderAdapter>>,
    >,
    // Workstream B (U4): optional embedding-search lens, moved into the detached worker thread.
    search_lens: Option<crate::services::zep_tools::GraphSearchLens>,
) {
    // Capture locale before spawning (report.py:125), re-apply in the thread.
    let locale = crate::i18n::get_locale();
    let task_id_worker = task_id.clone();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("report worker runtime build failed: {e}");
                crate::task::TaskManager::global().fail_task(&task_id_worker, e.to_string());
                return;
            }
        };
        rt.block_on(crate::i18n::with_locale(locale, async move {
            report_generate_worker(
                task_id_worker,
                graph_id,
                simulation_id,
                simulation_requirement,
                report_id,
                llm,
                graph,
                upload_folder,
                runner,
                search_lens,
            )
            .await;
        }));
    });
}

/// Inner worker — port of `run_generate` (report.py:128-176).
///
/// `pub(crate)` so tests can drive it directly on a current-thread runtime (bypassing
/// the OS-thread spawn), mirroring `prepare_worker`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn report_generate_worker(
    task_id: String,
    graph_id: String,
    simulation_id: String,
    simulation_requirement: String,
    report_id: String,
    llm: ProviderAdapter,
    graph: KnowledgeGraph,
    upload_folder: String,
    // U-024: optional live runner for interview_agents IPC. `None` in tests that
    // drive the worker directly without an `ApiState` runner.
    runner: Option<
        std::sync::Arc<crate::services::simulation_runner::SimulationRunner<ProviderAdapter>>,
    >,
    // Workstream B (U4): optional embedding-search lens for the ReACT search tools.
    search_lens: Option<crate::services::zep_tools::GraphSearchLens>,
) {
    use crate::task::{TaskManager, TaskStatus};

    // Step 1: PROCESSING, progress 0, initReportAgent (report.py:131-136).
    TaskManager::global().update_task(
        &task_id,
        Some(TaskStatus::Processing),
        Some(0),
        Some(crate::i18n::t("api.initReportAgent")),
        None,
        None,
        None,
    );

    // Step 2-4: build agent + tools + manager + sink, generate (report.py:139-157).
    let mut agent =
        crate::report::ReportAgent::new_react(&graph_id, &simulation_id, &simulation_requirement);
    // U-024: bind the live runner (if any) so the ReACT loop's interview_agents
    // tool reaches the batch-interview IPC seam.
    let tools =
        ReportTools::with_runner(&graph, &llm, runner.as_deref()).with_search_lens(search_lens);
    let manager = ReportManager::new(&upload_folder);
    let mut sink = TaskUpdateSink { task_id: task_id.clone() };
    let report = agent
        .generate_report(&tools, &llm, &manager, &mut sink, Some(report_id.clone()))
        .await;

    // Step 5: save_report (report.py:160). An I/O failure here maps to Python's
    // outer `except → fail_task(str(e))`.
    if let Err(e) = manager.save_report(&report) {
        TaskManager::global().fail_task(&task_id, format!("save_report failed: {e}"));
        return;
    }

    // Step 6: terminal transition (report.py:162-172).
    if report.status == ReportStatus::Completed {
        TaskManager::global().complete_task(
            &task_id,
            serde_json::json!({
                "report_id": report.report_id,
                "simulation_id": simulation_id,
                "status": "completed"
            }),
        );
    } else {
        // Python: `report.error or t('api.reportGenerateFailed')`.
        let err = report
            .error
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| crate::i18n::t("api.reportGenerateFailed"));
        TaskManager::global().fail_task(&task_id, err);
    }
}

// ---------------------------------------------------------------------------
// POST /generate  (report.py:25-200)
// ---------------------------------------------------------------------------
async fn generate_report_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = parse_tools_body(body);

    // Step 1: simulation_id required (report.py:53-58).
    let simulation_id =
        data.get("simulation_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if simulation_id.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.requireSimulationId"),
        ));
    }
    // Step 2: force_regenerate default false (report.py:60).
    let force_regenerate = data.get("force_regenerate").and_then(|v| v.as_bool()).unwrap_or(false);

    // Step 3: resolve simulation (report.py:63-70).
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

    // Step 4: existing-completed-report short-circuit unless force_regenerate (report.py:73-85).
    if !force_regenerate
        && let Some(existing) = report_manager(&state).get_report_by_simulation(&simulation_id)
        && existing.status == ReportStatus::Completed
    {
        return Ok(Json(serde_json::json!({
            "success": true,
            "data": {
                "simulation_id": simulation_id,
                "report_id": existing.report_id,
                "status": "completed",
                "message": crate::i18n::t("api.reportAlreadyExists"),
                "already_generated": true
            }
        })));
    }

    // Step 5: resolve project (report.py:88-93).
    let pm = crate::models::project::ProjectManager::from_config(&state.config);
    let project = pm.get_project(&sim_state.project_id).map_err(ApiError::server)?;
    let project = match project {
        None => {
            return Err(ApiError::client(
                StatusCode::NOT_FOUND,
                crate::i18n::t_args("api.projectNotFound", &[("id", &sim_state.project_id)]),
            ));
        }
        Some(p) => p,
    };

    // Step 6: graph_id = state.graph_id or project.graph_id (report.py:95-100).
    // NOTE: the missing-graph i18n key here is `missingGraphIdEnsure` (NOT chat's
    // `missingGraphId`).
    let graph_id = if !sim_state.graph_id.is_empty() {
        sim_state.graph_id.clone()
    } else {
        project.graph_id.clone().unwrap_or_default()
    };
    if graph_id.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.missingGraphIdEnsure"),
        ));
    }

    // Step 7: simulation_requirement REQUIRED here (report.py:102-107) — unlike /chat.
    let simulation_requirement = project.simulation_requirement.clone().unwrap_or_default();
    if simulation_requirement.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.missingSimRequirement"),
        ));
    }

    // Step 8: eagerly mint report_id (report.py:110-111).
    let report_id = format!("report_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);

    // Step 9: resolve the graph + llm in the route (`[≠] U027-f-GRAPHRESOLVE-EAGER`,
    // consistent with spawn_prepare_simulation). ZEP guard fires here.
    let graph = load_entity_reader_graph(&state, &graph_id).await?;
    let llm = crate::api::build_llm(&state.config);

    // Step 10: create the async task eagerly (report.py:114-122).
    let mut metadata: HashMap<String, Value> = HashMap::new();
    metadata.insert("simulation_id".to_string(), Value::String(simulation_id.clone()));
    metadata.insert("graph_id".to_string(), Value::String(graph_id.clone()));
    metadata.insert("report_id".to_string(), Value::String(report_id.clone()));
    let task_id = crate::task::TaskManager::global().create_task("report_generate", Some(metadata));

    // Step 11: spawn the worker (report.py:179-180).
    spawn_report_generation(
        task_id.clone(),
        graph_id,
        simulation_id.clone(),
        simulation_requirement,
        report_id.clone(),
        llm,
        graph,
        state.config.upload_folder.clone(),
        // U-024: pass the live runner so the generate ReACT loop can interview agents.
        Some(state.sim_runner.clone()),
        // Workstream B (U4): pass the embedding-search lens so the ReACT search tools run cosine.
        graph_search_lens(&state),
    );

    // Step 12: immediate response (report.py:182-192).
    Ok(Json(serde_json::json!({
        "success": true,
        "data": {
            "simulation_id": simulation_id,
            "report_id": report_id,
            "task_id": task_id,
            "status": "generating",
            "message": crate::i18n::t("api.reportGenerateStarted"),
            "already_generated": false
        }
    })))
}

// ---------------------------------------------------------------------------
// POST /generate/status  (report.py:203-272)
//   simulation_id completed short-circuit → then task_id lookup.
//   No reachable 500 in teri (get_report_by_simulation/get_task are infallible),
//   so source's no-traceback outer-except (report.py:269-272) is moot.
// ---------------------------------------------------------------------------
async fn generate_status_route(
    State(state): State<Arc<ApiState>>,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, ApiError> {
    let data = parse_tools_body(body);
    let task_id = data.get("task_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let simulation_id =
        data.get("simulation_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // Step 1: simulation_id completed short-circuit (report.py:232-245).
    // Python `if simulation_id:` is truthy — empty string skips.
    if !simulation_id.is_empty()
        && let Some(existing) = report_manager(&state).get_report_by_simulation(&simulation_id)
        && existing.status == ReportStatus::Completed
    {
        return Ok(Json(serde_json::json!({
            "success": true,
            "data": {
                "simulation_id": simulation_id,
                "report_id": existing.report_id,
                "status": "completed",
                "progress": 100,
                "message": crate::i18n::t("api.reportGenerated"),
                "already_completed": true
            }
        })));
    }

    // Step 2: task_id required (report.py:247-251).
    if task_id.is_empty() {
        return Err(ApiError::client(
            StatusCode::BAD_REQUEST,
            crate::i18n::t("api.requireTaskOrSimId"),
        ));
    }

    // Step 3: task lookup (report.py:253-264).
    match crate::task::TaskManager::global().get_task(&task_id) {
        None => Err(ApiError::client(
            StatusCode::NOT_FOUND,
            crate::i18n::t_args("api.taskNotFound", &[("id", &task_id)]),
        )),
        Some(task) => Ok(Json(serde_json::json!({
            "success": true,
            "data": task.to_dict()
        }))),
    }
}

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

    // ---- live SSE log tails (sub-cycle b') ---------------------------------

    /// Drive an SSE route to completion and return (status, content-type, full body text).
    /// The tails self-terminate (terminal status or unknown-grace), so `to_bytes` collects
    /// the whole `text/event-stream` body.
    async fn sse_body(app: axum::Router, uri: &str) -> (StatusCode, String, String) {
        let resp = app
            .oneshot(Request::builder().method("GET").uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, ct, String::from_utf8(bytes.to_vec()).unwrap())
    }

    /// A terminal report's agent-log SSE streams every entry as a `log` event then a
    /// `done` event carrying the terminal status — and closes.
    #[tokio::test]
    async fn agent_log_sse_streams_entries_then_done() {
        let (state, _t) = test_state();
        seed_agent_log(
            &state,
            "report_sse_a",
            &[
                serde_json::json!({"action": "report_start"}),
                serde_json::json!({"action": "report_complete"}),
            ],
        );
        // progress.json status=completed → the live terminal signal the tail watches.
        report_manager(&state)
            .update_progress("report_sse_a", "completed", 100, "done", None, None)
            .unwrap();
        let app = crate::server::create_app(state);
        let (status, ct, body) = sse_body(app, "/api/report/report_sse_a/agent-log/sse").await;
        assert_eq!(status, StatusCode::OK);
        assert!(ct.starts_with("text/event-stream"), "content-type: {ct}");
        assert!(body.contains("event: log"), "log events expected; body:\n{body}");
        assert!(body.contains("report_start"), "first entry streamed; body:\n{body}");
        assert!(body.contains("report_complete"), "second entry streamed; body:\n{body}");
        assert!(body.contains("event: done"), "done frame expected; body:\n{body}");
        assert!(body.contains("\"status\":\"completed\""), "done carries status; body:\n{body}");
    }

    /// A failed report's console-log SSE streams raw lines then a `done{status:"failed"}`.
    #[tokio::test]
    async fn console_log_sse_streams_lines_then_done() {
        let (state, _t) = test_state();
        seed_console_log(&state, "report_sse_c", &["[t] INFO: alpha", "[t] INFO: beta"]);
        report_manager(&state)
            .update_progress("report_sse_c", "failed", -1, "boom", None, None)
            .unwrap();
        let app = crate::server::create_app(state);
        let (status, ct, body) = sse_body(app, "/api/report/report_sse_c/console-log/sse").await;
        assert_eq!(status, StatusCode::OK);
        assert!(ct.starts_with("text/event-stream"), "content-type: {ct}");
        assert!(body.contains("data: [t] INFO: alpha"), "line 1 streamed; body:\n{body}");
        assert!(body.contains("data: [t] INFO: beta"), "line 2 streamed; body:\n{body}");
        assert!(body.contains("event: done"));
        assert!(body.contains("\"status\":\"failed\""), "done carries failed; body:\n{body}");
    }

    /// `?from_line=N` resumes the cursor: lines before N are NOT re-streamed.
    #[tokio::test]
    async fn console_log_sse_from_line_resumes_cursor() {
        let (state, _t) = test_state();
        seed_console_log(&state, "report_sse_r", &["one", "two", "three"]);
        report_manager(&state)
            .update_progress("report_sse_r", "completed", 100, "done", None, None)
            .unwrap();
        let app = crate::server::create_app(state);
        let (status, _ct, body) =
            sse_body(app, "/api/report/report_sse_r/console-log/sse?from_line=2").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.contains("data: one"), "pre-cursor line must not stream; body:\n{body}");
        assert!(!body.contains("data: two"), "pre-cursor line must not stream; body:\n{body}");
        assert!(body.contains("data: three"), "cursor line streamed; body:\n{body}");
        assert!(body.contains("event: done"));
    }

    /// Unparseable agent-log lines are skipped, but the cursor advances by the PHYSICAL
    /// line count — so `done.total_lines` is 3 even though only 2 entries parsed. This is
    /// the invariant that keeps a reconnecting `?from_line` cursor aligned with the file.
    #[tokio::test]
    async fn agent_log_sse_skips_unparseable_keeps_cursor_aligned() {
        let (state, _t) = test_state();
        let folder = report_manager(&state).ensure_report_folder("report_sse_skip").unwrap();
        std::fs::write(
            folder.join("agent_log.jsonl"),
            "{\"action\":\"first\"}\nNOT JSON HERE\n{\"action\":\"third\"}\n",
        )
        .unwrap();
        report_manager(&state)
            .update_progress("report_sse_skip", "completed", 100, "done", None, None)
            .unwrap();
        let app = crate::server::create_app(state);
        let (status, _ct, body) = sse_body(app, "/api/report/report_sse_skip/agent-log/sse").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("first"), "first entry streamed; body:\n{body}");
        assert!(body.contains("third"), "third entry streamed; body:\n{body}");
        assert!(
            body.contains("\"total_lines\":3"),
            "cursor must advance by physical line count (3), not parsed count (2); body:\n{body}"
        );
    }

    /// An unknown report id can't hang the connection: after the zero-evidence grace the
    /// tail emits an `error` frame and closes.
    #[tokio::test]
    async fn agent_log_sse_unknown_id_closes_with_error() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, _ct, body) = sse_body(app, "/api/report/report_ghost/agent-log/sse").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("event: error"), "error frame expected; body:\n{body}");
        assert!(body.contains("reportNotFound"), "error names the cause; body:\n{body}");
    }

    /// ROUTE-ORDER: /agent-log/sse (3-seg) vs /agent-log/stream (3-seg) vs /agent-log
    /// (2-seg) all resolve to distinct handlers — sse is genuine event-stream, stream is JSON.
    #[tokio::test]
    async fn agent_log_sse_vs_stream_distinct_surfaces() {
        let (state, _t) = test_state();
        seed_agent_log(&state, "report_sse_d", &[serde_json::json!({"action": "x"})]);
        report_manager(&state)
            .update_progress("report_sse_d", "completed", 100, "done", None, None)
            .unwrap();
        // /stream → JSON {logs,count}
        let app = crate::server::create_app(state.clone());
        let (_s, j) = req(app, "GET", "/api/report/report_sse_d/agent-log/stream").await;
        assert!(j["data"].get("count").is_some(), "stream stays JSON");
        // /sse → text/event-stream
        let app2 = crate::server::create_app(state);
        let (status, ct, _body) = sse_body(app2, "/api/report/report_sse_d/agent-log/sse").await;
        assert_eq!(status, StatusCode::OK);
        assert!(ct.starts_with("text/event-stream"), "sse is event-stream, not JSON: {ct}");
    }

    // ---- live report-generation event feed (sub-cycle b'') -----------------

    /// A terminal report's /events feed emits a `progress` frame, one `section` frame per
    /// generated section file (in order), then a `done` carrying the terminal status + count.
    #[tokio::test]
    async fn report_events_streams_progress_and_sections_then_done() {
        let (state, _t) = test_state();
        report_manager(&state)
            .update_progress("report_ev", "completed", 100, "all done", None, None)
            .unwrap();
        seed_section(&state, "report_ev", 0, "## Overview\n\nFirst.");
        seed_section(&state, "report_ev", 1, "## Outcome\n\nSecond.");
        let app = crate::server::create_app(state);
        let (status, ct, body) = sse_body(app, "/api/report/report_ev/events").await;
        assert_eq!(status, StatusCode::OK);
        assert!(ct.starts_with("text/event-stream"), "content-type: {ct}");
        assert!(body.contains("event: progress"), "progress frame expected; body:\n{body}");
        assert!(body.contains("all done"), "progress message streamed; body:\n{body}");
        assert!(body.contains("event: section"), "section frames expected; body:\n{body}");
        assert!(body.contains("First."), "section 0 content; body:\n{body}");
        assert!(body.contains("Second."), "section 1 content; body:\n{body}");
        assert!(body.contains("event: done"));
        assert!(body.contains("\"status\":\"completed\""), "done carries status; body:\n{body}");
        assert!(body.contains("\"sections\":2"), "done carries section count; body:\n{body}");
        // section frames carry the structured payload (filename/index), in order.
        let s0 = body.find("section_00.md").expect("section_00 frame");
        let s1 = body.find("section_01.md").expect("section_01 frame");
        assert!(s0 < s1, "sections must stream in filename order");
    }

    /// Unknown report id can't hang: after the zero-evidence grace, `error` + close.
    #[tokio::test]
    async fn report_events_unknown_id_closes_with_error() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, _ct, body) = sse_body(app, "/api/report/report_ev_ghost/events").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("event: error"), "error frame expected; body:\n{body}");
        assert!(body.contains("reportNotFound"));
    }

    /// /events (SSE) and /progress (one-shot JSON) are distinct surfaces — no downgrade of
    /// the existing poll route.
    #[tokio::test]
    async fn report_events_vs_progress_route_distinct() {
        let (state, _t) = test_state();
        report_manager(&state)
            .update_progress("report_ev_d", "completed", 100, "done", None, None)
            .unwrap();
        // /progress → one-shot JSON
        let app = crate::server::create_app(state.clone());
        let (s, j) = req(app, "GET", "/api/report/report_ev_d/progress").await;
        assert_eq!(s, StatusCode::OK);
        assert!(j.get("data").is_some() || j.get("success").is_some(), "progress stays JSON");
        // /events → text/event-stream
        let app2 = crate::server::create_app(state);
        let (status, ct, _body) = sse_body(app2, "/api/report/report_ev_d/events").await;
        assert_eq!(status, StatusCode::OK);
        assert!(ct.starts_with("text/event-stream"), "events is event-stream: {ct}");
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

    /// Build an app that selects the Zep backend with no key, so the inherited ZEP guard fires.
    ///
    /// Workstream B migration: the shared `load_entity_reader_graph` guard is backend-gated, so
    /// the report routes only hit it under the Zep backend; under Native they are keyless.
    fn test_app_no_zep() -> (axum::Router, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let mut config = crate::Config::build_test();
        config.upload_folder = tmp.path().to_string_lossy().to_string();
        config.graph_backend = crate::GraphBackendKind::Zep;
        config.graph_backend_raw = "zep".to_string();
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
    async fn tools_search_zep_guard_500_under_zep_backend() {
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

    /// Workstream B: under Native, /tools/search with a real graph returns 200 with no zep key
    /// (cosine-ranked SearchResult shape preserved). Uses a seeded graph so resolution succeeds.
    #[tokio::test]
    async fn tools_search_keyless_native_200_shape() {
        let _t = tempfile::TempDir::new().expect("temp dir");
        let mut config = crate::Config::build_test();
        config.upload_folder = _t.path().to_string_lossy().to_string();
        config.graph_backend = crate::GraphBackendKind::Native;
        config.graph_backend_raw = "native".to_string();
        config.zep_api_key = None; // truly keyless
        let state = Arc::new(ApiState::new(config));
        let graph_id = seed_graph_task();
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/tools/search",
            serde_json::json!({"graph_id": graph_id, "query": "Alice"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "keyless Native search must 200: {json}");
        assert_eq!(json["success"], true);
        let d = &json["data"];
        // SearchResult shape preserved (facts/edges/nodes/query/total_count keys present).
        for key in ["facts", "edges", "nodes", "query", "total_count"] {
            assert!(d.get(key).is_some(), "SearchResult must keep key {key}: {json}");
        }
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
    async fn tools_statistics_zep_guard_500_under_zep_backend() {
        let (app, _t) = test_app_no_zep();
        let (status, json) =
            post_json(app, "/api/report/tools/statistics", serde_json::json!({"graph_id": "g"}))
                .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(json["success"], false);
    }

    // =======================================================================
    // Sub-cycle (e) — chat route (POST /chat)
    //
    // The 200 success path drives a live LLM (ReACT loop) and is not unit-tested
    // here (`[!] U027-e-LLM-GATED`); these tests cover the ENTIRE pre-LLM contract:
    // both 400s, both 404s, the graph_id fallback (state vs project), and the ZEP
    // guard 500 (which fires only AFTER sim+project+graph_id all resolve, so it
    // doubles as proof the full resolution chain succeeded).
    // =======================================================================

    /// `parse_chat_history` role mapping — direct unit test (no LLM).
    #[test]
    fn chat_history_role_narrowing() {
        let raw = serde_json::json!([
            {"role": "user", "content": "u"},
            {"role": "assistant", "content": "a"},
            {"role": "system", "content": "s"},
            {"role": "weird", "content": "w"},   // unknown → user
            {"content": "no-role"}               // absent → user
        ]);
        let msgs = parse_chat_history(Some(&raw));
        assert_eq!(msgs.len(), 5);
        use crate::llm::ChatRole;
        assert!(matches!(msgs[0].role, ChatRole::User));
        assert!(matches!(msgs[1].role, ChatRole::Assistant));
        assert!(matches!(msgs[2].role, ChatRole::System));
        assert!(matches!(msgs[3].role, ChatRole::User), "unknown role → user");
        assert!(matches!(msgs[4].role, ChatRole::User), "absent role → user");
        assert_eq!(msgs[3].content, "w");
    }

    #[test]
    fn chat_history_absent_or_non_array_is_empty() {
        assert!(parse_chat_history(None).is_empty());
        assert!(parse_chat_history(Some(&serde_json::json!("not-an-array"))).is_empty());
    }

    #[tokio::test]
    async fn chat_missing_simulation_id_400() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) =
            post_json(app, "/api/report/chat", serde_json::json!({"message": "hi"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn chat_missing_message_400() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) =
            post_json(app, "/api/report/chat", serde_json::json!({"simulation_id": "sim_x"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn chat_simulation_not_found_404() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/chat",
            serde_json::json!({"simulation_id": "sim_ghost", "message": "hi"}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn chat_project_not_found_404() {
        let (state, _t) = test_state();
        // Sim references a project that does not exist → projectNotFound.
        let sim = state
            .sim_manager
            .create_simulation("ghost_project", "g1", true, true)
            .expect("create sim");
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/chat",
            serde_json::json!({"simulation_id": sim.simulation_id, "message": "hi"}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn chat_missing_graph_id_400() {
        let (state, _t) = test_state();
        // Project WITHOUT a graph_id; sim WITHOUT a graph_id → 400 missingGraphId.
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("NoGraph").expect("project");
        p.graph_id = None;
        pm.save_project(&mut p).expect("save");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "", true, true)
            .expect("create sim");
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/chat",
            serde_json::json!({"simulation_id": sim.simulation_id, "message": "hi"}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    /// graph_id fallback via state.graph_id → resolution succeeds → ZEP guard 500
    /// (proves the full sim+project+graph_id chain resolved before the graph load).
    #[tokio::test]
    async fn chat_resolves_via_state_graph_id_then_zep_guard_500() {
        let tmp = tempfile::TempDir::new().expect("temp");
        let mut config = crate::Config::build_test();
        config.upload_folder = tmp.path().to_string_lossy().to_string();
        config.oasis_simulation_data_dir = tmp.path().join("sims").to_string_lossy().to_string();
        // Workstream B: select the Zep backend so the guard still fires after resolution.
        config.graph_backend = crate::GraphBackendKind::Zep;
        config.graph_backend_raw = "zep".to_string();
        config.zep_api_key = None; // ZEP guard fires after resolution
        let state = Arc::new(ApiState::new(config));

        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("WithGraph").expect("project");
        p.graph_id = Some("proj_graph".to_string());
        pm.save_project(&mut p).expect("save");
        // sim carries its OWN graph_id "state_graph" → takes precedence over project's.
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "state_graph", true, true)
            .expect("create sim");

        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/chat",
            serde_json::json!({"simulation_id": sim.simulation_id, "message": "hi"}),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "ZEP guard fires post-resolution");
        assert_eq!(json["success"], false);
    }

    /// graph_id fallback via PROJECT graph_id (sim has none) → resolution succeeds →
    /// ZEP guard 500. Proves the `state.graph_id or project.graph_id` fallback branch.
    #[tokio::test]
    async fn chat_resolves_via_project_graph_id_then_zep_guard_500() {
        let tmp = tempfile::TempDir::new().expect("temp");
        let mut config = crate::Config::build_test();
        config.upload_folder = tmp.path().to_string_lossy().to_string();
        config.oasis_simulation_data_dir = tmp.path().join("sims").to_string_lossy().to_string();
        // Workstream B: select the Zep backend so the guard still fires after resolution.
        config.graph_backend = crate::GraphBackendKind::Zep;
        config.graph_backend_raw = "zep".to_string();
        config.zep_api_key = None;
        let state = Arc::new(ApiState::new(config));

        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("WithGraph").expect("project");
        p.graph_id = Some("proj_graph".to_string());
        pm.save_project(&mut p).expect("save");
        // sim graph_id EMPTY → falls back to project.graph_id.
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "", true, true)
            .expect("create sim");

        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/chat",
            serde_json::json!({"simulation_id": sim.simulation_id, "message": "hi"}),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "fallback resolves, then ZEP 500");
        assert_eq!(json["success"], false);
    }

    // =======================================================================
    // Sub-cycle (f) — async generate keystone (POST /generate, /generate/status)
    //
    // The 200 "generating" response returns BEFORE the detached worker drives the
    // LLM (`[!] U027-f-LLM-GATED`), so the route is testable without a live LLM. All
    // pre-spawn validation/resolution paths + both short-circuits + the status-route
    // contract are covered. (The worker itself is the U-024-verified generate_report
    // wired through TaskUpdateSink; its LLM round-trip is producer-gated.)
    // =======================================================================

    /// Seed a project (graph_id = a real graph_build task_id, requirement set) + a
    /// simulation referencing it, so /generate resolves all the way to the spawn.
    /// Returns the simulation_id.
    fn seed_generate_ready(state: &Arc<ApiState>) -> String {
        let graph_task_id = seed_graph_task();
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("GenProj").expect("project");
        p.graph_id = Some(graph_task_id);
        p.simulation_requirement = Some("Analyze the public sentiment.".to_string());
        pm.save_project(&mut p).expect("save");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "", true, true)
            .expect("create sim");
        sim.simulation_id
    }

    // ---- /generate ----------------------------------------------------------

    #[tokio::test]
    async fn generate_missing_simulation_id_400() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = post_json(app, "/api/report/generate", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn generate_simulation_not_found_404() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/generate",
            serde_json::json!({"simulation_id": "sim_ghost"}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
    }

    /// force_regenerate=false + an existing COMPLETED report → 200 already_generated.
    #[tokio::test]
    async fn generate_already_completed_short_circuit() {
        let (state, _t) = test_state();
        let sim = state
            .sim_manager
            .create_simulation("proj_x", "g1", true, true)
            .expect("create sim");
        seed(&state, &make_report("report_done", &sim.simulation_id, ReportStatus::Completed));
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/generate",
            serde_json::json!({"simulation_id": sim.simulation_id}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let d = &json["data"];
        assert_eq!(d["already_generated"], true);
        assert_eq!(d["status"], "completed");
        assert_eq!(d["report_id"], "report_done");
    }

    #[tokio::test]
    async fn generate_project_not_found_404() {
        let (state, _t) = test_state();
        let sim = state
            .sim_manager
            .create_simulation("ghost_project", "g1", true, true)
            .expect("create sim");
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/generate",
            serde_json::json!({"simulation_id": sim.simulation_id}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn generate_missing_graph_id_400() {
        let (state, _t) = test_state();
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("NoGraph").expect("project");
        p.graph_id = None;
        p.simulation_requirement = Some("req".to_string());
        pm.save_project(&mut p).expect("save");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "", true, true)
            .expect("create sim");
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/generate",
            serde_json::json!({"simulation_id": sim.simulation_id}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    /// graph_id present but simulation_requirement empty → 400 missingSimRequirement.
    #[tokio::test]
    async fn generate_missing_requirement_400() {
        let (state, _t) = test_state();
        let pm = crate::models::project::ProjectManager::from_config(&state.config);
        let mut p = pm.create_project("NoReq").expect("project");
        p.graph_id = Some("g1".to_string());
        p.simulation_requirement = None;
        pm.save_project(&mut p).expect("save");
        let sim = state
            .sim_manager
            .create_simulation(&p.project_id, "g1", true, true)
            .expect("create sim");
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/generate",
            serde_json::json!({"simulation_id": sim.simulation_id}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    /// Full happy path → 200 "generating" with report_id + task_id, already_generated:false.
    /// (The detached worker drives the LLM; we assert only the immediate response.)
    #[tokio::test]
    async fn generate_happy_returns_generating() {
        let (state, _t) = test_state();
        let simulation_id = seed_generate_ready(&state);
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/generate",
            serde_json::json!({"simulation_id": simulation_id}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "happy generate must 200; got {json}");
        let d = &json["data"];
        assert_eq!(d["status"], "generating");
        assert_eq!(d["already_generated"], false);
        assert_eq!(d["simulation_id"], simulation_id);
        assert!(d["report_id"].as_str().unwrap().starts_with("report_"), "report_id: {d}");
        assert!(!d["task_id"].as_str().unwrap().is_empty(), "task_id present");
    }

    // ---- /generate/status ---------------------------------------------------

    #[tokio::test]
    async fn status_require_task_or_sim_400() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) =
            post_json(app, "/api/report/generate/status", serde_json::json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn status_task_not_found_404() {
        let (state, _t) = test_state();
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/generate/status",
            serde_json::json!({"task_id": "task_ghost"}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(json["success"], false);
    }

    /// simulation_id with a COMPLETED report → 200 already_completed:true, progress 100.
    #[tokio::test]
    async fn status_simulation_completed_short_circuit() {
        let (state, _t) = test_state();
        seed(&state, &make_report("report_sc", "sim_sc", ReportStatus::Completed));
        let app = crate::server::create_app(state);
        let (status, json) = post_json(
            app,
            "/api/report/generate/status",
            serde_json::json!({"simulation_id": "sim_sc"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let d = &json["data"];
        assert_eq!(d["already_completed"], true);
        assert_eq!(d["status"], "completed");
        assert_eq!(d["progress"], 100);
        assert_eq!(d["report_id"], "report_sc");
    }

    /// A real task → 200 with task.to_dict (task_id echoed, status present).
    #[tokio::test]
    async fn status_task_found_returns_to_dict() {
        let (state, _t) = test_state();
        let task_id = crate::task::TaskManager::global().create_task("report_generate", None);
        let app = crate::server::create_app(state);
        let (status, json) =
            post_json(app, "/api/report/generate/status", serde_json::json!({"task_id": task_id}))
                .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["data"]["task_id"], task_id);
        assert!(json["data"]["status"].is_string());
    }

    /// `report_generate_worker` drives the task to a TERMINAL state. We run the worker
    /// directly (current-thread runtime via #[tokio::test]) over an empty graph with an
    /// unreachable LLM; `generate_report` has graceful LLM-failure fallbacks and still
    /// produces a report, so the worker reaches `complete_task` (or `fail_task` on a
    /// save error) — either way the task moves OFF pending/processing. This exercises
    /// the worker's PROCESSING→terminal transition + save_report + result/error wiring.
    #[tokio::test]
    async fn worker_drives_task_to_terminal_state() {
        let (state, _t) = test_state();
        let task_id = crate::task::TaskManager::global().create_task("report_generate", None);
        let llm = crate::api::build_llm(&state.config);
        let graph = crate::graph::KnowledgeGraph::new(); // empty graph
        report_generate_worker(
            task_id.clone(),
            "g1".to_string(),
            "sim_w".to_string(),
            "req".to_string(),
            format!("report_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
            llm,
            graph,
            state.config.upload_folder.clone(),
            None,
            None, // Workstream B: no search lens in this worker unit test
        )
        .await;
        let task = crate::task::TaskManager::global().get_task(&task_id).expect("task");
        let v = task.to_dict();
        let st = v["status"].as_str().unwrap();
        assert!(
            st == "completed" || st == "failed",
            "worker must reach a terminal state (not stuck pending/processing), got {v}"
        );
        // On completion the result carries the report_id + simulation_id (report.py:165-169).
        if st == "completed" {
            assert_eq!(v["result"]["simulation_id"], "sim_w");
            assert!(v["result"]["report_id"].as_str().unwrap().starts_with("report_"));
        }
    }
}
