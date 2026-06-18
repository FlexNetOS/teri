//! IPC protocol types — port of `backend/app/services/simulation_ipc.py` L25-92 (MiroFish).
//!
//! Sub-cycle (a): protocol message types only.
//! Sub-cycle (b) (`SimulationIPCClient` L95+ / `SimulationIPCServer` L288+`) is NOT ported here.
//!
//! # Symbols ported: S-453..S-476
//!
//! ## Fidelity notes
//!
//! ### .value strings (S-453..S-461)
//! Python's `CommandType(str, Enum)` / `CommandStatus(str, Enum)` make the enum
//! value itself a `str`.  Downstream callers access `.value` to get the
//! serialization string.  We use `#[serde(rename_all = "snake_case")]` so that
//! `serde_json::to_string(&CommandType::BatchInterview)` → `"batch_interview"`,
//! and expose `as_str(&self) -> &str` to mirror `.value` access.
//!
//! ### to_dict key order (S-467, S-475)
//! Python 3.7+ dict literals preserve insertion order.  `serde_json` is compiled
//! with the `preserve_order` feature (see `Cargo.toml`), so
//! `serde_json::Map::new()` + sequential `.insert(...)` calls preserve the same
//! 4-key / 5-key order as the Python source.
//!
//! ### Null-not-omitted (S-475)
//! `IPCResponse.to_dict()` always emits `"result": null` and `"error": null`
//! when those fields are `None` — matching Python's `json.dumps(self.to_dict())`
//! where `None` serialises as JSON `null`, not an omitted key.
//!
//! ### from_dict tolerance (S-468, S-476)
//! Mirrors Python `.get(key, default)`:
//! - `args` / `result` / `error` / `timestamp` — optional, fall back to their
//!   Python defaults (empty map / None / None / `datetime.now().isoformat()`).
//! - `command_id` / `command_type` / `status` — required; missing or unrecognised
//!   values return `Err(TeriError::Sim(...))`.
//!
//! ### Timestamp default
//! Both `IPCCommand` and `IPCResponse` default `timestamp` to
//! `crate::models::project::python_isoformat_local()` — local naive time with
//! microseconds omitted when zero — matching Python `datetime.now().isoformat()`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{Result, TeriError};
use crate::models::project::python_isoformat_local;

// ---------------------------------------------------------------------------
// CommandType (S-453..S-456)
// ---------------------------------------------------------------------------

/// Port of `CommandType(str, Enum)` (`simulation_ipc.py:25-29`).
///
/// Three variants serialised to their exact Python `.value` strings.
///
/// S-453 (type), S-454 (INTERVIEW), S-455 (BATCH_INTERVIEW), S-456 (CLOSE_ENV)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandType {
    /// `"interview"` — single-Agent interview.
    Interview,
    /// `"batch_interview"` — bulk interview.
    BatchInterview,
    /// `"close_env"` — shut down the environment.
    CloseEnv,
}

impl CommandType {
    /// Return the serialisation string, mirroring Python's `command_type.value`.
    ///
    /// ```
    /// # use teri::services::simulation_ipc::CommandType;
    /// assert_eq!(CommandType::Interview.as_str(),      "interview");
    /// assert_eq!(CommandType::BatchInterview.as_str(), "batch_interview");
    /// assert_eq!(CommandType::CloseEnv.as_str(),       "close_env");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Interview      => "interview",
            Self::BatchInterview => "batch_interview",
            Self::CloseEnv       => "close_env",
        }
    }
}

// ---------------------------------------------------------------------------
// CommandStatus (S-457..S-461)
// ---------------------------------------------------------------------------

/// Port of `CommandStatus(str, Enum)` (`simulation_ipc.py:32-37`).
///
/// Four variants serialised to their exact Python `.value` strings.
///
/// S-457 (type), S-458 (PENDING), S-459 (PROCESSING), S-460 (COMPLETED), S-461 (FAILED)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    /// `"pending"`
    Pending,
    /// `"processing"`
    Processing,
    /// `"completed"`
    Completed,
    /// `"failed"`
    Failed,
}

impl CommandStatus {
    /// Return the serialisation string, mirroring Python's `status.value`.
    ///
    /// ```
    /// # use teri::services::simulation_ipc::CommandStatus;
    /// assert_eq!(CommandStatus::Pending.as_str(),    "pending");
    /// assert_eq!(CommandStatus::Processing.as_str(), "processing");
    /// assert_eq!(CommandStatus::Completed.as_str(),  "completed");
    /// assert_eq!(CommandStatus::Failed.as_str(),     "failed");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending    => "pending",
            Self::Processing => "processing",
            Self::Completed  => "completed",
            Self::Failed     => "failed",
        }
    }
}

