//! Simulation state types and manager — port of `backend/app/services/simulation_manager.py`.
//!
//! Sub-cycle (b) — state types (L25-112):
//! - [`SimulationStatus`] — 8-variant enum (L25-34)
//! - [`PlatformType`]     — 2-variant enum (L37-40)
//! - [`SimulationState`]  — 17-field dataclass with `to_dict` / `to_simple_dict` (L43-112)
//!
//! Sub-cycle (c) — `SimulationManager` struct + FS persistence + getters (L115-529):
//! - [`SimulationManager`]         — struct with Mutex-guarded cache + FS root (S-668/S-669/S-670)
//! - `_get_simulation_dir`         — S-671
//! - `_save_simulation_state`      — S-672
//! - `_load_simulation_state`      — S-673
//! - `create_simulation`           — S-674
//! - `get_simulation`              — S-676
//! - `list_simulations`            — S-677
//! - `get_profiles`                — S-678
//! - `get_simulation_config`       — S-679
//! - `get_run_instructions`        — S-680 (DECISION-U026-2: [≠] narrowed; native-expressed)
//!
//! Sub-cycle (d) — `prepare_simulation` (L230-458, S-675) — PORTED.
//!
//! # Ledger corrections
//! The parity-ledger summary was wrong on two counts; the SOURCE is authoritative:
//! - `SimulationStatus` has **8** variants (not 4): CREATED, PREPARING, READY, RUNNING,
//!   PAUSED, STOPPED, COMPLETED, FAILED.
//! - `PlatformType` has **2** variants (not 3): TWITTER, REDDIT only (no BOTH).
//!
//! # Symbols: S-636..S-680 (excluding S-675)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::error::{Result, TeriError};
use crate::models::project::python_isoformat_local;

// ---------------------------------------------------------------------------
// PrepareProgress — progress event emitted by `prepare_simulation` (S-675)
// ---------------------------------------------------------------------------

/// Progress update emitted by [`SimulationManager::prepare_simulation`].
///
/// Ports the kwargs of MiroFish's `progress_callback(stage, progress, message,
/// current=, total=, item_name=)` (simulation_manager.py:273-327).
///
/// `item_name` in Python equals `message` at every callsite that passes it
/// (L318-327: `item_name=msg` where `msg` is already the message string), so
/// `item_name` is folded into `message` here — no second field needed.
///
/// This struct carries the FULL observable surface consumed by the U-026 SSE
/// stream.  Dropping any field would downgrade the API contract.
pub struct PrepareProgress<'a> {
    /// Stage name: `"reading"` | `"generating_profiles"` | `"generating_config"`.
    ///
    /// Mirrors Python's first positional arg `stage` (e.g. `"reading"`, etc.).
    pub stage: &'a str,

    /// Overall stage progress in 0..=100 (integer percentage).
    ///
    /// Mirrors Python's second positional arg `progress` (e.g. `0`, `30`, `100`).
    pub progress: i64,

    /// Human-readable status message (i18n string).
    ///
    /// Mirrors Python's third positional arg `message` AND `item_name` kwarg
    /// (they are identical at every callsite: L318-327 `item_name=msg`).
    pub message: String,

    /// Current item index (1-based within the stage), if applicable.
    ///
    /// Mirrors Python's `current=` kwarg.  `None` when the stage doesn't have
    /// a meaningful current item (e.g. "reading"/0 where nothing is loaded yet).
    pub current: Option<i64>,

    /// Total item count for the stage, if applicable.
    ///
    /// Mirrors Python's `total=` kwarg.
    pub total: Option<i64>,
}

// ---------------------------------------------------------------------------
// SimulationStatus
// ---------------------------------------------------------------------------

/// Port of `SimulationStatus(str, Enum)` (`simulation_manager.py:25-34`).
///
/// 8 variants; serde serializes each to its lowercase string value, matching
/// Python's `self.status.value` (which equals the Python enum's string member).
///
/// S-636 (type), S-637..S-644 (variants)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationStatus {
    /// `"created"` — simulation object constructed but not yet preparing.
    Created,
    /// `"preparing"` — running the 4-stage async preparation pipeline.
    Preparing,
    /// `"ready"` — preparation complete; ready to start.
    Ready,
    /// `"running"` — simulation actively running.
    Running,
    /// `"paused"` — simulation temporarily paused.
    Paused,
    /// `"stopped"` — simulation manually stopped by user.
    Stopped,
    /// `"completed"` — simulation ran to natural completion.
    Completed,
    /// `"failed"` — simulation encountered a fatal error.
    Failed,
}

impl SimulationStatus {
    /// Return the string value, mirroring Python's `self.status.value`.
    ///
    /// Downstream callers (including `to_dict`) must emit the lowercase string
    /// (e.g. `"created"`, not the variant name), so this provides a cheap ref.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Preparing => "preparing",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl std::fmt::Display for SimulationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// PlatformType
// ---------------------------------------------------------------------------

/// Port of `PlatformType(str, Enum)` (`simulation_manager.py:37-40`).
///
/// Exactly 2 variants: TWITTER and REDDIT.  (The ledger summary incorrectly
/// listed 3 variants including BOTH; the source has only these two.)
///
/// S-645 (type), S-646..S-647 (variants)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformType {
    /// `"twitter"`
    Twitter,
    /// `"reddit"`
    Reddit,
}

impl PlatformType {
    /// Return the string value, mirroring Python's `platform.value`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Twitter => "twitter",
            Self::Reddit => "reddit",
        }
    }
}

impl std::fmt::Display for PlatformType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// SimulationState
// ---------------------------------------------------------------------------

/// Port of `SimulationState` dataclass (`simulation_manager.py:43-112`).
///
/// 17 fields with the following defaults (matching the Python `@dataclass`
/// field defaults exactly):
///
/// | field              | default                          |
/// |--------------------|----------------------------------|
/// | simulation_id      | — required                       |
/// | project_id         | — required                       |
/// | graph_id           | — required                       |
/// | enable_twitter     | `true`                           |
/// | enable_reddit      | `true`                           |
/// | status             | `SimulationStatus::Created`      |
/// | entities_count     | `0`                              |
/// | profiles_count     | `0`                              |
/// | entity_types       | `[]`                             |
/// | config_generated   | `false`                          |
/// | config_reasoning   | `""`                             |
/// | current_round      | `0`                              |
/// | twitter_status     | `"not_started"`                  |
/// | reddit_status      | `"not_started"`                  |
/// | created_at         | `python_isoformat_local()`       |
/// | updated_at         | `python_isoformat_local()`       |
/// | error              | `None`                           |
///
/// S-648 (type), S-649..S-665 (fields), S-666 (to_dict), S-667 (to_simple_dict)
#[derive(Debug, Clone)]
pub struct SimulationState {
    // ---- required fields (no default in Python) ---------------------------
    /// S-649
    pub simulation_id: String,
    /// S-650
    pub project_id: String,
    /// S-651
    pub graph_id: String,

    // ---- platform enable flags (default true) -----------------------------
    /// S-652
    pub enable_twitter: bool,
    /// S-653
    pub enable_reddit: bool,

    // ---- status (default CREATED) ----------------------------------------
    /// S-654
    pub status: SimulationStatus,

    // ---- preparation-stage data ------------------------------------------
    /// S-655
    pub entities_count: i64,
    /// S-656
    pub profiles_count: i64,
    /// S-657
    pub entity_types: Vec<String>,

    // ---- config generation info ------------------------------------------
    /// S-658
    pub config_generated: bool,
    /// S-659
    pub config_reasoning: String,

    // ---- runtime data ----------------------------------------------------
    /// S-660
    pub current_round: i64,
    /// S-661
    pub twitter_status: String,
    /// S-662
    pub reddit_status: String,

    // ---- timestamps (set on construction) --------------------------------
    /// S-663
    pub created_at: String,
    /// S-664
    pub updated_at: String,

    // ---- error -----------------------------------------------------------
    /// S-665
    pub error: Option<String>,
}

impl SimulationState {
    /// Construct a new `SimulationState` with the required IDs and all
    /// optional fields set to their Python dataclass defaults.
    ///
    /// `created_at` and `updated_at` are stamped via `python_isoformat_local()`
    /// at construction time, matching `field(default_factory=lambda: datetime.now().isoformat())`.
    pub fn new(simulation_id: String, project_id: String, graph_id: String) -> Self {
        let now = python_isoformat_local();
        Self {
            simulation_id,
            project_id,
            graph_id,
            enable_twitter: true,
            enable_reddit: true,
            status: SimulationStatus::Created,
            entities_count: 0,
            profiles_count: 0,
            entity_types: Vec::new(),
            config_generated: false,
            config_reasoning: String::new(),
            current_round: 0,
            twitter_status: "not_started".to_string(),
            reddit_status: "not_started".to_string(),
            created_at: now.clone(),
            updated_at: now,
            error: None,
        }
    }

    /// Port of `SimulationState.to_dict()` (`simulation_manager.py:78-98`).
    ///
    /// Returns a `serde_json::Value::Object` with exactly **17 keys** in
    /// Python declaration order.  `status` is emitted as its lowercase string
    /// value (matching `self.status.value`).  `error` is `null` when `None`.
    ///
    /// Key order: simulation_id, project_id, graph_id, enable_twitter,
    /// enable_reddit, status, entities_count, profiles_count, entity_types,
    /// config_generated, config_reasoning, current_round, twitter_status,
    /// reddit_status, created_at, updated_at, error.
    ///
    /// S-666
    pub fn to_dict(&self) -> Value {
        // serde_json::Map (with preserve_order feature) is a LinkedHashMap that
        // maintains insertion order — identical to Python's ordered dict output.
        let mut map = Map::with_capacity(17);

        map.insert("simulation_id".to_string(), Value::String(self.simulation_id.clone()));
        map.insert("project_id".to_string(), Value::String(self.project_id.clone()));
        map.insert("graph_id".to_string(), Value::String(self.graph_id.clone()));
        map.insert("enable_twitter".to_string(), Value::Bool(self.enable_twitter));
        map.insert("enable_reddit".to_string(), Value::Bool(self.enable_reddit));
        // status emitted as lowercase string (.value), not as a struct
        map.insert("status".to_string(), Value::String(self.status.to_string()));
        map.insert("entities_count".to_string(), Value::Number(self.entities_count.into()));
        map.insert("profiles_count".to_string(), Value::Number(self.profiles_count.into()));
        map.insert(
            "entity_types".to_string(),
            Value::Array(self.entity_types.iter().map(|s| Value::String(s.clone())).collect()),
        );
        map.insert("config_generated".to_string(), Value::Bool(self.config_generated));
        map.insert("config_reasoning".to_string(), Value::String(self.config_reasoning.clone()));
        map.insert("current_round".to_string(), Value::Number(self.current_round.into()));
        map.insert("twitter_status".to_string(), Value::String(self.twitter_status.clone()));
        map.insert("reddit_status".to_string(), Value::String(self.reddit_status.clone()));
        map.insert("created_at".to_string(), Value::String(self.created_at.clone()));
        map.insert("updated_at".to_string(), Value::String(self.updated_at.clone()));
        // error: null when None, string when Some
        map.insert(
            "error".to_string(),
            match &self.error {
                Some(e) => Value::String(e.clone()),
                None => Value::Null,
            },
        );

        Value::Object(map)
    }

    /// Port of `SimulationState.to_simple_dict()` (`simulation_manager.py:100-112`).
    ///
    /// Returns a `serde_json::Value::Object` with exactly **9 keys** in
    /// Python declaration order (the subset used by the API layer).
    ///
    /// Key order: simulation_id, project_id, graph_id, status, entities_count,
    /// profiles_count, entity_types, config_generated, error.
    ///
    /// S-667
    pub fn to_simple_dict(&self) -> Value {
        let mut map = Map::with_capacity(9);

        map.insert("simulation_id".to_string(), Value::String(self.simulation_id.clone()));
        map.insert("project_id".to_string(), Value::String(self.project_id.clone()));
        map.insert("graph_id".to_string(), Value::String(self.graph_id.clone()));
        // status as lowercase string (.value)
        map.insert("status".to_string(), Value::String(self.status.to_string()));
        map.insert("entities_count".to_string(), Value::Number(self.entities_count.into()));
        map.insert("profiles_count".to_string(), Value::Number(self.profiles_count.into()));
        map.insert(
            "entity_types".to_string(),
            Value::Array(self.entity_types.iter().map(|s| Value::String(s.clone())).collect()),
        );
        map.insert("config_generated".to_string(), Value::Bool(self.config_generated));
        map.insert(
            "error".to_string(),
            match &self.error {
                Some(e) => Value::String(e.clone()),
                None => Value::Null,
            },
        );

        Value::Object(map)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- SimulationStatus serialization -------------------------------------

    /// Every status variant must round-trip through serde as its lowercase string.
    #[test]
    fn simulation_status_serde_all_variants() {
        let cases = [
            (SimulationStatus::Created, "\"created\""),
            (SimulationStatus::Preparing, "\"preparing\""),
            (SimulationStatus::Ready, "\"ready\""),
            (SimulationStatus::Running, "\"running\""),
            (SimulationStatus::Paused, "\"paused\""),
            (SimulationStatus::Stopped, "\"stopped\""),
            (SimulationStatus::Completed, "\"completed\""),
            (SimulationStatus::Failed, "\"failed\""),
        ];
        for (variant, expected_json) in &cases {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(
                json, *expected_json,
                "SimulationStatus::{:?} should serialize to {expected_json}",
                variant
            );
            // round-trip
            let back: SimulationStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, variant, "round-trip failed for {:?}", variant);
        }
    }

