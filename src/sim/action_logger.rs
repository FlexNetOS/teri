//! Action logger — Rust port of MiroFish `backend/scripts/action_logger.py` (305 lines).
//!
//! Symbols ported: S-070..S-097 (28 symbols).
//!
//! # Architecture decisions
//!
//! ## Timestamp (S-070 / shared concern)
//! All JSON entries use `datetime.now().isoformat()` — local naive time, µs omitted when zero.
//! We reuse `crate::models::project::python_isoformat_local()` (which was made `pub(crate)`)
//! to avoid duplicating the logic. Do NOT use Utc (task.rs uses Utc for a different Python call).
//!
//! ## Python `logging` two-sink port (S-078..S-087 `SimulationLogManager._setup_main_logger`)
//! Python uses the `logging` module with:
//!   - FILE handler: mode='w' (truncate), UTF-8, format `"%Y-%m-%d %H:%M:%S - LEVELNAME - message"`,
//!     level INFO.
//!   - CONSOLE handler (stderr): format `"[%H:%M:%S] message"`, level INFO.
//!   - Logger level INFO → DEBUG messages are SUPPRESSED (not written to either sink).
//!   - `propagate=False` — isolated from the root logger.
//!
//! We port this as a direct struct (`MainLogger`) that holds an `std::fs::File` (truncated at
//! setup) + writes formatted lines directly. We do NOT route through teri's global `tracing`
//! subscriber — that would diverge the format and mix this per-sim log into global structured logs.
//! File line: `"%Y-%m-%d %H:%M:%S - {LEVEL} - {message}\n"` (local time, seconds precision).
//! Console: `"[%H:%M:%S] {message}\n"` to stderr (local time, seconds precision).
//!
//! ## `log(message, level)` getattr fallback (S-082)
//! Python: `getattr(logger, level.lower(), logger.info)(message)`.
//! `getattr` resolves real `logging.Logger` method names before falling back to `logger.info`.
//! Rust match (after `level.to_lowercase()`):
//!   - `"info"` → INFO
//!   - `"warning" | "warn"` → WARNING  (`"warn"` is a real alias in Python's logging module)
//!   - `"error" | "exception"` → ERROR  (`"exception"` logs at ERROR level)
//!   - `"critical" | "fatal"` → CRITICAL (`"fatal"` is a real alias for `critical`)
//!   - `"debug"` → DEBUG (suppressed — below INFO)
//!   - any other string → INFO (getattr fallback: unknown name → `logger.info`)
//!
//! ### `"exception"` trailing traceback line [`≠`]
//! Python's `logger.exception()` logs at ERROR level AND appends the formatted `sys.exc_info()`.
//! With no active exception it prints a second line `NoneType: None`; inside a real `except`
//! block it prints the actual traceback. This depends on Python's ambient exception state
//! (`sys.exc_info()`), which has no equivalent in Rust — there is no ambient "current exception."
//! We port the contractual part (exception → ERROR level + the message). The trailing exc-info
//! line is an intentional divergence [`≠`]: genuinely inexpressible (Python `sys.exc_info()`
//! ambient state). No MiroFish caller passes `"exception"` as the level string; the verifier
//! confirmed this. The observable LEVEL output IS ported faithfully.
//!
//! ## `get_twitter_logger` / `get_reddit_logger` lazy-init (S-080..S-081)
//! Python: `if self.twitter_logger is None: self.twitter_logger = PlatformActionLogger(...)`.
//! Rust: `Option<PlatformActionLogger>` on `SimulationLogManager`; `&mut self` access is fine
//! because both callers in the Python source hold a mutable manager reference.
//!
//! ## Global singleton S-096/S-097 (`_global_logger` + `get_logger`)
//! Python uses a module-level `Optional[ActionLogger]` that can be (re)set by passing `log_path`.
//! Rust: `OnceLock<Mutex<Option<ActionLogger>>>` for the container; the inner `Option` allows
//! `get_logger(Some(path))` to replace the current instance. This matches task.rs's `TaskManager`
//! singleton pattern.
//!
//! ## `ActionLogger._ensure_dir` (S-088)
//! Python: `log_dir = os.path.dirname(log_path); if log_dir: os.makedirs(log_dir, exist_ok=True)`.
//! If `log_path` is a bare filename (e.g. `"actions.jsonl"`), `dirname` returns `""` and the
//! `if log_dir:` guard skips `makedirs`. We replicate: only create_dir_all when the parent
//! path is non-empty.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::Local;
use serde_json::Value;

use crate::models::project::python_isoformat_local;

// ---------------------------------------------------------------------------
// PlatformActionLogger  (S-070..S-077)
// ---------------------------------------------------------------------------

/// Per-platform JSONL action log writer.
///
/// Writes to `{base_dir}/{platform}/actions.jsonl` in APPEND mode, one JSON object per line.
/// Non-ASCII characters are written raw (serde_json default, matching `ensure_ascii=False`).
pub struct PlatformActionLogger {
    /// Platform name stored in the struct; used by log_simulation_start/end entries.
    pub platform: String,
    /// Absolute path to the JSONL file (`{base_dir}/{platform}/actions.jsonl`).
    pub log_path: PathBuf,
}

impl PlatformActionLogger {
    /// S-070 `__init__(platform, base_dir)`.
    ///
    /// Sets `log_dir = {base_dir}/{platform}`, `log_path = {log_dir}/actions.jsonl`,
    /// then creates the directory (= Python's `os.makedirs(exist_ok=True)`).
    pub fn new(platform: &str, base_dir: &Path) -> std::io::Result<Self> {
        let log_dir = base_dir.join(platform);
        std::fs::create_dir_all(&log_dir)?;
        let log_path = log_dir.join("actions.jsonl");
        Ok(Self { platform: platform.to_string(), log_path })
    }