// ---------------------------------------------------------------------------
// IPCCommand (S-462..S-468)
// ---------------------------------------------------------------------------

/// Port of `IPCCommand` dataclass (`simulation_ipc.py:40-63`).
///
/// | Python field    | Rust field      | Default                         |
/// |-----------------|-----------------|---------------------------------|
/// | `command_id`    | `command_id`    | — required                      |
/// | `command_type`  | `command_type`  | — required                      |
/// | `args`          | `args`          | `{}` (empty map)                |
/// | `timestamp`     | `timestamp`     | `python_isoformat_local()`      |
///
/// S-462 (type), S-463..S-466 (fields), S-467 (to_dict), S-468 (from_dict)
#[derive(Debug, Clone)]
pub struct IPCCommand {
    /// S-463
    pub command_id: String,
    /// S-464
    pub command_type: CommandType,
    /// S-465  Python `Dict[str, Any]` — empty map when absent in source data.
    pub args: Map<String, Value>,
    /// S-466  Python `field(default_factory=lambda: datetime.now().isoformat())`.
    pub timestamp: String,
}

impl IPCCommand {
    /// Port of `IPCCommand.to_dict()` (`simulation_ipc.py:48-54`).
    ///
    /// Emits a 4-key JSON object with keys in EXACT source order:
    /// `command_id`, `command_type` (the `.value` string), `args`, `timestamp`.
    ///
    /// S-467
    pub fn to_dict(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "command_id".to_string(),
            Value::String(self.command_id.clone()),
        );
        map.insert(
            "command_type".to_string(),
            Value::String(self.command_type.as_str().to_string()),
        );
        map.insert("args".to_string(), Value::Object(self.args.clone()));
        map.insert(
            "timestamp".to_string(),
            Value::String(self.timestamp.clone()),
        );
        Value::Object(map)
    }

    /// Port of `IPCCommand.from_dict()` (`simulation_ipc.py:56-63`).
    ///
    /// - `command_id`   — required; missing → `Err`.
    /// - `command_type` — required; unrecognised string → `Err`
    ///   (Python `CommandType(data["command_type"])` raises `ValueError`).
    /// - `args`         — optional; absent → empty map (Python `.get("args", {})`).
    /// - `timestamp`    — optional; absent → `python_isoformat_local()`
    ///   (Python `.get("timestamp", datetime.now().isoformat())`).
    ///
    /// S-468
    pub fn from_dict(data: &Value) -> Result<Self> {
        let obj = data.as_object().ok_or_else(|| {
            TeriError::Sim("IPCCommand.from_dict: data must be a JSON object".to_string())
        })?;

        // command_id — required (Python: data["command_id"])
        let command_id = obj
            .get("command_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                TeriError::Sim(
                    "IPCCommand.from_dict: missing required field 'command_id'".to_string(),
                )
            })?
            .to_string();

        // command_type — required; parse the string value
        // Python: CommandType(data["command_type"]) raises ValueError on unknown
        let command_type_str = obj
            .get("command_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                TeriError::Sim(
                    "IPCCommand.from_dict: missing required field 'command_type'".to_string(),
                )
            })?;
        let command_type: CommandType =
            serde_json::from_value(Value::String(command_type_str.to_string())).map_err(|_| {
                TeriError::Sim(format!(
                    "IPCCommand.from_dict: unrecognised command_type {command_type_str:?} \
                     (Python CommandType(str) raises ValueError on unknown value)"
                ))
            })?;

        // args — optional; default empty map (Python: data.get("args", {}))
        let args: Map<String, Value> = obj
            .get("args")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        // timestamp — optional; default now (Python: data.get("timestamp", datetime.now().isoformat()))
        let timestamp = obj
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(python_isoformat_local);

        Ok(Self {
            command_id,
            command_type,
            args,
            timestamp,
        })
    }
}

// ---------------------------------------------------------------------------
// IPCResponse (S-469..S-476)
// ---------------------------------------------------------------------------

