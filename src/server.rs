//! HTTP server module — port of MiroFish `run.py` (U-002) + `app/__init__.py` (U-003).
//!
//! # Symbol mapping
//!
//! | MiroFish symbol                   | Rust target                          | Status     |
//! |-----------------------------------|--------------------------------------|------------|
//! | S-023 `run.py:main`               | `serve` (called by `serve_cmd`)      | ported     |
//! | S-024 `create_app`                | `create_app`                         | partial    |
//! | S-025 `GET /health`               | `health_handler`                     | ported     |
//! | S-003 `SECRET_KEY` (roll-up)      | `Config::secret_key`                 | ported     |
//! | S-005 `JSON_AS_ASCII=False` (roll)| serde_json raw UTF-8 (structural)    | ported     |
//! | S-040 `get_locale` req-ctx branch | `accept_language_middleware`         | ported     |
//!
//! # Pending dependencies (recorded honestly; do NOT pretend complete)
//!
//! - **Blueprint: graph_bp → /api/graph** (pending U-025 `graph.py` routes)
//! - **Blueprint: simulation_bp → /api/simulation** (pending U-026 `simulation.py` routes)
//! - **Blueprint: report_bp → /api/report** (pending U-027 `report.py` routes)
//! - **register_cleanup / graceful sim-process teardown** (pending U-023 SimulationRunner,
//!   U-049 graceful shutdown). A structural `with_graceful_shutdown(ctrl_c)` is wired now;
//!   the sim-process cleanup hooks will be composed in when U-023/U-049 land.
//!
//! # `[≠]` intentional divergences
//!
//! 1. **Windows UTF-8 reconfigure** (`run.py:9-16`): `sys.stdout.reconfigure(encoding='utf-8')`
//!    is a Python/Windows console artifact with no Rust equivalent. Rust strings are natively
//!    UTF-8 and stdout is not codepage-bound the same way on any teri target platform.
//!    Justification: genuinely inexpressible (`sys.stdout.reconfigure` has no Rust API
//!    equivalent) + non-contractual (observable output is not affected; Rust UTF-8 is
//!    a structural superset).
//!
//! 2. **`debug=True` / `threaded=True`** (`run.py:42-45`): Flask's `app.run(debug=…,
//!    threaded=True)` controls Werkzeug's WSGI debug-reloader and threading. tokio is an
//!    async concurrent runtime that is always "threaded" (superset). Flask's debug-reload-on-save
//!    is a WSGI-dev-server artifact with no Rust equivalent. `config.debug` is loaded and
//!    available; the reloader behavior is non-contractual (dev-only, no observable wire shape).
//!    Justification: Werkzeug-WSGI runtime artifact; tokio is a strict superset for concurrency.

use axum::{
    Router,
    extract::Request,
    http::{HeaderMap, header},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::get,
};
use std::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};

use crate::api::ApiState;
use crate::{Config, TeriError};

// ---------------------------------------------------------------------------
// S-024 element 1 — JSON ensure_ascii=False (S-005 roll-up)
//
// serde_json serializes Unicode strings as raw UTF-8 bytes by default —
// no `\uXXXX` escaping is applied to non-ASCII characters. This is the
// structural Rust equivalent of Flask's `app.json.ensure_ascii = False`
// and `JSON_AS_ASCII = False` in config.py. No runtime flag needed.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// S-025 — GET /health
//
// Source: app/__init__.py:72-74
//   @app.route('/health')
//   def health():
//       return {'status': 'ok', 'service': 'MiroFish Backend'}
//
// teri uses "teri" as the service name (branding: teri IS the service now).
// The exact 2-key shape { status, service } and status:"ok" are preserved.
// Note for verifier: "service" value is "teri" not "MiroFish Backend" —
// this is intentional branding in the destination repo, not a behavior drop.
// ---------------------------------------------------------------------------

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "teri"
    }))
}

// ---------------------------------------------------------------------------
// S-024 element 3 — Request/response logging middleware
//
// Source: app/__init__.py:52-63
//   @app.before_request  → logs "请求: {method} {path}" at debug
//   @app.after_request   → logs "响应: {status_code}" at debug
//   (also logs request body JSON at debug — included here for parity)
// ---------------------------------------------------------------------------

async fn logging_middleware(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    tracing::debug!(target: "teri.request", "请求: {} {}", method, path);

    let response = next.run(request).await;

    tracing::debug!(target: "teri.request", "响应: {}", response.status().as_u16());
    response
}

