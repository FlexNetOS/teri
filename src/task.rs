//! Task state management.
//!
//! Port of `backend/app/models/task.py` (MiroFish). Tracks long-running operations such as
//! graph builds via a process-global, thread-safe in-memory registry.
//!
//! # Singleton idiom-map
//! Python uses `__new__` double-checked locking to build a class-level singleton.  In Rust the
//! idiomatic equivalent is `std::sync::OnceLock<TaskManager>` for the once-init and
//! `parking_lot::Mutex<HashMap>` for the per-registry lock.  `parking_lot` is already a
//! dependency in this crate and never poisons (no `PoisonError` branch needed).  The observable
//! contract is identical: one shared registry visible across all threads, safe concurrent
//! create/update/get.
//!
//! # Locale strings (S-163/S-164)
//! `complete_task` / `fail_task` set the `message` field via teri's i18n system:
//! `crate::i18n::t("progress.taskComplete")` / `crate::i18n::t("progress.taskFailed")`
//! (matching MiroFish `task.py:153,162`).  The active task-local locale determines the string;
//! when no locale is set (the default) both return the `zh` values `"任务完成"` / `"任务失败"`,
//! so the existing tests remain green.  U-005 is now fully ported — PENDING-U-005 removed.

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Locale helpers (S-163/S-164) — now routing through i18n::t().
// ---------------------------------------------------------------------------

/// Localised message for task completion.  Routes through `i18n::t` so the
/// string follows the caller's active task-local locale.  Default (zh) = "任务完成".
#[inline]
fn msg_task_complete() -> String {
    crate::i18n::t("progress.taskComplete")
}

/// Localised message for task failure.  Routes through `i18n::t` so the
/// string follows the caller's active task-local locale.  Default (zh) = "任务失败".
#[inline]
fn msg_task_failed() -> String {
    crate::i18n::t("progress.taskFailed")
}

/// Format a UTC datetime exactly like Python's `datetime.isoformat()`: emit the microsecond
/// fraction ONLY when it is non-zero (Python omits `.000000` for whole-second times), with no
/// timezone suffix. teri's `to_dict` must be wire-identical to MiroFish's, so a whole-second
/// timestamp serialises as `2024-01-01T12:30:45` (not `...:45.000000`).
fn python_isoformat(dt: &DateTime<Utc>) -> String {
    if dt.timestamp_subsec_micros() == 0 {
        dt.format("%Y-%m-%dT%H:%M:%S").to_string()
    } else {
        dt.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
    }
}

// ---------------------------------------------------------------------------
// TaskStatus
// ---------------------------------------------------------------------------

/// Task state.  Serialises to the same lowercase string values as the Python
/// `TaskStatus(str, Enum)` — `"pending"`, `"processing"`, `"completed"`, `"failed"` — so
/// `to_dict` output is wire-identical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    /// Waiting to be picked up.
    Pending,
    /// Currently running.
    Processing,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed,
}

impl TaskStatus {
    /// Returns the lowercase string value used by the Python enum `.value`.
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Processing => "processing",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
        }
    }
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

/// A single tracked operation.
///
/// Field mapping vs. Python dataclass:
///
/// | Python field       | Rust field         | Type                             |
/// |--------------------|--------------------|----------------------------------|
/// | `task_id`          | `task_id`          | `String`                         |
/// | `task_type`        | `task_type`        | `String`                         |
/// | `status`           | `status`           | `TaskStatus`                     |
/// | `created_at`       | `created_at`       | `DateTime<Utc>`                  |
/// | `updated_at`       | `updated_at`       | `DateTime<Utc>`                  |
/// | `progress`         | `progress`         | `i64` (0–100)                    |
/// | `message`          | `message`          | `String`                         |
/// | `result`           | `result`           | `Option<Value>`                  |
/// | `error`            | `error`            | `Option<String>`                 |
/// | `metadata`         | `metadata`         | `HashMap<String, Value>`         |
/// | `progress_detail`  | `progress_detail`  | `HashMap<String, Value>`         |
///
/// `result` maps to `Option<Dict>` in Python; `serde_json::Value` (object variant) is the
/// equivalent and round-trips through `to_dict` as a JSON object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub task_type: String,
    pub status: TaskStatus,
    /// UTC creation time.  Serialises as ISO 8601 string (`datetime.isoformat()` equivalent).
    pub created_at: DateTime<Utc>,
    /// UTC last-update time.  Bumped on every mutation.
    pub updated_at: DateTime<Utc>,
    /// Overall progress percentage, 0–100.
    pub progress: i64,
    /// Human-readable status message.
    pub message: String,
    /// Task output (set when completed).
    pub result: Option<Value>,
    /// Error description (set when failed).
    pub error: Option<String>,
    /// Arbitrary caller-supplied metadata.
    pub metadata: HashMap<String, Value>,
    /// Fine-grained progress breakdown.
    pub progress_detail: HashMap<String, Value>,
}