    /// Append a JSON line to the JSONL file.
    fn append_line(&self, entry: &Value) -> std::io::Result<()> {
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut file = OpenOptions::new().create(true).append(true).open(&self.log_path)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    /// S-071 `log_action` — 8-key entry: round, timestamp, agent_id, agent_name,
    /// action_type, action_args (or `{}` when None), result (null when None), success.
    ///
    /// `success` defaults to `true` in Python; callers may pass `false` explicitly.
    #[allow(clippy::too_many_arguments)]
    pub fn log_action(
        &self,
        round_num: i64,
        agent_id: i64,
        agent_name: &str,
        action_type: &str,
        action_args: Option<&Value>,
        result: Option<&str>,
        success: bool,
    ) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "round": round_num,
            "timestamp": python_isoformat_local(),
            "agent_id": agent_id,
            "agent_name": agent_name,
            "action_type": action_type,
            "action_args": action_args.cloned().unwrap_or_else(|| serde_json::json!({})),
            "result": result,
            "success": success,
        });
        self.append_line(&entry)
    }

    /// S-072 `log_round_start` — 4-key entry: round, timestamp, event_type:"round_start",
    /// simulated_hour.
    pub fn log_round_start(&self, round_num: i64, simulated_hour: i64) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "round": round_num,
            "timestamp": python_isoformat_local(),
            "event_type": "round_start",
            "simulated_hour": simulated_hour,
        });
        self.append_line(&entry)
    }

    /// S-073 `log_round_end` — 4-key entry: round, timestamp, event_type:"round_end",
    /// actions_count.
    pub fn log_round_end(&self, round_num: i64, actions_count: i64) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "round": round_num,
            "timestamp": python_isoformat_local(),
            "event_type": "round_end",
            "actions_count": actions_count,
        });
        self.append_line(&entry)
    }

    /// S-074 `log_simulation_start` — 5-key entry.
    ///
    /// Faithfully replicates the Python `.get(...).get(...)` default-72 chain:
    /// `total_rounds = config.get("time_config", {}).get("total_simulation_hours", 72) * 2`.
    /// `agents_count = len(config.get("agent_configs", []))` — 0 when key absent.
    pub fn log_simulation_start(&self, config: &Value) -> std::io::Result<()> {
        let total_simulation_hours = config
            .get("time_config")
            .and_then(|tc| tc.get("total_simulation_hours"))
            .and_then(|v| v.as_i64())
            .unwrap_or(72);
        let total_rounds = total_simulation_hours * 2;

        let agents_count = config
            .get("agent_configs")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len() as i64)
            .unwrap_or(0);

        let entry = serde_json::json!({
            "timestamp": python_isoformat_local(),
            "event_type": "simulation_start",
            "platform": self.platform,
            "total_rounds": total_rounds,
            "agents_count": agents_count,
        });
        self.append_line(&entry)
    }

    /// S-075 `log_simulation_end` — 5-key entry.
    pub fn log_simulation_end(&self, total_rounds: i64, total_actions: i64) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "timestamp": python_isoformat_local(),
            "event_type": "simulation_end",
            "platform": self.platform,
            "total_rounds": total_rounds,
            "total_actions": total_actions,
        });
        self.append_line(&entry)
    }
}

// ---------------------------------------------------------------------------
// MainLogger — the Python `logging.Logger` two-sink port  (S-079)
// ---------------------------------------------------------------------------

/// Internal struct that drives the two-sink main logger for `SimulationLogManager`.
///
/// The FILE is opened with truncation at construction (mode='w' equivalent).
/// All writes go directly to the file + stderr; no global tracing subscriber involvement.
struct MainLogger {
    file: Mutex<File>,
}

/// Log level for the main logger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogLevel {
    Info,
    Warning,
    Error,
    /// Critical maps Python's `logging.CRITICAL` (and its `fatal` alias).
    /// It is AT or above INFO and is NOT suppressed.
    Critical,
    /// Debug is BELOW INFO: suppressed from both sinks (faithful to logger.setLevel(INFO)).
    Debug,
}

impl LogLevel {
    fn as_str(self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warning => "WARNING",
            LogLevel::Error => "ERROR",
            LogLevel::Critical => "CRITICAL",
            LogLevel::Debug => "DEBUG",
        }
    }

    fn is_at_or_above_info(self) -> bool {
        !matches!(self, LogLevel::Debug)
    }
}

impl MainLogger {
    /// Open/truncate `log_path` and return a `MainLogger`.
    /// Mode='w' equivalent: `File::create` truncates if the file already exists.
    fn new(log_path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = log_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(log_path)?;
        Ok(Self { file: Mutex::new(file) })
    }

    /// Write a log line at the given level.
    ///
    /// File format: `"%Y-%m-%d %H:%M:%S - {LEVEL} - {message}\n"` (local, seconds precision).
    /// Console (stderr): `"[%H:%M:%S] {message}\n"` (local, seconds precision).
    ///
    /// Debug is suppressed (not written to either sink — logger.setLevel(INFO)).
    fn log(&self, level: LogLevel, message: &str) {
        if !level.is_at_or_above_info() {
            // DEBUG < INFO → suppressed from both sinks.
            return;
        }

        let now = Local::now().naive_local();
        let file_ts = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let console_ts = now.format("%H:%M:%S").to_string();

        let file_line = format!("{} - {} - {}\n", file_ts, level.as_str(), message);
        let console_line = format!("[{}] {}\n", console_ts, message);

        // Write to file (hold the lock for the duration).
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(file_line.as_bytes());
        }

        // Write to stderr.
        let _ = std::io::stderr().write_all(console_line.as_bytes());
    }
}

// ---------------------------------------------------------------------------
// SimulationLogManager  (S-078..S-087)
// ---------------------------------------------------------------------------

/// Simulation log manager.
///
/// Aggregates a main text log (`simulation.log`) and lazy per-platform action loggers.
/// The main log implements the Python `logging` two-sink pattern as a direct file writer.
pub struct SimulationLogManager {
    simulation_dir: PathBuf,
    twitter_logger: Option<PlatformActionLogger>,
    reddit_logger: Option<PlatformActionLogger>,
    main_logger: MainLogger,
}

