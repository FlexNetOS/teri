//! Simulation state types — port of `backend/app/services/simulation_manager.py` L25-112.
//!
//! This module contains ONLY the state types (sub-cycle b):
//! - [`SimulationStatus`] — 8-variant enum (L25-34)
//! - [`PlatformType`]     — 2-variant enum (L37-40)
//! - [`SimulationState`]  — 17-field dataclass with `to_dict` / `to_simple_dict` (L43-112)
//!
//! The `SimulationManager` class (L115+) is ported in sub-cycles c/d.
//!
//! # Ledger corrections
//! The parity-ledger summary was wrong on two counts; the SOURCE is authoritative:
//! - `SimulationStatus` has **8** variants (not 4): CREATED, PREPARING, READY, RUNNING,
//!   PAUSED, STOPPED, COMPLETED, FAILED.
//! - `PlatformType` has **2** variants (not 3): TWITTER, REDDIT only (no BOTH).
//!
//! # Symbols: S-636..S-667

use serde::{Deserialize, Serialize};
use serde_json::{Value, Map};

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