impl Task {
    /// Converts the task to a JSON `Value` with the same shape as the Python `to_dict()`.
    ///
    /// Python `to_dict` output (field order matches Python dict for readability, but JSON dicts
    /// are unordered):
    /// ```json
    /// {
    ///   "task_id": "...",
    ///   "task_type": "...",
    ///   "status": "pending",
    ///   "created_at": "2024-01-01T00:00:00.000000",
    ///   "updated_at": "2024-01-01T00:00:00.000000",
    ///   "progress": 0,
    ///   "message": "",
    ///   "progress_detail": {},
    ///   "result": null,
    ///   "error": null,
    ///   "metadata": {}
    /// }
    /// ```
    ///
    /// Timestamps use [`python_isoformat`], which reproduces Python `datetime.isoformat()`
    /// exactly: a microsecond fraction ONLY when microseconds != 0 (Python omits `.000000` for
    /// whole-second times), and no timezone suffix (matching Python `datetime.now()`'s naive
    /// datetime; we use UTC since teri has no tzlocal dependency — the shape is identical).
    pub fn to_dict(&self) -> Value {
        serde_json::json!({
            "task_id": self.task_id,
            "task_type": self.task_type,
            "status": self.status.as_str(),
            "created_at": python_isoformat(&self.created_at),
            "updated_at": python_isoformat(&self.updated_at),
            "progress": self.progress,
            "message": self.message,
            "progress_detail": self.progress_detail,
            "result": self.result,
            "error": self.error,
            "metadata": self.metadata,
        })
    }
}

// ---------------------------------------------------------------------------
// TaskManager — process-global singleton
// ---------------------------------------------------------------------------

/// Process-wide, thread-safe task registry.
///
/// # Singleton
/// `TaskManager::global()` always returns a reference to the same process-level instance
/// (backed by `OnceLock`), replicating the Python `__new__` singleton pattern.
///
/// # Thread safety
/// All registry mutations are guarded by a `parking_lot::Mutex` (non-poisoning).  Concurrent
/// `create_task` / `update_task` / `get_task` calls from multiple threads are safe and
/// serialised, matching the Python `threading.Lock` on `_task_lock`.
pub struct TaskManager {
    tasks: Mutex<HashMap<String, Task>>,
}

static TASK_MANAGER: OnceLock<TaskManager> = OnceLock::new();