    /// Verify as_str returns the correct lowercase value for all 8 variants.
    #[test]
    fn simulation_status_as_str_all_variants() {
        assert_eq!(SimulationStatus::Created.as_str(), "created");
        assert_eq!(SimulationStatus::Preparing.as_str(), "preparing");
        assert_eq!(SimulationStatus::Ready.as_str(), "ready");
        assert_eq!(SimulationStatus::Running.as_str(), "running");
        assert_eq!(SimulationStatus::Paused.as_str(), "paused");
        assert_eq!(SimulationStatus::Stopped.as_str(), "stopped");
        assert_eq!(SimulationStatus::Completed.as_str(), "completed");
        assert_eq!(SimulationStatus::Failed.as_str(), "failed");
    }

    /// Display must match as_str.
    #[test]
    fn simulation_status_display() {
        assert_eq!(SimulationStatus::Running.to_string(), "running");
        assert_eq!(SimulationStatus::Failed.to_string(), "failed");
    }

    // -- PlatformType serialization -----------------------------------------

    /// Both platform variants round-trip correctly.
    #[test]
    fn platform_type_serde_all_variants() {
        let cases = [(PlatformType::Twitter, "\"twitter\""), (PlatformType::Reddit, "\"reddit\"")];
        for (variant, expected_json) in &cases {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(json, *expected_json);
            let back: PlatformType = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, variant);
        }
    }

    /// Exactly 2 variants — no BOTH variant exists.
    #[test]
    fn platform_type_has_exactly_two_variants() {
        // The only valid string values are "twitter" and "reddit".
        assert!(serde_json::from_str::<PlatformType>("\"twitter\"").is_ok());
        assert!(serde_json::from_str::<PlatformType>("\"reddit\"").is_ok());
        assert!(
            serde_json::from_str::<PlatformType>("\"both\"").is_err(),
            "PlatformType must NOT have a BOTH variant"
        );
    }

    // -- SimulationState construction defaults ------------------------------

    /// Verify all default field values match Python's dataclass defaults.
    #[test]
    fn simulation_state_new_defaults() {
        let s = SimulationState::new(
            "sim-001".to_string(),
            "proj-001".to_string(),
            "graph-001".to_string(),
        );
        assert_eq!(s.simulation_id, "sim-001");
        assert_eq!(s.project_id, "proj-001");
        assert_eq!(s.graph_id, "graph-001");
        assert!(s.enable_twitter, "enable_twitter default must be true");
        assert!(s.enable_reddit, "enable_reddit default must be true");
        assert_eq!(s.status, SimulationStatus::Created);
        assert_eq!(s.entities_count, 0);
        assert_eq!(s.profiles_count, 0);
        assert!(s.entity_types.is_empty(), "entity_types default must be []");
        assert!(!s.config_generated, "config_generated default must be false");
        assert_eq!(s.config_reasoning, "");
        assert_eq!(s.current_round, 0);
        assert_eq!(s.twitter_status, "not_started");
        assert_eq!(s.reddit_status, "not_started");
        assert!(!s.created_at.is_empty(), "created_at must be set on construction");
        assert!(!s.updated_at.is_empty(), "updated_at must be set on construction");
        assert!(s.error.is_none(), "error default must be None");
    }

    // -- to_dict key count and order ----------------------------------------

    /// to_dict must emit exactly 17 keys in Python declaration order.
    #[test]
    fn simulation_state_to_dict_key_count_and_order() {
        let s =
            SimulationState::new("sim-1".to_string(), "proj-1".to_string(), "graph-1".to_string());
        let dict = s.to_dict();
        let obj = dict.as_object().expect("to_dict must return a JSON object");
        assert_eq!(obj.len(), 17, "to_dict must emit exactly 17 keys");

        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        let expected = vec![
            "simulation_id",
            "project_id",
            "graph_id",
            "enable_twitter",
            "enable_reddit",
            "status",
            "entities_count",
            "profiles_count",
            "entity_types",
            "config_generated",
            "config_reasoning",
            "current_round",
            "twitter_status",
            "reddit_status",
            "created_at",
            "updated_at",
            "error",
        ];
        assert_eq!(keys, expected, "to_dict key order must match Python's to_dict()");
    }

    /// to_dict must emit status as its lowercase string value (not as a struct).
    #[test]
    fn simulation_state_to_dict_status_as_string() {
        let mut s = SimulationState::new("s".to_string(), "p".to_string(), "g".to_string());
        s.status = SimulationStatus::Running;
        let dict = s.to_dict();
        let obj = dict.as_object().unwrap();
        assert_eq!(obj["status"], Value::String("running".to_string()));
    }

    /// to_dict with error=None must emit `"error": null`.
    #[test]
    fn simulation_state_to_dict_error_null_when_none() {
        let s = SimulationState::new("s".to_string(), "p".to_string(), "g".to_string());
        let dict = s.to_dict();
        let obj = dict.as_object().unwrap();
        assert_eq!(obj["error"], Value::Null, "error=None must serialize as JSON null");
    }

    /// to_dict with error=Some("msg") must emit `"error": "msg"`.
    #[test]
    fn simulation_state_to_dict_error_string_when_some() {
        let mut s = SimulationState::new("s".to_string(), "p".to_string(), "g".to_string());
        s.error = Some("something went wrong".to_string());
        let dict = s.to_dict();
        let obj = dict.as_object().unwrap();
        assert_eq!(obj["error"], Value::String("something went wrong".to_string()));
    }

    /// to_dict entity_types must serialise as a JSON array of strings.
    #[test]
    fn simulation_state_to_dict_entity_types() {
        let mut s = SimulationState::new("s".to_string(), "p".to_string(), "g".to_string());
        s.entity_types = vec!["Person".to_string(), "Organization".to_string()];
        let dict = s.to_dict();
        let obj = dict.as_object().unwrap();
        let arr = obj["entity_types"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], Value::String("Person".to_string()));
        assert_eq!(arr[1], Value::String("Organization".to_string()));
    }

    /// to_dict with default enable flags must emit bool true for both.
    #[test]
    fn simulation_state_to_dict_enable_flags_default_true() {
        let s = SimulationState::new("s".to_string(), "p".to_string(), "g".to_string());
        let dict = s.to_dict();
        let obj = dict.as_object().unwrap();
        assert_eq!(obj["enable_twitter"], Value::Bool(true));
        assert_eq!(obj["enable_reddit"], Value::Bool(true));
    }

    // -- to_simple_dict key count and order ---------------------------------

    /// to_simple_dict must emit exactly 9 keys in Python declaration order.
    #[test]
    fn simulation_state_to_simple_dict_key_count_and_order() {
        let s =
            SimulationState::new("sim-1".to_string(), "proj-1".to_string(), "graph-1".to_string());
        let dict = s.to_simple_dict();
        let obj = dict.as_object().expect("to_simple_dict must return a JSON object");
        assert_eq!(obj.len(), 9, "to_simple_dict must emit exactly 9 keys");

        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        let expected = vec![
            "simulation_id",
            "project_id",
            "graph_id",
            "status",
            "entities_count",
            "profiles_count",
            "entity_types",
            "config_generated",
            "error",
        ];
        assert_eq!(keys, expected, "to_simple_dict key order must match Python's to_simple_dict()");
    }

    /// to_simple_dict status must also be a lowercase string.
    #[test]
    fn simulation_state_to_simple_dict_status_as_string() {
        let mut s = SimulationState::new("s".to_string(), "p".to_string(), "g".to_string());
        s.status = SimulationStatus::Completed;
        let dict = s.to_simple_dict();
        let obj = dict.as_object().unwrap();
        assert_eq!(obj["status"], Value::String("completed".to_string()));
    }

    /// to_simple_dict error=None → null.
    #[test]
    fn simulation_state_to_simple_dict_error_null() {
        let s = SimulationState::new("s".to_string(), "p".to_string(), "g".to_string());
        let dict = s.to_simple_dict();
        let obj = dict.as_object().unwrap();
        assert_eq!(obj["error"], Value::Null);
    }

    /// to_simple_dict error=Some("e") → string.
    #[test]
    fn simulation_state_to_simple_dict_error_string() {
        let mut s = SimulationState::new("s".to_string(), "p".to_string(), "g".to_string());
        s.error = Some("fatal error".to_string());
        let dict = s.to_simple_dict();
        let obj = dict.as_object().unwrap();
        assert_eq!(obj["error"], Value::String("fatal error".to_string()));
    }

    // -- Status-variant propagation through to_dict/to_simple_dict ---------

    /// Each of the 8 status values should survive the to_dict round-trip.
    #[test]
    fn simulation_state_to_dict_all_status_variants() {
        let variants = [
            (SimulationStatus::Created, "created"),
            (SimulationStatus::Preparing, "preparing"),
            (SimulationStatus::Ready, "ready"),
            (SimulationStatus::Running, "running"),
            (SimulationStatus::Paused, "paused"),
            (SimulationStatus::Stopped, "stopped"),
            (SimulationStatus::Completed, "completed"),
            (SimulationStatus::Failed, "failed"),
        ];
        for (variant, expected_str) in variants {
            let mut s = SimulationState::new("s".to_string(), "p".to_string(), "g".to_string());
            s.status = variant;
            let dict = s.to_dict();
            let obj = dict.as_object().unwrap();
            assert_eq!(
                obj["status"],
                Value::String(expected_str.to_string()),
                "to_dict status for {expected_str}"
            );
            let simple = s.to_simple_dict();
            let simple_obj = simple.as_object().unwrap();
            assert_eq!(
                simple_obj["status"],
                Value::String(expected_str.to_string()),
                "to_simple_dict status for {expected_str}"
            );
        }
    }

    // -- Full default-state JSON snapshot -----------------------------------

    /// Snapshot: a freshly-constructed SimulationState serializes to the
    /// exact shape expected by the Python API (key names, types, null/false
    /// defaults — NOT timestamps which are non-deterministic).
    #[test]
    fn simulation_state_to_dict_shape_snapshot() {
        let s = SimulationState::new("abc".to_string(), "xyz".to_string(), "ggg".to_string());
        let dict = s.to_dict();
        let obj = dict.as_object().unwrap();

        assert_eq!(obj["simulation_id"], Value::String("abc".to_string()));
        assert_eq!(obj["project_id"], Value::String("xyz".to_string()));
        assert_eq!(obj["graph_id"], Value::String("ggg".to_string()));
        assert_eq!(obj["enable_twitter"], Value::Bool(true));
        assert_eq!(obj["enable_reddit"], Value::Bool(true));
        assert_eq!(obj["status"], Value::String("created".to_string()));
        assert_eq!(obj["entities_count"], Value::Number(0.into()));
        assert_eq!(obj["profiles_count"], Value::Number(0.into()));
        assert_eq!(obj["entity_types"], Value::Array(vec![]));
        assert_eq!(obj["config_generated"], Value::Bool(false));
        assert_eq!(obj["config_reasoning"], Value::String(String::new()));
        assert_eq!(obj["current_round"], Value::Number(0.into()));
        assert_eq!(obj["twitter_status"], Value::String("not_started".to_string()));
        assert_eq!(obj["reddit_status"], Value::String("not_started".to_string()));
        // created_at / updated_at: non-empty ISO strings (non-deterministic, just check type)
        assert!(obj["created_at"].is_string());
        assert!(obj["updated_at"].is_string());
        assert_eq!(obj["error"], Value::Null);
    }
}

// =============================================================================
// SimulationManager — sub-cycle (c)
// =============================================================================
//
// Port of `SimulationManager` class (`simulation_manager.py` L115-529).
//
// ## Interior-mutability choice (S-670)
//
// Python's methods take `self` (not explicit mutation) but mutate `self._simulations`
// dict in-place.  In Rust, callers (including the axum API layer) hold `Arc<SimulationManager>`
// and call methods via `&self`.  We therefore wrap the cache in `Mutex<HashMap>` for interior
// mutability — this is the idiomatic teri/axum pattern for shared state (consistent with how
// `AppState` holds other services as `Arc<T>` with interior Mutex).
//
// ## SIMULATION_DATA_DIR convention (S-669)
//
// Python computes `SIMULATION_DATA_DIR` relative to the module file
// (`../../uploads/simulations` from `services/`).  In teri this is the
// `oasis_simulation_data_dir` config field (env `OASIS_SIMULATION_DATA_DIR`,
// default `"./uploads/simulations"`).  The `from_config` constructor takes that
// value; `new(path)` is provided for tests (matching the `ProjectManager` pattern).
//
// ## get_run_instructions native guidance (S-680) — DECISION-U026-2
//
// The Python `get_run_instructions` returned OASIS Python-script subprocess commands:
//   `python {scripts_dir}/run_twitter_simulation.py --config {config_path}`
//   `conda activate MiroFish`  etc.
//
// DECISION-U026-2 (EXTEND-X, NOT [≠]): teri now emits NATIVE run-guidance.
// Python's per-platform subprocess strings become per-platform HTTP start calls:
//   `POST /api/simulation/start  body: {"simulation_id":"…","platform":"twitter"}`
//   `POST /api/simulation/start  body: {"simulation_id":"…","platform":"reddit"}`
//   `POST /api/simulation/start  body: {"simulation_id":"…","platform":"parallel"}`
//
// The [≠] is NARROWED to only: `scripts_dir` (no scripts dir in teri) and the
// literal `python run_*.py` / `conda activate` strings (no Python, no conda).
// Everything else is NATIVE-EXPRESSED: simulation_dir, config_file, commands (map),
// instructions (string), substrate_note (retained for self-documentation).
// The carry-forward gate from U-023 (S-680 partial) is SATISFIED by this unit.