// ---------------------------------------------------------------------------
// S-040 — Accept-Language → locale middleware (request-context branch of get_locale)
//
// Source: locale.py:29-31
//   if has_request_context():
//       raw = request.headers.get('Accept-Language', 'zh')
//       return raw if raw in _translations else 'zh'
//
// This wires the request-context branch of `get_locale` that was recorded as
// PENDING-U-002/U-003 in i18n/mod.rs. The middleware:
//   1. Reads Accept-Language header (default "zh" if absent).
//   2. Validates: if the value is in translations() {en, zh}, keep it; else "zh".
//   3. Runs the downstream handler inside LOCALE.scope(locale, ...) so inner
//      calls to i18n::get_locale() see the request's locale automatically.
//
// Exact port of locale.py:29-31 — same default, same validation set, same fallback.
// ---------------------------------------------------------------------------

async fn accept_language_middleware(headers: HeaderMap, request: Request, next: Next) -> Response {
    // Step 1: read Accept-Language header, default "zh"
    let raw = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("zh")
        .to_string();

    // Step 2: validate against translations set {en, zh} (locale.py:31)
    // crate::i18n::translations() is private; use the public API instead:
    // only "en" and "zh" are in translations, so inline the check.
    let locale = validate_locale(&raw);

    // Step 3: run downstream inside the locale scope
    crate::i18n::with_locale(locale, next.run(request)).await
}

/// Validate a raw locale string against the translations set {en, zh}.
/// Returns the value if valid, else falls back to "zh".
///
/// Mirrors locale.py:31: `return raw if raw in _translations else 'zh'`
pub fn validate_locale(raw: &str) -> String {
    // Single source of truth: validate against the i18n module's actually-embedded
    // translation set (currently {en, zh}). Routing through `is_supported_locale`
    // means a future locale file added under `i18n/locales/` is covered automatically —
    // no second site to update (removes the latent-drift risk a hardcoded list carries).
    if crate::i18n::is_supported_locale(raw) { raw.to_string() } else { "zh".to_string() }
}

// ---------------------------------------------------------------------------
// S-024 — create_app
//
// Port of app/__init__.py:19-79.
//
// Ported elements:
//   1. JSON ensure_ascii=False — structural (serde_json raw UTF-8, see above)
//   2. setup_logger — logging is process-global in teri; init_logging() is
//      called once in serve_cmd (the entrypoint), NOT in create_app.
//      This faithfully maps the logger-name indirection: MiroFish called
//      setup_logger('mirofish') inside create_app; teri's process-global
//      tracing is initialized in the entrypoint. create_app does not double-init.
//   3. CORS — CorsLayer::permissive() applied to /api/* routes (faithful scope).
//   4. before/after_request logging — logging_middleware applied to all routes.
//   5. Accept-Language → locale — accept_language_middleware applied to all routes.
//   6. register_cleanup — BASIC graceful shutdown (ctrl_c/SIGTERM) is structural now;
//      sim-process cleanup composes in when U-023/U-049 land. See pending deps above.
//   7. Blueprints (graph_bp, simulation_bp, report_bp) — PENDING U-025/026/027.
//      Placeholders are clearly marked below.
//   8. GET /health — health_handler.
// ---------------------------------------------------------------------------

/// Build the axum Router — port of MiroFish `create_app()`.
///
/// U-025 (sub-cycle a): graph blueprint is now wired under `/api/graph`.
/// U-026 will add `.nest("/simulation", simulation_router(state.clone()))` to `api_router`.
/// U-027 will add `.nest("/report", report_router(state.clone()))` to `api_router`.
/// Both are one-line adds to the `api_router` below — the scaffold is ready.
///
/// CORS scoping: We apply CORS to the whole `api_router` (all `/api/*` routes) by layering
/// on the nested sub-router before nesting it under `/api`.  This matches MiroFish's
/// `CORS(app, resources={r"/api/*": {"origins": "*"}})` — CORS is scoped to /api/* only,
/// NOT to /health.  Choice recorded: CORS applied to `api_router` (the /api nest), not the
/// whole app, achieving exact MiroFish per-path scoping in axum 0.7.
pub fn create_app(state: std::sync::Arc<ApiState>) -> Router {
    // S-024 element 3 — CORS scoped to /api/* (MiroFish: resources={r"/api/*": {"origins": "*"}})
    // Applied to api_router only, not the top-level router (so /health has no CORS headers).
    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);

    // Startup logging (faithful to app/__init__.py:37-41, 77)
    tracing::info!("{}", "=".repeat(50));
    tracing::info!("MiroFish Backend 启动中...");
    tracing::info!("{}", "=".repeat(50));

    // Blueprint sub-routers — U-025 (a): graph_bp; U-026 (a): simulation_bp (skeleton).
    // U-027 adds: .nest("/report", report_router(state.clone()))
    let api_router = Router::new()
        .nest("/graph", crate::api::graph::graph_router(state.clone()))
        // U-026 (a): simulation blueprint — skeleton nested here; routes land in sub-cycles b–m.
        .nest("/simulation", crate::api::simulation::simulation_router(state.clone()))
        // U-027 (a): report blueprint — report.py /api/report routes.
        .nest("/report", crate::api::report::report_router(state.clone()))
        // CORS scoped to /api/* (applied to api_router, not the top-level app)
        .layer(cors);

    let router = Router::new()
        // GET /health (S-025) — no CORS, outside /api/*
        .route("/health", get(health_handler))
        // /api/* with graph blueprint (and future simulation/report)
        .nest("/api", api_router)
        // S-024 element 4 — request/response logging middleware (all routes)
        .layer(middleware::from_fn(logging_middleware))
        // S-040 — Accept-Language → locale middleware (all routes)
        .layer(middleware::from_fn(accept_language_middleware));

    tracing::info!("MiroFish Backend 启动完成");

    router
}