impl SimulationLogManager {
    /// S-078 `__init__(simulation_dir)`.
    ///
    /// Sets up the main logger (truncates `{simulation_dir}/simulation.log`).
    /// Twitter/Reddit loggers are lazy; they start as `None`.
    pub fn new(simulation_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(simulation_dir)?;
        let log_path = simulation_dir.join("simulation.log");
        let main_logger = MainLogger::new(&log_path)?;
        Ok(Self {
            simulation_dir: simulation_dir.to_path_buf(),
            twitter_logger: None,
            reddit_logger: None,
            main_logger,
        })
    }

    /// S-080 `get_twitter_logger` — lazy-init, returns same instance on repeat calls.
    pub fn get_twitter_logger(&mut self) -> std::io::Result<&mut PlatformActionLogger> {
        if self.twitter_logger.is_none() {
            self.twitter_logger =
                Some(PlatformActionLogger::new("twitter", &self.simulation_dir)?);
        }
        Ok(self.twitter_logger.as_mut().unwrap())
    }

    /// S-081 `get_reddit_logger` — lazy-init, returns same instance on repeat calls.
    pub fn get_reddit_logger(&mut self) -> std::io::Result<&mut PlatformActionLogger> {
        if self.reddit_logger.is_none() {
            self.reddit_logger =
                Some(PlatformActionLogger::new("reddit", &self.simulation_dir)?);
        }
        Ok(self.reddit_logger.as_mut().unwrap())
    }

    /// S-082 `log(message, level="info")` — dispatch with getattr fallback.
    ///
    /// Python: `getattr(logger, level.lower(), logger.info)(message)`.
    /// Resolved level strings (after `lower()`):
    ///   - "info"              → INFO
    ///   - "warning" | "warn" → WARNING  ("warn" is a real logging alias)
    ///   - "error" | "exception" → ERROR  ("exception" logs at ERROR; see module doc for the [≠]
    ///     note on the trailing traceback line — depends on Python ambient exc state, no Rust equiv)
    ///   - "critical" | "fatal"  → CRITICAL ("fatal" is a real logging alias)
    ///   - "debug"            → DEBUG (suppressed — below INFO)
    ///   - any other string   → INFO (getattr default: unknown attr → logger.info)
    pub fn log(&self, message: &str, level: &str) {
        let lvl = match level.to_lowercase().as_str() {
            "info" => LogLevel::Info,
            "warning" | "warn" => LogLevel::Warning,
            // "exception" dispatches logger.exception() which logs at ERROR level.
            // The trailing exc-info line (NoneType: None / traceback) is [≠] — see module doc.
            "error" | "exception" => LogLevel::Error,
            // "fatal" is a real alias for logging.critical in Python's logging module.
            "critical" | "fatal" => LogLevel::Critical,
            "debug" => LogLevel::Debug,
            // Genuinely unknown level string → getattr returns logger.info as the default callable.
            _ => LogLevel::Info,
        };
        self.main_logger.log(lvl, message);
    }

    /// S-083 `info(message)` — thin wrapper.
    pub fn info(&self, message: &str) {
        self.log(message, "info");
    }

    /// S-084 `warning(message)` — thin wrapper.
    pub fn warning(&self, message: &str) {
        self.log(message, "warning");
    }

    /// S-085 `error(message)` — thin wrapper.
    pub fn error(&self, message: &str) {
        self.log(message, "error");
    }

    /// S-086 `debug(message)` — thin wrapper.
    ///
    /// Debug is BELOW INFO → suppressed from both file and console sinks (faithful to
    /// Python's `logger.setLevel(logging.INFO)` which filters `logger.debug()` calls).
    pub fn debug(&self, message: &str) {
        self.log(message, "debug");
    }
}

// ---------------------------------------------------------------------------
// ActionLogger — legacy single-file logger  (S-088..S-095)
// ---------------------------------------------------------------------------

/// Legacy single-file action logger (compatibility interface).
///
/// Unlike `PlatformActionLogger`, this takes an explicit `platform` parameter on
/// every method call and includes `platform` in each JSON entry.
///
/// Recommendation: prefer `SimulationLogManager` for new code.
pub struct ActionLogger {
    pub log_path: PathBuf,
}

