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
//! - `get_run_instructions`        — S-680 (partial: see substrate note below)
//!
//! Sub-cycle (d) — `prepare_simulation` (L230-458, S-675) — NOT YET PORTED.
//!
//! # Ledger corrections
//! The parity-ledger summary was wrong on two counts; the SOURCE is authoritative:
//! - `SimulationStatus` has **8** variants (not 4): CREATED, PREPARING, READY, RUNNING,
//!   PAUSED, STOPPED, COMPLETED, FAILED.
//! - `PlatformType` has **2** variants (not 3): TWITTER, REDDIT only (no BOTH).
//!
//! # Symbols: S-636..S-680 (excluding S-675)

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, Map};
use uuid::Uuid;

use crate::error::{Result, TeriError};
use crate::models::project::python_isoformat_local;

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
            Self::Created   => "created",
            Self::Preparing => "preparing",
            Self::Ready     => "ready",
            Self::Running   => "running",
            Self::Paused    => "paused",
            Self::Stopped   => "stopped",
            Self::Completed => "completed",
            Self::Failed    => "failed",
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
            Self::Reddit  => "reddit",
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

        map.insert("simulation_id".to_string(),   Value::String(self.simulation_id.clone()));
        map.insert("project_id".to_string(),      Value::String(self.project_id.clone()));
        map.insert("graph_id".to_string(),        Value::String(self.graph_id.clone()));
        map.insert("enable_twitter".to_string(),  Value::Bool(self.enable_twitter));
        map.insert("enable_reddit".to_string(),   Value::Bool(self.enable_reddit));
        // status emitted as lowercase string (.value), not as a struct
        map.insert("status".to_string(),          Value::String(self.status.to_string()));
        map.insert("entities_count".to_string(),  Value::Number(self.entities_count.into()));
        map.insert("profiles_count".to_string(),  Value::Number(self.profiles_count.into()));
        map.insert(
            "entity_types".to_string(),
            Value::Array(
                self.entity_types
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
        map.insert("config_generated".to_string(), Value::Bool(self.config_generated));
        map.insert("config_reasoning".to_string(),  Value::String(self.config_reasoning.clone()));
        map.insert("current_round".to_string(),     Value::Number(self.current_round.into()));
        map.insert("twitter_status".to_string(),    Value::String(self.twitter_status.clone()));
        map.insert("reddit_status".to_string(),     Value::String(self.reddit_status.clone()));
        map.insert("created_at".to_string(),        Value::String(self.created_at.clone()));
        map.insert("updated_at".to_string(),        Value::String(self.updated_at.clone()));
        // error: null when None, string when Some
        map.insert(
            "error".to_string(),
            match &self.error {
                Some(e) => Value::String(e.clone()),
                None    => Value::Null,
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

        map.insert("simulation_id".to_string(),   Value::String(self.simulation_id.clone()));
        map.insert("project_id".to_string(),      Value::String(self.project_id.clone()));
        map.insert("graph_id".to_string(),        Value::String(self.graph_id.clone()));
        // status as lowercase string (.value)
        map.insert("status".to_string(),          Value::String(self.status.to_string()));
        map.insert("entities_count".to_string(),  Value::Number(self.entities_count.into()));
        map.insert("profiles_count".to_string(),  Value::Number(self.profiles_count.into()));
        map.insert(
            "entity_types".to_string(),
            Value::Array(
                self.entity_types
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
        map.insert("config_generated".to_string(), Value::Bool(self.config_generated));
        map.insert(
            "error".to_string(),
            match &self.error {
                Some(e) => Value::String(e.clone()),
                None    => Value::Null,
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
            (SimulationStatus::Created,   "\"created\""),
            (SimulationStatus::Preparing, "\"preparing\""),
            (SimulationStatus::Ready,     "\"ready\""),
            (SimulationStatus::Running,   "\"running\""),
            (SimulationStatus::Paused,    "\"paused\""),
            (SimulationStatus::Stopped,   "\"stopped\""),
            (SimulationStatus::Completed, "\"completed\""),
            (SimulationStatus::Failed,    "\"failed\""),
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
        assert_eq!(SimulationStatus::Created.as_str(),   "created");
        assert_eq!(SimulationStatus::Preparing.as_str(), "preparing");
        assert_eq!(SimulationStatus::Ready.as_str(),     "ready");
        assert_eq!(SimulationStatus::Running.as_str(),   "running");
        assert_eq!(SimulationStatus::Paused.as_str(),    "paused");
        assert_eq!(SimulationStatus::Stopped.as_str(),   "stopped");
        assert_eq!(SimulationStatus::Completed.as_str(), "completed");
        assert_eq!(SimulationStatus::Failed.as_str(),    "failed");
    }

    /// Display must match as_str.
    #[test]
    fn simulation_status_display() {
        assert_eq!(SimulationStatus::Running.to_string(), "running");
        assert_eq!(SimulationStatus::Failed.to_string(),  "failed");
    }

    // -- PlatformType serialization -----------------------------------------

    /// Both platform variants round-trip correctly.
    #[test]
    fn platform_type_serde_all_variants() {
        let cases = [
            (PlatformType::Twitter, "\"twitter\""),
            (PlatformType::Reddit,  "\"reddit\""),
        ];
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
        assert!(serde_json::from_str::<PlatformType>("\"both\"").is_err(),
            "PlatformType must NOT have a BOTH variant");
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
        assert_eq!(s.project_id,    "proj-001");
        assert_eq!(s.graph_id,      "graph-001");
        assert!(s.enable_twitter,  "enable_twitter default must be true");
        assert!(s.enable_reddit,   "enable_reddit default must be true");
        assert_eq!(s.status,           SimulationStatus::Created);
        assert_eq!(s.entities_count,   0);
        assert_eq!(s.profiles_count,   0);
        assert!(s.entity_types.is_empty(), "entity_types default must be []");
        assert!(!s.config_generated,  "config_generated default must be false");
        assert_eq!(s.config_reasoning, "");
        assert_eq!(s.current_round,    0);
        assert_eq!(s.twitter_status,  "not_started");
        assert_eq!(s.reddit_status,   "not_started");
        assert!(!s.created_at.is_empty(), "created_at must be set on construction");
        assert!(!s.updated_at.is_empty(), "updated_at must be set on construction");
        assert!(s.error.is_none(), "error default must be None");
    }

    // -- to_dict key count and order ----------------------------------------

    /// to_dict must emit exactly 17 keys in Python declaration order.
    #[test]
    fn simulation_state_to_dict_key_count_and_order() {
        let s = SimulationState::new(
            "sim-1".to_string(),
            "proj-1".to_string(),
            "graph-1".to_string(),
        );
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
        assert_eq!(
            obj["error"],
            Value::String("something went wrong".to_string())
        );
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
        assert_eq!(obj["enable_reddit"],  Value::Bool(true));
    }

    // -- to_simple_dict key count and order ---------------------------------

    /// to_simple_dict must emit exactly 9 keys in Python declaration order.
    #[test]
    fn simulation_state_to_simple_dict_key_count_and_order() {
        let s = SimulationState::new(
            "sim-1".to_string(),
            "proj-1".to_string(),
            "graph-1".to_string(),
        );
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
            (SimulationStatus::Created,   "created"),
            (SimulationStatus::Preparing, "preparing"),
            (SimulationStatus::Ready,     "ready"),
            (SimulationStatus::Running,   "running"),
            (SimulationStatus::Paused,    "paused"),
            (SimulationStatus::Stopped,   "stopped"),
            (SimulationStatus::Completed, "completed"),
            (SimulationStatus::Failed,    "failed"),
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
        let s = SimulationState::new(
            "abc".to_string(),
            "xyz".to_string(),
            "ggg".to_string(),
        );
        let dict = s.to_dict();
        let obj = dict.as_object().unwrap();

        assert_eq!(obj["simulation_id"],  Value::String("abc".to_string()));
        assert_eq!(obj["project_id"],     Value::String("xyz".to_string()));
        assert_eq!(obj["graph_id"],       Value::String("ggg".to_string()));
        assert_eq!(obj["enable_twitter"], Value::Bool(true));
        assert_eq!(obj["enable_reddit"],  Value::Bool(true));
        assert_eq!(obj["status"],         Value::String("created".to_string()));
        assert_eq!(obj["entities_count"], Value::Number(0.into()));
        assert_eq!(obj["profiles_count"], Value::Number(0.into()));
        assert_eq!(obj["entity_types"],   Value::Array(vec![]));
        assert_eq!(obj["config_generated"], Value::Bool(false));
        assert_eq!(obj["config_reasoning"], Value::String(String::new()));
        assert_eq!(obj["current_round"],  Value::Number(0.into()));
        assert_eq!(obj["twitter_status"], Value::String("not_started".to_string()));
        assert_eq!(obj["reddit_status"],  Value::String("not_started".to_string()));
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
// ## get_run_instructions substrate note (S-680) [≠]
//
// The Python `get_run_instructions` returns command strings that run MiroFish's
// OASIS Python subprocess scripts:
//   `python {scripts_dir}/run_twitter_simulation.py --config {config_path}`
//   `conda activate MiroFish`  etc.
//
// teri has NO `scripts/run_*_simulation.py` and no conda env — it runs the
// SimEngine in-process (substrate decision, locked by the port architect).
// Fabricating these Python-script command strings would produce commands that
// CANNOT run in teri's substrate, which is a worse downgrade than admitting
// the gap.  This is a genuine [≠]-substrate case (NOT "won't use" — the strings
// are inexpressible in teri's runtime).
//
// The structural fields that ARE expressible (simulation_dir, config_file) are
// ported faithfully.  The commands/instructions strings are omitted with a clear
// key and note indicating teri's native invocation path.

// ---------------------------------------------------------------------------
// RunInstructions
// ---------------------------------------------------------------------------

/// Return value of [`SimulationManager::get_run_instructions`].
///
/// Structural fields from Python are ported; the OASIS Python-script command strings
/// are not expressible in teri's substrate (see module-level note) and are replaced
/// with a teri-native indicator.
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

    /// [≠]-substrate: the Python source also returns `scripts_dir` (pointing to
    /// MiroFish's `backend/scripts/`) and `commands` dict (Python subprocess invocations
    /// for `run_twitter_simulation.py`, `run_reddit_simulation.py`,
    /// `run_parallel_simulation.py`) plus `instructions` string (conda activate steps).
    /// These are genuinely inexpressible in teri's substrate: teri uses the native
    /// in-process `SimEngine` (no Python scripts, no conda env), so fabricating
    /// those path strings would produce commands that cannot run.  Callers should
    /// invoke `SimEngine::run` instead of shelling out to Python.
    pub substrate_note: &'static str,
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
        SimulationManager {
            sim_data_dir: sim_data_dir.into(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Create a `SimulationManager` from teri's `Config`.
    ///
    /// Uses `config.oasis_simulation_data_dir` (env `OASIS_SIMULATION_DATA_DIR`,
    /// default `"./uploads/simulations"`) — the teri equivalent of Python's
    /// `SIMULATION_DATA_DIR = os.path.join(dirname(__file__), '../../uploads/simulations')`.
    pub fn from_config(config: &crate::config::Config) -> Self {
        SimulationManager::new(Path::new(&config.oasis_simulation_data_dir).to_path_buf())
    }

    // -----------------------------------------------------------------------
    // _get_simulation_dir (S-671)
    // -----------------------------------------------------------------------

    /// Return the path to `{sim_data_dir}/{simulation_id}`, creating it if absent.
    ///
    /// Port of `_get_simulation_dir` (`simulation_manager.py:139-143`).
    ///
    /// Python: `sim_dir = os.path.join(SIMULATION_DATA_DIR, simulation_id); os.makedirs(sim_dir, exist_ok=True); return sim_dir`
    fn get_simulation_dir(&self, simulation_id: &str) -> Result<PathBuf> {
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
    fn save_simulation_state(&self, state: &mut SimulationState) -> Result<()> {
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

        let project_id = obj.get("project_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let graph_id = obj.get("graph_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let enable_twitter = obj.get("enable_twitter")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let enable_reddit = obj.get("enable_reddit")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Step 4: status — Python `SimulationStatus(data.get("status", "created"))`.
        // An invalid string raises ValueError in Python; we return Err here (faithful).
        let status_str = obj.get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("created");
        let status: SimulationStatus = serde_json::from_value(Value::String(status_str.to_string()))
            .map_err(|_| TeriError::Sim(format!(
                "invalid SimulationStatus {status_str:?} in {simulation_id}/state.json \
                 (Python SimulationStatus(str) raises ValueError on unknown value)"
            )))?;

        let entities_count = obj.get("entities_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let profiles_count = obj.get("profiles_count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let entity_types: Vec<String> = obj.get("entity_types")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let config_generated = obj.get("config_generated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let config_reasoning = obj.get("config_reasoning")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let current_round = obj.get("current_round")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let twitter_status = obj.get("twitter_status")
            .and_then(|v| v.as_str())
            .unwrap_or("not_started")
            .to_string();

        let reddit_status = obj.get("reddit_status")
            .and_then(|v| v.as_str())
            .unwrap_or("not_started")
            .to_string();

        // Python: `data.get("created_at", datetime.now().isoformat())`
        let created_at = obj.get("created_at")
            .and_then(|v| v.as_str())
            .unwrap_or(&now)
            .to_string();

        // Python: `data.get("updated_at", datetime.now().isoformat())`
        let updated_at = obj.get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or(&now)
            .to_string();

        // Python: `data.get("error")` — None if key absent or value is null
        let error = obj.get("error")
            .and_then(|v| v.as_str())
            .map(str::to_string);

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
    // get_simulation (S-676)
    // -----------------------------------------------------------------------

    /// Return the current state of a simulation, or `None` if it does not exist.
    ///
    /// Port of `get_simulation` (`simulation_manager.py:459-461`).
    /// Thin delegation to `_load_simulation_state`.
    pub fn get_simulation(&self, simulation_id: &str) -> Result<Option<SimulationState>> {
        self.load_simulation_state(simulation_id)
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
    pub fn get_profiles(
        &self,
        simulation_id: &str,
        platform: &str,
    ) -> Result<Vec<Value>> {
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

        Ok(RunInstructions {
            simulation_dir: sim_dir,
            config_file,
            // [≠]-substrate: MiroFish Python OASIS subprocess commands cannot run in teri.
            // teri uses SimEngine::run in-process.  See module-level substrate note.
            substrate_note: "OASIS Python subprocess commands (run_twitter_simulation.py, \
                             run_reddit_simulation.py, run_parallel_simulation.py, conda activate) \
                             are inexpressible in teri's substrate. \
                             Use SimEngine::run() instead.",
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
        assert_eq!(hex_part.len(), 12, "hex suffix must be exactly 12 chars: {}", state.simulation_id);
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
        assert_ne!(
            state.updated_at, created_updated_at,
            "updated_at must be bumped on save"
        );

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
        std::fs::write(
            hidden.join("state.json"),
            r#"{"project_id":"proj-h","status":"created"}"#
        ).unwrap();

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
        assert!(
            result.is_err(),
            "missing state must return Err (Python raises ValueError)"
        );
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
        let profiles_json = r#"[{"username":"alice","bio":"test"},{"username":"bob","bio":"test2"}]"#;
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
        std::fs::write(
            sim_dir.join("twitter_profiles.json"),
            r#"[{"username":"charlie"}]"#
        ).unwrap();

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

        let instr = mgr.get_run_instructions(&state.simulation_id).unwrap();

        assert!(
            instr.simulation_dir.ends_with(&state.simulation_id),
            "simulation_dir must point to sim dir: {:?}",
            instr.simulation_dir
        );
        assert!(
            instr.config_file.ends_with("simulation_config.json"),
            "config_file must end with simulation_config.json: {:?}",
            instr.config_file
        );
        assert!(
            !instr.substrate_note.is_empty(),
            "substrate_note must explain the [≠] gap"
        );
        assert!(
            instr.substrate_note.contains("SimEngine"),
            "substrate_note must direct caller to SimEngine: {}",
            instr.substrate_note
        );
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

        let unique: std::collections::HashSet<&str> =
            ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(unique.len(), ids.len(), "all simulation IDs must be unique");
    }
}
