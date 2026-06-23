//! S1 (TASK-UI-1) — Web UI ↔ engine API contract smoke.
//!
//! The Vue SPA (`frontend/`) talks to `teri serve` through a thin axios layer
//! (`frontend/src/api/index.js`) that:
//!   * sets `Accept-Language: <locale>` on every request,
//!   * unwraps a `{success, data, error}` envelope, rejecting when `success === false`,
//!   * reaches the engine cross-origin in some deploys (and via the Vite dev proxy otherwise).
//!
//! A static `npm run build` cannot catch envelope/CORS/locale mismatches against the *running*
//! engine. This test boots the **real** `create_app` router (the exact one `teri serve` mounts)
//! and asserts the UI-facing contract for a representative LLM-free endpoint from each of the five
//! wizard steps, plus CORS and Accept-Language — turning the one-time manual smoke into a
//! regression gate.
//!
//! Endpoints are chosen to be LLM-free (list/health/unknown-id) so the contract is exercised
//! without a live inference backend.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt; // for `oneshot`

/// Build the real serve router rooted under a temp dir (no global state written).
fn test_app() -> (axum::Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let upload = root.join("uploads");
    let sim_data = upload.join("simulations");
    let mem_db = root.join("data").join("memory");
    let graph_db = root.join("data").join("graph");
    for d in [&upload, &sim_data, &mem_db, &graph_db] {
        std::fs::create_dir_all(d).unwrap();
    }
    // SAFETY: set within a single-threaded test before the app reads config.
    unsafe {
        std::env::set_var("LLM_API_KEY", "test-key");
        std::env::set_var("UPLOAD_FOLDER", upload.to_str().unwrap());
        std::env::set_var("OASIS_SIMULATION_DATA_DIR", sim_data.to_str().unwrap());
        std::env::set_var("MEMORY_DB_PATH", mem_db.join("mem.redb").to_str().unwrap());
        std::env::set_var("GRAPH_DB_PATH", graph_db.to_str().unwrap());
    }
    let config = teri::Config::load().expect("config load");
    let state = Arc::new(teri::api::ApiState::new(config));
    (teri::server::create_app(state), tmp)
}

async fn get(app: &axum::Router, uri: &str, accept_language: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::builder().method("GET").uri(uri);
    if let Some(lang) = accept_language {
        req = req.header("Accept-Language", lang);
    }
    let resp = app.clone().oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        panic!("response from {uri} was not JSON: {:?}", String::from_utf8_lossy(&bytes))
    });
    (status, json)
}

// --- /health (outside /api, no envelope, no CORS) ---------------------------

#[tokio::test]
async fn health_is_ok_and_teri_branded() {
    let (app, _tmp) = test_app();
    let (status, json) = get(&app, "/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
    // Branding: the health surface must self-identify as teri, never the upstream name.
    assert_eq!(json["service"], "teri", "health must self-identify as teri");
}

// --- Success envelope on each wizard step's list endpoint -------------------

/// Each of these is an LLM-free GET the UI hits to render a step; all must return the
/// `{success:true, data:...}` envelope the axios layer unwraps.
#[tokio::test]
async fn wizard_list_endpoints_return_success_envelope() {
    let (app, _tmp) = test_app();
    // Step 1 (Graph Build): project list.  Step 2 (Env Setup) / Step 3 (Simulation):
    // simulation list.  Step 4 (Report) / Step 5 (Interaction): report list.
    for uri in ["/api/graph/project/list", "/api/simulation/list", "/api/report/list"] {
        let (status, json) = get(&app, uri, None).await;
        assert_eq!(status, StatusCode::OK, "{uri} must be 200");
        assert_eq!(json["success"], true, "{uri} must return success:true envelope, got {json}");
        assert!(
            !json["data"].is_null(),
            "{uri} success envelope must carry a `data` field the UI unwraps, got {json}"
        );
    }
}

// --- Error envelope: unknown id must be {success:false, error} --------------

/// The axios layer rejects on `success === false` and surfaces `error`. An error path that
/// omits that envelope leaves the UI unable to show the failure — exactly the kind of mismatch
/// the static build can't catch.
#[tokio::test]
async fn unknown_id_returns_error_envelope() {
    let (app, _tmp) = test_app();
    let (status, json) = get(&app, "/api/graph/data/does-not-exist-00000000", None).await;
    assert!(
        status.is_client_error() || status.is_server_error(),
        "unknown graph id must be a non-2xx, got {status}"
    );
    assert_eq!(json["success"], false, "error path must carry success:false, got {json}");
    assert!(
        json["error"].is_string(),
        "error path must carry a string `error` the UI displays, got {json}"
    );
}

// --- CORS: /api/* is cross-origin-reachable, /health is not -----------------

#[tokio::test]
async fn api_routes_carry_cors_headers() {
    let (app, _tmp) = test_app();
    // A CORS preflight (OPTIONS + Origin + Access-Control-Request-Method) on an /api route must
    // be answered with an allow-origin header so a cross-origin SPA can call the engine.
    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/graph/project/list")
        .header("Origin", "http://localhost:3000")
        .header("Access-Control-Request-Method", "GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert!(
        resp.headers().contains_key("access-control-allow-origin"),
        "/api/* must answer CORS preflight with access-control-allow-origin"
    );
}

#[tokio::test]
async fn health_has_no_cors_header() {
    let (app, _tmp) = test_app();
    // CORS is scoped to /api/* only (faithful to MiroFish): /health must NOT carry it.
    let req = Request::builder()
        .method("OPTIONS")
        .uri("/health")
        .header("Origin", "http://localhost:3000")
        .header("Access-Control-Request-Method", "GET")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert!(
        !resp.headers().contains_key("access-control-allow-origin"),
        "/health is outside /api/* and must not carry CORS headers"
    );
}

// --- Accept-Language: both locales accepted, request not broken -------------

#[tokio::test]
async fn accept_language_header_is_honored() {
    let (app, _tmp) = test_app();
    // The axios interceptor always sends Accept-Language. The locale middleware must accept both
    // supported locales without breaking the request (en/zh are the supported set).
    for lang in ["en", "zh", "en-US,en;q=0.9"] {
        let (status, json) = get(&app, "/api/graph/project/list", Some(lang)).await;
        assert_eq!(status, StatusCode::OK, "Accept-Language {lang} must not break the request");
        assert_eq!(json["success"], true, "Accept-Language {lang}: {json}");
    }
}