impl ActionLogger {
    /// S-088 `__init__(log_path)`.
    ///
    /// Faithfully replicates Python's `_ensure_dir`:
    ///   `log_dir = os.path.dirname(log_path); if log_dir: os.makedirs(log_dir, exist_ok=True)`.
    /// A bare filename (no directory component) → dirname is `""` → skips makedirs.
    pub fn new(log_path: &str) -> std::io::Result<Self> {
        let path = PathBuf::from(log_path);
        // Python: log_dir = os.path.dirname(log_path); if log_dir: makedirs(...)
        // Path::parent() of "actions.jsonl" is Some("") / returns "" — skip create.
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self { log_path: path })
    }

    /// Append a JSON line to the log file.
    fn append_line(&self, entry: &Value) -> std::io::Result<()> {
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut file = OpenOptions::new().create(true).append(true).open(&self.log_path)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    /// S-089 `log_action` — 9-key entry including `platform`.
    ///
    /// Keys: round, timestamp, platform, agent_id, agent_name, action_type,
    ///       action_args (or `{}` when None), result (null when None), success.
    #[allow(clippy::too_many_arguments)]
    pub fn log_action(
        &self,
        round_num: i64,
        platform: &str,
        agent_id: i64,
        agent_name: &str,
        action_type: &str,
        action_args: Option<&Value>,
        result: Option<&str>,
        success: bool,
    ) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "round": round_num,
            "timestamp": python_isoformat_local(),
            "platform": platform,
            "agent_id": agent_id,
            "agent_name": agent_name,
            "action_type": action_type,
            "action_args": action_args.cloned().unwrap_or_else(|| serde_json::json!({})),
            "result": result,
            "success": success,
        });
        self.append_line(&entry)
    }

    /// S-090 `log_round_start` — 5-key entry including `platform`.
    pub fn log_round_start(
        &self,
        round_num: i64,
        simulated_hour: i64,
        platform: &str,
    ) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "round": round_num,
            "timestamp": python_isoformat_local(),
            "platform": platform,
            "event_type": "round_start",
            "simulated_hour": simulated_hour,
        });
        self.append_line(&entry)
    }

    /// S-091 `log_round_end` — 5-key entry including `platform`.
    pub fn log_round_end(
        &self,
        round_num: i64,
        actions_count: i64,
        platform: &str,
    ) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "round": round_num,
            "timestamp": python_isoformat_local(),
            "platform": platform,
            "event_type": "round_end",
            "actions_count": actions_count,
        });
        self.append_line(&entry)
    }

    /// S-092 `log_simulation_start(platform, config)` — 5-key entry including `platform`.
    ///
    /// Same `.get(...).get(...)` default-72 chain as `PlatformActionLogger::log_simulation_start`.
    pub fn log_simulation_start(&self, platform: &str, config: &Value) -> std::io::Result<()> {
        let total_simulation_hours = config
            .get("time_config")
            .and_then(|tc| tc.get("total_simulation_hours"))
            .and_then(|v| v.as_i64())
            .unwrap_or(72);
        let total_rounds = total_simulation_hours * 2;

        let agents_count = config
            .get("agent_configs")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len() as i64)
            .unwrap_or(0);

        let entry = serde_json::json!({
            "timestamp": python_isoformat_local(),
            "platform": platform,
            "event_type": "simulation_start",
            "total_rounds": total_rounds,
            "agents_count": agents_count,
        });
        self.append_line(&entry)
    }

    /// S-093 `log_simulation_end(platform, total_rounds, total_actions)` — 5-key entry.
    pub fn log_simulation_end(
        &self,
        platform: &str,
        total_rounds: i64,
        total_actions: i64,
    ) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "timestamp": python_isoformat_local(),
            "platform": platform,
            "event_type": "simulation_end",
            "total_rounds": total_rounds,
            "total_actions": total_actions,
        });
        self.append_line(&entry)
    }
}

// ---------------------------------------------------------------------------
// Global singleton  (S-096..S-097)
// ---------------------------------------------------------------------------

/// Module-global logger instance — mirrors Python's `_global_logger: Optional[ActionLogger]`.
///
/// The `OnceLock` initialises the `Mutex<Option<...>>` container once; the inner `Option`
/// allows `get_logger(Some(path))` to replace the stored instance at any time.
static GLOBAL_LOGGER: OnceLock<Mutex<Option<ActionLogger>>> = OnceLock::new();

fn global_logger_container() -> &'static Mutex<Option<ActionLogger>> {
    GLOBAL_LOGGER.get_or_init(|| Mutex::new(None))
}

/// S-097 `get_logger(log_path=None)` — get or initialise the global logger.
///
/// Faithful semantics:
/// - `Some(path)` → (re)create `ActionLogger(path)` and store it (always replaces).
/// - `None` + global already set → return existing.
/// - `None` + global is None → create default `ActionLogger("actions.jsonl")`.
///
/// Returns `std::io::Result<()>` for the write half; the actual `ActionLogger` is accessed
/// via `with_global_logger` below. (Rust cannot return a `&mut ActionLogger` from a global
/// `Mutex` without a guard lifetime — callers use the functional accessor pattern instead.)
///
/// The Python caller does `logger = get_logger(path); logger.log_action(...)`. Rust callers
/// use `get_logger(Some(path))?; with_global_logger(|l| l.log_action(...))`.
pub fn get_logger(log_path: Option<&str>) -> std::io::Result<()> {
    let container = global_logger_container();
    let mut guard = container.lock().unwrap();

    if let Some(path) = log_path {
        // Always replace with the new path (matching Python: `_global_logger = ActionLogger(path)`).
        *guard = Some(ActionLogger::new(path)?);
    } else if guard.is_none() {
        // No existing instance and no path → use default "actions.jsonl".
        *guard = Some(ActionLogger::new("actions.jsonl")?);
    }
    Ok(())
}

/// Call a closure with a mutable reference to the global `ActionLogger`.
///
/// Ensures the logger is initialised (with the default path) before calling `f`.
pub fn with_global_logger<F, R>(f: F) -> std::io::Result<R>
where
    F: FnOnce(&mut ActionLogger) -> std::io::Result<R>,
{
    // Ensure initialised.
    get_logger(None)?;
    let container = global_logger_container();
    let mut guard = container.lock().unwrap();
    let logger = guard.as_mut().unwrap();
    f(logger)
}

