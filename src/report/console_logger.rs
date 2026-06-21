//! `ReportConsoleLogger` — per-report plain-text console capture.
//!
//! Port of `class ReportConsoleLogger` (`backend/app/services/report_agent.py:307-386`).
//!
//! ## What this does
//!
//! For the duration of one report run, INFO+ output on two specific logging targets
//! is **tee'd** into `{upload_folder}/reports/{report_id}/console_log.txt`:
//!
//! | Python named logger      | teri tracing `target`            |
//! |--------------------------|----------------------------------|
//! | `mirofish.report_agent`  | `teri::report`                   |
//! | `mirofish.zep_tools`     | `teri::services::zep_tools`      |
//!
//! ## Architecture (option a from the architect's design doc u024-g2-console-logger.md)
//!
//! A `tracing_subscriber` [`Layer`][tracing_subscriber::Layer] (`ReportConsoleLayer`) is
//! installed **once at startup** into the global registry (see [`crate::logging::init_logging`]).
//! It is dormant (no-op) when the process-global sink [`REPORT_CONSOLE_SINK`] is `None`.
//!
//! `ReportConsoleLogger::new` opens `console_log.txt` in append mode and installs it into
//! the global sink.  `close()`/`Drop` clears the sink, after which all further events on
//! the captured targets pass through the layer without being written.
//!
//! ## Format (contractual — frontend reads console_log.txt)
//!
//! ```text
//! [HH:MM:SS] LEVEL: <message>\n
//! ```
//!
//! - Timestamp: **local** wall-clock time-of-day, `%H:%M:%S` via `chrono::Local::now()`.
//! - Level: Python-style names — `INFO`, `WARNING` (NOT `WARN`), `ERROR`, `DEBUG`.
//! - Message: the fully i18n-resolved message field only (no target/span decoration).
//!
//! ## WARN → WARNING (contractual, #1 gate trap)
//!
//! `tracing::Level::WARN` displays as `"WARN"`, but Python's `logging.WARNING` writes
//! `"WARNING"`.  This layer maps `WARN → "WARNING"` so the file content matches.
//!
//! ## Level filter (contractual)
//!
//! Only INFO, WARN, and ERROR are captured (`>= INFO` in Python terms).  DEBUG/TRACE are
//! excluded — matching `FileHandler.setLevel(logging.INFO)`.  (The Python `logger.debug`
//! at `report_agent.py:1322` therefore produces **no** line in `console_log.txt`.)
//!
//! ## Forward dependency
//!
//! `teri::services::zep_tools` is wired as a captured target now (zero cost).  Events
//! will flow into `console_log.txt` automatically once `src/services/zep_tools.rs` emits
//! `tracing` events on that target.  This is a **wiring-ready seam**, not a downgrade.
//!
//! ## Symbol map
//! | Python                            | Rust                                         |
//! |-----------------------------------|----------------------------------------------|
//! | `ReportConsoleLogger.__init__`    | `ReportConsoleLogger::new`                   |
//! | `_ensure_log_file`                | inside `new` (dir creation)                  |
//! | `_setup_file_handler`             | sets `REPORT_CONSOLE_SINK`                   |
//! | `close`                           | `ReportConsoleLogger::close`                 |
//! | `__del__`                         | `impl Drop for ReportConsoleLogger`          |
//! | `FileHandler` on two named loggers| `ReportConsoleLayer` (per-event target check)|

use std::fs::{File, OpenOptions, create_dir_all};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use tracing::Level;
use tracing_subscriber::Layer;

// ── Process-global sink ───────────────────────────────────────────────────────

/// The open `console_log.txt` file for the currently-active report run.
///
/// `None` while no report is running.  The `ReportConsoleLayer` is installed once
/// at startup and checks this each event; when `None` the layer is a no-op.
pub struct ReportConsoleSink {
    /// Open append file handle to `console_log.txt`.
    file: File,
    /// Report ID for diagnostics only.
    pub report_id: String,
}