// ---------------------------------------------------------------------------
// RunInstructions
// ---------------------------------------------------------------------------

/// Per-platform native start invocations (mirrors Python `commands{twitter,reddit,parallel}`).
///
/// Python emitted shell commands like `python run_twitter_simulation.py --config …`.
/// teri's native analog is the HTTP start call with the platform in the JSON body.
///
/// S-680 (DECISION-U026-2)
#[derive(Debug, Clone)]
pub struct RunCommands {
    /// Native invocation string for the Twitter platform.
    pub twitter: String,
    /// Native invocation string for the Reddit platform.
    pub reddit: String,
    /// Native invocation string for the parallel (both-platform) run.
    pub parallel: String,
}

/// Return value of [`SimulationManager::get_run_instructions`].
///
/// Native teri run-guidance for a prepared (READY) simulation.
/// Python returned Python-script subprocess commands; teri returns the native
/// in-process invocation: `POST /api/simulation/start` (SimulationRunner→SimEngine).
///
/// DECISION-U026-2: `[≠]` narrowed to `scripts_dir` and the literal Python/conda strings only.
/// All other fields (`simulation_dir`, `config_file`, `commands`, `instructions`) are now
/// NATIVE-EXPRESSED. The carry-forward gate from U-023 (S-680) is satisfied.
///
/// S-680
#[derive(Debug, Clone)]
pub struct RunInstructions {
    /// Absolute path to the simulation's data directory.
    ///
    /// Port of Python `"simulation_dir"` key.
    pub simulation_dir: PathBuf,

    /// Path to `simulation_config.json` inside the simulation directory.
    ///
    /// Port of Python `"config_file"` key.
    pub config_file: PathBuf,

    /// NATIVE analog of Python `"commands"`. Per-platform native HTTP start invocations.
    /// Keys: twitter, reddit, parallel (same three as Python). Values: the HTTP start call
    /// carrying that platform — `POST /api/simulation/start  body: {"simulation_id":"…","platform":"…"}`.
    ///
    /// DECISION-U026-2 (NATIVE-EXPRESSED).
    pub commands: RunCommands,

    /// NATIVE analog of Python `"instructions"`. Human-readable description of the
    /// in-process SimEngine run path (no conda, no Python scripts).
    ///
    /// DECISION-U026-2 (NATIVE-EXPRESSED).
    pub instructions: String,

    /// [≠]-substrate marker: documents that `scripts_dir` / Python-script / conda commands
    /// are inexpressible in teri's substrate. Retained so existing test coverage stays green
    /// and the gap remains self-documenting in the API payload.
    ///
    /// [≠] residual: `scripts_dir` (no scripts dir in teri) + Python/conda literal strings.
    pub substrate_note: &'static str,
}

impl RunInstructions {
    /// Serialize to a `serde_json::Value::Object` using `serde_json::Map` (preserve_order).
    ///
    /// Exact key order (matches DECISION-U026-2 contract):
    /// `simulation_dir`, `config_file`, `commands` (nested `{twitter,reddit,parallel}`),
    /// `instructions`, `substrate_note`.
    ///
    /// `scripts_dir` is the ONLY omitted Python key ([≠] `scripts_dir` — teri has no scripts dir).
    pub fn to_dict(&self) -> Value {
        let mut map = Map::with_capacity(5);

        map.insert(
            "simulation_dir".to_string(),
            Value::String(self.simulation_dir.to_string_lossy().into_owned()),
        );
        map.insert(
            "config_file".to_string(),
            Value::String(self.config_file.to_string_lossy().into_owned()),
        );

        // commands: nested object with twitter/reddit/parallel (Python key order)
        let mut cmds = Map::with_capacity(3);
        cmds.insert("twitter".to_string(), Value::String(self.commands.twitter.clone()));
        cmds.insert("reddit".to_string(), Value::String(self.commands.reddit.clone()));
        cmds.insert("parallel".to_string(), Value::String(self.commands.parallel.clone()));
        map.insert("commands".to_string(), Value::Object(cmds));

        map.insert("instructions".to_string(), Value::String(self.instructions.clone()));
        map.insert("substrate_note".to_string(), Value::String(self.substrate_note.to_string()));

        Value::Object(map)
    }
}

// ---------------------------------------------------------------------------
// SimulationManager
// ---------------------------------------------------------------------------

/// Filesystem-backed simulation manager.
///
/// Port of `SimulationManager` (`simulation_manager.py:115-529`, sub-cycles c/d).
///
/// Holds an in-memory cache (`_simulations` in Python) guarded by a `Mutex` for
/// interior mutability so callers can hold `Arc<SimulationManager>` and call methods
/// via `&self` (matching the axum `AppState` sharing pattern).
///
/// # Symbols
/// S-668 (type), S-669 (SIMULATION_DATA_DIR / `sim_data_dir` field),
/// S-670 (`__init__` / `new`/`from_config`), S-671..S-680 (methods).
pub struct SimulationManager {
    /// S-669: equivalent of Python's `SIMULATION_DATA_DIR` class variable.
    /// In Python: `os.path.join(os.path.dirname(__file__), '../../uploads/simulations')`.
    /// In teri: `config.oasis_simulation_data_dir` (env `OASIS_SIMULATION_DATA_DIR`,
    /// default `"./uploads/simulations"`).
    sim_data_dir: PathBuf,

    /// S-670: `self._simulations` — in-memory cache of loaded states.
    /// Python mutates this freely from `self`; Rust uses `Mutex` for interior mutability.
    cache: Mutex<HashMap<String, SimulationState>>,
}

impl SimulationManager {
    // -----------------------------------------------------------------------
    // Constructors (S-670)
    // -----------------------------------------------------------------------

    /// Create a `SimulationManager` pointed at an explicit directory.
    /// Used in tests; production callers use `from_config`.
    ///
    /// Faithfully implements Python's `os.makedirs(self.SIMULATION_DATA_DIR, exist_ok=True)`
    /// (the directory is created lazily on first use in each method, matching Python's
    /// per-call `_get_simulation_dir` → `os.makedirs`).
    pub fn new(sim_data_dir: impl Into<PathBuf>) -> Self {
        SimulationManager { sim_data_dir: sim_data_dir.into(), cache: Mutex::new(HashMap::new()) }
    }

    /// Create a `SimulationManager` from teri's `Config`.
    ///
    /// Uses `config.oasis_simulation_data_dir` (env `OASIS_SIMULATION_DATA_DIR`,
    /// default `"./uploads/simulations"`) — the teri equivalent of Python's
    /// `SIMULATION_DATA_DIR = os.path.join(dirname(__file__), '../../uploads/simulations')`.
    pub fn from_config(config: &crate::config::Config) -> Self {
        SimulationManager::new(Path::new(&config.oasis_simulation_data_dir).to_path_buf())
    }