// ---------------------------------------------------------------------------
// Tests  (S-070..S-097)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    /// Create a unique test subdirectory under std::env::temp_dir().
    fn unique_test_dir(name: &str) -> PathBuf {
        let id = uuid::Uuid::new_v4().to_string();
        let dir = temp_dir().join(format!("teri_test_{name}_{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // -----------------------------------------------------------------------
    // S-071 PlatformActionLogger::log_action — exact 8-key JSONL line
    // -----------------------------------------------------------------------

    #[test]
    fn test_platform_log_action_8_keys() {
        let base = unique_test_dir("pal_action");
        let logger = PlatformActionLogger::new("twitter", &base).unwrap();
        logger.log_action(1, 42, "Alice", "CREATE_POST", None, None, true).unwrap();

        let content = std::fs::read_to_string(&logger.log_path).unwrap();
        let line = content.trim();
        let parsed: Value = serde_json::from_str(line).unwrap();
        let obj = parsed.as_object().unwrap();

        // Exact 8 keys.
        let keys: std::collections::HashSet<&str> =
            obj.keys().map(|s| s.as_str()).collect();
        let expected: std::collections::HashSet<&str> = [
            "round", "timestamp", "agent_id", "agent_name", "action_type",
            "action_args", "result", "success",
        ]
        .iter()
        .copied()
        .collect();
        assert_eq!(keys, expected, "log_action must have exactly 8 keys");

        assert_eq!(obj["round"], 1);
        assert_eq!(obj["agent_id"], 42);
        assert_eq!(obj["agent_name"], "Alice");
        assert_eq!(obj["action_type"], "CREATE_POST");
        // action_args None → {}
        assert_eq!(obj["action_args"], serde_json::json!({}));
        // result None → null
        assert!(obj["result"].is_null());
        // success default true
        assert_eq!(obj["success"], true);

        // Cleanup
        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // action_args Some(value) and result Some("ok")
    // -----------------------------------------------------------------------

    #[test]
    fn test_platform_log_action_with_args_and_result() {
        let base = unique_test_dir("pal_args");
        let logger = PlatformActionLogger::new("reddit", &base).unwrap();
        let args = serde_json::json!({"content": "hello"});
        logger
            .log_action(2, 7, "Bob", "COMMENT", Some(&args), Some("ok"), false)
            .unwrap();

        let content = std::fs::read_to_string(&logger.log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["action_args"], args);
        assert_eq!(parsed["result"], "ok");
        assert_eq!(parsed["success"], false);

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // Append mode: two writes → two lines
    // -----------------------------------------------------------------------

    #[test]
    fn test_platform_log_action_appends_not_overwrites() {
        let base = unique_test_dir("pal_append");
        let logger = PlatformActionLogger::new("twitter", &base).unwrap();
        logger.log_action(1, 1, "A", "T1", None, None, true).unwrap();
        logger.log_action(2, 2, "B", "T2", None, None, true).unwrap();

        let content = std::fs::read_to_string(&logger.log_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "two appended writes must produce two JSONL lines");

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // S-074 log_simulation_start: total_rounds default (72*2=144 when absent)
    // -----------------------------------------------------------------------

    #[test]
    fn test_platform_log_simulation_start_default_hours() {
        let base = unique_test_dir("pal_simstart_default");
        let logger = PlatformActionLogger::new("twitter", &base).unwrap();
        // No time_config in config → default 72 hours → total_rounds = 144
        let config = serde_json::json!({});
        logger.log_simulation_start(&config).unwrap();

        let content = std::fs::read_to_string(&logger.log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["total_rounds"], 144, "default 72h * 2 = 144 rounds");
        assert_eq!(parsed["agents_count"], 0, "absent agent_configs → 0 agents");
        assert_eq!(parsed["event_type"], "simulation_start");
        assert_eq!(parsed["platform"], "twitter");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_platform_log_simulation_start_custom_hours_and_agents() {
        let base = unique_test_dir("pal_simstart_custom");
        let logger = PlatformActionLogger::new("reddit", &base).unwrap();
        let config = serde_json::json!({
            "time_config": { "total_simulation_hours": 24 },
            "agent_configs": [{"id": 1}, {"id": 2}, {"id": 3}]
        });
        logger.log_simulation_start(&config).unwrap();

        let content = std::fs::read_to_string(&logger.log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["total_rounds"], 48, "24h * 2 = 48 rounds");
        assert_eq!(parsed["agents_count"], 3);

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // S-072/S-073 log_round_start / log_round_end keys
    // -----------------------------------------------------------------------

    #[test]
    fn test_platform_log_round_start_keys() {
        let base = unique_test_dir("pal_roundstart");
        let logger = PlatformActionLogger::new("twitter", &base).unwrap();
        logger.log_round_start(5, 10).unwrap();

        let content = std::fs::read_to_string(&logger.log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        let keys: std::collections::HashSet<&str> =
            obj.keys().map(|s| s.as_str()).collect();
        let expected: std::collections::HashSet<&str> =
            ["round", "timestamp", "event_type", "simulated_hour"].iter().copied().collect();
        assert_eq!(keys, expected);
        assert_eq!(obj["event_type"], "round_start");
        assert_eq!(obj["round"], 5);
        assert_eq!(obj["simulated_hour"], 10);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_platform_log_round_end_keys() {
        let base = unique_test_dir("pal_roundend");
        let logger = PlatformActionLogger::new("twitter", &base).unwrap();
        logger.log_round_end(5, 99).unwrap();

        let content = std::fs::read_to_string(&logger.log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        let keys: std::collections::HashSet<&str> =
            obj.keys().map(|s| s.as_str()).collect();
        let expected: std::collections::HashSet<&str> =
            ["round", "timestamp", "event_type", "actions_count"].iter().copied().collect();
        assert_eq!(keys, expected);
        assert_eq!(obj["event_type"], "round_end");
        assert_eq!(obj["actions_count"], 99);

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // S-075 log_simulation_end keys
    // -----------------------------------------------------------------------

    #[test]
    fn test_platform_log_simulation_end_keys() {
        let base = unique_test_dir("pal_simend");
        let logger = PlatformActionLogger::new("twitter", &base).unwrap();
        logger.log_simulation_end(144, 500).unwrap();

        let content = std::fs::read_to_string(&logger.log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        let keys: std::collections::HashSet<&str> =
            obj.keys().map(|s| s.as_str()).collect();
        let expected: std::collections::HashSet<&str> =
            ["timestamp", "event_type", "platform", "total_rounds", "total_actions"]
                .iter()
                .copied()
                .collect();
        assert_eq!(keys, expected);
        assert_eq!(obj["event_type"], "simulation_end");
        assert_eq!(obj["total_rounds"], 144);
        assert_eq!(obj["total_actions"], 500);
        assert_eq!(obj["platform"], "twitter");

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // Non-ASCII raw (no \u escapes)
    // -----------------------------------------------------------------------

    #[test]
    fn test_non_ascii_agent_name_written_raw() {
        let base = unique_test_dir("pal_nonascii");
        let logger = PlatformActionLogger::new("twitter", &base).unwrap();
        logger.log_action(1, 1, "中文用户", "CREATE_POST", None, None, true).unwrap();

        // Read bytes to check no \u escape appears
        let raw_bytes = std::fs::read(&logger.log_path).unwrap();
        let raw_str = std::str::from_utf8(&raw_bytes).unwrap();

        // Chinese chars appear verbatim in the UTF-8 stream.
        assert!(
            raw_str.contains("中文用户"),
            "Non-ASCII must be written raw, not escaped"
        );
        // No \u escape for the first char of 中 (U+4E2D → 中)
        assert!(
            !raw_str.contains("\\u4e2d"),
            "\\u escapes must NOT appear for non-ASCII chars"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // S-078..S-087 SimulationLogManager tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_sim_log_manager_get_twitter_logger_same_instance() {
        let base = unique_test_dir("slm_twitter");
        let mut mgr = SimulationLogManager::new(&base).unwrap();

        // Get twice; compare log_path to verify same instance.
        let path1 = mgr.get_twitter_logger().unwrap().log_path.clone();
        let path2 = mgr.get_twitter_logger().unwrap().log_path.clone();
        assert_eq!(path1, path2, "get_twitter_logger must return the same instance");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_sim_log_manager_get_reddit_logger_same_instance() {
        let base = unique_test_dir("slm_reddit");
        let mut mgr = SimulationLogManager::new(&base).unwrap();

        let path1 = mgr.get_reddit_logger().unwrap().log_path.clone();
        let path2 = mgr.get_reddit_logger().unwrap().log_path.clone();
        assert_eq!(path1, path2, "get_reddit_logger must return the same instance");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_sim_log_manager_twitter_and_reddit_are_separate() {
        let base = unique_test_dir("slm_sep");
        let mut mgr = SimulationLogManager::new(&base).unwrap();

        let tp = mgr.get_twitter_logger().unwrap().log_path.clone();
        let rp = mgr.get_reddit_logger().unwrap().log_path.clone();
        assert_ne!(tp, rp, "twitter and reddit loggers must be separate paths");
        assert!(tp.to_str().unwrap().contains("twitter"));
        assert!(rp.to_str().unwrap().contains("reddit"));

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // Main log file: simulation.log format check
    // -----------------------------------------------------------------------

    #[test]
    fn test_sim_log_manager_info_line_format() {
        let base = unique_test_dir("slm_format");
        let mgr = SimulationLogManager::new(&base).unwrap();
        mgr.info("hello world");

        let log_path = base.join("simulation.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line = content.lines().next().unwrap();
        // Must match "YYYY-MM-DD HH:MM:SS - INFO - hello world"
        assert!(
            line.contains(" - INFO - hello world"),
            "info line must contain ' - INFO - hello world', got: {line:?}"
        );
        // Timestamp part: YYYY-MM-DD HH:MM:SS (no microseconds)
        let ts_part = &line[..19];
        assert!(
            ts_part.chars().all(|c| c.is_ascii_digit() || c == '-' || c == ' ' || c == ':'),
            "timestamp part must be YYYY-MM-DD HH:MM:SS, got: {ts_part:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_sim_log_manager_warning_line_format() {
        let base = unique_test_dir("slm_warn");
        let mgr = SimulationLogManager::new(&base).unwrap();
        mgr.warning("something wrong");

        let log_path = base.join("simulation.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line = content.lines().next().unwrap();
        assert!(
            line.contains(" - WARNING - something wrong"),
            "warning line must have WARNING level, got: {line:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_sim_log_manager_error_line_format() {
        let base = unique_test_dir("slm_err");
        let mgr = SimulationLogManager::new(&base).unwrap();
        mgr.error("critical failure");

        let log_path = base.join("simulation.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line = content.lines().next().unwrap();
        assert!(
            line.contains(" - ERROR - critical failure"),
            "error line must have ERROR level, got: {line:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // Debug suppressed (below INFO)
    // -----------------------------------------------------------------------

    #[test]
    fn test_sim_log_manager_debug_suppressed() {
        let base = unique_test_dir("slm_debug");
        let mgr = SimulationLogManager::new(&base).unwrap();
        mgr.debug("this should not appear");

        let log_path = base.join("simulation.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            content.is_empty(),
            "debug message must be suppressed (not written to file), got: {content:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // log() with bogus level → falls back to INFO
    // -----------------------------------------------------------------------

    #[test]
    fn test_sim_log_manager_log_bogus_level_falls_back_to_info() {
        let base = unique_test_dir("slm_bogus");
        let mgr = SimulationLogManager::new(&base).unwrap();
        mgr.log("test message", "unknown_level_xyz");

        let log_path = base.join("simulation.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line = content.lines().next().unwrap();
        // Unknown level → INFO
        assert!(
            line.contains(" - INFO - test message"),
            "bogus level must fall back to INFO, got: {line:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // S-082 regression: level-dispatch aliases (critical / warn / exception / unknown)
    // -----------------------------------------------------------------------

    /// "critical" must render CRITICAL (not INFO) — Python logging.Logger has a real critical()
    /// method that maps to the CRITICAL level (50), above ERROR (40) and INFO (20).
    #[test]
    fn test_sim_log_manager_log_critical_alias() {
        let base = unique_test_dir("slm_critical");
        let mgr = SimulationLogManager::new(&base).unwrap();
        mgr.log("crit-message", "critical");

        let log_path = base.join("simulation.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line = content.lines().next().unwrap();
        assert!(
            line.contains(" - CRITICAL - crit-message"),
            "\"critical\" must dispatch to CRITICAL level, got: {line:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    /// "fatal" is a real alias for critical in Python's logging module (logging.fatal = logging.critical).
    #[test]
    fn test_sim_log_manager_log_fatal_alias() {
        let base = unique_test_dir("slm_fatal");
        let mgr = SimulationLogManager::new(&base).unwrap();
        mgr.log("fatal-message", "fatal");

        let log_path = base.join("simulation.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line = content.lines().next().unwrap();
        assert!(
            line.contains(" - CRITICAL - fatal-message"),
            "\"fatal\" must dispatch to CRITICAL level (Python alias), got: {line:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    /// "warn" is a real alias for warning in Python's logging module (logging.warn = logging.warning).
    #[test]
    fn test_sim_log_manager_log_warn_alias() {
        let base = unique_test_dir("slm_warn_alias");
        let mgr = SimulationLogManager::new(&base).unwrap();
        mgr.log("warn-message", "warn");

        let log_path = base.join("simulation.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line = content.lines().next().unwrap();
        assert!(
            line.contains(" - WARNING - warn-message"),
            "\"warn\" must dispatch to WARNING level (Python alias), got: {line:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    /// "exception" dispatches logger.exception() which logs at ERROR level.
    /// The trailing exc-info traceback line is [≠] (depends on Python sys.exc_info() ambient
    /// state; no Rust equivalent) — we verify only the ERROR level dispatch here.
    #[test]
    fn test_sim_log_manager_log_exception_dispatches_error() {
        let base = unique_test_dir("slm_exception");
        let mgr = SimulationLogManager::new(&base).unwrap();
        mgr.log("exc-message", "exception");

        let log_path = base.join("simulation.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line = content.lines().next().unwrap();
        assert!(
            line.contains(" - ERROR - exc-message"),
            "\"exception\" must dispatch to ERROR level, got: {line:?}"
        );
        // [≠] There is no second "NoneType: None" line — Python sys.exc_info() ambient state
        // is genuinely inexpressible in Rust. Only one log line is produced.
        assert_eq!(
            content.lines().count(),
            1,
            "only one log line expected (no trailing exc-info; that is [≠])"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    /// Genuinely unknown level string must still fall back to INFO (getattr default).
    #[test]
    fn test_sim_log_manager_log_unknown_level_falls_back_to_info() {
        let base = unique_test_dir("slm_unknown");
        let mgr = SimulationLogManager::new(&base).unwrap();
        mgr.log("unknown-message", "bogus_unknown");

        let log_path = base.join("simulation.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let line = content.lines().next().unwrap();
        assert!(
            line.contains(" - INFO - unknown-message"),
            "unknown level must fall back to INFO, got: {line:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // Main logger mode='w': re-setup truncates
    // -----------------------------------------------------------------------

    #[test]
    fn test_sim_log_manager_setup_truncates_existing_log() {
        let base = unique_test_dir("slm_truncate");
        let log_path = base.join("simulation.log");

        // First manager writes something.
        {
            let mgr = SimulationLogManager::new(&base).unwrap();
            mgr.info("first session line");
        }
        let after_first = std::fs::read_to_string(&log_path).unwrap();
        assert!(!after_first.is_empty(), "first session must write content");

        // Second manager construction truncates (mode='w').
        {
            let mgr2 = SimulationLogManager::new(&base).unwrap();
            mgr2.info("second session line");
        }
        let after_second = std::fs::read_to_string(&log_path).unwrap();
        // Must not contain the first session's line (truncated).
        assert!(
            !after_second.contains("first session line"),
            "simulation.log must be truncated on new SimulationLogManager setup"
        );
        assert!(
            after_second.contains("second session line"),
            "second session line must be present"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // S-089 ActionLogger legacy: entry includes `platform`
    // -----------------------------------------------------------------------

    #[test]
    fn test_legacy_log_action_includes_platform() {
        let base = unique_test_dir("legacy_action");
        let log_path = base.join("actions.jsonl");
        let logger = ActionLogger::new(log_path.to_str().unwrap()).unwrap();

        logger.log_action(1, "twitter", 10, "Alice", "REPOST", None, None, true).unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        // Must have 9 keys including platform.
        let keys: std::collections::HashSet<&str> =
            obj.keys().map(|s| s.as_str()).collect();
        let expected: std::collections::HashSet<&str> = [
            "round", "timestamp", "platform", "agent_id", "agent_name",
            "action_type", "action_args", "result", "success",
        ]
        .iter()
        .copied()
        .collect();
        assert_eq!(keys, expected, "legacy log_action must have 9 keys with platform");
        assert_eq!(obj["platform"], "twitter");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_legacy_log_round_start_includes_platform() {
        let base = unique_test_dir("legacy_roundstart");
        let log_path = base.join("actions.jsonl");
        let logger = ActionLogger::new(log_path.to_str().unwrap()).unwrap();
        logger.log_round_start(3, 6, "reddit").unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        let keys: std::collections::HashSet<&str> =
            obj.keys().map(|s| s.as_str()).collect();
        let expected: std::collections::HashSet<&str> =
            ["round", "timestamp", "platform", "event_type", "simulated_hour"]
                .iter()
                .copied()
                .collect();
        assert_eq!(keys, expected);
        assert_eq!(obj["platform"], "reddit");
        assert_eq!(obj["event_type"], "round_start");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_legacy_log_simulation_start_keys() {
        let base = unique_test_dir("legacy_simstart");
        let log_path = base.join("actions.jsonl");
        let logger = ActionLogger::new(log_path.to_str().unwrap()).unwrap();
        let config = serde_json::json!({
            "time_config": { "total_simulation_hours": 48 },
            "agent_configs": [{"id": 1}]
        });
        logger.log_simulation_start("twitter", &config).unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        let keys: std::collections::HashSet<&str> =
            obj.keys().map(|s| s.as_str()).collect();
        let expected: std::collections::HashSet<&str> =
            ["timestamp", "platform", "event_type", "total_rounds", "agents_count"]
                .iter()
                .copied()
                .collect();
        assert_eq!(keys, expected, "legacy sim_start must have 5 keys with platform");
        assert_eq!(obj["total_rounds"], 96, "48h * 2 = 96");
        assert_eq!(obj["agents_count"], 1);

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // S-088 _ensure_dir: bare filename (no dir) must not error
    // -----------------------------------------------------------------------

    #[test]
    fn test_legacy_ensure_dir_bare_filename_no_error() {
        // "actions.jsonl" has no directory component → dirname="" → skip makedirs.
        // This must not panic or return Err.
        // We cannot actually write to "actions.jsonl" in cwd from a test, so just
        // verify construction succeeds without touching the filesystem (the dir create
        // is the only OS call in new() for a bare path).
        let result = ActionLogger::new("actions.jsonl");
        // Should succeed (no dir to create = no Err).
        assert!(result.is_ok(), "bare filename must not error in new()");
    }

    // -----------------------------------------------------------------------
    // S-097 get_logger: with path resets global; without returns existing/default
    // -----------------------------------------------------------------------

    // Process-wide serialization mutex for tests that touch the GLOBAL_LOGGER singleton.
    // Mirrors the proven ENV_LOCK pattern from config.rs (U-001).
    // All three tests below acquire this lock first (poison-tolerant) and reset the
    // inner Option to None so each test starts from a known blank state regardless of
    // execution order.
    #[cfg(test)]
    static GLOBAL_LOGGER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_get_logger_with_path_resets_global() {
        let _guard = GLOBAL_LOGGER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Reset global to None so this test is order-independent.
        *global_logger_container().lock().unwrap_or_else(|e| e.into_inner()) = None;

        let base = unique_test_dir("global_reset");
        let path_a = base.join("a.jsonl");
        let path_b = base.join("b.jsonl");

        // Set to path_a.
        get_logger(Some(path_a.to_str().unwrap())).unwrap();
        {
            let guard = global_logger_container().lock().unwrap_or_else(|e| e.into_inner());
            let logger = guard.as_ref().unwrap();
            assert_eq!(logger.log_path, path_a, "global must be path_a after first set");
        }

        // Reset to path_b.
        get_logger(Some(path_b.to_str().unwrap())).unwrap();
        {
            let guard = global_logger_container().lock().unwrap_or_else(|e| e.into_inner());
            let logger = guard.as_ref().unwrap();
            assert_eq!(logger.log_path, path_b, "global must be path_b after reset");
        }

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_get_logger_without_path_returns_existing() {
        let _guard = GLOBAL_LOGGER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Reset global to None so this test is order-independent.
        *global_logger_container().lock().unwrap_or_else(|e| e.into_inner()) = None;

        let base = unique_test_dir("global_existing");
        let path_a = base.join("existing.jsonl");

        // Set to a known path.
        get_logger(Some(path_a.to_str().unwrap())).unwrap();

        // Call without path — must NOT replace.
        get_logger(None).unwrap();
        {
            let guard = global_logger_container().lock().unwrap_or_else(|e| e.into_inner());
            let logger = guard.as_ref().unwrap();
            assert_eq!(
                logger.log_path, path_a,
                "get_logger(None) must not replace existing global"
            );
        }

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // with_global_logger functional accessor
    // -----------------------------------------------------------------------

    #[test]
    fn test_with_global_logger_writes_entry() {
        let _guard = GLOBAL_LOGGER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Reset global to None so this test is order-independent.
        *global_logger_container().lock().unwrap_or_else(|e| e.into_inner()) = None;

        let base = unique_test_dir("global_write");
        let log_path = base.join("log.jsonl");

        // Set the global to our test path.
        get_logger(Some(log_path.to_str().unwrap())).unwrap();

        // Use the functional accessor.
        with_global_logger(|logger| {
            logger.log_action(10, "twitter", 5, "GlobalUser", "FOLLOW", None, None, true)
        })
        .unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["agent_name"], "GlobalUser");
        assert_eq!(parsed["platform"], "twitter");

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // Legacy log_round_end and log_simulation_end keys
    // -----------------------------------------------------------------------

    #[test]
    fn test_legacy_log_round_end_includes_platform() {
        let base = unique_test_dir("legacy_roundend");
        let log_path = base.join("actions.jsonl");
        let logger = ActionLogger::new(log_path.to_str().unwrap()).unwrap();
        logger.log_round_end(4, 17, "twitter").unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["platform"], "twitter");
        assert_eq!(parsed["event_type"], "round_end");
        assert_eq!(parsed["actions_count"], 17);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_legacy_log_simulation_end_keys() {
        let base = unique_test_dir("legacy_simend");
        let log_path = base.join("actions.jsonl");
        let logger = ActionLogger::new(log_path.to_str().unwrap()).unwrap();
        logger.log_simulation_end("reddit", 144, 3200).unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        let obj = parsed.as_object().unwrap();

        let keys: std::collections::HashSet<&str> =
            obj.keys().map(|s| s.as_str()).collect();
        let expected: std::collections::HashSet<&str> =
            ["timestamp", "platform", "event_type", "total_rounds", "total_actions"]
                .iter()
                .copied()
                .collect();
        assert_eq!(keys, expected);
        assert_eq!(obj["platform"], "reddit");
        assert_eq!(obj["event_type"], "simulation_end");
        assert_eq!(obj["total_rounds"], 144);
        assert_eq!(obj["total_actions"], 3200);

        std::fs::remove_dir_all(&base).ok();
    }

    // -----------------------------------------------------------------------
    // Timestamp: must not contain microseconds when µs == 0
    // (structural check — we can't force µs to be exactly 0, so just validate format)
    // -----------------------------------------------------------------------

    #[test]
    fn test_timestamp_format_in_jsonl() {
        let base = unique_test_dir("ts_format");
        let logger = PlatformActionLogger::new("twitter", &base).unwrap();
        logger.log_action(1, 1, "T", "X", None, None, true).unwrap();

        let content = std::fs::read_to_string(&logger.log_path).unwrap();
        let parsed: Value = serde_json::from_str(content.trim()).unwrap();
        let ts = parsed["timestamp"].as_str().unwrap();

        // Must start with YYYY-MM-DDTHH:MM:SS (ISO 8601 local naive, no tz suffix).
        assert!(ts.len() >= 19, "timestamp must be at least 19 chars, got {ts:?}");
        assert_eq!(&ts[4..5], "-", "must have dash at pos 4");
        assert_eq!(&ts[7..8], "-", "must have dash at pos 7");
        assert_eq!(&ts[10..11], "T", "must have T separator at pos 10");
        // Must NOT end with Z or +offset (naive datetime, no tz).
        assert!(
            !ts.ends_with('Z') && !ts.contains('+'),
            "timestamp must be naive (no timezone suffix), got {ts:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }
}