impl TaskManager {
    /// Returns the process-global `TaskManager` instance, initialising it on first call.
    ///
    /// Idiom-maps Python's `__new__` double-checked-lock singleton to `OnceLock::get_or_init`.
    /// Observable contract preserved: every caller in every thread receives the same registry.
    pub fn global() -> &'static TaskManager {
        TASK_MANAGER.get_or_init(|| TaskManager { tasks: Mutex::new(HashMap::new()) })
    }

    // ------------------------------------------------------------------
    // create_task  (S-160)
    // ------------------------------------------------------------------

    /// Creates a new task, inserts it as PENDING, and returns its UUID string.
    ///
    /// Matches Python:
    /// ```python
    /// task_id = str(uuid.uuid4())
    /// now = datetime.now()
    /// task = Task(task_id=task_id, task_type=task_type, status=TaskStatus.PENDING,
    ///             created_at=now, updated_at=now, metadata=metadata or {})
    /// self._tasks[task_id] = task
    /// return task_id
    /// ```
    pub fn create_task(
        &self,
        task_type: impl Into<String>,
        metadata: Option<HashMap<String, Value>>,
    ) -> String {
        let task_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let task = Task {
            task_id: task_id.clone(),
            task_type: task_type.into(),
            status: TaskStatus::Pending,
            created_at: now,
            updated_at: now,
            progress: 0,
            message: String::new(),
            result: None,
            error: None,
            metadata: metadata.unwrap_or_default(),
            progress_detail: HashMap::new(),
        };

        self.tasks.lock().insert(task_id.clone(), task);
        task_id
    }

    // ------------------------------------------------------------------
    // get_task  (S-161)
    // ------------------------------------------------------------------

    /// Returns a clone of the task for the given id, or `None` if not found.
    ///
    /// Matches Python `self._tasks.get(task_id)`.  Clones out of the lock so callers cannot
    /// hold a reference across mutations.
    pub fn get_task(&self, task_id: &str) -> Option<Task> {
        self.tasks.lock().get(task_id).cloned()
    }

    // ------------------------------------------------------------------
    // update_task  (S-162)
    // ------------------------------------------------------------------

    /// Partially updates a task.  Only `Some` parameters are applied; `None` leaves the field
    /// unchanged.  `updated_at` is always bumped when the task exists, matching the Python
    /// implementation.
    ///
    /// ```python
    /// task.updated_at = datetime.now()
    /// if status is not None: task.status = status
    /// if progress is not None: task.progress = progress
    /// if message is not None: task.message = message
    /// if result is not None: task.result = result
    /// if error is not None: task.error = error
    /// if progress_detail is not None: task.progress_detail = progress_detail
    /// ```
    /// # Note on argument count
    /// The Python source has 7 optional keyword arguments (`status`, `progress`, `message`,
    /// `result`, `error`, `progress_detail`) plus `task_id`.  All must be expressible as
    /// `None` to support partial updates.  We allow clippy's `too_many_arguments` here rather
    /// than introducing an intermediate builder that would change the call sites in the port.
    #[allow(clippy::too_many_arguments)]
    pub fn update_task(
        &self,
        task_id: &str,
        status: Option<TaskStatus>,
        progress: Option<i64>,
        message: Option<String>,
        result: Option<Value>,
        error: Option<String>,
        progress_detail: Option<HashMap<String, Value>>,
    ) {
        let mut guard = self.tasks.lock();
        if let Some(task) = guard.get_mut(task_id) {
            task.updated_at = Utc::now();
            if let Some(s) = status {
                task.status = s;
            }
            if let Some(p) = progress {
                task.progress = p;
            }
            if let Some(m) = message {
                task.message = m;
            }
            if let Some(r) = result {
                task.result = Some(r);
            }
            if let Some(e) = error {
                task.error = Some(e);
            }
            if let Some(pd) = progress_detail {
                task.progress_detail = pd;
            }
        }
    }

    // ------------------------------------------------------------------
    // complete_task  (S-163)
    // ------------------------------------------------------------------

    /// Marks the task COMPLETED with progress=100, a localised message, and the given result.
    ///
    /// Matches Python:
    /// ```python
    /// self.update_task(task_id, status=TaskStatus.COMPLETED, progress=100,
    ///                  message=t('progress.taskComplete'), result=result)
    /// ```
    pub fn complete_task(&self, task_id: &str, result: Value) {
        self.update_task(
            task_id,
            Some(TaskStatus::Completed),
            Some(100),
            Some(msg_task_complete()),
            Some(result),
            None,
            None,
        );
    }

    // ------------------------------------------------------------------
    // fail_task  (S-164)
    // ------------------------------------------------------------------

    /// Marks the task FAILED with a localised message and the given error string.
    ///
    /// Matches Python:
    /// ```python
    /// self.update_task(task_id, status=TaskStatus.FAILED,
    ///                  message=t('progress.taskFailed'), error=error)
    /// ```
    ///
    /// Note: Python does NOT set `progress=100` on failure — neither do we.
    pub fn fail_task(&self, task_id: &str, error: impl Into<String>) {
        self.update_task(
            task_id,
            Some(TaskStatus::Failed),
            None,
            Some(msg_task_failed()),
            None,
            Some(error.into()),
            None,
        );
    }

    // ------------------------------------------------------------------
    // list_tasks  (S-165)
    // ------------------------------------------------------------------

    /// Lists tasks as `to_dict()` JSON values, sorted newest-first by `created_at`.
    ///
    /// Optional `task_type` filter: when `Some`, only tasks whose `task_type` matches are
    /// returned.  Matches Python:
    /// ```python
    /// tasks = list(self._tasks.values())
    /// if task_type:
    ///     tasks = [t for t in tasks if t.task_type == task_type]
    /// return [t.to_dict() for t in sorted(tasks, key=lambda x: x.created_at, reverse=True)]
    /// ```
    pub fn list_tasks(&self, task_type: Option<&str>) -> Vec<Value> {
        let guard = self.tasks.lock();
        let mut tasks: Vec<&Task> = match task_type {
            Some(ty) => guard.values().filter(|t| t.task_type == ty).collect(),
            None => guard.values().collect(),
        };
        // Newest first, matching `reverse=True` in Python.
        tasks.sort_by_key(|t| std::cmp::Reverse(t.created_at));
        tasks.iter().map(|t| t.to_dict()).collect()
    }

    // ------------------------------------------------------------------
    // cleanup_old_tasks  (S-166)
    // ------------------------------------------------------------------

    /// Removes COMPLETED or FAILED tasks whose `created_at` is older than `max_age_hours`.
    ///
    /// Matches Python:
    /// ```python
    /// cutoff = datetime.now() - timedelta(hours=max_age_hours)
    /// old_ids = [tid for tid, task in self._tasks.items()
    ///            if task.created_at < cutoff
    ///            and task.status in [TaskStatus.COMPLETED, TaskStatus.FAILED]]
    /// for tid in old_ids:
    ///     del self._tasks[tid]
    /// ```
    ///
    /// PENDING and PROCESSING tasks are never removed regardless of age, exactly as in Python.
    pub fn cleanup_old_tasks(&self, max_age_hours: i64) {
        let cutoff = Utc::now() - Duration::hours(max_age_hours);
        let mut guard = self.tasks.lock();
        guard.retain(|_, task| {
            let is_terminal = matches!(task.status, TaskStatus::Completed | TaskStatus::Failed);
            let is_old = task.created_at < cutoff;
            // retain = NOT (terminal AND old)
            !(is_terminal && is_old)
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Helper: create a fresh, isolated TaskManager for unit tests so tests do not share the
    // process-global singleton's state with each other.
    fn fresh() -> TaskManager {
        TaskManager { tasks: Mutex::new(HashMap::new()) }
    }

    #[test]
    fn test_python_isoformat_matches_datetime_isoformat() {
        use chrono::{TimeZone, Timelike};
        // Whole-second time: Python datetime.isoformat() omits the fraction entirely.
        let whole = Utc.with_ymd_and_hms(2024, 1, 1, 12, 30, 45).unwrap();
        assert_eq!(python_isoformat(&whole), "2024-01-01T12:30:45");
        // Sub-second time: Python emits a 6-digit microsecond fraction.
        let frac = Utc
            .with_ymd_and_hms(2024, 1, 1, 12, 30, 45)
            .unwrap()
            .with_nanosecond(123_456_000)
            .unwrap();
        assert_eq!(python_isoformat(&frac), "2024-01-01T12:30:45.123456");
        // No timezone suffix in either case (matches Python's naive datetime.now()).
        assert!(!python_isoformat(&whole).contains('+'));
        assert!(!python_isoformat(&whole).ends_with('Z'));
    }

    // ------------------------------------------------------------------
    // TaskStatus serialization — must match Python enum .value strings exactly
    // ------------------------------------------------------------------

    #[test]
    fn task_status_as_str_matches_python_values() {
        assert_eq!(TaskStatus::Pending.as_str(), "pending");
        assert_eq!(TaskStatus::Processing.as_str(), "processing");
        assert_eq!(TaskStatus::Completed.as_str(), "completed");
        assert_eq!(TaskStatus::Failed.as_str(), "failed");
    }

    #[test]
    fn task_status_serde_roundtrip() {
        for (status, expected) in [
            (TaskStatus::Pending, "\"pending\""),
            (TaskStatus::Processing, "\"processing\""),
            (TaskStatus::Completed, "\"completed\""),
            (TaskStatus::Failed, "\"failed\""),
        ] {
            let serialized = serde_json::to_string(&status).unwrap();
            assert_eq!(serialized, expected, "Status {:?} must serialize to {}", status, expected);
            let back: TaskStatus = serde_json::from_str(&serialized).unwrap();
            assert_eq!(back, status);
        }
    }

    // ------------------------------------------------------------------
    // Task.to_dict() shape — must match Python to_dict() field names and types
    // ------------------------------------------------------------------

    #[test]
    fn to_dict_contains_all_fields_with_correct_keys() {
        let tm = fresh();
        let task_id = tm.create_task("build_graph", None);
        let task = tm.get_task(&task_id).unwrap();
        let dict = task.to_dict();

        // All 11 fields present with exact key names from Python to_dict():
        assert!(dict.get("task_id").is_some(), "missing task_id");
        assert!(dict.get("task_type").is_some(), "missing task_type");
        assert!(dict.get("status").is_some(), "missing status");
        assert!(dict.get("created_at").is_some(), "missing created_at");
        assert!(dict.get("updated_at").is_some(), "missing updated_at");
        assert!(dict.get("progress").is_some(), "missing progress");
        assert!(dict.get("message").is_some(), "missing message");
        assert!(dict.get("progress_detail").is_some(), "missing progress_detail");
        assert!(dict.get("result").is_some(), "missing result"); // null initially
        assert!(dict.get("error").is_some(), "missing error"); // null initially
        assert!(dict.get("metadata").is_some(), "missing metadata");
    }

    #[test]
    fn to_dict_status_is_lowercase_string() {
        let tm = fresh();
        let task_id = tm.create_task("test", None);
        let task = tm.get_task(&task_id).unwrap();
        let dict = task.to_dict();
        assert_eq!(dict["status"], json!("pending"));
    }

    #[test]
    fn to_dict_timestamps_are_iso8601_strings() {
        let tm = fresh();
        let task_id = tm.create_task("test", None);
        let task = tm.get_task(&task_id).unwrap();
        let dict = task.to_dict();

        let created_at = dict["created_at"].as_str().expect("created_at must be a string");
        let updated_at = dict["updated_at"].as_str().expect("updated_at must be a string");

        // Must be parseable as ISO 8601 datetime.
        assert!(created_at.contains('T'), "created_at must be ISO 8601: {}", created_at);
        assert!(updated_at.contains('T'), "updated_at must be ISO 8601: {}", updated_at);
        // Must have microsecond precision (6 fractional digits), matching Python datetime.isoformat()
        let frac_part = created_at.split('.').nth(1).unwrap_or("");
        assert_eq!(frac_part.len(), 6, "timestamp must have 6 fractional digits: {}", created_at);
    }

    #[test]
    fn to_dict_initial_values() {
        let tm = fresh();
        let task_id = tm.create_task(
            "build_graph",
            Some({
                let mut m = HashMap::new();
                m.insert("key".to_string(), json!("val"));
                m
            }),
        );
        let task = tm.get_task(&task_id).unwrap();
        let dict = task.to_dict();

        assert_eq!(dict["task_type"], json!("build_graph"));
        assert_eq!(dict["status"], json!("pending"));
        assert_eq!(dict["progress"], json!(0));
        assert_eq!(dict["message"], json!(""));
        assert_eq!(dict["result"], json!(null));
        assert_eq!(dict["error"], json!(null));
        assert_eq!(dict["metadata"], json!({"key": "val"}));
        assert_eq!(dict["progress_detail"], json!({}));
    }

    // ------------------------------------------------------------------
    // create_task / get_task round-trip
    // ------------------------------------------------------------------

    #[test]
    fn create_get_roundtrip() {
        let tm = fresh();
        let id = tm.create_task("sim_run", None);

        assert!(!id.is_empty(), "task_id must not be empty");
        // Must be a valid UUID
        let _parsed: uuid::Uuid = id.parse().expect("task_id must be a UUID");

        let task = tm.get_task(&id).expect("get_task must return the created task");
        assert_eq!(task.task_id, id);
        assert_eq!(task.task_type, "sim_run");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.progress, 0);
        assert!(task.message.is_empty());
        assert!(task.result.is_none());
        assert!(task.error.is_none());
    }

    #[test]
    fn get_task_returns_none_for_unknown_id() {
        let tm = fresh();
        assert!(tm.get_task("nonexistent-id").is_none());
    }

    #[test]
    fn create_task_generates_unique_ids() {
        let tm = fresh();
        let ids: Vec<String> = (0..10).map(|_| tm.create_task("t", None)).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 10, "all task IDs must be unique");
    }

    // ------------------------------------------------------------------
    // update_task — partial updates + updated_at bump
    // ------------------------------------------------------------------

    #[test]
    fn update_task_applies_all_fields() {
        let tm = fresh();
        let id = tm.create_task("test", None);
        let before = tm.get_task(&id).unwrap();

        // Small sleep to ensure updated_at actually changes (UTC has sub-millisecond precision)
        std::thread::sleep(std::time::Duration::from_millis(2));

        let mut pd = HashMap::new();
        pd.insert("step".to_string(), json!(3));

        tm.update_task(
            &id,
            Some(TaskStatus::Processing),
            Some(42),
            Some("halfway".to_string()),
            None,
            None,
            Some(pd),
        );

        let after = tm.get_task(&id).unwrap();
        assert_eq!(after.status, TaskStatus::Processing);
        assert_eq!(after.progress, 42);
        assert_eq!(after.message, "halfway");
        assert_eq!(after.progress_detail["step"], json!(3));
        assert!(after.updated_at > before.updated_at, "updated_at must be bumped");
    }

    #[test]
    fn update_task_partial_none_fields_unchanged() {
        let tm = fresh();
        let id = tm.create_task("test", None);

        // Set initial state
        tm.update_task(
            &id,
            Some(TaskStatus::Processing),
            Some(50),
            Some("msg".to_string()),
            None,
            None,
            None,
        );

        // Update only progress — other fields must remain
        tm.update_task(&id, None, Some(75), None, None, None, None);

        let task = tm.get_task(&id).unwrap();
        assert_eq!(task.status, TaskStatus::Processing, "status must be unchanged");
        assert_eq!(task.progress, 75);
        assert_eq!(task.message, "msg", "message must be unchanged");
    }

    #[test]
    fn update_task_nonexistent_is_noop() {
        let tm = fresh();
        // Must not panic
        tm.update_task("no-such-id", Some(TaskStatus::Failed), None, None, None, None, None);
    }

    // ------------------------------------------------------------------
    // complete_task (S-163)
    // ------------------------------------------------------------------

    #[test]
    fn complete_task_sets_status_progress_message_result() {
        let tm = fresh();
        let id = tm.create_task("test", None);
        let result = json!({"answer": 42});

        tm.complete_task(&id, result.clone());

        let task = tm.get_task(&id).unwrap();
        assert_eq!(task.status, TaskStatus::Completed);
        assert_eq!(task.progress, 100);
        assert_eq!(task.message, "任务完成"); // i18n::t("progress.taskComplete"), zh default
        assert_eq!(task.result.as_ref().unwrap(), &result);
        assert!(task.error.is_none());
    }

    #[test]
    fn complete_task_message_matches_python_locale_string() {
        let tm = fresh();
        let id = tm.create_task("test", None);
        tm.complete_task(&id, json!({}));
        let task = tm.get_task(&id).unwrap();
        assert_eq!(task.message, "任务完成");
    }

    // ------------------------------------------------------------------
    // fail_task (S-164)
    // ------------------------------------------------------------------

    #[test]
    fn fail_task_sets_status_message_error() {
        let tm = fresh();
        let id = tm.create_task("test", None);

        tm.fail_task(&id, "connection refused");

        let task = tm.get_task(&id).unwrap();
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.message, "任务失败"); // i18n::t("progress.taskFailed"), zh default
        assert_eq!(task.error.as_deref(), Some("connection refused"));
        assert!(task.result.is_none(), "fail_task must not set result");
        // Python does NOT set progress=100 on failure
        assert_eq!(task.progress, 0, "fail_task must not change progress");
    }

    #[test]
    fn fail_task_message_matches_python_locale_string() {
        let tm = fresh();
        let id = tm.create_task("test", None);
        tm.fail_task(&id, "err");
        let task = tm.get_task(&id).unwrap();
        assert_eq!(task.message, "任务失败");
    }

    // ------------------------------------------------------------------
    // list_tasks (S-165)
    // ------------------------------------------------------------------

    #[test]
    fn list_tasks_returns_all_as_dicts() {
        let tm = fresh();
        tm.create_task("type_a", None);
        tm.create_task("type_b", None);
        tm.create_task("type_a", None);

        let all = tm.list_tasks(None);
        assert_eq!(all.len(), 3);
        // Each element must have a task_id key
        for item in &all {
            assert!(item.get("task_id").is_some());
        }
    }

    #[test]
    fn list_tasks_with_type_filter() {
        let tm = fresh();
        tm.create_task("type_a", None);
        tm.create_task("type_b", None);
        tm.create_task("type_a", None);

        let filtered = tm.list_tasks(Some("type_a"));
        assert_eq!(filtered.len(), 2);
        for item in &filtered {
            assert_eq!(item["task_type"], json!("type_a"));
        }
    }

    #[test]
    fn list_tasks_sorted_newest_first() {
        let tm = fresh();
        // Create 3 tasks with a small delay between each to get distinct timestamps
        let id1 = tm.create_task("t", None);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _id2 = tm.create_task("t", None);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let id3 = tm.create_task("t", None);

        let all = tm.list_tasks(None);
        assert_eq!(all.len(), 3);
        // Newest (id3) must come first
        assert_eq!(all[0]["task_id"], json!(id3));
        // Oldest (id1) must come last
        assert_eq!(all[2]["task_id"], json!(id1));
    }

    #[test]
    fn list_tasks_empty_type_filter_returns_none_when_no_match() {
        let tm = fresh();
        tm.create_task("type_a", None);

        let filtered = tm.list_tasks(Some("type_z"));
        assert!(filtered.is_empty());
    }

    // ------------------------------------------------------------------
    // cleanup_old_tasks (S-166)
    // ------------------------------------------------------------------

    #[test]
    fn cleanup_removes_old_completed_and_failed_tasks() {
        let tm = fresh();

        // Create tasks and manually backdate their created_at to simulate age.
        let id_old_completed = {
            let id = tm.create_task("t", None);
            let mut guard = tm.tasks.lock();
            let task = guard.get_mut(&id).unwrap();
            task.status = TaskStatus::Completed;
            task.created_at = Utc::now() - Duration::hours(25);
            id
        };
        let id_old_failed = {
            let id = tm.create_task("t", None);
            let mut guard = tm.tasks.lock();
            let task = guard.get_mut(&id).unwrap();
            task.status = TaskStatus::Failed;
            task.created_at = Utc::now() - Duration::hours(25);
            id
        };
        let id_recent_completed = {
            let id = tm.create_task("t", None);
            let mut guard = tm.tasks.lock();
            let task = guard.get_mut(&id).unwrap();
            task.status = TaskStatus::Completed;
            // created_at stays at Utc::now() — recent, must NOT be removed
            id
        };
        let id_old_pending = {
            let id = tm.create_task("t", None);
            let mut guard = tm.tasks.lock();
            let task = guard.get_mut(&id).unwrap();
            task.status = TaskStatus::Pending;
            task.created_at = Utc::now() - Duration::hours(25);
            id
        };
        let id_old_processing = {
            let id = tm.create_task("t", None);
            let mut guard = tm.tasks.lock();
            let task = guard.get_mut(&id).unwrap();
            task.status = TaskStatus::Processing;
            task.created_at = Utc::now() - Duration::hours(25);
            id
        };

        tm.cleanup_old_tasks(24);

        // Old completed and failed must be gone
        assert!(tm.get_task(&id_old_completed).is_none(), "old COMPLETED must be removed");
        assert!(tm.get_task(&id_old_failed).is_none(), "old FAILED must be removed");
        // Recent completed must remain
        assert!(tm.get_task(&id_recent_completed).is_some(), "recent COMPLETED must survive");
        // Old PENDING and PROCESSING must remain (Python only removes COMPLETED/FAILED)
        assert!(tm.get_task(&id_old_pending).is_some(), "old PENDING must survive");
        assert!(tm.get_task(&id_old_processing).is_some(), "old PROCESSING must survive");
    }

    #[test]
    fn cleanup_exactly_at_boundary_not_removed() {
        let tm = fresh();
        // Task created exactly at the cutoff is NOT older-than, so must survive.
        // Python: `task.created_at < cutoff` — strictly less than.
        let id = {
            let id = tm.create_task("t", None);
            let mut guard = tm.tasks.lock();
            let task = guard.get_mut(&id).unwrap();
            task.status = TaskStatus::Completed;
            // Set created_at to exactly (now - 24h) + 1 second — should NOT be removed
            task.created_at = Utc::now() - Duration::hours(24) + Duration::seconds(1);
            id
        };

        tm.cleanup_old_tasks(24);
        assert!(tm.get_task(&id).is_some(), "task at boundary+1s must not be removed");
    }

    // ------------------------------------------------------------------
    // Singleton contract
    // ------------------------------------------------------------------

    #[test]
    fn global_returns_same_instance_across_calls() {
        let a = TaskManager::global() as *const TaskManager;
        let b = TaskManager::global() as *const TaskManager;
        assert_eq!(a, b, "global() must return the same instance");
    }

    #[test]
    fn global_registry_is_shared_across_calls() {
        // This test uses the real global singleton.  Use a task_type prefix unlikely to collide
        // with other tests to avoid cross-test contamination.
        let tm1 = TaskManager::global();
        let tm2 = TaskManager::global();

        let id = tm1.create_task("singleton_test_type", None);
        let task = tm2.get_task(&id);
        assert!(task.is_some(), "task created via tm1 must be visible via tm2");

        // Cleanup
        tm1.fail_task(&id, "cleanup");
        tm1.cleanup_old_tasks(0);
    }

    // ------------------------------------------------------------------
    // Thread safety smoke test
    // ------------------------------------------------------------------

    #[test]
    fn concurrent_creates_are_safe_and_unique() {
        let tm = std::sync::Arc::new(fresh());
        let n = 50;

        let handles: Vec<_> = (0..n)
            .map(|_| {
                let tm_clone = tm.clone();
                std::thread::spawn(move || tm_clone.create_task("concurrent", None))
            })
            .collect();

        let ids: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();

        assert_eq!(unique.len(), n, "all concurrent task IDs must be unique");
        assert_eq!(tm.list_tasks(None).len(), n, "all tasks must be registered");
    }

    #[test]
    fn concurrent_update_complete_fail_is_safe() {
        let tm = std::sync::Arc::new(fresh());
        let id = tm.create_task("concurrent_update", None);

        let handles: Vec<_> = (0..20)
            .map(|i| {
                let tm_clone = tm.clone();
                let id_clone = id.clone();
                std::thread::spawn(move || {
                    if i % 2 == 0 {
                        tm_clone.update_task(
                            &id_clone,
                            Some(TaskStatus::Processing),
                            Some(i as i64),
                            None,
                            None,
                            None,
                            None,
                        );
                    } else {
                        tm_clone.complete_task(&id_clone, json!({"i": i}));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
        // Must not panic and task must still be gettable
        assert!(tm.get_task(&id).is_some());
    }

    // ------------------------------------------------------------------
    // Metadata is preserved through create → get
    // ------------------------------------------------------------------

    #[test]
    fn metadata_preserved_in_create_and_to_dict() {
        let tm = fresh();
        let mut meta = HashMap::new();
        meta.insert("graph_id".to_string(), json!("g-123"));
        meta.insert("user".to_string(), json!("alice"));

        let id = tm.create_task("build_graph", Some(meta.clone()));
        let task = tm.get_task(&id).unwrap();
        let dict = task.to_dict();

        assert_eq!(dict["metadata"]["graph_id"], json!("g-123"));
        assert_eq!(dict["metadata"]["user"], json!("alice"));
    }
}