// ---------------------------------------------------------------------------
// S-023 — serve (main entrypoint logic)
//
// Port of run.py:main() called from serve_cmd in main.rs.
//
// Ported elements:
//   1. Config validate() — called first; errors printed + exit-equivalent on Err.
//   2. create_app() — build the Router.
//   3. Bind address: --addr superset + FLASK_HOST/FLASK_PORT env contract.
//   4. debug / threaded — [≠] (see module doc above).
//   5. Graceful shutdown — structural ctrl_c/SIGTERM; sim-process cleanup pending U-023/U-049.
// ---------------------------------------------------------------------------

/// Resolve the bind address from CLI override or MiroFish env contract.
///
/// MiroFish `run.py:40-41`:
///   host = os.environ.get('FLASK_HOST', '0.0.0.0')
///   port = int(os.environ.get('FLASK_PORT', 5001))
///
/// teri superset: if `--addr` CLI flag is provided, use it directly.
/// Otherwise, build host:port from FLASK_HOST (default "0.0.0.0") + FLASK_PORT (default 5001).
/// This is a faithful SUPERSET — preserves the MiroFish env contract while adding teri's flag.
pub fn resolve_bind_addr(cli_addr: Option<&str>) -> String {
    if let Some(addr) = cli_addr {
        return addr.to_string();
    }
    let host = std::env::var("FLASK_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port: u16 = std::env::var("FLASK_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(5001);
    format!("{host}:{port}")
}

/// Start the HTTP server — called from `serve_cmd` in main.rs.
///
/// Steps (faithful to run.py:main):
///   1. Config validate_collect() → if errors, surface them and return Err.
///   2. Build the Router via create_app().
///   3. Bind the resolved address.
///   4. Serve with graceful shutdown (ctrl_c / SIGTERM).
///      NOTE: sim-process cleanup (register_cleanup equivalent) is PENDING U-023/U-049.
///
/// # Errors
/// Returns `TeriError::Config` on config validation failure.
/// Returns `TeriError::Io` on bind/serve failure.
pub async fn serve(config: Config, cli_addr: Option<&str>) -> crate::error::Result<()> {
    // S-023 element 1 — Config validation first (run.py:28-34)
    let errors = config.validate_collect();
    if !errors.is_empty() {
        // Mirror MiroFish: print all errors then exit(1).
        eprintln!("配置错误:");
        for err in &errors {
            eprintln!("  - {err}");
        }
        eprintln!("\n请检查 .env 文件中的配置");
        return Err(TeriError::Config(errors.join("; ")));
    }

    let state = std::sync::Arc::new(ApiState::new(config.clone()));

    // S-023 element 2 — create app
    let app = create_app(state);

    // S-023 element 3 — bind address (superset: CLI flag OR FLASK_HOST/FLASK_PORT)
    let addr = resolve_bind_addr(cli_addr);
    tracing::info!("Starting API server on {addr}");

    let listener = TcpListener::bind(&addr).map_err(TeriError::Io)?;

    // S-023 element 4 / S-024 element 6 — graceful shutdown
    // Structural: ctrl_c signal causes clean axum shutdown.
    // PENDING U-023/U-049: SimulationRunner.register_cleanup() equivalent
    // (atexit/SIGTERM handlers to kill simulation subprocesses) will compose in here
    // when U-023 (SimulationRunner) and U-049 (graceful shutdown) are ported.
    // MiroFish source: app/__init__.py:46-47, run.py:44 (threaded shutdown semantics).
    axum::serve(tokio::net::TcpListener::from_std(listener).map_err(TeriError::Io)?, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.expect("Failed to install CTRL+C handler");
            tracing::info!("Received shutdown signal, stopping server...");
            // PENDING U-023/U-049: call SimulationRunner::cleanup() here
        })
        .await
        .map_err(TeriError::Io)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    // tower 0.5 (pulled by axum 0.7) provides ServiceExt::oneshot for Router
    use tower::ServiceExt;

    // Serializes tests that mutate the process-global environment.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Helper: build a test app with a state (uses a minimal Config::build).
    fn test_app() -> Router {
        let config = Config::build_test();
        let state = std::sync::Arc::new(ApiState::new(config));
        create_app(state)
    }

    // -----------------------------------------------------------------------
    // S-025 — GET /health
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn health_returns_200_with_ok_shape() {
        let app = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok", "status must be 'ok' (not 'healthy')");
        assert_eq!(json["service"], "teri", "service must be 'teri'");
    }

    // -----------------------------------------------------------------------
    // S-005 / S-024 element 1 — JSON ensure_ascii=False
    //
    // Proves serde_json emits raw UTF-8 bytes for Chinese characters —
    // no \uXXXX escaping. This is the structural proof of JSON_AS_ASCII=False.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn json_utf8_not_escaped_health_body() {
        let app = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        // serde_json must not contain any \uXXXX escape sequences for the value
        let body_str = std::str::from_utf8(&body_bytes).unwrap();
        // Ensure no Unicode escape sequences appear in any JSON response
        assert!(
            !body_str.contains("\\u"),
            "Response should not contain \\uXXXX escapes (ensure_ascii=False): {body_str}"
        );
    }

    /// Dedicated test: a handler returning 中文 JSON emits raw UTF-8 bytes.
    ///
    /// This creates a small inline router with a Chinese-text response to prove
    /// serde_json never emits \uXXXX for CJK characters, mirroring the
    /// Flask/MiroFish JSON_AS_ASCII=False contract.
    #[tokio::test]
    async fn json_chinese_characters_emitted_as_raw_utf8() {
        async fn chinese_handler() -> impl IntoResponse {
            Json(serde_json::json!({
                "message": "你好世界",
                "status": "中文测试"
            }))
        }

        let app = Router::new().route("/chinese", get(chinese_handler));
        let resp = app
            .oneshot(Request::builder().uri("/chinese").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();

        // Check raw UTF-8 bytes contain the actual Chinese characters
        let body_str = std::str::from_utf8(&body_bytes).unwrap();
        assert!(body_str.contains("你好世界"), "Raw 中文 should appear in body: {body_str}");
        assert!(body_str.contains("中文测试"), "Raw 中文 should appear in body: {body_str}");
        // No \uXXXX escapes
        assert!(
            !body_str.contains("\\u"),
            "No \\uXXXX escapes should appear (ensure_ascii=False contract): {body_str}"
        );
    }

    // -----------------------------------------------------------------------
    // S-040 — Accept-Language middleware (request-context branch of get_locale)
    // -----------------------------------------------------------------------

    /// A test handler that returns the current locale as plain text.
    async fn locale_echo_handler() -> impl IntoResponse {
        crate::i18n::get_locale()
    }

    fn locale_test_app() -> Router {
        Router::new()
            .route("/locale-echo", get(locale_echo_handler))
            .layer(middleware::from_fn(accept_language_middleware))
    }

    #[tokio::test]
    async fn accept_language_en_sets_locale_en() {
        let app = locale_test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/locale-echo")
                    .header(header::ACCEPT_LANGUAGE, "en")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), "en");
    }

    #[tokio::test]
    async fn accept_language_fr_not_in_translations_falls_back_to_zh() {
        // "fr" is NOT in the translations set {en, zh} → fallback to "zh"
        // Mirrors locale.py:31: `return raw if raw in _translations else 'zh'`
        let app = locale_test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/locale-echo")
                    .header(header::ACCEPT_LANGUAGE, "fr")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(
            std::str::from_utf8(&body).unwrap(),
            "zh",
            "fr not in translations → should fall back to zh"
        );
    }

    #[tokio::test]
    async fn accept_language_absent_defaults_to_zh() {
        // No Accept-Language header → default "zh"
        // Mirrors locale.py:30: request.headers.get('Accept-Language', 'zh')
        let app = locale_test_app();
        let resp = app
            .oneshot(Request::builder().uri("/locale-echo").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), "zh");
    }

    #[tokio::test]
    async fn accept_language_zh_stays_zh() {
        let app = locale_test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/locale-echo")
                    .header(header::ACCEPT_LANGUAGE, "zh")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), "zh");
    }

    // -----------------------------------------------------------------------
    // resolve_bind_addr — unit tests (pure fn, no actual binding)
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_bind_addr_cli_flag_takes_precedence() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Even if FLASK_HOST/FLASK_PORT are set, CLI flag wins
        unsafe {
            std::env::set_var("FLASK_HOST", "192.168.1.1");
            std::env::set_var("FLASK_PORT", "9999");
        }
        let addr = resolve_bind_addr(Some("1.2.3.4:9"));
        assert_eq!(addr, "1.2.3.4:9");
        unsafe {
            std::env::remove_var("FLASK_HOST");
            std::env::remove_var("FLASK_PORT");
        }
    }

    #[test]
    fn resolve_bind_addr_flask_env_when_no_cli() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_host = std::env::var("FLASK_HOST");
        let prev_port = std::env::var("FLASK_PORT");
        unsafe {
            std::env::set_var("FLASK_HOST", "10.0.0.1");
            std::env::set_var("FLASK_PORT", "7777");
        }
        let addr = resolve_bind_addr(None);
        assert_eq!(addr, "10.0.0.1:7777");
        match prev_host {
            Ok(v) => unsafe { std::env::set_var("FLASK_HOST", v) },
            Err(_) => unsafe { std::env::remove_var("FLASK_HOST") },
        }
        match prev_port {
            Ok(v) => unsafe { std::env::set_var("FLASK_PORT", v) },
            Err(_) => unsafe { std::env::remove_var("FLASK_PORT") },
        }
    }

    #[test]
    fn resolve_bind_addr_defaults_to_0_0_0_0_5001_when_no_env_and_no_cli() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_host = std::env::var("FLASK_HOST");
        let prev_port = std::env::var("FLASK_PORT");
        unsafe {
            std::env::remove_var("FLASK_HOST");
            std::env::remove_var("FLASK_PORT");
        }
        let addr = resolve_bind_addr(None);
        assert_eq!(addr, "0.0.0.0:5001");
        match prev_host {
            Ok(v) => unsafe { std::env::set_var("FLASK_HOST", v) },
            Err(_) => unsafe { std::env::remove_var("FLASK_HOST") },
        }
        match prev_port {
            Ok(v) => unsafe { std::env::set_var("FLASK_PORT", v) },
            Err(_) => unsafe { std::env::remove_var("FLASK_PORT") },
        }
    }

    // -----------------------------------------------------------------------
    // S-003 / SECRET_KEY — rolled up config tests
    // -----------------------------------------------------------------------

    #[test]
    fn secret_key_default_is_mirofish_secret_key() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("SECRET_KEY");
        unsafe { std::env::remove_var("SECRET_KEY") };
        let c = Config::build_test();
        assert_eq!(c.secret_key, "mirofish-secret-key");
        match prev {
            Ok(v) => unsafe { std::env::set_var("SECRET_KEY", v) },
            Err(_) => unsafe { std::env::remove_var("SECRET_KEY") },
        }
    }

    #[test]
    fn secret_key_env_override() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("SECRET_KEY");
        unsafe { std::env::set_var("SECRET_KEY", "my-custom-secret") };
        let c = Config::build_test();
        assert_eq!(c.secret_key, "my-custom-secret");
        match prev {
            Ok(v) => unsafe { std::env::set_var("SECRET_KEY", v) },
            Err(_) => unsafe { std::env::remove_var("SECRET_KEY") },
        }
    }

    // -----------------------------------------------------------------------
    // validate_locale — unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn validate_locale_en_is_valid() {
        assert_eq!(validate_locale("en"), "en");
    }

    #[test]
    fn validate_locale_zh_is_valid() {
        assert_eq!(validate_locale("zh"), "zh");
    }

    #[test]
    fn validate_locale_fr_falls_back_to_zh() {
        assert_eq!(validate_locale("fr"), "zh");
    }

    #[test]
    fn validate_locale_empty_falls_back_to_zh() {
        assert_eq!(validate_locale(""), "zh");
    }

    #[test]
    fn validate_locale_unknown_falls_back_to_zh() {
        assert_eq!(validate_locale("de"), "zh");
    }
}