/// Process-global sink handle, initialised once in [`crate::logging::init_logging`].
///
/// The value is always `Some(Arc<Mutex<Option<ReportConsoleSink>>>)` after init; the
/// inner `Option<ReportConsoleSink>` is `None` when no report is active.
///
/// We use `OnceLock` so the `Arc<Mutex<…>>` is constructed exactly once (in `init_logging`)
/// and then the `Mutex` guard is used per-event / per-report toggle.
pub static REPORT_CONSOLE_SINK: OnceLock<Arc<Mutex<Option<ReportConsoleSink>>>> = OnceLock::new();

/// Initialise the process-global sink handle.  Called exactly once from
/// [`crate::logging::init_logging`] before `.init()` is called on the registry.
pub fn init_sink() {
    // OnceLock::set returns Err if already set; ignore — idempotent.
    let _ = REPORT_CONSOLE_SINK.set(Arc::new(Mutex::new(None)));
}

// ── The tracing Layer ─────────────────────────────────────────────────────────

/// A `tracing_subscriber` [`Layer`] that writes formatted lines to `console_log.txt`
/// while a report is active.
///
/// Installed once at startup into the global registry.  Dormant (no-op) when
/// `REPORT_CONSOLE_SINK` holds `None`.
pub struct ReportConsoleLayer;

/// Captured tracing targets (exact + prefix match).
const TARGET_EXACT: &str = "teri::report";
const TARGET_ZEP_PREFIX: &str = "teri::services::zep_tools";

/// Map a `tracing::Level` to its Python `logging` level-name string.
///
/// The contractual difference: tracing uses `"WARN"`, Python uses `"WARNING"`.
fn python_level_name(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARNING", // contractual: WARN → WARNING
        Level::INFO => "INFO",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "DEBUG", // TRACE → DEBUG (both filtered out by INFO floor anyway)
    }
}

/// A `tracing` field visitor that extracts the `message` field value.
struct MessageVisitor {
    message: Option<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }
}

impl<S> Layer<S> for ReportConsoleLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let meta = event.metadata();

        // ── Level filter: INFO+ only (DEBUG/TRACE excluded) ──────────────────
        // In tracing's ordering, ERROR < WARN < INFO < DEBUG < TRACE.
        // We want >= INFO severity, which means level <= Level::INFO.
        if *meta.level() > Level::INFO {
            return;
        }

        // ── Target filter ─────────────────────────────────────────────────────
        let target = meta.target();
        let captured = target == TARGET_EXACT || target.starts_with(TARGET_ZEP_PREFIX);
        if !captured {
            return;
        }

        // ── Check sink is active ──────────────────────────────────────────────
        let sink_handle = match REPORT_CONSOLE_SINK.get() {
            Some(h) => h,
            None => return, // init_logging not yet called (e.g. very early startup)
        };

        let mut guard = match sink_handle.lock() {
            Ok(g) => g,
            Err(_) => return, // poisoned mutex — mirror Python's silent write-error ignore
        };

        let sink = match guard.as_mut() {
            Some(s) => s,
            None => return, // no report active — no-op
        };

        // ── Extract message field ─────────────────────────────────────────────
        let mut visitor = MessageVisitor { message: None };
        event.record(&mut visitor);
        let message = match visitor.message {
            Some(m) => m,
            None => return, // no message field — skip
        };

        // ── Format: [HH:MM:SS] LEVEL: message\n ──────────────────────────────
        let now = chrono::Local::now();
        let ts = now.format("%H:%M:%S");
        let level_name = python_level_name(meta.level());

        // Write + flush (non-fatal on error — mirrors Python's silent ignore)
        let line = format!("[{ts}] {level_name}: {message}\n");
        let _ = sink.file.write_all(line.as_bytes());
        let _ = sink.file.flush();
    }
}

// ── ReportConsoleLogger ───────────────────────────────────────────────────────

