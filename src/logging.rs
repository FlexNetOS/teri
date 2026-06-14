//! Logging initialisation for teri.
//!
//! Matches MiroFish `backend/app/utils/logger.py` — all behaviours preserved:
//!
//! * **Rotating file appender** (opt-in): when `TERI_LOG_DIR` is set, a
//!   size-based rotating file is opened in that directory.  Rotation matches
//!   `RotatingFileHandler(maxBytes=10*1024*1024, backupCount=5)`:
//!   - `ContentLimit::Bytes(10 * 1024 * 1024)` — rotate at 10 MB
//!   - `AppendCount::new(5)`                    — keep 5 backup files
//!   - File-appender logs at DEBUG+ (MiroFish `file_handler.setLevel(DEBUG)`)
//!
//! * **Console layer**: always present, filtered by the caller-supplied `level`
//!   string (or `RUST_LOG`, or falls back to "info").
//!
//! * **Default = console-only**: when `TERI_LOG_DIR` is not set, behaviour is
//!   identical to the original `init_logging` (no files, no new deps loaded).
//!
//! ## Idiomatic-mapping notes (no dropped behaviours)
//!
//! | MiroFish Python                          | Rust / teri equivalent                           |
//! |------------------------------------------|--------------------------------------------------|
//! | `setup_logger(name)` / `get_logger(name)`| `tracing::info!(target: "name", …)` — tracing   |
//! |                                          | is process-global; named loggers are tracing     |
//! |                                          | targets (the `target:` field on every macro).    |
//! | `debug(msg)` / `info(msg)` / …          | `tracing::debug!(…)` / `tracing::info!(…)` etc  |
//! | `_ensure_utf8_stdout` (Windows-only)     | N/A: Rust's stdout is always UTF-8 on all        |
//! |                                          | platforms teri targets (Linux/macOS/Windows via  |
//! |                                          | the Rust runtime); no reconfiguration needed.    |
//!
//! ## Testability
//!
//! The `build_rotating_writer` function is public and fully testable without
//! touching the process-global tracing subscriber.  Tests can call it, write
//! bytes, and inspect the rotated files.

use crate::error::Result;
use file_rotate::{ContentLimit, FileRotate, compression::Compression, suffix::AppendCount};
use std::path::{Path, PathBuf};
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

// ── constants matching MiroFish's RotatingFileHandler defaults ───────────────

/// Maximum bytes per log file before rotation (MiroFish `maxBytes=10*1024*1024`).
pub const MAX_LOG_BYTES: usize = 10 * 1024 * 1024;

/// Number of backup files to keep (MiroFish `backupCount=5`).
pub const LOG_BACKUP_COUNT: usize = 5;

/// Environment variable that enables file logging.  When set to a directory
/// path, logs are written to a rotating file in that directory.
/// Equivalent to MiroFish's `LOG_DIR = os.path.join(…, 'logs')`.
pub const LOG_DIR_ENV: &str = "TERI_LOG_DIR";

// ── testable builder ─────────────────────────────────────────────────────────

/// Build a size-based rotating file writer for the given directory.
///
/// Creates the directory (and all parents) if it does not exist, then opens a
/// `FileRotate` writer with:
/// - 10 MB per-file limit (`ContentLimit::Bytes(MAX_LOG_BYTES)`)
/// - 5 backup files (`AppendCount::new(LOG_BACKUP_COUNT)`)
/// - No compression
///
/// This function is intentionally separate from `init_logging` so that tests
/// can exercise the rotation logic without touching the process-global tracing
/// subscriber.
///
/// # Errors
/// Returns an error if the directory cannot be created.
pub fn build_rotating_writer(log_dir: &Path, filename: &str) -> Result<FileRotate<AppendCount>> {
    std::fs::create_dir_all(log_dir).map_err(|e| {
        crate::error::TeriError::Config(format!(
            "Failed to create log directory '{}': {e}",
            log_dir.display()
        ))
    })?;

    let log_path: PathBuf = log_dir.join(filename);

    Ok(FileRotate::new(
        log_path,
        AppendCount::new(LOG_BACKUP_COUNT),
        ContentLimit::Bytes(MAX_LOG_BYTES),
        Compression::None,
        None,
    ))
}

// ── public initialisation ────────────────────────────────────────────────────