/// Port of `IPCResponse` dataclass (`simulation_ipc.py:66-92`).
///
/// | Python field  | Rust field    | Default                        |
/// |---------------|---------------|--------------------------------|
/// | `command_id`  | `command_id`  | — required                     |
/// | `status`      | `status`      | — required                     |
/// | `result`      | `result`      | `None`                         |
/// | `error`       | `error`       | `None`                         |
/// | `timestamp`   | `timestamp`   | `python_isoformat_local()`     |
///
/// S-469 (type), S-470..S-474 (fields), S-475 (to_dict), S-476 (from_dict)
#[derive(Debug, Clone)]
pub struct IPCResponse {
    /// S-470
    pub command_id: String,
    /// S-471
    pub status: CommandStatus,
    /// S-472  Python `Optional[Dict[str, Any]] = None`.
    pub result: Option<Map<String, Value>>,
    /// S-473  Python `Optional[str] = None`.
    pub error: Option<String>,
    /// S-474  Python `field(default_factory=lambda: datetime.now().isoformat())`.
    pub timestamp: String,
}

impl IPCResponse {
    /// Port of `IPCResponse.to_dict()` (`simulation_ipc.py:75-81`).
    ///
    /// Emits a 5-key JSON object with keys in EXACT source order:
    /// `command_id`, `status` (the `.value` string), `result`, `error`, `timestamp`.
    ///
    /// `result=None` → JSON `null` (Python `json.dumps` encodes `None` as `null`,
    /// never omits the key).  Same for `error=None`.
    ///
    /// S-475
    pub fn to_dict(&self) -> Value {
        let mut map = Map::new();
        map.insert(
            "command_id".to_string(),
            Value::String(self.command_id.clone()),
        );
        map.insert(
            "status".to_string(),
            Value::String(self.status.as_str().to_string()),
        );
        // result: None → null (key always present)
        map.insert(
            "result".to_string(),
            match &self.result {
                Some(m) => Value::Object(m.clone()),
                None    => Value::Null,
            },
        );
        // error: None → null (key always present)
        map.insert(
            "error".to_string(),
            match &self.error {
                Some(s) => Value::String(s.clone()),
                None    => Value::Null,
            },
        );
        map.insert(
            "timestamp".to_string(),
            Value::String(self.timestamp.clone()),
        );
        Value::Object(map)
    }