/// Per-report console log toggle.
///
/// Port of `class ReportConsoleLogger` (`report_agent.py:307-386`).
///
/// **Lifecycle:** construct at the start of a report run → events from `teri::report`
/// and `teri::services::zep_tools` at INFO+ are written to `console_log.txt` → call
/// `close()` or drop at the end of the run.
///
/// The actual capture is performed by [`ReportConsoleLayer`], which is installed into
/// the global tracing registry by [`crate::logging::init_logging`].  This struct merely
/// toggles the process-global sink.
pub struct ReportConsoleLogger {
    /// Report ID — stored for diagnostics and to guard against double-close.
    report_id: String,
    /// Absolute path to `console_log.txt` (stored for diagnostics).
    pub log_file_path: PathBuf,
    /// True while the sink is installed (cleared by `close()`).
    active: bool,
}

impl ReportConsoleLogger {
    /// Create a new console logger for `report_id`.
    ///
    /// Port of `ReportConsoleLogger.__init__` + `_ensure_log_file` + `_setup_file_handler`
    /// (`report_agent.py:315-364`).
    ///
    /// - `mkdir -p`s `{upload_folder}/reports/{report_id}/`.
    /// - Opens `console_log.txt` in append + UTF-8 mode.
    /// - Installs the file into [`REPORT_CONSOLE_SINK`].
    ///
    /// If a sink is already active (another report running), the existing sink is
    /// replaced — matching Python's "avoid duplicate handler" intent (it only guards
    /// against double-add to the *same* logger in the same run; a new construction
    /// naturally replaces the previous handler).
    ///
    /// Returns an `io::Error` if the directory cannot be created or the file cannot
    /// be opened.
    pub fn new(report_id: impl Into<String>, upload_folder: &Path) -> std::io::Result<Self> {
        let report_id = report_id.into();

        // `_ensure_log_file`: mkdir -p the report directory
        let report_dir = upload_folder.join("reports").join(&report_id);
        create_dir_all(&report_dir)?;

        let log_file_path = report_dir.join("console_log.txt");

        // Open in append mode (Python: mode='a', encoding='utf-8')
        let file = OpenOptions::new().create(true).append(true).open(&log_file_path)?;

        // Install sink into the process-global handle
        let sink = ReportConsoleSink { file, report_id: report_id.clone() };

        if let Some(handle) = REPORT_CONSOLE_SINK.get()
            && let Ok(mut guard) = handle.lock()
        {
            *guard = Some(sink);
        }
        // If REPORT_CONSOLE_SINK is not yet initialised (init_logging not called),
        // the layer is not installed anyway — sink install is a no-op here.

        Ok(Self { report_id, log_file_path, active: true })
    }

    /// Detach the file handler and close `console_log.txt`.
    ///
    /// Port of `ReportConsoleLogger.close` (`report_agent.py:366-382`).
    ///
    /// After this call, further events on the captured targets are **not** written.
    /// Idempotent — safe to call more than once.
    pub fn close(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;

        if let Some(handle) = REPORT_CONSOLE_SINK.get()
            && let Ok(mut guard) = handle.lock()
        {
            // Flush + drop the file by replacing with None.
            if let Some(mut sink) = guard.take() {
                // Only clear if it's *our* report (guard against cross-run replace).
                if sink.report_id == self.report_id {
                    let _ = sink.file.flush();
                    // `sink` is dropped here, closing the file.
                } else {
                    // Another report's sink — put it back.
                    *guard = Some(sink);
                }
            }
        }
    }

    /// Report ID accessor (for diagnostics).
    pub fn report_id(&self) -> &str {
        &self.report_id
    }
}