/// Initialise the global tracing subscriber.
///
/// **Console layer** (always): filtered by `level` (respects `RUST_LOG`).
///
/// **File layer** (opt-in): when `TERI_LOG_DIR` is set to a directory path,
/// a rotating file appender is composed in *addition* to the console layer.
/// The file appender logs at DEBUG+ (matching MiroFish's `file_handler.setLevel(DEBUG)`).
///
/// Calling this function more than once per process will panic (tracing's
/// subscriber is set globally via `init()`).
pub fn init_logging(level: &str) -> Result<()> {
    let console_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    match std::env::var(LOG_DIR_ENV) {
        Ok(dir) if !dir.is_empty() => {
            // File + console — both layers composed via Registry
            let log_dir = PathBuf::from(dir);
            let writer = build_rotating_writer(&log_dir, "teri.log")?;

            // File layer always at DEBUG+ (MiroFish: file_handler.setLevel(DEBUG))
            let file_filter = EnvFilter::try_new("debug").expect("static filter string is valid");

            let file_layer = fmt::layer()
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(writer))
                .with_filter(file_filter);

            let console_layer =
                fmt::layer().with_target(true).with_level(true).with_filter(console_filter);

            tracing_subscriber::registry().with(console_layer).with(file_layer).init();
        }
        _ => {
            // Console-only — identical to the original implementation
            fmt().with_env_filter(console_filter).with_target(true).with_level(true).init();
        }
    }

    Ok(())
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    /// `build_rotating_writer` creates the directory when it does not exist.
    #[test]
    fn test_build_rotating_writer_creates_dir() {
        let base = TempDir::new().unwrap();
        let log_dir = base.path().join("logs").join("sub");

        // Directory does not yet exist.
        assert!(!log_dir.exists());

        let _writer = build_rotating_writer(&log_dir, "teri.log").unwrap();

        // Directory was created.
        assert!(log_dir.exists());
    }

    /// Write exactly MAX_LOG_BYTES+1 bytes to trigger one rotation, then verify
    /// a backup file appears (teri.log.1 or similar).
    #[test]
    fn test_rotation_produces_backup_after_size_limit() {
        let base = TempDir::new().unwrap();
        let log_dir = base.path().join("logs");

        let mut writer = build_rotating_writer(&log_dir, "teri.log").unwrap();

        // Write slightly more than 10 MB to force one rotation.
        let chunk = vec![b'A'; 1024 * 1024]; // 1 MB
        for _ in 0..11 {
            writer.write_all(&chunk).unwrap();
        }
        writer.flush().unwrap();

        // After 11 MB written with a 10 MB limit, the writer must have rotated.
        // file-rotate names backups with a numeric suffix: teri.log.1
        let backup = log_dir.join("teri.log.1");
        assert!(
            backup.exists(),
            "Expected backup file teri.log.1 to exist after rotation; dir contents: {:?}",
            std::fs::read_dir(&log_dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect::<Vec<_>>()
        );
    }

    /// Verify the number of backup files does not exceed LOG_BACKUP_COUNT.
    #[test]
    fn test_rotation_keeps_at_most_backup_count_files() {
        let base = TempDir::new().unwrap();
        let log_dir = base.path().join("logs");

        // Use a tiny limit so we can trigger many rotations quickly.
        // Use the public constants but with a tiny helper size for testability.
        std::fs::create_dir_all(&log_dir).unwrap();
        let log_path = log_dir.join("teri-small.log");

        let mut writer: FileRotate<AppendCount> = FileRotate::new(
            &log_path,
            AppendCount::new(LOG_BACKUP_COUNT),
            ContentLimit::Bytes(100), // tiny limit for fast rotation
            Compression::None,
            None,
        );

        // Write 8 * 101 bytes — triggers at least 8 rotations.
        for _ in 0..8 {
            writer.write_all(&[b'X'; 101]).unwrap();
            writer.flush().unwrap();
        }

        // Count backup files.
        let backup_count = std::fs::read_dir(&log_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with("teri-small.log.") // numbered backups
            })
            .count();

        assert!(
            backup_count <= LOG_BACKUP_COUNT,
            "Expected at most {LOG_BACKUP_COUNT} backup files, found {backup_count}"
        );
    }

    /// Content written to the rotating writer is legible.
    #[test]
    fn test_writer_produces_expected_content() {
        let base = TempDir::new().unwrap();
        let log_dir = base.path().join("logs");

        let mut writer = build_rotating_writer(&log_dir, "teri.log").unwrap();

        let msg = b"hello from teri logging test\n";
        writer.write_all(msg).unwrap();
        writer.flush().unwrap();

        let content = std::fs::read(log_dir.join("teri.log")).unwrap();
        assert!(
            content.windows(msg.len()).any(|w| w == msg),
            "Expected written bytes to appear in the log file"
        );
    }

    /// Constants are exactly what MiroFish specifies.
    #[test]
    fn test_constants_match_mirofish_contract() {
        assert_eq!(MAX_LOG_BYTES, 10 * 1024 * 1024, "maxBytes=10MB");
        assert_eq!(LOG_BACKUP_COUNT, 5, "backupCount=5");
    }
}