    /// Evict a single entry from the in-memory cache.
    ///
    /// Test-only helper: after patching `state.json` on disk, call this so the next
    /// `get_simulation` re-reads the file rather than returning the cached (stale) state.
    #[cfg(test)]
    pub fn evict_cache_for_test(&self, simulation_id: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(simulation_id);
        }
    }

    // -----------------------------------------------------------------------
    // _get_simulation_dir (S-671)
    // -----------------------------------------------------------------------

    /// Return the path to `{sim_data_dir}/{simulation_id}`, creating it if absent.
    ///
    /// Port of `_get_simulation_dir` (`simulation_manager.py:139-143`).
    ///
    /// Python: `sim_dir = os.path.join(SIMULATION_DATA_DIR, simulation_id); os.makedirs(sim_dir, exist_ok=True); return sim_dir`
    pub(crate) fn get_simulation_dir(&self, simulation_id: &str) -> Result<PathBuf> {
        let sim_dir = self.sim_data_dir.join(simulation_id);
        std::fs::create_dir_all(&sim_dir)?;
        Ok(sim_dir)
    }

    // -----------------------------------------------------------------------
    // _save_simulation_state (S-672)
    // -----------------------------------------------------------------------

    /// Bump `state.updated_at`, write `{sim_dir}/state.json`, update cache.
    ///
    /// Port of `_save_simulation_state` (`simulation_manager.py:145-155`).
    ///
    /// Python order (faithfully matched):
    ///   1. `state.updated_at = datetime.now().isoformat()`
    ///   2. `json.dump(state.to_dict(), f, ensure_ascii=False, indent=2)`
    ///   3. `self._simulations[state.simulation_id] = state`
    ///
    /// `ensure_ascii=False` + `indent=2` is matched by `serde_json::to_string_pretty`
    /// which does NOT escape non-ASCII (UTF-8 raw), with 2-space indentation.
    // [!] SAVE-STATE-VISIBILITY — made pub(crate) for /start + /stop handlers (U-026 g1/g2).
    pub(crate) fn save_simulation_state(&self, state: &mut SimulationState) -> Result<()> {
        // Step 1: bump updated_at (Python `datetime.now().isoformat()`)
        state.updated_at = python_isoformat_local();

        // Step 2: write state.json
        let sim_dir = self.get_simulation_dir(&state.simulation_id)?;
        let state_file = sim_dir.join("state.json");
        // serde_json::to_string_pretty: 2-space indent, no ASCII escaping of non-ASCII chars.
        let json = serde_json::to_string_pretty(&state.to_dict())?;
        std::fs::write(&state_file, json.as_bytes())?;

        // Step 3: update cache
        let mut cache = self.cache.lock().expect("simulation_manager cache lock poisoned");
        cache.insert(state.simulation_id.clone(), state.clone());

        Ok(())
    }

    // -----------------------------------------------------------------------
    // _load_simulation_state (S-673)
    // -----------------------------------------------------------------------

    /// Cache-first load of simulation state.
    ///
    /// Port of `_load_simulation_state` (`simulation_manager.py:157-192`).
    ///
    /// Behavior (faithfully matched):
    ///   1. Return cached state if present (Python `if simulation_id in self._simulations`)
    ///   2. Check `{sim_dir}/state.json` exists — if not, return `None`
    ///      (Python `if not os.path.exists(state_file): return None`)
    ///   3. Parse JSON with `.get(key, default)` tolerance for every field
    ///   4. Invalid status string → `Err` (Python `SimulationStatus(str)` raises `ValueError`)
    ///   5. Cache and return
    ///
    /// Field defaults (matching Python L171-189 exactly):
    ///   project_id        → `""`
    ///   graph_id          → `""`
    ///   enable_twitter    → `true`
    ///   enable_reddit     → `true`
    ///   status            → `"created"` (string default, then parsed)
    ///   entities_count    → `0`
    ///   profiles_count    → `0`
    ///   entity_types      → `[]`
    ///   config_generated  → `false`
    ///   config_reasoning  → `""`
    ///   current_round     → `0`
    ///   twitter_status    → `"not_started"`
    ///   reddit_status     → `"not_started"`
    ///   created_at        → `datetime.now().isoformat()` (i.e. a fresh local timestamp)
    ///   updated_at        → `datetime.now().isoformat()`
    ///   error             → `None`
    fn load_simulation_state(&self, simulation_id: &str) -> Result<Option<SimulationState>> {
        // Step 1: cache-first
        {
            let cache = self.cache.lock().expect("simulation_manager cache lock poisoned");
            if let Some(state) = cache.get(simulation_id) {
                return Ok(Some(state.clone()));
            }
        }

        // Step 2: check FS (note: _get_simulation_dir creates the dir, so we check the file)
        let sim_dir = self.get_simulation_dir(simulation_id)?;
        let state_file = sim_dir.join("state.json");

        if !state_file.exists() {
            return Ok(None);
        }

        // Step 3: parse JSON
        let raw = std::fs::read_to_string(&state_file)?;
        let data: Value = serde_json::from_str(&raw)?;

        let obj = data.as_object().ok_or_else(|| {
            TeriError::Sim(format!("state.json for {simulation_id} is not a JSON object"))
        })?;

        let now = python_isoformat_local();

        let project_id = obj.get("project_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let graph_id = obj.get("graph_id").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let enable_twitter = obj.get("enable_twitter").and_then(|v| v.as_bool()).unwrap_or(true);

        let enable_reddit = obj.get("enable_reddit").and_then(|v| v.as_bool()).unwrap_or(true);

        // Step 4: status — Python `SimulationStatus(data.get("status", "created"))`.
        // An invalid string raises ValueError in Python; we return Err here (faithful).
        let status_str = obj.get("status").and_then(|v| v.as_str()).unwrap_or("created");
        let status: SimulationStatus =
            serde_json::from_value(Value::String(status_str.to_string())).map_err(|_| {
                TeriError::Sim(format!(
                    "invalid SimulationStatus {status_str:?} in {simulation_id}/state.json \
                 (Python SimulationStatus(str) raises ValueError on unknown value)"
                ))
            })?;

        let entities_count = obj.get("entities_count").and_then(|v| v.as_i64()).unwrap_or(0);

        let profiles_count = obj.get("profiles_count").and_then(|v| v.as_i64()).unwrap_or(0);

        let entity_types: Vec<String> = obj
            .get("entity_types")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
            .unwrap_or_default();

        let config_generated =
            obj.get("config_generated").and_then(|v| v.as_bool()).unwrap_or(false);

        let config_reasoning =
            obj.get("config_reasoning").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let current_round = obj.get("current_round").and_then(|v| v.as_i64()).unwrap_or(0);

        let twitter_status = obj
            .get("twitter_status")
            .and_then(|v| v.as_str())
            .unwrap_or("not_started")
            .to_string();

        let reddit_status = obj
            .get("reddit_status")
            .and_then(|v| v.as_str())
            .unwrap_or("not_started")
            .to_string();

        // Python: `data.get("created_at", datetime.now().isoformat())`
        let created_at = obj.get("created_at").and_then(|v| v.as_str()).unwrap_or(&now).to_string();

        // Python: `data.get("updated_at", datetime.now().isoformat())`
        let updated_at = obj.get("updated_at").and_then(|v| v.as_str()).unwrap_or(&now).to_string();

        // Python: `data.get("error")` — None if key absent or value is null
        let error = obj.get("error").and_then(|v| v.as_str()).map(str::to_string);

        let state = SimulationState {
            simulation_id: simulation_id.to_string(),
            project_id,
            graph_id,
            enable_twitter,
            enable_reddit,
            status,
            entities_count,
            profiles_count,
            entity_types,
            config_generated,
            config_reasoning,
            current_round,
            twitter_status,
            reddit_status,
            created_at,
            updated_at,
            error,
        };

        // Step 5: cache + return
        let mut cache = self.cache.lock().expect("simulation_manager cache lock poisoned");
        cache.insert(simulation_id.to_string(), state.clone());

        Ok(Some(state))
    }

    // -----------------------------------------------------------------------
    // create_simulation (S-674)
    // -----------------------------------------------------------------------

    /// Create a new simulation, persist its initial state, and return it.
    ///
    /// Port of `create_simulation` (`simulation_manager.py:194-228`).
    ///
    /// `simulation_id` format: `"sim_"` + first 12 hex chars of a random UUID v4.
    /// This matches Python `f"sim_{uuid.uuid4().hex[:12]}"` exactly.
    ///
    /// Python defaults: `enable_twitter=True`, `enable_reddit=True`.
    pub fn create_simulation(
        &self,
        project_id: &str,
        graph_id: &str,
        enable_twitter: bool,
        enable_reddit: bool,
    ) -> Result<SimulationState> {
        // sim_id = f"sim_{uuid.uuid4().hex[:12]}"  (12 lowercase hex chars, no hyphens)
        let hex = Uuid::new_v4().simple().to_string(); // 32 hex chars, no hyphens
        let simulation_id = format!("sim_{}", &hex[..12]);

        let mut state = SimulationState {
            simulation_id,
            project_id: project_id.to_string(),
            graph_id: graph_id.to_string(),
            enable_twitter,
            enable_reddit,
            status: SimulationStatus::Created,
            entities_count: 0,
            profiles_count: 0,
            entity_types: Vec::new(),
            config_generated: false,
            config_reasoning: String::new(),
            current_round: 0,
            twitter_status: "not_started".to_string(),
            reddit_status: "not_started".to_string(),
            created_at: python_isoformat_local(),
            updated_at: python_isoformat_local(),
            error: None,
        };

        self.save_simulation_state(&mut state)?;

        tracing::info!(
            simulation_id = %state.simulation_id,
            project_id = %project_id,
            graph_id = %graph_id,
            "created simulation"
        );

        Ok(state)
    }

    // -----------------------------------------------------------------------
    // prepare_simulation (S-675)
    // -----------------------------------------------------------------------

    /// Fully prepare a simulation: read entities → generate profiles → generate config.
    ///
    /// Port of `prepare_simulation` (`simulation_manager.py:230-458`).
    ///
    /// ## 4-stage pipeline (faithful to source)
    ///
    /// **Pre-load:** `load_simulation_state(simulation_id)` → `None` → `Err` with
    ///   Chinese ValueError message `"模拟不存在: {simulation_id}"` (Python L263-264).
    ///   Uses `TeriError::Sim` (maps Python's `ValueError` — the established pattern
    ///   in this file for "business-logic precondition failure").
    ///
    /// **Exception wrapper:** stages 1-3 run under a try/except: on any error, sets
    ///   `state.status = FAILED`, `state.error = e.to_string()`, saves state, returns `Err`
    ///   (Python `except Exception as e: … state.status=FAILED; raise`).
    ///
    /// **Stage 1 ("reading"):** `KnowledgeGraphEntityReader::new(graph)` →
    ///   `filter_defined_entities(defined_entity_types, enrich_with_edges=true)`.
    ///   Sets `entities_count`/`entity_types`.  Zero entities → FAILED + **`Ok(state)`**
    ///   (Python L298-302: `return state` — Ok return, NOT raise).
    ///
    /// **Stage 2 ("generating_profiles"):** realtime_output = Reddit > Twitter > None.
    ///   Calls `generate_profiles_from_entities` with live `parallel_count` knob.
    ///   Final saves: `enable_reddit` → reddit_profiles.json; `enable_twitter` →
    ///   twitter_profiles.csv (two independent `if` branches, NOT elif — Python L361-374).
    ///
    /// **Stage 3 ("generating_config"):** `config_generator.generate_config(…).await`.
    ///   Writes `simulation_config.json`; sets `config_generated=true`, `config_reasoning`.
    ///
    /// **Finish:** status=READY, save, `tracing::info!`, return `Ok(state)`.
    ///
    /// ## No `force_regenerate`
    /// The source has NO stage-skipping flag — every run always executes all 3 stages.
    ///
    /// S-675
    #[allow(clippy::too_many_arguments)]
    pub async fn prepare_simulation<L: crate::llm::LlmClient>(
        &self,
        simulation_id: &str,
        simulation_requirement: &str,
        document_text: &str,
        defined_entity_types: Option<&[String]>,
        use_llm_for_profiles: bool,
        parallel_profile_count: usize,
        llm: &L,
        graph: &crate::graph::KnowledgeGraph,
        persona_generator: &crate::agent::PersonaGenerator,
        config_generator: &crate::services::simulation_config::SimulationConfigGenerator<L>,
        mut progress_callback: Option<&mut dyn FnMut(PrepareProgress<'_>)>,
    ) -> Result<SimulationState> {
        use crate::i18n::{t, t_args};
        use crate::services::entity_reader::KnowledgeGraphEntityReader;
        use crate::services::oasis_profile_export::{
            OutputPlatform, generate_profiles_from_entities, save_profiles,
        };

        // --- Pre-load: missing simulation → Err (Python L262-264: raise ValueError) ---
        let mut state = match self.load_simulation_state(simulation_id)? {
            Some(s) => s,
            None => {
                return Err(TeriError::Sim(format!("模拟不存在: {simulation_id}")));
            }
        };

        // --- status = PREPARING + save (Python L267-268) ---
        state.status = SimulationStatus::Preparing;
        self.save_simulation_state(&mut state)?;

        let sim_dir = self.get_simulation_dir(simulation_id)?;

        // ===== Stage 1: Reading entities (Python L272-302) =====

        // reading/0 — "正在连接Zep图谱..."
        if let Some(cb) = progress_callback.as_mut() {
            cb(PrepareProgress {
                stage: "reading",
                progress: 0,
                message: t("progress.connectingZepGraph"),
                current: None,
                total: None,
            });
        }

        let reader = KnowledgeGraphEntityReader::new(graph);

        // reading/30 — "正在读取节点数据..."
        if let Some(cb) = progress_callback.as_mut() {
            cb(PrepareProgress {
                stage: "reading",
                progress: 30,
                message: t("progress.readingNodeData"),
                current: None,
                total: None,
            });
        }

        let filtered = reader.filter_defined_entities(defined_entity_types, true);

        state.entities_count = filtered.filtered_count;
        // list(filtered.entity_types) — Python set→list (unspecified order in both)
        state.entity_types = filtered.entity_types.iter().cloned().collect();

        // reading/100 — "完成，共 N 个实体"
        if let Some(cb) = progress_callback.as_mut() {
            cb(PrepareProgress {
                stage: "reading",
                progress: 100,
                message: t_args("progress.readingComplete", &[("count", &filtered.filtered_count)]),
                current: Some(filtered.filtered_count),
                total: Some(filtered.filtered_count),
            });
        }

        // Zero entities → FAILED, save, return Ok(state)
        // Python L298-302: `return state` (NOT raise — this is the Ok path, NOT Err).
        // This is a contractual distinction: zero-entities is a FAILED terminal state,
        // not an exception; the route layer gets Ok(state) with status=FAILED.
        if filtered.filtered_count == 0 {
            state.status = SimulationStatus::Failed;
            state.error = Some("没有找到符合条件的实体，请检查图谱是否正确构建".to_string());
            self.save_simulation_state(&mut state)?;
            return Ok(state);
        }

        // --- Exception wrapper: stages 2+3 run under try/except (Python L266+L450) ---
        // Any error from stages 2-3 → FAILED + save + re-raise.
        // We run the stages inline with `?`; a macro captures errors to apply FAILED-save.
        macro_rules! try_stage {
            ($expr:expr) => {
                match $expr {
                    Ok(v) => v,
                    Err(e) => {
                        // Python L450-457: except Exception as e: … raise
                        tracing::error!("模拟准备失败: {}, error={}", simulation_id, e);
                        state.status = SimulationStatus::Failed;
                        state.error = Some(e.to_string());
                        // Best-effort save; if this fails too we still return original error.
                        let _ = self.save_simulation_state(&mut state);
                        return Err(e);
                    }
                }
            };
        }

        // ===== Stage 2: Generating profiles (Python L304-382) =====

        let total_entities = filtered.entities.len();

        // generating_profiles/0 — "开始生成..." current=0 total=N
        if let Some(cb) = progress_callback.as_mut() {
            cb(PrepareProgress {
                stage: "generating_profiles",
                progress: 0,
                message: t("progress.startGenerating"),
                current: Some(0),
                total: Some(total_entities as i64),
            });
        }

        // Realtime output path: Reddit > Twitter > None (Python L329-337)
        let realtime_output: Option<(std::path::PathBuf, OutputPlatform)> = if state.enable_reddit {
            Some((sim_dir.join("reddit_profiles.json"), OutputPlatform::Reddit))
        } else if state.enable_twitter {
            Some((sim_dir.join("twitter_profiles.csv"), OutputPlatform::Twitter))
        } else {
            None
        };

        // Build the profile_progress closure (Python L318-327).
        // Maps (current, total, msg) → generating_profiles/pct/msg with current/total.
        //
        // We need to pass `progress_callback` into the inner closure while also using it
        // after the generate call. We use a raw pointer to avoid a second mutable borrow.
        // SAFETY: The closure is invoked synchronously from `generate_profiles_from_entities`
        // (via buffer_unordered on the current task — no spawn), so `cb_ptr` is always
        // valid and there is no concurrent aliasing. The pointer's target (`progress_callback`)
        // lives on this function's stack and outlives the closure.
        let cb_ptr: *mut Option<&mut dyn FnMut(PrepareProgress<'_>)> = &raw mut progress_callback;

        let mut profile_progress = |current: i64, total: i64, msg: String| {
            let cb = unsafe { &mut *cb_ptr };
            if let Some(outer_cb) = cb.as_mut() {
                let pct = if total > 0 { (current * 100) / total } else { 0 };
                outer_cb(PrepareProgress {
                    stage: "generating_profiles",
                    progress: pct,
                    message: msg,
                    current: Some(current),
                    total: Some(total),
                });
            }
        };

        let rt_ref: Option<(&std::path::Path, OutputPlatform)> =
            realtime_output.as_ref().map(|(p, pl)| (p.as_path(), *pl));

        let profiles = generate_profiles_from_entities(
            persona_generator,
            llm,
            &filtered.entities,
            Some(graph),
            use_llm_for_profiles,
            parallel_profile_count,
            rt_ref,
            &mut profile_progress,
        )
        .await;

        // Drop the raw-ptr closure before next use of progress_callback.
        let _ = profile_progress;

        state.profiles_count = profiles.len() as i64;

        // generating_profiles/95 — "保存Profile文件..." current=N total=N
        if let Some(cb) = progress_callback.as_mut() {
            cb(PrepareProgress {
                stage: "generating_profiles",
                progress: 95,
                message: t("progress.savingProfiles"),
                current: Some(total_entities as i64),
                total: Some(total_entities as i64),
            });
        }

        // Final saves (Python L361-374: two independent `if` branches, NOT elif).
        if state.enable_reddit {
            try_stage!(
                save_profiles(
                    &profiles,
                    &sim_dir.join("reddit_profiles.json"),
                    OutputPlatform::Reddit,
                )
                .map_err(TeriError::from)
            );
        }
        if state.enable_twitter {
            try_stage!(
                save_profiles(
                    &profiles,
                    &sim_dir.join("twitter_profiles.csv"),
                    OutputPlatform::Twitter,
                )
                .map_err(TeriError::from)
            );
        }

        // generating_profiles/100 — "完成，共 N 个Profile"
        let profile_count = profiles.len();
        if let Some(cb) = progress_callback.as_mut() {
            cb(PrepareProgress {
                stage: "generating_profiles",
                progress: 100,
                message: t_args("progress.profilesComplete", &[("count", &(profile_count as i64))]),
                current: Some(profile_count as i64),
                total: Some(profile_count as i64),
            });
        }

        // ===== Stage 3: Generating config (Python L384-436) =====

        // generating_config/0 — "正在分析模拟需求..." current=0 total=3
        if let Some(cb) = progress_callback.as_mut() {
            cb(PrepareProgress {
                stage: "generating_config",
                progress: 0,
                message: t("progress.analyzingRequirements"),
                current: Some(0),
                total: Some(3),
            });
        }

        // generating_config/30 — "正在调用LLM生成配置..." current=1 total=3
        if let Some(cb) = progress_callback.as_mut() {
            cb(PrepareProgress {
                stage: "generating_config",
                progress: 30,
                message: t("progress.callingLLMConfig"),
                current: Some(1),
                total: Some(3),
            });
        }

        let sim_params = config_generator
            .generate_config(
                simulation_id,
                state.project_id.as_str(),
                state.graph_id.as_str(),
                simulation_requirement,
                document_text,
                &filtered.entities,
                state.enable_twitter,
                state.enable_reddit,
                None,
            )
            .await;

        // generating_config/70 — "正在保存配置文件..." current=2 total=3
        if let Some(cb) = progress_callback.as_mut() {
            cb(PrepareProgress {
                stage: "generating_config",
                progress: 70,
                message: t("progress.savingConfigFiles"),
                current: Some(2),
                total: Some(3),
            });
        }

        // Write simulation_config.json (Python L423-425: open+write as UTF-8)
        let config_path = sim_dir.join("simulation_config.json");
        try_stage!(
            std::fs::write(&config_path, sim_params.to_json().as_bytes()).map_err(TeriError::from)
        );

        state.config_generated = true;
        state.config_reasoning = sim_params.generation_reasoning.clone();

        // generating_config/100 — "配置生成完成" current=3 total=3
        if let Some(cb) = progress_callback.as_mut() {
            cb(PrepareProgress {
                stage: "generating_config",
                progress: 100,
                message: t("progress.configComplete"),
                current: Some(3),
                total: Some(3),
            });
        }

        // --- Finish: status=READY, save, log (Python L441-448) ---
        state.status = SimulationStatus::Ready;
        self.save_simulation_state(&mut state)?;
        tracing::info!(
            "模拟准备完成: {}, entities={}, profiles={}",
            simulation_id,
            state.entities_count,
            state.profiles_count
        );
        Ok(state)
    }

    // -----------------------------------------------------------------------
    // get_simulation (S-676)
    // -----------------------------------------------------------------------

    /// Return the current state of a simulation, or `None` if it does not exist.
    ///
    /// Port of `get_simulation` (`simulation_manager.py:459-461`).
    /// Thin delegation to `_load_simulation_state`.
    pub fn get_simulation(&self, simulation_id: &str) -> Result<Option<SimulationState>> {
        self.load_simulation_state(simulation_id)
    }

    /// Mark `{sim_dir}/state.json`'s `status` as `"stopped"` (+ refresh `updated_at`),
    /// preserving every other key already in the file.
    ///
    /// This is the U-023-owned realization of the **secondary `state.json` write** in
    /// MiroFish `cleanup_all_simulations` (`simulation_runner.py:1244-1259`): when the
    /// server shuts down, each running simulation's `state.json` is partially edited
    /// (`state_data['status'] = 'stopped'`, `state_data['updated_at'] = now`) and written
    /// back — a *raw read-modify-write* that keeps all other keys intact. The runner
    /// (U-022) calls this rather than editing JSON directly (DECISION-17 §17.0 Area 4:
    /// "write via the `SimulationManager`, not a raw json edit").
    ///
    /// Faithful behavior (matching Python L1248-1259):
    /// - If `state.json` does not exist → no-op, `Ok(false)` (Python logs a warning and
    ///   skips; teri returns `false` to signal "no file touched").
    /// - If it exists → parse as a JSON object, set `status`/`updated_at`, re-serialize
    ///   with 2-space indent (`ensure_ascii=False, indent=2`), write back. `Ok(true)`.
    /// - Parse/IO errors propagate as `Err` (Python wraps this in a try/except that logs
    ///   and continues per-simulation; the caller — `cleanup_all` — applies that
    ///   catch-log-continue policy so one bad file does not abort the whole cleanup).
    ///
    /// The in-memory cache is invalidated for this `simulation_id` so a subsequent
    /// `get_simulation` reflects the on-disk change rather than a stale cached state.
    pub fn mark_state_json_stopped(&self, simulation_id: &str) -> Result<bool> {
        let state_file = self.sim_data_dir.join(simulation_id).join("state.json");
        if !state_file.exists() {
            // Python L1256-1257: logs "state.json 不存在" and skips. No file to touch.
            return Ok(false);
        }

        let raw = std::fs::read_to_string(&state_file)?;
        let mut data: Value = serde_json::from_str(&raw)?;
        let obj = data.as_object_mut().ok_or_else(|| {
            TeriError::Sim(format!("state.json for {simulation_id} is not a JSON object"))
        })?;

        // Partial edit (Python L1251-1252) — preserves all other keys.
        obj.insert("status".to_string(), Value::String("stopped".to_string()));
        obj.insert("updated_at".to_string(), Value::String(python_isoformat_local()));

        // Re-serialize: 2-space indent, no ASCII escaping (Python `indent=2, ensure_ascii=False`).
        let json = serde_json::to_string_pretty(&data)?;
        std::fs::write(&state_file, json.as_bytes())?;

        // Invalidate the cache entry so the next load reflects the on-disk status.
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(simulation_id);
        }

        Ok(true)
    }

    // -----------------------------------------------------------------------
    // list_simulations (S-677)
    // -----------------------------------------------------------------------

    /// List all simulations, optionally filtered by `project_id`.
    ///
    /// Port of `list_simulations` (`simulation_manager.py:463-479`).
    ///
    /// Skip logic (faithful):
    ///   - skip entries whose name starts with `'.'` (e.g. `.DS_Store`, `.gitkeep`)
    ///   - skip non-directory entries
    ///   - skip entries for which `_load_simulation_state` returns `None`
    ///     (Python: `if sim_id.startswith('.') or not os.path.isdir(sim_path): continue`)
    ///
    /// Ordering: Python `os.listdir` order is FS-dependent and unspecified.
    /// This method returns results in `read_dir` order (also FS-dependent and unspecified).
    /// No sort is applied — matching Python's contract (no sort, no stated order).
    ///
    /// If `sim_data_dir` does not exist, returns an empty list (matches Python's
    /// `if os.path.exists(self.SIMULATION_DATA_DIR): ...` guard).
    pub fn list_simulations(&self, project_id: Option<&str>) -> Result<Vec<SimulationState>> {
        if !self.sim_data_dir.exists() {
            return Ok(Vec::new());
        }

        let mut simulations = Vec::new();

        for entry in std::fs::read_dir(&self.sim_data_dir)? {
            let entry = entry?;
            let entry_name = entry.file_name();
            let sim_id = entry_name.to_string_lossy();

            // Skip hidden entries (Python: `if sim_id.startswith('.')`)
            if sim_id.starts_with('.') {
                continue;
            }

            // Skip non-directory entries (Python: `not os.path.isdir(sim_path)`)
            if !entry.path().is_dir() {
                continue;
            }

            if let Some(state) = self.load_simulation_state(sim_id.as_ref())? {
                // Filter by project_id if provided
                if project_id.is_some_and(|pid| state.project_id != pid) {
                    continue;
                }
                simulations.push(state);
            }
        }

        Ok(simulations)
    }

    // -----------------------------------------------------------------------
    // get_profiles (S-678)
    // -----------------------------------------------------------------------

    /// Return the agent profiles for a simulation (from `{platform}_profiles.json`).
    ///
    /// Port of `get_profiles` (`simulation_manager.py:481-494`).
    ///
    /// Behavior:
    ///   - State missing  → `Err` (Python `raise ValueError(f"模拟不存在: {simulation_id}")`)
    ///   - State present, profile JSON file missing → `Ok(Vec::new())` (empty array — NOT Err)
    ///   - State present, profile JSON file present → `Ok(parsed array)`
    ///
    /// The raise-vs-empty distinction is load-bearing: callers must distinguish
    /// "simulation not found" (hard error) from "profiles not yet generated" (empty list).
    ///
    /// `platform` default in Python: `"reddit"`. Callers must pass it explicitly in Rust.
    pub fn get_profiles(&self, simulation_id: &str, platform: &str) -> Result<Vec<Value>> {
        // Missing state → Err (Python `raise ValueError`)
        let state = self.load_simulation_state(simulation_id)?;
        if state.is_none() {
            return Err(TeriError::Sim(format!(
                "simulation not found: {simulation_id} \
                 (Python raises ValueError here — missing state is hard error)"
            )));
        }

        // Profile file missing → [] (Python `if not os.path.exists(profile_path): return []`)
        let sim_dir = self.get_simulation_dir(simulation_id)?;
        let profile_path = sim_dir.join(format!("{platform}_profiles.json"));

        if !profile_path.exists() {
            return Ok(Vec::new());
        }

        let raw = std::fs::read_to_string(&profile_path)?;
        let parsed: Vec<Value> = serde_json::from_str(&raw)?;
        Ok(parsed)
    }

    // -----------------------------------------------------------------------
    // get_simulation_config (S-679)
    // -----------------------------------------------------------------------

    /// Return the simulation config JSON, or `None` if not yet generated.
    ///
    /// Port of `get_simulation_config` (`simulation_manager.py:496-505`).
    ///
    /// Config file missing → `Ok(None)` (NOT an error — matches Python `return None`).
    /// File present → `Ok(Some(parsed JSON value))`.
    pub fn get_simulation_config(&self, simulation_id: &str) -> Result<Option<Value>> {
        let sim_dir = self.get_simulation_dir(simulation_id)?;
        let config_path = sim_dir.join("simulation_config.json");

        if !config_path.exists() {
            return Ok(None);
        }

        let raw = std::fs::read_to_string(&config_path)?;
        let parsed: Value = serde_json::from_str(&raw)?;
        Ok(Some(parsed))
    }

    // -----------------------------------------------------------------------
    // get_run_instructions (S-680) — partial port with [≠]-substrate gap
    // -----------------------------------------------------------------------

    /// Return the structural run-instructions for a simulation.
    ///
    /// Port of `get_run_instructions` (`simulation_manager.py:507-529`).
    ///
    /// ## Substrate divergence [≠] (NOT a skip — genuinely inexpressible)
    ///
    /// Python returns a dict with:
    ///   - `simulation_dir` (path)  ← ported
    ///   - `config_file` (path)     ← ported
    ///   - `scripts_dir` (path to MiroFish's `backend/scripts/`)  ← [≠]
    ///   - `commands` dict with `python {scripts_dir}/run_twitter_simulation.py --config ...`  ← [≠]
    ///   - `instructions` string with `conda activate MiroFish; python ...`  ← [≠]
    ///
    /// The `scripts_dir`, `commands`, and `instructions` fields describe running
    /// MiroFish's Python OASIS subprocess scripts under a conda environment.
    /// teri has NO these scripts and no conda env — it runs the SimEngine in-process
    /// (port-architect decision, locked).  Fabricating those Python-script paths
    /// would produce commands that CANNOT execute in teri's substrate.
    ///
    /// This is a genuine [≠]-substrate case: the commands are **inexpressible** in
    /// teri's runtime, not merely "unused" or "unwanted".  The `substrate_note`
    /// field on `RunInstructions` explains the gap and directs callers to
    /// `SimEngine::run` (teri's native invocation).
    pub fn get_run_instructions(&self, simulation_id: &str) -> Result<RunInstructions> {
        let sim_dir = self.get_simulation_dir(simulation_id)?;
        let config_file = sim_dir.join("simulation_config.json");

        // DECISION-U026-2: native run-guidance via the HTTP start endpoint
        // (SimulationRunner→SimEngine). The authoritative start route is `POST /start`
        // (`simulation.py:1451`, teri S-820) which takes `simulation_id` AND `platform` in
        // the JSON BODY — there is NO `/<id>/start` path route. Python's per-platform
        // `python run_*.py --config` strings become per-platform body-id start invocations.
        let endpoint = "POST /api/simulation/start";
        let mk = |platform: &str| {
            format!(
                r#"{endpoint}  body: {{"simulation_id":"{simulation_id}","platform":"{platform}"}}"#
            )
        };

        Ok(RunInstructions {
            simulation_dir: sim_dir,
            config_file,
            commands: RunCommands {
                twitter: mk("twitter"),
                reddit: mk("reddit"),
                parallel: mk("parallel"),
            },
            instructions: format!(
                "teri runs this prepared simulation in-process via SimEngine (no Python scripts, \
                 no conda env). Start it through the running API server:\n\
                 1. Ensure `teri serve` is running.\n\
                 2. POST /api/simulation/start with JSON body \
                 {{\"simulation_id\": \"{simulation_id}\", \
                 \"platform\": \"twitter\"|\"reddit\"|\"parallel\", \"max_rounds\": <opt int>, \
                 \"enable_graph_memory_update\": <opt bool>, \"force\": <opt bool>}}.\n\
                 The default platform is \"parallel\". The runner drives SimEngine directly; \
                 no subprocess is spawned."
            ),
            // [≠] residual (NARROWED by DECISION-U026-2): only scripts_dir and the literal
            // python/conda command strings are inexpressible in teri's substrate.
            substrate_note: "MiroFish's Python OASIS subprocess commands \
                (run_twitter_simulation.py, run_reddit_simulation.py, run_parallel_simulation.py) \
                and `conda activate MiroFish` are inexpressible in teri's substrate (no Python scripts, \
                no conda). teri runs the SimEngine in-process via the /start endpoint above.",
        })
    }
}

// =============================================================================
// Tests — SimulationManager (sub-cycle c)
// =============================================================================

#[cfg(test)]
mod manager_tests {
    use super::*;
    use std::env;

    /// Helper: create a temp dir rooted in std::env::temp_dir() (no /tmp hardcode).
    fn temp_sim_dir(suffix: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("teri_test_simulations_{}_{suffix}", std::process::id()));
        p
    }

    // -----------------------------------------------------------------------
    // create_simulation — id format, state.json written
    // -----------------------------------------------------------------------

    #[test]
    fn create_simulation_id_format() {
        let dir = temp_sim_dir("id_format");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("proj-1", "graph-1", true, true).unwrap();

        // id must be "sim_" + exactly 12 lowercase hex chars
        assert!(
            state.simulation_id.starts_with("sim_"),
            "sim id must start with 'sim_': {}",
            state.simulation_id
        );
        let hex_part = &state.simulation_id["sim_".len()..];
        assert_eq!(
            hex_part.len(),
            12,
            "hex suffix must be exactly 12 chars: {}",
            state.simulation_id
        );
        // Valid hex chars are 0-9 and a-f (all lowercase). Digits are neither
        // uppercase nor lowercase — so we check: is_ascii_digit OR is_ascii_lowercase.
        assert!(
            hex_part.chars().all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f')),
            "hex suffix must be lowercase hex (0-9, a-f): {}",
            hex_part
        );

        // state.json must exist on FS
        let state_json = dir.join(&state.simulation_id).join("state.json");
        assert!(state_json.exists(), "state.json must be written: {:?}", state_json);
    }

    #[test]
    fn create_simulation_fields() {
        let dir = temp_sim_dir("fields");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("proj-abc", "graph-xyz", false, true).unwrap();

        assert_eq!(state.project_id, "proj-abc");
        assert_eq!(state.graph_id, "graph-xyz");
        assert!(!state.enable_twitter);
        assert!(state.enable_reddit);
        assert_eq!(state.status, SimulationStatus::Created);
        assert_eq!(state.entities_count, 0);
        assert!(state.error.is_none());
    }

    #[test]
    fn create_simulation_state_json_readable() {
        // Written state.json must be valid UTF-8 pretty JSON with 2-space indent.
        let dir = temp_sim_dir("json_readable");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("p", "g", true, true).unwrap();

        let state_json_path = dir.join(&state.simulation_id).join("state.json");
        let raw = std::fs::read_to_string(&state_json_path).unwrap();

        // Must parse as JSON object
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let obj = parsed.as_object().unwrap();
        assert_eq!(obj["simulation_id"].as_str().unwrap(), state.simulation_id);
        assert_eq!(obj["status"].as_str().unwrap(), "created");

        // 2-space indent (pretty-printed): second line should start with "  "
        let second_line = raw.lines().nth(1).unwrap_or("");
        assert!(second_line.starts_with("  "), "state.json must be 2-space indented");
    }

    // -----------------------------------------------------------------------
    // _save_simulation_state — updated_at bumped, cache updated
    // -----------------------------------------------------------------------

    #[test]
    fn save_bumps_updated_at_before_write() {
        let dir = temp_sim_dir("save_bump");
        let mgr = SimulationManager::new(&dir);

        let mut state = mgr.create_simulation("p", "g", true, true).unwrap();
        let created_updated_at = state.updated_at.clone();

        // Force a small delay to ensure the timestamp changes
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Mutate something and save
        state.status = SimulationStatus::Preparing;
        mgr.save_simulation_state(&mut state).unwrap();

        // updated_at must have been bumped BEFORE the write
        assert_ne!(state.updated_at, created_updated_at, "updated_at must be bumped on save");

        // What's on disk must match the bumped timestamp
        let state_json_path = dir.join(&state.simulation_id).join("state.json");
        let raw = std::fs::read_to_string(&state_json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            parsed["updated_at"].as_str().unwrap(),
            state.updated_at,
            "persisted updated_at must match the mutated state"
        );
    }

    // -----------------------------------------------------------------------
    // _load_simulation_state — cache-first, missing→None
    // -----------------------------------------------------------------------

    #[test]
    fn load_missing_returns_none() {
        let dir = temp_sim_dir("load_missing");
        let mgr = SimulationManager::new(&dir);

        let result = mgr.load_simulation_state("sim_doesnotexist").unwrap();
        assert!(result.is_none(), "missing sim should return None");
    }

    #[test]
    fn load_cache_first_returns_cached_value() {
        let dir = temp_sim_dir("cache_first");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("proj", "graph", true, true).unwrap();
        let sim_id = state.simulation_id.clone();

        // Directly mutate the cache to a different value
        {
            let mut cache = mgr.cache.lock().unwrap();
            let cached = cache.get_mut(&sim_id).unwrap();
            cached.status = SimulationStatus::Running; // not on disk
        }

        // load must return cached version (Running), not the FS version (Created)
        let loaded = mgr.load_simulation_state(&sim_id).unwrap().unwrap();
        assert_eq!(
            loaded.status,
            SimulationStatus::Running,
            "cache-first: should return cached Running, not FS Created"
        );
    }

    #[test]
    fn load_from_disk_after_cache_cleared() {
        let dir = temp_sim_dir("load_disk");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("proj-d", "graph-d", true, false).unwrap();
        let sim_id = state.simulation_id.clone();

        // Clear the cache to force disk read
        {
            let mut cache = mgr.cache.lock().unwrap();
            cache.clear();
        }

        let loaded = mgr.load_simulation_state(&sim_id).unwrap().unwrap();
        assert_eq!(loaded.simulation_id, sim_id);
        assert_eq!(loaded.project_id, "proj-d");
        assert!(!loaded.enable_reddit);
        assert_eq!(loaded.status, SimulationStatus::Created);
    }

    // -----------------------------------------------------------------------
    // Round-trip: create → load
    // -----------------------------------------------------------------------

    #[test]
    fn create_get_round_trip() {
        let dir = temp_sim_dir("round_trip");
        let mgr = SimulationManager::new(&dir);

        let created = mgr.create_simulation("proj-rt", "graph-rt", true, true).unwrap();
        let sim_id = created.simulation_id.clone();

        let loaded = mgr.get_simulation(&sim_id).unwrap().unwrap();

        assert_eq!(loaded.simulation_id, created.simulation_id);
        assert_eq!(loaded.project_id, created.project_id);
        assert_eq!(loaded.graph_id, created.graph_id);
        assert_eq!(loaded.status, SimulationStatus::Created);
        assert_eq!(loaded.enable_twitter, created.enable_twitter);
        assert_eq!(loaded.enable_reddit, created.enable_reddit);
    }

    // -----------------------------------------------------------------------
    // list_simulations — skip hidden, skip non-dirs, filter by project_id
    // -----------------------------------------------------------------------

    #[test]
    fn list_simulations_skips_hidden_entries() {
        let dir = temp_sim_dir("list_hidden");
        let mgr = SimulationManager::new(&dir);

        // Create a real simulation
        let state = mgr.create_simulation("proj-h", "g", true, true).unwrap();

        // Create a hidden directory (should be skipped)
        let hidden = dir.join(".DS_Store");
        std::fs::create_dir_all(&hidden).unwrap();
        // Write a fake state.json in there too
        std::fs::write(hidden.join("state.json"), r#"{"project_id":"proj-h","status":"created"}"#)
            .unwrap();

        // Create a file (not a dir) — should be skipped
        let not_a_dir = dir.join("readme.txt");
        std::fs::write(&not_a_dir, "not a dir").unwrap();

        let list = mgr.list_simulations(None).unwrap();
        // Only the real simulation should appear
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].simulation_id, state.simulation_id);
    }

    #[test]
    fn list_simulations_filters_by_project_id() {
        let dir = temp_sim_dir("list_filter");
        let mgr = SimulationManager::new(&dir);

        let s1 = mgr.create_simulation("proj-A", "g", true, true).unwrap();
        let _s2 = mgr.create_simulation("proj-B", "g", true, true).unwrap();
        let s3 = mgr.create_simulation("proj-A", "g", true, true).unwrap();

        let list = mgr.list_simulations(Some("proj-A")).unwrap();
        assert_eq!(list.len(), 2);
        let ids: Vec<&str> = list.iter().map(|s| s.simulation_id.as_str()).collect();
        assert!(ids.contains(&s1.simulation_id.as_str()));
        assert!(ids.contains(&s3.simulation_id.as_str()));
    }

    #[test]
    fn list_simulations_no_filter_returns_all() {
        let dir = temp_sim_dir("list_all");
        let mgr = SimulationManager::new(&dir);

        mgr.create_simulation("proj-1", "g", true, true).unwrap();
        mgr.create_simulation("proj-2", "g", true, true).unwrap();

        let list = mgr.list_simulations(None).unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn list_simulations_nonexistent_dir_returns_empty() {
        // If sim_data_dir doesn't exist yet, list returns [] (not an error)
        let dir = temp_sim_dir("list_nonexistent");
        // Do NOT create the dir
        let mgr = SimulationManager::new(&dir);
        let list = mgr.list_simulations(None).unwrap();
        assert!(list.is_empty());
    }

    // -----------------------------------------------------------------------
    // get_profiles — missing state→Err, missing file→[], present→array
    // -----------------------------------------------------------------------

    #[test]
    fn get_profiles_missing_state_returns_err() {
        let dir = temp_sim_dir("profiles_no_state");
        let mgr = SimulationManager::new(&dir);

        // No simulation created
        let result = mgr.get_profiles("sim_doesnotexist", "reddit");
        assert!(result.is_err(), "missing state must return Err (Python raises ValueError)");
    }

    #[test]
    fn get_profiles_missing_file_returns_empty_vec() {
        let dir = temp_sim_dir("profiles_no_file");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("p", "g", true, true).unwrap();

        // No profiles file written — must return [] not Err
        let profiles = mgr.get_profiles(&state.simulation_id, "reddit").unwrap();
        assert!(profiles.is_empty(), "missing profiles file must return [], not Err");
    }

    #[test]
    fn get_profiles_present_returns_array() {
        let dir = temp_sim_dir("profiles_present");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("p", "g", true, true).unwrap();

        // Write a profiles JSON file
        let profiles_json =
            r#"[{"username":"alice","bio":"test"},{"username":"bob","bio":"test2"}]"#;
        let sim_dir = dir.join(&state.simulation_id);
        std::fs::write(sim_dir.join("reddit_profiles.json"), profiles_json).unwrap();

        let profiles = mgr.get_profiles(&state.simulation_id, "reddit").unwrap();
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0]["username"].as_str().unwrap(), "alice");
    }

    #[test]
    fn get_profiles_platform_twitter_reads_twitter_file() {
        let dir = temp_sim_dir("profiles_twitter");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("p", "g", true, true).unwrap();

        let sim_dir = dir.join(&state.simulation_id);
        // Reddit file absent, twitter file present
        std::fs::write(sim_dir.join("twitter_profiles.json"), r#"[{"username":"charlie"}]"#)
            .unwrap();

        // platform="reddit" → file absent → []
        let reddit = mgr.get_profiles(&state.simulation_id, "reddit").unwrap();
        assert!(reddit.is_empty());

        // platform="twitter" → file present → 1 profile
        let twitter = mgr.get_profiles(&state.simulation_id, "twitter").unwrap();
        assert_eq!(twitter.len(), 1);
    }

    // -----------------------------------------------------------------------
    // get_simulation_config — missing→None, present→Some(Value)
    // -----------------------------------------------------------------------

    #[test]
    fn get_simulation_config_missing_returns_none() {
        let dir = temp_sim_dir("config_missing");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("p", "g", true, true).unwrap();

        let cfg = mgr.get_simulation_config(&state.simulation_id).unwrap();
        assert!(cfg.is_none(), "missing config file must return None, not Err");
    }

    #[test]
    fn get_simulation_config_present_returns_some() {
        let dir = temp_sim_dir("config_present");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("p", "g", true, true).unwrap();

        let config_json = r#"{"max_rounds":10,"agent_count":50}"#;
        let sim_dir = dir.join(&state.simulation_id);
        std::fs::write(sim_dir.join("simulation_config.json"), config_json).unwrap();

        let cfg = mgr.get_simulation_config(&state.simulation_id).unwrap().unwrap();
        assert_eq!(cfg["max_rounds"].as_i64().unwrap(), 10);
        assert_eq!(cfg["agent_count"].as_i64().unwrap(), 50);
    }

    // -----------------------------------------------------------------------
    // _load_simulation_state — invalid status → Err
    // -----------------------------------------------------------------------

    #[test]
    fn load_invalid_status_returns_err() {
        let dir = temp_sim_dir("invalid_status");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("p", "g", true, true).unwrap();
        let sim_id = state.simulation_id.clone();

        // Corrupt the status in state.json
        let state_json_path = dir.join(&sim_id).join("state.json");
        let mut parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&state_json_path).unwrap()).unwrap();
        parsed["status"] = serde_json::Value::String("bogus_status".to_string());
        std::fs::write(&state_json_path, serde_json::to_string_pretty(&parsed).unwrap()).unwrap();

        // Clear the cache to force FS read
        mgr.cache.lock().unwrap().clear();

        let result = mgr.load_simulation_state(&sim_id);
        assert!(
            result.is_err(),
            "invalid status string must return Err (Python SimulationStatus('bogus') raises ValueError)"
        );
    }

    // -----------------------------------------------------------------------
    // get_run_instructions — structural fields present, substrate note present
    // -----------------------------------------------------------------------

    #[test]
    fn get_run_instructions_structural_fields() {
        let dir = temp_sim_dir("run_instructions");
        let mgr = SimulationManager::new(&dir);

        let state = mgr.create_simulation("p", "g", true, true).unwrap();
        let sim_id = &state.simulation_id;

        let instr = mgr.get_run_instructions(sim_id).unwrap();

        // --- Existing asserts (simulation_dir / config_file / substrate_note) ---
        assert!(
            instr.simulation_dir.ends_with(sim_id),
            "simulation_dir must point to sim dir: {:?}",
            instr.simulation_dir
        );
        assert!(
            instr.config_file.ends_with("simulation_config.json"),
            "config_file must end with simulation_config.json: {:?}",
            instr.config_file
        );
        assert!(!instr.substrate_note.is_empty(), "substrate_note must explain the [≠] gap");
        assert!(
            instr.substrate_note.contains("SimEngine"),
            "substrate_note must direct caller to SimEngine: {}",
            instr.substrate_note
        );

        // --- DECISION-U026-2: new commands / instructions asserts ---

        // Each per-platform command must reference the HTTP start endpoint and that platform.
        for (platform, cmd) in [
            ("twitter", &instr.commands.twitter),
            ("reddit", &instr.commands.reddit),
            ("parallel", &instr.commands.parallel),
        ] {
            assert!(
                cmd.contains("/start"),
                "commands.{platform} must reference the /start endpoint: {cmd}"
            );
            assert!(
                cmd.contains(platform),
                "commands.{platform} must name its platform in the body: {cmd}"
            );
            assert!(
                cmd.contains(sim_id.as_str()),
                "commands.{platform} must contain the simulation id: {cmd}"
            );
            // Route SHAPE (regression guard, parity-gate FAIL fix): the authoritative start
            // route is `POST /api/simulation/start` with simulation_id in the BODY — there is
            // NO `/<id>/start` path route (simulation.py:1451, S-820). The guidance must point
            // at the real, routable endpoint, and must NOT put the id in the URL path.
            assert!(
                cmd.contains("POST /api/simulation/start"),
                "commands.{platform} must reference the body-id start route exactly: {cmd}"
            );
            assert!(
                !cmd.contains(&format!("/simulation/{sim_id}/start")),
                "commands.{platform} must NOT use the nonexistent id-in-path /<id>/start route: {cmd}"
            );
            assert!(
                cmd.contains(&format!(r#""simulation_id":"{sim_id}""#)),
                "commands.{platform} must carry simulation_id in the JSON body: {cmd}"
            );
        }

        // instructions must mention SimEngine (in-process) and be non-empty.
        assert!(
            !instr.instructions.is_empty(),
            "instructions must be non-empty (DECISION-U026-2)"
        );
        assert!(
            instr.instructions.contains("SimEngine"),
            "instructions must mention SimEngine (in-process path): {}",
            instr.instructions
        );

        // to_dict: spot-check key presence and nested commands shape.
        let dict = instr.to_dict();
        assert!(dict.get("simulation_dir").is_some(), "to_dict must contain simulation_dir");
        assert!(dict.get("config_file").is_some(), "to_dict must contain config_file");
        assert!(dict.get("commands").is_some(), "to_dict must contain commands");
        assert!(dict.get("instructions").is_some(), "to_dict must contain instructions");
        assert!(dict.get("substrate_note").is_some(), "to_dict must contain substrate_note");
        assert!(dict.get("scripts_dir").is_none(), "[≠] scripts_dir must NOT appear in to_dict");

        let cmds = dict.get("commands").unwrap();
        assert!(cmds.get("twitter").is_some(), "commands must have twitter key");
        assert!(cmds.get("reddit").is_some(), "commands must have reddit key");
        assert!(cmds.get("parallel").is_some(), "commands must have parallel key");
    }

    // -----------------------------------------------------------------------
    // Unique sim IDs across multiple creates
    // -----------------------------------------------------------------------

    #[test]
    fn create_simulation_ids_are_unique() {
        let dir = temp_sim_dir("unique_ids");
        let mgr = SimulationManager::new(&dir);

        let ids: Vec<String> = (0..10)
            .map(|_| mgr.create_simulation("p", "g", true, true).unwrap().simulation_id)
            .collect();

        let unique: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(unique.len(), ids.len(), "all simulation IDs must be unique");
    }

    // -----------------------------------------------------------------------
    // mark_state_json_stopped (U-022 S-625 secondary state.json write)
    // -----------------------------------------------------------------------

    #[test]
    fn mark_state_json_stopped_missing_file_is_noop() {
        let dir = temp_sim_dir("mark_stopped_missing");
        let mgr = SimulationManager::new(&dir);
        // No state.json created → returns Ok(false), touches nothing.
        let touched = mgr.mark_state_json_stopped("nonexistent-sim").unwrap();
        assert!(!touched, "missing state.json must be a no-op returning false");
    }

    #[test]
    fn mark_state_json_stopped_partial_edit_preserves_other_keys() {
        let dir = temp_sim_dir("mark_stopped_partial");
        let mgr = SimulationManager::new(&dir);
        let state = mgr.create_simulation("proj-x", "graph-y", true, true).unwrap();
        let sim_id = &state.simulation_id;

        let state_file = dir.join(sim_id).join("state.json");
        // Capture the original keys + a couple of values to prove they survive.
        let before: Value =
            serde_json::from_str(&std::fs::read_to_string(&state_file).unwrap()).unwrap();
        let before_obj = before.as_object().unwrap();
        let orig_project = before_obj.get("project_id").cloned().unwrap();
        let orig_keys: std::collections::HashSet<String> = before_obj.keys().cloned().collect();

        let touched = mgr.mark_state_json_stopped(sim_id).unwrap();
        assert!(touched, "existing state.json must be edited, returning true");

        let after: Value =
            serde_json::from_str(&std::fs::read_to_string(&state_file).unwrap()).unwrap();
        let after_obj = after.as_object().unwrap();

        // status flipped to "stopped"
        assert_eq!(after_obj.get("status"), Some(&Value::String("stopped".into())));
        // updated_at present and non-empty
        assert!(
            after_obj
                .get("updated_at")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
        );
        // All other keys preserved (no key dropped by the partial edit).
        let after_keys: std::collections::HashSet<String> = after_obj.keys().cloned().collect();
        assert_eq!(orig_keys, after_keys, "partial edit must not add/drop keys");
        // A non-touched value (project_id) is unchanged.
        assert_eq!(after_obj.get("project_id"), Some(&orig_project));
    }

    #[test]
    fn mark_state_json_stopped_invalidates_cache() {
        let dir = temp_sim_dir("mark_stopped_cache");
        let mgr = SimulationManager::new(&dir);
        let state = mgr.create_simulation("proj-c", "graph-c", true, true).unwrap();
        let sim_id = state.simulation_id.clone();

        // Warm the cache (get_simulation loads + caches via load path).
        let loaded = mgr.get_simulation(&sim_id).unwrap().unwrap();
        assert_eq!(loaded.status, SimulationStatus::Created);

        mgr.mark_state_json_stopped(&sim_id).unwrap();

        // After the secondary write, get_simulation must reflect "stopped" (cache invalidated).
        let reloaded = mgr.get_simulation(&sim_id).unwrap().unwrap();
        assert_eq!(reloaded.status, SimulationStatus::Stopped);
    }
}

// =============================================================================
// Tests — prepare_simulation (S-675)
// =============================================================================

#[cfg(test)]
mod prepare_tests {
    use super::*;
    use crate::agent::PersonaGenerator;
    use crate::graph::{Entity, EntityKind, KnowledgeGraph};
    use crate::services::simulation_config::SimulationConfigGenerator;
    use std::env;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn temp_prepare_dir(suffix: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("teri_prepare_sim_{}_{suffix}", std::process::id()));
        p
    }

    // ── MockLlm (reused from oasis_profile_export tests — kept local) ─────────

    struct MockLlm {
        profile_response: String,
    }

    impl MockLlm {
        fn all_ok() -> Self {
            Self {
                profile_response: r#"{
                    "bio": "Test bio.",
                    "persona": "Test persona.",
                    "karma": 500,
                    "friend_count": 50,
                    "follower_count": 80,
                    "statuses_count": 200,
                    "age": 30,
                    "gender": "female",
                    "mbti": "INFP",
                    "country": "China",
                    "profession": "Researcher",
                    "interested_topics": ["Science"],
                    "posting_style": "Thoughtful"
                }"#
                .to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::llm::LlmClient for MockLlm {
        async fn complete(&self, _prompt: &str) -> crate::error::Result<String> {
            Ok(self.profile_response.clone())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(
            &self,
            _prompt: &str,
        ) -> crate::error::Result<T> {
            serde_json::from_str(&self.profile_response)
                .map_err(|e| crate::error::TeriError::Unknown(e.to_string()))
        }
        async fn stream(
            &self,
            _prompt: &str,
        ) -> crate::error::Result<
            std::pin::Pin<Box<dyn futures::Stream<Item = crate::error::Result<String>> + Send>>,
        > {
            use futures::stream;
            Ok(Box::pin(stream::iter(vec![Ok(self.profile_response.clone())])))
        }
        async fn chat(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> crate::error::Result<String> {
            Ok(self.profile_response.clone())
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> crate::error::Result<T> {
            serde_json::from_str(&self.profile_response)
                .map_err(|e| crate::error::TeriError::Unknown(e.to_string()))
        }
    }

    /// Build a KnowledgeGraph with N entities of the given kind.
    fn make_graph_with_entities(names: &[&str], kind: EntityKind) -> KnowledgeGraph {
        let mut g = KnowledgeGraph::new();
        for name in names {
            let entity =
                Entity { id: uuid::Uuid::new_v4(), name: name.to_string(), kind: kind.clone() };
            g.add_entity(entity).expect("add_entity must succeed");
        }
        g
    }

    // ── Test 1: missing simulation_id → Err "模拟不存在" ────────────────────────

    #[tokio::test]
    async fn prepare_simulation_missing_id_returns_err() {
        let dir = temp_prepare_dir("missing_id");
        let mgr = SimulationManager::new(&dir);
        let graph = KnowledgeGraph::new();
        let pg = PersonaGenerator::new();
        let llm = MockLlm::all_ok();
        let config_gen =
            SimulationConfigGenerator::new(MockLlm::all_ok(), "test-model", "http://localhost");

        let result = mgr
            .prepare_simulation(
                "sim_doesnotexist",
                "test requirement",
                "test document",
                None,
                false,
                1,
                &llm,
                &graph,
                &pg,
                &config_gen,
                None,
            )
            .await;

        assert!(result.is_err(), "missing simulation must return Err");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("模拟不存在"), "error must contain '模拟不存在': {msg}");
        assert!(msg.contains("sim_doesnotexist"), "error must contain the id: {msg}");
    }

    // ── Test 2: zero filtered entities → Ok(state) with status=FAILED ───────────

    #[tokio::test]
    async fn prepare_simulation_zero_entities_returns_ok_with_failed_status() {
        let dir = temp_prepare_dir("zero_entities");
        let mgr = SimulationManager::new(&dir);
        // Graph has NO entities → filter returns 0
        let graph = KnowledgeGraph::new();
        let pg = PersonaGenerator::new();
        let llm = MockLlm::all_ok();
        let config_gen =
            SimulationConfigGenerator::new(MockLlm::all_ok(), "test-model", "http://localhost");

        let state = mgr.create_simulation("proj", "graph", true, true).unwrap();

        let result = mgr
            .prepare_simulation(
                &state.simulation_id,
                "req",
                "doc",
                None,
                false,
                1,
                &llm,
                &graph,
                &pg,
                &config_gen,
                None,
            )
            .await;

        // Python `return state` → Ok (NOT Err)
        assert!(
            result.is_ok(),
            "zero entities must return Ok(state), not Err: {:?}",
            result.err()
        );
        let final_state = result.unwrap();
        assert_eq!(
            final_state.status,
            SimulationStatus::Failed,
            "zero entities must set status=FAILED"
        );
        assert!(
            final_state.error.as_deref().unwrap_or("").contains("没有找到"),
            "error message must mention 没有找到: {:?}",
            final_state.error
        );

        // state.json on disk must also have FAILED
        let state_json_path = dir.join(&state.simulation_id).join("state.json");
        let raw = std::fs::read_to_string(&state_json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            parsed["status"].as_str().unwrap(),
            "failed",
            "persisted state.json must have status=failed"
        );
    }

    // ── Test 3: happy path (enable_reddit only) ─────────────────────────────────

    #[tokio::test]
    async fn prepare_simulation_happy_path_reddit_only() {
        let dir = temp_prepare_dir("happy_reddit");
        let mgr = SimulationManager::new(&dir);

        let graph = make_graph_with_entities(&["Alice", "Bob"], EntityKind::Person);
        let pg = PersonaGenerator::new();
        let llm = MockLlm::all_ok();
        let config_gen =
            SimulationConfigGenerator::new(MockLlm::all_ok(), "test-model", "http://localhost");

        let mut init = mgr.create_simulation("proj", "graph", false, true).unwrap();
        // enable_reddit=true, enable_twitter=false
        init.enable_twitter = false;
        init.enable_reddit = true;
        mgr.save_simulation_state(&mut init).unwrap();

        let result = mgr
            .prepare_simulation(
                &init.simulation_id,
                "Test simulation requirement",
                "Test document text",
                None,
                false,
                1,
                &llm,
                &graph,
                &pg,
                &config_gen,
                None,
            )
            .await;

        let final_state = result.expect("happy path must succeed");

        assert_eq!(final_state.status, SimulationStatus::Ready, "must be READY");
        assert!(final_state.entities_count > 0, "entities_count must be set");
        assert!(final_state.profiles_count > 0, "profiles_count must be set");
        assert!(final_state.config_generated, "config_generated must be true");
        // config_reasoning may be empty for the mock LLM (SimulationConfigGenerator has
        // internal fallback, so reasoning is set to the fallback text or empty string).
        // We only assert config_generated=true (the contractual field).
        let _ = &final_state.config_reasoning; // accessed for completeness; not asserted

        let sim_dir = dir.join(&init.simulation_id);

        // reddit_profiles.json must exist
        let reddit_path = sim_dir.join("reddit_profiles.json");
        assert!(reddit_path.exists(), "reddit_profiles.json must be written");
        let reddit_content = std::fs::read_to_string(&reddit_path).unwrap();
        let reddit: Vec<serde_json::Value> = serde_json::from_str(&reddit_content).unwrap();
        assert!(!reddit.is_empty(), "reddit_profiles.json must have profiles");

        // twitter_profiles.csv must NOT exist (enable_twitter=false)
        let twitter_path = sim_dir.join("twitter_profiles.csv");
        assert!(
            !twitter_path.exists(),
            "twitter_profiles.csv must NOT be written when enable_twitter=false"
        );

        // simulation_config.json must exist
        let config_path = sim_dir.join("simulation_config.json");
        assert!(config_path.exists(), "simulation_config.json must be written");
        let config_raw = std::fs::read_to_string(&config_path).unwrap();
        assert!(!config_raw.is_empty(), "simulation_config.json must not be empty");
        // Must be valid JSON
        let _parsed: serde_json::Value =
            serde_json::from_str(&config_raw).expect("simulation_config.json must be valid JSON");

        // state.json must have status=ready
        let state_json = std::fs::read_to_string(sim_dir.join("state.json")).unwrap();
        let state_val: serde_json::Value = serde_json::from_str(&state_json).unwrap();
        assert_eq!(state_val["status"].as_str().unwrap(), "ready");
    }

    // ── Test 4: enable_twitter only ─────────────────────────────────────────────

    #[tokio::test]
    async fn prepare_simulation_twitter_only() {
        let dir = temp_prepare_dir("twitter_only");
        let mgr = SimulationManager::new(&dir);

        let graph = make_graph_with_entities(&["Charlie"], EntityKind::Person);
        let pg = PersonaGenerator::new();
        let llm = MockLlm::all_ok();
        let config_gen =
            SimulationConfigGenerator::new(MockLlm::all_ok(), "test-model", "http://localhost");

        let mut init = mgr.create_simulation("proj", "graph", true, false).unwrap();
        init.enable_twitter = true;
        init.enable_reddit = false;
        mgr.save_simulation_state(&mut init).unwrap();

        let result = mgr
            .prepare_simulation(
                &init.simulation_id,
                "req",
                "doc",
                None,
                false,
                1,
                &llm,
                &graph,
                &pg,
                &config_gen,
                None,
            )
            .await;

        let final_state = result.expect("twitter-only happy path must succeed");
        assert_eq!(final_state.status, SimulationStatus::Ready);

        let sim_dir = dir.join(&init.simulation_id);

        // twitter_profiles.csv must exist
        let twitter_path = sim_dir.join("twitter_profiles.csv");
        assert!(twitter_path.exists(), "twitter_profiles.csv must be written");

        // reddit_profiles.json must NOT exist (enable_reddit=false)
        // Note: realtime_output might have written one if enable_reddit were true.
        // With enable_reddit=false, no reddit file should exist.
        let reddit_path = sim_dir.join("reddit_profiles.json");
        assert!(
            !reddit_path.exists(),
            "reddit_profiles.json must NOT exist when enable_reddit=false"
        );
    }

    // ── Test 5: progress callback receives staged sequence ─────────────────────

    #[tokio::test]
    async fn prepare_simulation_progress_callback_receives_all_stages() {
        let dir = temp_prepare_dir("progress_stages");
        let mgr = SimulationManager::new(&dir);

        let graph = make_graph_with_entities(&["Dave"], EntityKind::Person);
        let pg = PersonaGenerator::new();
        let llm = MockLlm::all_ok();
        let config_gen =
            SimulationConfigGenerator::new(MockLlm::all_ok(), "test-model", "http://localhost");

        let init = mgr.create_simulation("proj", "graph", false, true).unwrap();

        // Collect stages + progress values seen
        let mut stages_seen: Vec<(String, i64)> = Vec::new();
        let mut current_totals: Vec<(Option<i64>, Option<i64>)> = Vec::new();

        {
            let stages_ref = &mut stages_seen;
            let ct_ref = &mut current_totals;
            let mut cb = |p: PrepareProgress<'_>| {
                stages_ref.push((p.stage.to_string(), p.progress));
                ct_ref.push((p.current, p.total));
            };

            mgr.prepare_simulation(
                &init.simulation_id,
                "req",
                "doc",
                None,
                false,
                1,
                &llm,
                &graph,
                &pg,
                &config_gen,
                Some(&mut cb),
            )
            .await
            .expect("happy path must succeed");
        }

        // Stage sequence must start with reading, then generating_profiles, then generating_config
        let stage_names: Vec<&str> = stages_seen.iter().map(|(s, _)| s.as_str()).collect();
        assert!(stage_names.contains(&"reading"), "must have 'reading' stage events");
        assert!(
            stage_names.contains(&"generating_profiles"),
            "must have 'generating_profiles' stage events"
        );
        assert!(
            stage_names.contains(&"generating_config"),
            "must have 'generating_config' stage events"
        );

        // reading must come before generating_profiles, which must come before generating_config
        let first_reading = stage_names.iter().position(|&s| s == "reading").unwrap();
        let first_profile = stage_names.iter().position(|&s| s == "generating_profiles").unwrap();
        let first_config = stage_names.iter().position(|&s| s == "generating_config").unwrap();
        assert!(first_reading < first_profile, "reading must precede generating_profiles");
        assert!(
            first_profile < first_config,
            "generating_profiles must precede generating_config"
        );

        // reading/0 must have current=None, total=None
        let reading_0 = stages_seen.iter().position(|(s, p)| s == "reading" && *p == 0);
        assert!(reading_0.is_some(), "must have reading/0 event");
        let (cur0, tot0) = &current_totals[reading_0.unwrap()];
        assert!(cur0.is_none(), "reading/0 must have current=None");
        assert!(tot0.is_none(), "reading/0 must have total=None");

        // reading/100 must have current and total set
        let reading_100 = stages_seen.iter().position(|(s, p)| s == "reading" && *p == 100);
        assert!(reading_100.is_some(), "must have reading/100 event");
        let (cur100, tot100) = &current_totals[reading_100.unwrap()];
        assert!(cur100.is_some(), "reading/100 must have current set");
        assert!(tot100.is_some(), "reading/100 must have total set");

        // generating_config/0 must have current=Some(0), total=Some(3)
        let cfg_0 = stages_seen.iter().position(|(s, p)| s == "generating_config" && *p == 0);
        assert!(cfg_0.is_some(), "must have generating_config/0 event");
        let (cfg_cur, cfg_tot) = &current_totals[cfg_0.unwrap()];
        assert_eq!(*cfg_cur, Some(0), "generating_config/0 must have current=Some(0)");
        assert_eq!(*cfg_tot, Some(3), "generating_config/0 must have total=Some(3)");
    }

    // ── Test 6: exception path → status FAILED saved + Err returned ─────────────
    //
    // We simulate a stage-2 IO failure by creating the simulation dir, then making
    // the reddit_profiles.json path be a directory (so write fails with EISDIR).
    // This verifies that: Err → state.status=FAILED saved to disk.

    #[tokio::test]
    async fn prepare_simulation_exception_sets_failed_and_returns_err() {
        let dir = temp_prepare_dir("exception_path");
        let mgr = SimulationManager::new(&dir);

        let graph = make_graph_with_entities(&["Eve"], EntityKind::Person);
        let pg = PersonaGenerator::new();
        let llm = MockLlm::all_ok();
        let config_gen =
            SimulationConfigGenerator::new(MockLlm::all_ok(), "test-model", "http://localhost");

        let mut init = mgr.create_simulation("proj", "graph", false, true).unwrap();
        init.enable_reddit = true;
        init.enable_twitter = false;
        mgr.save_simulation_state(&mut init).unwrap();

        // Sabotage: create reddit_profiles.json as a DIRECTORY so the final save_profiles
        // call (which tries to write a file there) will fail with EISDIR.
        let sim_dir = dir.join(&init.simulation_id);
        std::fs::create_dir_all(sim_dir.join("reddit_profiles.json")).expect("create sabotage dir");

        let result = mgr
            .prepare_simulation(
                &init.simulation_id,
                "req",
                "doc",
                None,
                false,
                1,
                &llm,
                &graph,
                &pg,
                &config_gen,
                None,
            )
            .await;

        // The final save_profiles call will fail (EISDIR); the exception handler must
        // set status=FAILED and save state.json, then return Err.
        // (generate_profiles_from_entities' realtime write to the same path may also fail
        // but those are logged+ignored, not propagated — only the final save_profiles is fatal.)
        assert!(result.is_err(), "IO sabotage must result in Err");

        // Read state.json (the realtime save by the exception handler must have updated it).
        // Note: sim_dir/state.json is the file; we need to read past the dir entry.
        let state_json_path = sim_dir.join("state.json");
        if state_json_path.exists() {
            let state_json = std::fs::read_to_string(&state_json_path).unwrap();
            let state_val: serde_json::Value = serde_json::from_str(&state_json).unwrap();
            assert_eq!(
                state_val["status"].as_str().unwrap(),
                "failed",
                "on Err, persisted state must have status=failed"
            );
            assert!(
                !state_val["error"].is_null(),
                "on Err, persisted state must have error field set"
            );
        }
    }

    // ── Test 7: entities_count, profiles_count, config_generated, config_reasoning ──

    #[tokio::test]
    async fn prepare_simulation_state_fields_populated() {
        let dir = temp_prepare_dir("state_fields");
        let mgr = SimulationManager::new(&dir);

        let graph = make_graph_with_entities(&["Alice", "Bob", "Carol"], EntityKind::Person);
        let pg = PersonaGenerator::new();
        let llm = MockLlm::all_ok();
        let config_gen =
            SimulationConfigGenerator::new(MockLlm::all_ok(), "test-model", "http://localhost");

        let mut init = mgr.create_simulation("proj", "graph", true, true).unwrap();
        // Both platforms enabled
        init.enable_reddit = true;
        init.enable_twitter = true;
        mgr.save_simulation_state(&mut init).unwrap();

        let final_state = mgr
            .prepare_simulation(
                &init.simulation_id,
                "test requirement",
                "test document",
                None,
                false,
                1,
                &llm,
                &graph,
                &pg,
                &config_gen,
                None,
            )
            .await
            .expect("must succeed");

        assert_eq!(final_state.entities_count, 3, "entities_count must be 3");
        assert_eq!(final_state.profiles_count, 3, "profiles_count must be 3");
        assert!(final_state.config_generated, "config_generated must be true");
        assert!(!final_state.entity_types.is_empty(), "entity_types must be populated");

        // Both files written
        let sim_dir = dir.join(&init.simulation_id);
        assert!(sim_dir.join("reddit_profiles.json").exists());
        assert!(sim_dir.join("twitter_profiles.csv").exists());
        assert!(sim_dir.join("simulation_config.json").exists());
    }
}