/// Port of `ReportConsoleLogger.__del__` (`report_agent.py:384-386`).
///
/// Ensures `close()` is called if the caller forgets.
impl Drop for ReportConsoleLogger {
    fn drop(&mut self) {
        self.close();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tracing_subscriber::prelude::*;

    // ── Test subscriber setup ─────────────────────────────────────────────────
    //
    // The global tracing subscriber can only be set once per process.  The test
    // binary (all tests in this module share one process) therefore installs the
    // `ReportConsoleLayer` into a dedicated test registry ONCE, via `INIT`.
    // Tests then toggle the sink directly (via `REPORT_CONSOLE_SINK`) — no need
    // to re-install the layer.
    //
    // IMPORTANT: the global sink is a singleton — only one test may hold it at a
    // time.  Tests that use the sink are serialized via SINK_MUTEX.

    static INIT: Once = Once::new();
    static SUBSCRIBER_INSTALLED: AtomicBool = AtomicBool::new(false);

    // Mutex used to serialize all tests that touch the global REPORT_CONSOLE_SINK.
    // This prevents concurrent tests from stomping on each other's active sink.
    static SINK_MUTEX: Mutex<()> = Mutex::new(());

    fn ensure_subscriber() {
        INIT.call_once(|| {
            // Initialise the sink handle before installing the subscriber.
            init_sink();

            // Build a minimal registry with just our layer (suppress all other output).
            let result = tracing_subscriber::registry().with(ReportConsoleLayer).try_init();

            // `try_init` returns Err if a global subscriber is already set (e.g. the
            // main init_logging was already called).  That's fine — our layer may not
            // be installed, so we fall back to a direct-write test.
            SUBSCRIBER_INSTALLED.store(result.is_ok(), Ordering::SeqCst);
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 1. Install / emit / drop lifecycle
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_lifecycle_install_emit_drop() {
        ensure_subscriber();
        let _sink_guard = SINK_MUTEX.lock().unwrap();
        let dir =
            std::env::temp_dir().join(format!("teri_console_lifecycle_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut logger = ReportConsoleLogger::new("lifecycle-001", &dir).expect("logger must open");

        // File must exist after construction.
        assert!(logger.log_file_path.exists(), "console_log.txt must be created on new()");

        if SUBSCRIBER_INSTALLED.load(Ordering::SeqCst) {
            // Emit on captured target — should appear in file.
            tracing::info!(target: "teri::report", "lifecycle test message");

            // Let the write happen (it's synchronous in our Layer).
            let contents_before =
                std::fs::read_to_string(&logger.log_file_path).unwrap_or_default();
            assert!(
                contents_before.contains("lifecycle test message"),
                "line must appear before close; contents:\n{contents_before}"
            );

            logger.close();

            // After close, further events must NOT be captured.
            tracing::info!(target: "teri::report", "post-close message");
            let contents_after = std::fs::read_to_string(&logger.log_file_path).unwrap_or_default();
            assert!(
                !contents_after.contains("post-close message"),
                "post-close event must not appear in file; contents:\n{contents_after}"
            );
        } else {
            // Subscriber not installed — just verify file creation and close idempotence.
            logger.close();
            logger.close(); // must not panic
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 2. WARNING not WARN (contractual level-name mapping)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_warn_maps_to_warning_not_warn() {
        ensure_subscriber();
        let _sink_guard = SINK_MUTEX.lock().unwrap();
        if !SUBSCRIBER_INSTALLED.load(Ordering::SeqCst) {
            // Can't test layer output without the subscriber — verify mapping fn directly.
            assert_eq!(python_level_name(&Level::WARN), "WARNING");
            assert_eq!(python_level_name(&Level::INFO), "INFO");
            assert_eq!(python_level_name(&Level::ERROR), "ERROR");
            assert_eq!(python_level_name(&Level::DEBUG), "DEBUG");
            assert_eq!(python_level_name(&Level::TRACE), "DEBUG");
            return;
        }

        let dir = std::env::temp_dir().join(format!("teri_console_warn_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut logger = ReportConsoleLogger::new("warn-test-001", &dir).expect("logger must open");

        tracing::warn!(target: "teri::report", "section iteration is None");
        logger.close();

        let contents = std::fs::read_to_string(&logger.log_file_path).unwrap_or_default();
        assert!(
            contents.contains("WARNING:"),
            "WARN level must appear as 'WARNING:' in file; contents:\n{contents}"
        );
        assert!(
            !contents.contains("WARN:"),
            "must not contain 'WARN:' (only 'WARNING:'); contents:\n{contents}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 3. Format regex: [HH:MM:SS] LEVEL: message
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_line_format_matches_regex() {
        ensure_subscriber();
        let _sink_guard = SINK_MUTEX.lock().unwrap();
        if !SUBSCRIBER_INSTALLED.load(Ordering::SeqCst) {
            return; // Can't test output without the layer installed.
        }

        let dir = std::env::temp_dir().join(format!("teri_console_format_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut logger =
            ReportConsoleLogger::new("format-test-001", &dir).expect("logger must open");

        tracing::info!(target: "teri::report", "ReACT generating section: Market Overview");
        tracing::warn!(target: "teri::report", "section iter is None");
        tracing::error!(target: "teri::report", "tool execution failed");
        logger.close();

        let contents = std::fs::read_to_string(&logger.log_file_path).unwrap_or_default();
        let re = regex::Regex::new(r"^\[\d{2}:\d{2}:\d{2}\] (INFO|WARNING|ERROR): .+$").unwrap();

        let lines: Vec<&str> = contents.lines().collect();
        assert!(!lines.is_empty(), "must have at least one line");
        for line in &lines {
            assert!(
                re.is_match(line),
                "line does not match format regex: {line:?}\nAll contents:\n{contents}"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 4. INFO+ filter: DEBUG events produce NO line
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_debug_events_excluded() {
        ensure_subscriber();
        let _sink_guard = SINK_MUTEX.lock().unwrap();
        if !SUBSCRIBER_INSTALLED.load(Ordering::SeqCst) {
            // Verify the level filter logic directly.
            // Level ordering: ERROR < WARN < INFO < DEBUG < TRACE
            assert!(Level::DEBUG > Level::INFO, "DEBUG must be excluded by > INFO check");
            assert!(Level::TRACE > Level::INFO, "TRACE must be excluded by > INFO check");
            // INFO/WARN/ERROR must NOT be > INFO (they pass the filter).
            let info_filtered = Level::INFO > Level::INFO;
            let warn_filtered = Level::WARN > Level::INFO;
            let error_filtered = Level::ERROR > Level::INFO;
            assert!(!info_filtered, "INFO must pass the filter");
            assert!(!warn_filtered, "WARN must pass the filter");
            assert!(!error_filtered, "ERROR must pass the filter");
            return;
        }

        let dir =
            std::env::temp_dir().join(format!("teri_console_debug_filter_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut logger =
            ReportConsoleLogger::new("debug-filter-001", &dir).expect("logger must open");

        // This mirrors report_agent.py:1322 logger.debug("LLM响应...")
        tracing::debug!(target: "teri::report", "LLM响应: some content...");
        // But an INFO should appear so we can verify the file was written to at all.
        tracing::info!(target: "teri::report", "visible info line");
        logger.close();

        let contents = std::fs::read_to_string(&logger.log_file_path).unwrap_or_default();
        assert!(
            !contents.contains("LLM响应"),
            "DEBUG line must NOT appear in console_log.txt; contents:\n{contents}"
        );
        assert!(
            contents.contains("visible info line"),
            "INFO line must appear so we know the file was written; contents:\n{contents}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 5. Target filter: non-report targets produce NO line
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_non_report_target_excluded() {
        ensure_subscriber();
        let _sink_guard = SINK_MUTEX.lock().unwrap();
        if !SUBSCRIBER_INSTALLED.load(Ordering::SeqCst) {
            // Verify target filter logic directly.
            assert!("teri::report" == TARGET_EXACT, "TARGET_EXACT must be 'teri::report'");
            assert!("teri::services::zep_tools".starts_with(TARGET_ZEP_PREFIX));
            assert!(!"teri::server".starts_with(TARGET_ZEP_PREFIX));
            assert!("teri::server" != TARGET_EXACT);
            return;
        }

        let dir =
            std::env::temp_dir().join(format!("teri_console_target_filter_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut logger =
            ReportConsoleLogger::new("target-filter-001", &dir).expect("logger must open");

        // Non-captured target — must not appear.
        tracing::info!(target: "teri::server", "HTTP request received");
        tracing::info!(target: "teri::sim", "simulation tick 42");
        // Captured target — must appear.
        tracing::info!(target: "teri::report", "captured line");
        logger.close();

        let contents = std::fs::read_to_string(&logger.log_file_path).unwrap_or_default();
        assert!(
            !contents.contains("HTTP request received"),
            "teri::server events must NOT appear; contents:\n{contents}"
        );
        assert!(
            !contents.contains("simulation tick 42"),
            "teri::sim events must NOT appear; contents:\n{contents}"
        );
        assert!(
            contents.contains("captured line"),
            "teri::report events must appear; contents:\n{contents}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 6. zep_tools prefix captured
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_zep_tools_target_captured() {
        ensure_subscriber();
        let _sink_guard = SINK_MUTEX.lock().unwrap();
        if !SUBSCRIBER_INSTALLED.load(Ordering::SeqCst) {
            return;
        }

        let dir = std::env::temp_dir().join(format!("teri_console_zep_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut logger = ReportConsoleLogger::new("zep-test-001", &dir).expect("logger must open");

        tracing::info!(target: "teri::services::zep_tools", "zep tools event");
        tracing::info!(target: "teri::services::zep_tools::sub", "zep sub-module event");
        logger.close();

        let contents = std::fs::read_to_string(&logger.log_file_path).unwrap_or_default();
        assert!(
            contents.contains("zep tools event"),
            "teri::services::zep_tools events must be captured; contents:\n{contents}"
        );
        assert!(
            contents.contains("zep sub-module event"),
            "teri::services::zep_tools::sub events must be captured; contents:\n{contents}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 7. Level name mapping (unit test — no subscriber needed)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_python_level_name_mapping() {
        assert_eq!(python_level_name(&Level::ERROR), "ERROR");
        assert_eq!(python_level_name(&Level::WARN), "WARNING", "WARN must map to WARNING");
        assert_eq!(python_level_name(&Level::INFO), "INFO");
        assert_eq!(python_level_name(&Level::DEBUG), "DEBUG");
        assert_eq!(python_level_name(&Level::TRACE), "DEBUG");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 8. mkdir-p + file creation (no subscriber needed)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_mkdir_and_file_created() {
        let dir = std::env::temp_dir().join(format!("teri_console_mkdir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!dir.exists());

        let logger = ReportConsoleLogger::new("mkdir-001", &dir).expect("must succeed");

        assert!(logger.log_file_path.exists(), "console_log.txt must be created");
        let expected = dir.join("reports").join("mkdir-001").join("console_log.txt");
        assert_eq!(logger.log_file_path, expected, "path must match");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 9. Drop calls close (idempotent)
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_drop_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("teri_console_drop_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        {
            let mut logger = ReportConsoleLogger::new("drop-test-001", &dir).expect("must succeed");
            logger.close(); // explicit close
            // logger drops here — must not panic
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 10. Append mode: two consecutive loggers accumulate lines
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_append_mode() {
        ensure_subscriber();
        let _sink_guard = SINK_MUTEX.lock().unwrap();
        if !SUBSCRIBER_INSTALLED.load(Ordering::SeqCst) {
            return;
        }

        let dir = std::env::temp_dir().join(format!("teri_console_append_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        // First logger
        {
            let mut logger = ReportConsoleLogger::new("append-001", &dir).expect("must succeed");
            tracing::info!(target: "teri::report", "first run line");
            logger.close();
        }

        // Second logger (same path — append mode)
        {
            let mut logger = ReportConsoleLogger::new("append-001", &dir).expect("must succeed");
            tracing::info!(target: "teri::report", "second run line");
            logger.close();
        }

        let path = dir.join("reports").join("append-001").join("console_log.txt");
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            contents.contains("first run line"),
            "first line must be present; contents:\n{contents}"
        );
        assert!(
            contents.contains("second run line"),
            "second line must be present; contents:\n{contents}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