    /// Port of `IPCResponse.from_dict()` (`simulation_ipc.py:83-92`).
    ///
    /// - `command_id` — required; missing → `Err`.
    /// - `status`     — required; unrecognised string → `Err`
    ///   (Python `CommandStatus(data["status"])` raises `ValueError`).
    /// - `result`     — optional; absent OR JSON null → `None`
    ///   (Python `.get("result")` defaults to `None`).
    /// - `error`      — optional; absent OR JSON null → `None`
    ///   (Python `.get("error")` defaults to `None`).
    /// - `timestamp`  — optional; absent → `python_isoformat_local()`.
    ///
    /// S-476
    pub fn from_dict(data: &Value) -> Result<Self> {
        let obj = data.as_object().ok_or_else(|| {
            TeriError::Sim("IPCResponse.from_dict: data must be a JSON object".to_string())
        })?;

        // command_id — required (Python: data["command_id"])
        let command_id = obj
            .get("command_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                TeriError::Sim(
                    "IPCResponse.from_dict: missing required field 'command_id'".to_string(),
                )
            })?
            .to_string();

        // status — required; parse the string value
        // Python: CommandStatus(data["status"]) raises ValueError on unknown
        let status_str = obj
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                TeriError::Sim(
                    "IPCResponse.from_dict: missing required field 'status'".to_string(),
                )
            })?;
        let status: CommandStatus =
            serde_json::from_value(Value::String(status_str.to_string())).map_err(|_| {
                TeriError::Sim(format!(
                    "IPCResponse.from_dict: unrecognised status {status_str:?} \
                     (Python CommandStatus(str) raises ValueError on unknown value)"
                ))
            })?;

        // result — optional; absent OR JSON null → None
        // Python: data.get("result") → None when key absent
        let result: Option<Map<String, Value>> = obj
            .get("result")
            .and_then(|v| v.as_object())
            .cloned();

        // error — optional; absent OR JSON null → None
        // Python: data.get("error") → None when key absent
        let error: Option<String> = obj
            .get("error")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        // timestamp — optional; default now
        let timestamp = obj
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(python_isoformat_local);

        Ok(Self {
            command_id,
            status,
            result,
            error,
            timestamp,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------------
    // CommandType serialisation
    // -----------------------------------------------------------------------

    /// Each CommandType variant serialises to its exact lowercase-string value.
    #[test]
    fn command_type_serde_all_variants() {
        let cases = [
            (CommandType::Interview,      "\"interview\""),
            (CommandType::BatchInterview, "\"batch_interview\""),
            (CommandType::CloseEnv,       "\"close_env\""),
        ];
        for (variant, expected_json) in &cases {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(
                json, *expected_json,
                "CommandType::{:?} should serialise to {expected_json}",
                variant
            );
            // round-trip
            let back: CommandType = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, variant, "round-trip failed for {:?}", variant);
        }
    }

    /// as_str() returns the Python .value string for each variant.
    #[test]
    fn command_type_as_str_all_variants() {
        assert_eq!(CommandType::Interview.as_str(),      "interview");
        assert_eq!(CommandType::BatchInterview.as_str(), "batch_interview");
        assert_eq!(CommandType::CloseEnv.as_str(),       "close_env");
    }

    // -----------------------------------------------------------------------
    // CommandStatus serialisation
    // -----------------------------------------------------------------------

    /// Each CommandStatus variant serialises to its exact lowercase-string value.
    #[test]
    fn command_status_serde_all_variants() {
        let cases = [
            (CommandStatus::Pending,    "\"pending\""),
            (CommandStatus::Processing, "\"processing\""),
            (CommandStatus::Completed,  "\"completed\""),
            (CommandStatus::Failed,     "\"failed\""),
        ];
        for (variant, expected_json) in &cases {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(
                json, *expected_json,
                "CommandStatus::{:?} should serialise to {expected_json}",
                variant
            );
            let back: CommandStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, variant, "round-trip failed for {:?}", variant);
        }
    }

    /// as_str() returns the Python .value string for each variant.
    #[test]
    fn command_status_as_str_all_variants() {
        assert_eq!(CommandStatus::Pending.as_str(),    "pending");
        assert_eq!(CommandStatus::Processing.as_str(), "processing");
        assert_eq!(CommandStatus::Completed.as_str(),  "completed");
        assert_eq!(CommandStatus::Failed.as_str(),     "failed");
    }

    // -----------------------------------------------------------------------
    // IPCCommand::to_dict
    // -----------------------------------------------------------------------

    /// to_dict emits exactly 4 keys in source order; command_type is the .value string.
    #[test]
    fn ipc_command_to_dict_key_order_and_command_type_string() {
        let cmd = IPCCommand {
            command_id:   "cmd-001".to_string(),
            command_type: CommandType::Interview,
            args:         Map::new(),
            timestamp:    "2024-01-01T10:00:00".to_string(),
        };
        let dict = cmd.to_dict();
        let obj = dict.as_object().expect("to_dict must return a JSON object");

        // Exactly 4 keys
        assert_eq!(obj.len(), 4, "to_dict must emit exactly 4 keys");

        // Key ORDER (preserve_order feature)
        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["command_id", "command_type", "args", "timestamp"],
            "to_dict key order must match Python source"
        );

        // command_type is the .value string, not enum Debug
        assert_eq!(obj["command_type"], json!("interview"));
        assert_eq!(obj["command_id"],   json!("cmd-001"));
        assert_eq!(obj["timestamp"],    json!("2024-01-01T10:00:00"));
        assert_eq!(obj["args"],         json!({}));
    }

    /// to_dict with BatchInterview emits "batch_interview" (not "BatchInterview").
    #[test]
    fn ipc_command_to_dict_batch_interview_value_string() {
        let cmd = IPCCommand {
            command_id:   "cmd-002".to_string(),
            command_type: CommandType::BatchInterview,
            args:         {
                let mut m = Map::new();
                m.insert("agents".to_string(), json!(["a", "b"]));
                m
            },
            timestamp: "2024-01-01T11:00:00".to_string(),
        };
        let dict = cmd.to_dict();
        let obj = dict.as_object().unwrap();
        assert_eq!(obj["command_type"], json!("batch_interview"));
    }

    // -----------------------------------------------------------------------
    // IPCCommand::from_dict round-trip and defaults
    // -----------------------------------------------------------------------

    /// Round-trip: to_dict → from_dict preserves all fields.
    #[test]
    fn ipc_command_round_trip() {
        let original = IPCCommand {
            command_id:   "cmd-rt-001".to_string(),
            command_type: CommandType::CloseEnv,
            args:         {
                let mut m = Map::new();
                m.insert("key".to_string(), json!("value"));
                m
            },
            timestamp: "2024-06-17T09:30:00.123456".to_string(),
        };
        let dict = original.to_dict();
        let restored = IPCCommand::from_dict(&dict).unwrap();
        assert_eq!(restored.command_id,   original.command_id);
        assert_eq!(restored.command_type, original.command_type);
        assert_eq!(restored.args,         original.args);
        assert_eq!(restored.timestamp,    original.timestamp);
    }

    /// from_dict with absent args → empty map (Python .get("args", {})).
    #[test]
    fn ipc_command_from_dict_absent_args_defaults_to_empty_map() {
        let data = json!({
            "command_id":   "cmd-003",
            "command_type": "interview",
            "timestamp":    "2024-01-01T12:00:00"
        });
        let cmd = IPCCommand::from_dict(&data).unwrap();
        assert!(cmd.args.is_empty(), "absent args must default to empty map");
    }

    /// from_dict with absent timestamp → a non-empty default string.
    #[test]
    fn ipc_command_from_dict_absent_timestamp_defaults_to_now() {
        let data = json!({
            "command_id":   "cmd-004",
            "command_type": "close_env"
        });
        let cmd = IPCCommand::from_dict(&data).unwrap();
        assert!(
            !cmd.timestamp.is_empty(),
            "absent timestamp must default to python_isoformat_local()"
        );
    }

    /// from_dict with all fields present parses correctly.
    #[test]
    fn ipc_command_from_dict_all_fields() {
        let data = json!({
            "command_id":   "cmd-full",
            "command_type": "batch_interview",
            "args": {"n": 5},
            "timestamp": "2024-01-01T13:00:00"
        });
        let cmd = IPCCommand::from_dict(&data).unwrap();
        assert_eq!(cmd.command_id,   "cmd-full");
        assert_eq!(cmd.command_type, CommandType::BatchInterview);
        assert_eq!(cmd.args["n"],    json!(5));
        assert_eq!(cmd.timestamp,    "2024-01-01T13:00:00");
    }

    /// from_dict with missing command_id → Err.
    #[test]
    fn ipc_command_from_dict_missing_command_id_is_err() {
        let data = json!({"command_type": "interview"});
        assert!(
            IPCCommand::from_dict(&data).is_err(),
            "missing command_id must return Err"
        );
    }

    /// from_dict with unknown command_type string → Err.
    #[test]
    fn ipc_command_from_dict_unknown_command_type_is_err() {
        let data = json!({
            "command_id":   "cmd-005",
            "command_type": "totally_unknown"
        });
        assert!(
            IPCCommand::from_dict(&data).is_err(),
            "unrecognised command_type must return Err (mirrors Python ValueError)"
        );
    }

    // -----------------------------------------------------------------------
    // IPCResponse::to_dict — null-not-omitted
    // -----------------------------------------------------------------------

    /// to_dict with result=None and error=None emits null for both keys (never omits).
    #[test]
    fn ipc_response_to_dict_null_not_omitted() {
        let resp = IPCResponse {
            command_id: "cmd-r-001".to_string(),
            status:     CommandStatus::Completed,
            result:     None,
            error:      None,
            timestamp:  "2024-01-01T14:00:00".to_string(),
        };
        let dict = resp.to_dict();
        let obj = dict.as_object().expect("to_dict must return a JSON object");

        // Exactly 5 keys
        assert_eq!(obj.len(), 5, "to_dict must emit exactly 5 keys");

        // Key ORDER
        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["command_id", "status", "result", "error", "timestamp"],
            "to_dict key order must match Python source"
        );

        // status is the .value string
        assert_eq!(obj["status"], json!("completed"));

        // result and error are JSON null (NOT omitted)
        assert_eq!(obj["result"], Value::Null, "result=None must be JSON null, not omitted");
        assert_eq!(obj["error"],  Value::Null, "error=None must be JSON null, not omitted");

        // Verify the serialised JSON string contains the keys
        let serialised = serde_json::to_string(&dict).unwrap();
        assert!(
            serialised.contains("\"result\":null"),
            "serialised JSON must contain '\"result\":null', got: {serialised}"
        );
        assert!(
            serialised.contains("\"error\":null"),
            "serialised JSON must contain '\"error\":null', got: {serialised}"
        );
    }

    /// to_dict with result=Some and error=Some emits the values (not null).
    #[test]
    fn ipc_response_to_dict_with_result_and_error() {
        let mut result_map = Map::new();
        result_map.insert("outcome".to_string(), json!("success"));

        let resp = IPCResponse {
            command_id: "cmd-r-002".to_string(),
            status:     CommandStatus::Failed,
            result:     Some(result_map),
            error:      Some("something went wrong".to_string()),
            timestamp:  "2024-01-01T15:00:00".to_string(),
        };
        let dict = resp.to_dict();
        let obj = dict.as_object().unwrap();

        assert_eq!(obj["status"],           json!("failed"));
        assert_eq!(obj["result"]["outcome"], json!("success"));
        assert_eq!(obj["error"],             json!("something went wrong"));
    }

    // -----------------------------------------------------------------------
    // IPCResponse::from_dict round-trip and defaults
    // -----------------------------------------------------------------------

    /// Round-trip: to_dict → from_dict preserves all fields.
    #[test]
    fn ipc_response_round_trip() {
        let mut result_map = Map::new();
        result_map.insert("data".to_string(), json!(42));

        let original = IPCResponse {
            command_id: "cmd-rt-002".to_string(),
            status:     CommandStatus::Processing,
            result:     Some(result_map),
            error:      None,
            timestamp:  "2024-06-17T10:00:00.000001".to_string(),
        };
        let dict = original.to_dict();
        let restored = IPCResponse::from_dict(&dict).unwrap();
        assert_eq!(restored.command_id, original.command_id);
        assert_eq!(restored.status,     original.status);
        assert_eq!(restored.result,     original.result);
        assert_eq!(restored.error,      original.error);
        assert_eq!(restored.timestamp,  original.timestamp);
    }

    /// from_dict where result is JSON null → None.
    #[test]
    fn ipc_response_from_dict_null_result_is_none() {
        let data = json!({
            "command_id": "cmd-r-003",
            "status":     "completed",
            "result":     null,
            "error":      null,
            "timestamp":  "2024-01-01T16:00:00"
        });
        let resp = IPCResponse::from_dict(&data).unwrap();
        assert!(resp.result.is_none(), "JSON null result must deserialise to None");
        assert!(resp.error.is_none(),  "JSON null error must deserialise to None");
    }

    /// from_dict with absent result and error → None (Python .get() default).
    #[test]
    fn ipc_response_from_dict_absent_optional_fields_default_to_none() {
        let data = json!({
            "command_id": "cmd-r-004",
            "status":     "pending"
        });
        let resp = IPCResponse::from_dict(&data).unwrap();
        assert!(resp.result.is_none());
        assert!(resp.error.is_none());
        assert!(!resp.timestamp.is_empty(), "absent timestamp must default to now");
    }

    /// from_dict with missing command_id → Err.
    #[test]
    fn ipc_response_from_dict_missing_command_id_is_err() {
        let data = json!({"status": "completed"});
        assert!(IPCResponse::from_dict(&data).is_err());
    }

    /// from_dict with unknown status string → Err.
    #[test]
    fn ipc_response_from_dict_unknown_status_is_err() {
        let data = json!({
            "command_id": "cmd-r-005",
            "status":     "bogus_status"
        });
        assert!(
            IPCResponse::from_dict(&data).is_err(),
            "unrecognised status must return Err (mirrors Python ValueError)"
        );
    }

    // -----------------------------------------------------------------------
    // Key-order assertion via serialised JSON string
    // -----------------------------------------------------------------------

    /// IPCCommand to_dict serialised string starts with "command_id" key first.
    #[test]
    fn ipc_command_to_dict_serialised_key_order() {
        let cmd = IPCCommand {
            command_id:   "ord-test".to_string(),
            command_type: CommandType::Interview,
            args:         Map::new(),
            timestamp:    "2024-01-01T00:00:00".to_string(),
        };
        let s = serde_json::to_string(&cmd.to_dict()).unwrap();
        // The first key in the serialised string must be "command_id"
        assert!(
            s.starts_with(r#"{"command_id":"#),
            "serialised IPCCommand must start with command_id, got: {s}"
        );
    }

    /// IPCResponse to_dict serialised string starts with "command_id" and has "status" second.
    #[test]
    fn ipc_response_to_dict_serialised_key_order() {
        let resp = IPCResponse {
            command_id: "ord-test-r".to_string(),
            status:     CommandStatus::Pending,
            result:     None,
            error:      None,
            timestamp:  "2024-01-01T00:00:00".to_string(),
        };
        let s = serde_json::to_string(&resp.to_dict()).unwrap();
        assert!(
            s.starts_with(r#"{"command_id":"#),
            "serialised IPCResponse must start with command_id, got: {s}"
        );
        assert!(
            s.contains(r#","status":"pending","#),
            "status must be second key with value 'pending', got: {s}"
        );
    }
}
