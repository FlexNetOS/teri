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
            Self::Interview => "interview",
            Self::BatchInterview => "batch_interview",
            Self::CloseEnv => "close_env",
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
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
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
        map.insert("command_id".to_string(), Value::String(self.command_id.clone()));
        map.insert(
            "command_type".to_string(),
            Value::String(self.command_type.as_str().to_string()),
        );
        map.insert("args".to_string(), Value::Object(self.args.clone()));
        map.insert("timestamp".to_string(), Value::String(self.timestamp.clone()));
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
        let command_type_str =
            obj.get("command_type").and_then(|v| v.as_str()).ok_or_else(|| {
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
        let args: Map<String, Value> =
            obj.get("args").and_then(|v| v.as_object()).cloned().unwrap_or_default();

        // timestamp — optional; default now (Python: data.get("timestamp", datetime.now().isoformat()))
        let timestamp = obj
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(python_isoformat_local);

        Ok(Self { command_id, command_type, args, timestamp })
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
        map.insert("command_id".to_string(), Value::String(self.command_id.clone()));
        map.insert("status".to_string(), Value::String(self.status.as_str().to_string()));
        // result: None → null (key always present)
        map.insert(
            "result".to_string(),
            match &self.result {
                Some(m) => Value::Object(m.clone()),
                None => Value::Null,
            },
        );
        // error: None → null (key always present)
        map.insert(
            "error".to_string(),
            match &self.error {
                Some(s) => Value::String(s.clone()),
                None => Value::Null,
            },
        );
        map.insert("timestamp".to_string(), Value::String(self.timestamp.clone()));
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
        let status_str = obj.get("status").and_then(|v| v.as_str()).ok_or_else(|| {
            TeriError::Sim("IPCResponse.from_dict: missing required field 'status'".to_string())
        })?;
        let status: CommandStatus = serde_json::from_value(Value::String(status_str.to_string()))
            .map_err(|_| {
            TeriError::Sim(format!(
                "IPCResponse.from_dict: unrecognised status {status_str:?} \
                     (Python CommandStatus(str) raises ValueError on unknown value)"
            ))
        })?;

        // result — optional; absent OR JSON null → None
        // Python: data.get("result") → None when key absent
        let result: Option<Map<String, Value>> =
            obj.get("result").and_then(|v| v.as_object()).cloned();

        // error — optional; absent OR JSON null → None
        // Python: data.get("error") → None when key absent
        let error: Option<String> = obj.get("error").and_then(|v| v.as_str()).map(str::to_string);

        // timestamp — optional; default now
        let timestamp = obj
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(python_isoformat_local);

        Ok(Self { command_id, status, result, error, timestamp })
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
            (CommandType::Interview, "\"interview\""),
            (CommandType::BatchInterview, "\"batch_interview\""),
            (CommandType::CloseEnv, "\"close_env\""),
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
        assert_eq!(CommandType::Interview.as_str(), "interview");
        assert_eq!(CommandType::BatchInterview.as_str(), "batch_interview");
        assert_eq!(CommandType::CloseEnv.as_str(), "close_env");
    }

    // -----------------------------------------------------------------------
    // CommandStatus serialisation
    // -----------------------------------------------------------------------

    /// Each CommandStatus variant serialises to its exact lowercase-string value.
    #[test]
    fn command_status_serde_all_variants() {
        let cases = [
            (CommandStatus::Pending, "\"pending\""),
            (CommandStatus::Processing, "\"processing\""),
            (CommandStatus::Completed, "\"completed\""),
            (CommandStatus::Failed, "\"failed\""),
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
        assert_eq!(CommandStatus::Pending.as_str(), "pending");
        assert_eq!(CommandStatus::Processing.as_str(), "processing");
        assert_eq!(CommandStatus::Completed.as_str(), "completed");
        assert_eq!(CommandStatus::Failed.as_str(), "failed");
    }

    // -----------------------------------------------------------------------
    // IPCCommand::to_dict
    // -----------------------------------------------------------------------

    /// to_dict emits exactly 4 keys in source order; command_type is the .value string.
    #[test]
    fn ipc_command_to_dict_key_order_and_command_type_string() {
        let cmd = IPCCommand {
            command_id: "cmd-001".to_string(),
            command_type: CommandType::Interview,
            args: Map::new(),
            timestamp: "2024-01-01T10:00:00".to_string(),
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
        assert_eq!(obj["command_id"], json!("cmd-001"));
        assert_eq!(obj["timestamp"], json!("2024-01-01T10:00:00"));
        assert_eq!(obj["args"], json!({}));
    }

    /// to_dict with BatchInterview emits "batch_interview" (not "BatchInterview").
    #[test]
    fn ipc_command_to_dict_batch_interview_value_string() {
        let cmd = IPCCommand {
            command_id: "cmd-002".to_string(),
            command_type: CommandType::BatchInterview,
            args: {
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
            command_id: "cmd-rt-001".to_string(),
            command_type: CommandType::CloseEnv,
            args: {
                let mut m = Map::new();
                m.insert("key".to_string(), json!("value"));
                m
            },
            timestamp: "2024-06-17T09:30:00.123456".to_string(),
        };
        let dict = original.to_dict();
        let restored = IPCCommand::from_dict(&dict).unwrap();
        assert_eq!(restored.command_id, original.command_id);
        assert_eq!(restored.command_type, original.command_type);
        assert_eq!(restored.args, original.args);
        assert_eq!(restored.timestamp, original.timestamp);
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
        assert_eq!(cmd.command_id, "cmd-full");
        assert_eq!(cmd.command_type, CommandType::BatchInterview);
        assert_eq!(cmd.args["n"], json!(5));
        assert_eq!(cmd.timestamp, "2024-01-01T13:00:00");
    }

    /// from_dict with missing command_id → Err.
    #[test]
    fn ipc_command_from_dict_missing_command_id_is_err() {
        let data = json!({"command_type": "interview"});
        assert!(IPCCommand::from_dict(&data).is_err(), "missing command_id must return Err");
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
            status: CommandStatus::Completed,
            result: None,
            error: None,
            timestamp: "2024-01-01T14:00:00".to_string(),
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
        assert_eq!(obj["error"], Value::Null, "error=None must be JSON null, not omitted");

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
            status: CommandStatus::Failed,
            result: Some(result_map),
            error: Some("something went wrong".to_string()),
            timestamp: "2024-01-01T15:00:00".to_string(),
        };
        let dict = resp.to_dict();
        let obj = dict.as_object().unwrap();

        assert_eq!(obj["status"], json!("failed"));
        assert_eq!(obj["result"]["outcome"], json!("success"));
        assert_eq!(obj["error"], json!("something went wrong"));
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
            status: CommandStatus::Processing,
            result: Some(result_map),
            error: None,
            timestamp: "2024-06-17T10:00:00.000001".to_string(),
        };
        let dict = original.to_dict();
        let restored = IPCResponse::from_dict(&dict).unwrap();
        assert_eq!(restored.command_id, original.command_id);
        assert_eq!(restored.status, original.status);
        assert_eq!(restored.result, original.result);
        assert_eq!(restored.error, original.error);
        assert_eq!(restored.timestamp, original.timestamp);
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
        assert!(resp.error.is_none(), "JSON null error must deserialise to None");
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
            command_id: "ord-test".to_string(),
            command_type: CommandType::Interview,
            args: Map::new(),
            timestamp: "2024-01-01T00:00:00".to_string(),
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
            status: CommandStatus::Pending,
            result: None,
            error: None,
            timestamp: "2024-01-01T00:00:00".to_string(),
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

// ---------------------------------------------------------------------------
// Sub-cycle (b): SimulationIPCClient + SimulationIPCServer (S-477..S-492)
// ---------------------------------------------------------------------------
//
// Port of `SimulationIPCClient` (simulation_ipc.py:95-285) and
// `SimulationIPCServer` (simulation_ipc.py:288-395).
//
// # Transport choice: (A) mpsc<IpcEnvelope> + per-command oneshot<IPCResponse>
//
// Python uses filesystem IPC (`ipc_commands/` dir → scan → `ipc_responses/` dir)
// to cross the OS-process boundary between the Flask app and a separate simulation
// subprocess.  teri runs the simulation IN-PROCESS, so there is no OS boundary to
// bridge and the filesystem transport is **structurally absent** (same class as the
// Zep-network→petgraph substitution in DECISION-14).
//
// The in-process analog that preserves the OBSERVABLE contract is:
//   - mpsc channel carries `IpcEnvelope` (command + embedded oneshot reply sink)
//   - client awaits the oneshot reply, wrapped in `tokio::time::timeout`
//   - server drains the mpsc with `try_recv` (non-blocking, matches the source's
//     non-blocking dir-scan called inside the sim tick loop)
//   - liveness = shared `Arc<AtomicBool>` (replaces `env_status.json`)
//
// # `[≠]` file-transport artifacts (all rest on the locked in-process substrate)
//
// | Artifact                            | Why genuinely inexpressible in-process |
// |-------------------------------------|----------------------------------------|
// | `ipc_commands/` + `ipc_responses/`  | filesystem channel between 2 OS procs; |
// | dirs (+ `os.makedirs`)              | one proc → no FS boundary; replaced by |
// |                                     | mpsc+oneshot (same delivery, no obs.   |
// |                                     | change)                                |
// | `env_status.json` + timestamp       | cross-proc liveness file; liveness is  |
// |                                     | a shared in-mem AtomicBool             |
// | `os.remove` cleanup                 | nothing to clean up — mpsc consumes    |
// |                                     | the envelope, oneshot consumes itself  |
// | mtime-ordered dir scan              | mpsc is already FIFO → oldest-first    |
// |                                     | preserved; the mtime sort mechanism    |
// |                                     | is moot                                |
// | `poll_interval` (0.5 s)             | channel wakes awaiter immediately;     |
// |                                     | nothing to poll                        |
// | `JSONDecodeError`-retry             | file-write-race artifact; in-process   |
// |                                     | values are moved whole, never          |
// |                                     | partially observable                   |
//
// # Ported (the observable contract)
//
// - Command types + arg shapes: interview {agent_id, prompt, platform?},
//   batch_interview {interviews, platform?}, close_env {}
// - Conditional platform-key insertion (only when Some, matching `if platform:`)
// - Timeouts as REAL elapsed awaits: send_interview 60 s, send_batch_interview
//   120 s, send_close_env 30 s
// - IPCResponse status/result/error construction with command_id round-trip
// - FIFO oldest-first delivery ordering (mpsc preserves send order)
// - Log lines for send + receive (发送IPC命令 / 收到IPC响应)
// - check_env_alive semantics (true iff server has called start() and not stop())
// - start()/stop() liveness transitions

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tracing::{error, info};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// IpcEnvelope (the in-process unit crossing the mpsc channel)
// ---------------------------------------------------------------------------

/// The command envelope that crosses the in-process channel.
///
/// Carries the protocol `IPCCommand` PLUS the `oneshot` reply sink that replaces
/// the `{cmd_id}.json` response file.  By embedding the reply sink in the
/// envelope, correlation is **automatic** — the server does not need to match
/// `command_id` → file path; it fires the reply on the sender it already holds.
///
/// `reply` is private: only `SimulationIPCServer::send_response` (and the
/// `send_success`/`send_error` helpers) should fire it.  Callers inspect
/// `command` freely.
pub struct IpcEnvelope {
    /// The protocol command (command_id, command_type, args, timestamp).
    pub command: IPCCommand,
    /// Oneshot reply sink.  Fires exactly once, then the sender is consumed.
    /// `[≠]` replaces the `{cmd_id}.json` response file + `os.remove` cleanup.
    reply: oneshot::Sender<IPCResponse>,
}

// ---------------------------------------------------------------------------
// SimulationIPCClient (S-477..S-483)
// ---------------------------------------------------------------------------

/// Port of `SimulationIPCClient` (`simulation_ipc.py:95-285`).
///
/// Submits commands to the simulation server and awaits `IPCResponse` replies.
///
/// # `[≠]` construction (S-478)
///
/// Python's `__init__(simulation_dir)` creates `ipc_commands/` and
/// `ipc_responses/` directories on disk — the inter-process transport.  teri
/// runs the simulation in-process, so there is no filesystem boundary to bridge.
/// Construction is replaced by the `channel(buffer)` factory, which returns a
/// paired `(SimulationIPCClient, SimulationIPCServer)` sharing the mpsc channel
/// and a liveness `AtomicBool`.
///
/// # Clonable
///
/// `SimulationIPCClient` implements `Clone` — the mpsc `Sender` is cheap to clone
/// and allows multiple callers (the in-process analog of multiple Flask routes
/// writing to `ipc_commands/`).
///
/// S-477 (type), S-478 (`__init__`/`channel`), S-479..S-483 (methods).
#[derive(Clone)]
pub struct SimulationIPCClient {
    /// Mpsc sender — carries `IpcEnvelope` to the server.
    /// `[≠]` replaces `self.commands_dir` + file-write + `os.makedirs`.
    tx: mpsc::Sender<IpcEnvelope>,
    /// Shared liveness flag written by the server (`start`→true, `stop`→false).
    /// `[≠]` replaces reading `env_status.json` and checking `status=="alive"`.
    alive: Arc<AtomicBool>,
}

impl SimulationIPCClient {
    /// Send a command and await the response, up to `timeout`.
    ///
    /// Port of `send_command(command_type, args, timeout=60.0, poll_interval=0.5)`
    /// (`simulation_ipc.py:117-187`).
    ///
    /// # Transport mapping
    ///
    /// Python writes `{cmd_id}.json` to `ipc_commands/`, then busy-polls
    /// `ipc_responses/{cmd_id}.json` every `poll_interval` seconds until the file
    /// appears or `timeout` elapses.  teri replaces this with:
    ///   1. Build `IPCCommand` (fresh uuid v4 `command_id`).
    ///   2. Create a `oneshot` channel for the reply.
    ///   3. Send `IpcEnvelope { command, reply }` on the mpsc.
    ///   4. `tokio::time::timeout(timeout, reply_rx.await)` — a real elapsed-time
    ///      await matching Python's wall-clock `timeout` semantic.
    ///   5. On elapsed → `Err(TeriError::Sim("…timeout…"))` (mirrors `TimeoutError`).
    ///
    /// # `[≠]` omissions
    ///
    /// - `poll_interval` — irrelevant to a channel (wakes immediately on receipt);
    ///   the parameter does not appear in the Rust signature (omission documented
    ///   here; callers never observed poll granularity as an output).
    /// - File write / os.remove cleanup — transport artifact.
    /// - `JSONDecodeError` retry — file-race artifact.
    ///
    /// S-479
    pub async fn send_command(
        &self,
        command_type: CommandType,
        args: serde_json::Map<String, Value>,
        timeout: Duration,
    ) -> crate::error::Result<IPCResponse> {
        // Build command — command_id = fresh uuid v4 (mirrors Python str(uuid.uuid4()))
        let command_id = Uuid::new_v4().to_string();
        let command = IPCCommand {
            command_id: command_id.clone(),
            command_type,
            args,
            timestamp: crate::models::project::python_isoformat_local(),
        };

        // Log the send (mirrors Python logger.info(f"发送IPC命令: {command_type.value}, command_id=…"))
        info!("Sending IPC command: {}, command_id={}", command_type.as_str(), command_id);

        // Create the oneshot reply pair
        let (reply_tx, reply_rx) = oneshot::channel::<IPCResponse>();

        // Send the envelope — waits only if the bounded channel is full
        self.tx.send(IpcEnvelope { command, reply: reply_tx }).await.map_err(|_| {
            crate::error::TeriError::Sim(format!(
                "IPC channel closed before send (command_id={command_id})"
            ))
        })?;

        // Await the oneshot reply with the source-matching timeout
        match tokio::time::timeout(timeout, reply_rx).await {
            Ok(Ok(response)) => {
                // Log the receipt (mirrors Python logger.info(f"收到IPC响应: command_id=…, status=…"))
                info!(
                    "Received IPC response: command_id={}, status={}",
                    response.command_id,
                    response.status.as_str()
                );
                Ok(response)
            }
            Ok(Err(_)) => {
                // Sender dropped before firing — server shut down mid-command
                Err(crate::error::TeriError::Sim(format!(
                    "IPC reply sender dropped (server shutdown?) for command_id={command_id}"
                )))
            }
            Err(_elapsed) => {
                // Mirrors Python:
                //   logger.error(f"等待IPC响应超时: command_id={command_id}")
                //   raise TimeoutError(f"等待命令响应超时 ({timeout}秒)")
                error!("Timeout waiting for IPC response: command_id={}", command_id);
                // `{:?}` on f64 matches Python's `str(float)` (shortest round-trip with
                // trailing `.0` for integral values) → "60.0秒".  `[≠] U028-c1-TIMEOUTMSG-NUMFMT`:
                // when the API route passes an INTEGER timeout (`data.get('timeout', 60)`), Python
                // renders "60秒" while teri renders "60.0秒" (the int/float type is collapsed into
                // a `Duration` by design — non-recoverable).  Cosmetic, in the 504 `error` string
                // only, and on the producer-pending path; flagged, not silently dropped.
                // `TeriError::Timeout` (not `Sim`) so the API layer maps it to the faithful
                // status: Python raises `TimeoutError` here, which interview routes turn into a
                // 504 and `close_simulation_env` swallows into a graceful 200.
                Err(crate::error::TeriError::Timeout(format!(
                    "Timeout waiting for command response ({:?}s)",
                    timeout.as_secs_f64()
                )))
            }
        }
    }

    /// Send a single-agent interview command.
    ///
    /// Port of `send_interview(agent_id, prompt, platform=None, timeout=60.0)`
    /// (`simulation_ipc.py:189-222`).
    ///
    /// Args map = `{"agent_id": agent_id, "prompt": prompt}`, with `"platform"`
    /// inserted **only when `Some`** (matches Python `if platform: args["platform"] = platform`).
    ///
    /// Default timeout: 60 s (matches Python default).
    ///
    /// S-480
    pub async fn send_interview(
        &self,
        agent_id: i64,
        prompt: &str,
        platform: Option<&str>,
        timeout: Duration,
    ) -> crate::error::Result<IPCResponse> {
        let mut args = serde_json::Map::new();
        args.insert("agent_id".to_string(), Value::Number(agent_id.into()));
        args.insert("prompt".to_string(), Value::String(prompt.to_string()));
        // Conditional platform key — only when Some (matches Python `if platform:`)
        if let Some(p) = platform {
            args.insert("platform".to_string(), Value::String(p.to_string()));
        }
        self.send_command(CommandType::Interview, args, timeout).await
    }

    /// Send a batch interview command.
    ///
    /// Port of `send_batch_interview(interviews, platform=None, timeout=120.0)`
    /// (`simulation_ipc.py:224-252`).
    ///
    /// Args map = `{"interviews": interviews}`, with `"platform"` inserted only
    /// when `Some`.  Default timeout: 120 s.
    ///
    /// `interviews` mirrors the Python `List[Dict[str, Any]]` — each element is a
    /// JSON value (typically an object with `agent_id`, `prompt`, optional
    /// `platform`).
    ///
    /// S-481
    pub async fn send_batch_interview(
        &self,
        interviews: Vec<Value>,
        platform: Option<&str>,
        timeout: Duration,
    ) -> crate::error::Result<IPCResponse> {
        let mut args = serde_json::Map::new();
        args.insert("interviews".to_string(), Value::Array(interviews));
        // Conditional platform key (matches Python `if platform:`)
        if let Some(p) = platform {
            args.insert("platform".to_string(), Value::String(p.to_string()));
        }
        self.send_command(CommandType::BatchInterview, args, timeout).await
    }

    /// Send the close-environment command.
    ///
    /// Port of `send_close_env(timeout=30.0)` (`simulation_ipc.py:254-268`).
    ///
    /// Args = `{}`.  Default timeout: 30 s.
    ///
    /// S-482
    pub async fn send_close_env(&self, timeout: Duration) -> crate::error::Result<IPCResponse> {
        self.send_command(CommandType::CloseEnv, serde_json::Map::new(), timeout).await
    }

    /// Check whether the simulation environment is alive.
    ///
    /// Port of `check_env_alive() -> bool` (`simulation_ipc.py:270-285`).
    ///
    /// Python reads `env_status.json` and checks `status == "alive"`.  teri
    /// replaces the file read with a load from the shared `AtomicBool` that the
    /// server sets in `start()` / `stop()`.
    ///
    /// `[≠]` `env_status.json` file read + `JSONDecodeError` / `OSError` handling —
    /// replaced by the in-process boolean; there is no cross-process file to read.
    ///
    /// S-483
    pub fn check_env_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// SimulationIPCServer (S-484..S-492)
// ---------------------------------------------------------------------------

/// Port of `SimulationIPCServer` (`simulation_ipc.py:288-395`).
///
/// Receives commands from the in-process mpsc channel, services them, and fires
/// `IPCResponse` replies via the per-command oneshot.  Intended to be held by
/// the simulation engine (`SimEngine` / U-022) and called in its tick loop.
///
/// # `[≠]` construction (S-485)
///
/// Python's `__init__(simulation_dir)` creates directories and sets
/// `_running = False`.  teri replaces construction with the `channel(buffer)`
/// factory (see below).
///
/// S-484 (type), S-485 (`__init__`/`channel`), S-486..S-492 (methods).
pub struct SimulationIPCServer {
    /// Mpsc receiver — drains `IpcEnvelope` messages from clients.
    /// `[≠]` replaces the `ipc_commands/` directory scan.
    rx: mpsc::Receiver<IpcEnvelope>,
    /// Shared liveness flag.  Written by `start()`/`stop()`; read by the client's
    /// `check_env_alive()`.
    /// `[≠]` replaces `env_status.json` + `_update_env_status`.
    running: Arc<AtomicBool>,
}

/// Outcome of a non-blocking command poll that distinguishes "no command yet" from "all
/// clients gone".
///
/// `poll_commands()` collapses both `Empty` and `Disconnected` into `None`, which is the right
/// shape for a one-shot poll. The wait-for-commands service loop (`run_sim_body`) needs the
/// distinction: `Empty` → keep waiting (the env stays alive for more interview commands);
/// `Disconnected` → every `SimulationIPCClient` has been dropped (the run handle was removed),
/// so no command can ever arrive — exit the loop and let the env close. Python relied on the OS
/// killing the subprocess for that teardown; teri detects it from the channel state.
pub enum CommandPoll {
    /// A pending command envelope (command + reply sink).
    Command(IpcEnvelope),
    /// The queue is empty but at least one client sender is still alive.
    Empty,
    /// All client senders have been dropped — no further command can arrive.
    Disconnected,
}

impl SimulationIPCServer {
    /// Mark the server as running and set the liveness flag to `true`.
    ///
    /// Port of `start()` (`simulation_ipc.py:313-316`).
    ///
    /// Python sets `self._running = True` and calls `_update_env_status("alive")`,
    /// which writes `env_status.json`.  teri stores `true` on the shared
    /// `AtomicBool` — both sides see the change immediately.
    ///
    /// `[≠]` `_update_env_status` / `env_status.json` file write — the file is
    /// the cross-process delivery mechanism for a boolean; the boolean is already
    /// in-process.
    ///
    /// S-486
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
    }

    /// Mark the server as stopped and set the liveness flag to `false`.
    ///
    /// Port of `stop()` (`simulation_ipc.py:318-320`).
    ///
    /// S-487
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    // S-488 `_update_env_status` — `[≠]` fully absorbed into `start`/`stop` above.
    // Python wrote `{"status": status, "timestamp": datetime.now().isoformat()}` to
    // `env_status.json`.  The status boolean is now the AtomicBool; the timestamp
    // is a file-transport artifact (nothing consumes it in-process).

    /// Poll for the next pending command (non-blocking).
    ///
    /// Port of `poll_commands() -> Optional[IPCCommand]` (`simulation_ipc.py:332-360`).
    ///
    /// Python scans `ipc_commands/` sorted by mtime (oldest first), reads the first
    /// file it can parse, and returns the deserialized `IPCCommand`.  teri calls
    /// `try_recv()` on the mpsc receiver — non-blocking, returning `None` if the
    /// queue is empty.  **FIFO is preserved:** mpsc delivers messages in the order
    /// they were sent, which is the same ordering the mtime sort imposed on the
    /// filesystem.
    ///
    /// Returns `Some(IpcEnvelope)` — the envelope (command + reply sink), NOT a
    /// bare `IPCCommand`.  The reply sink must travel with the command because it
    /// is the mechanism for sending the response; the file impl re-derived the
    /// response path from `command_id + responses_dir` at send time.
    ///
    /// `[≠]` mtime-ordered dir scan, `JSONDecodeError`-retry-on-partial-file —
    /// file-race artifacts; mpsc delivers whole envelopes.
    ///
    /// S-489
    pub fn poll_commands(&mut self) -> Option<IpcEnvelope> {
        self.rx.try_recv().ok()
    }

    /// Non-blocking poll that distinguishes `Empty` from `Disconnected`.
    ///
    /// Used by the wait-for-commands service loop in `run_sim_body` (the post-simulation window
    /// where the env stays alive answering interview/close-env commands — the in-process analog
    /// of `IPCHandler.process_commands`, `run_twitter_simulation.py:343`). The loop keeps waiting
    /// on `Empty` and exits on `Disconnected` (all clients dropped) or a `close_env` command.
    pub fn try_poll(&mut self) -> CommandPoll {
        match self.rx.try_recv() {
            Ok(env) => CommandPoll::Command(env),
            Err(mpsc::error::TryRecvError::Empty) => CommandPoll::Empty,
            Err(mpsc::error::TryRecvError::Disconnected) => CommandPoll::Disconnected,
        }
    }

    /// Fire the oneshot reply for the given envelope.
    ///
    /// Port of `send_response(response)` (`simulation_ipc.py:362-378`).
    ///
    /// Python writes `{cmd_id}.json` to `ipc_responses/` and then removes the
    /// command file.  teri fires the embedded oneshot — the sender is consumed
    /// (drop = cleanup), and the client's `reply_rx.await` wakes immediately.
    ///
    /// `[≠]` file write + `os.remove` cleanup — the oneshot consumes itself.
    ///
    /// S-490
    pub fn send_response(envelope: IpcEnvelope, response: IPCResponse) {
        // Ignore the Err variant: the client timed out and dropped its receiver.
        // Python's `os.remove(command_file)` also silently ignores `OSError`.
        let _ = envelope.reply.send(response);
    }

    /// Send a success response for the given envelope.
    ///
    /// Port of `send_success(command_id, result)` (`simulation_ipc.py:380-386`).
    ///
    /// Constructs `IPCResponse { status: Completed, result: Some(result), error: None }`
    /// with `command_id` taken from `envelope.command.command_id` (preserved for
    /// protocol/log fidelity per DECISION-16 §16.4).
    ///
    /// S-491
    pub fn send_success(envelope: IpcEnvelope, result: serde_json::Map<String, Value>) {
        let command_id = envelope.command.command_id.clone();
        let response = IPCResponse {
            command_id,
            status: CommandStatus::Completed,
            result: Some(result),
            error: None,
            timestamp: crate::models::project::python_isoformat_local(),
        };
        Self::send_response(envelope, response);
    }

    /// Send an error response for the given envelope.
    ///
    /// Port of `send_error(command_id, error)` (`simulation_ipc.py:388-394`).
    ///
    /// Constructs `IPCResponse { status: Failed, error: Some(error), result: None }`.
    ///
    /// S-492
    pub fn send_error(envelope: IpcEnvelope, error: String) {
        let command_id = envelope.command.command_id.clone();
        let response = IPCResponse {
            command_id,
            status: CommandStatus::Failed,
            result: None,
            error: Some(error),
            timestamp: crate::models::project::python_isoformat_local(),
        };
        Self::send_response(envelope, response);
    }
}

// ---------------------------------------------------------------------------
// channel() factory (analog of both halves sharing `simulation_dir`) — S-478/S-485
// ---------------------------------------------------------------------------

/// Create a paired `(SimulationIPCClient, SimulationIPCServer)`.
///
/// Port of the shared-`simulation_dir` construction convention (Python:
/// `SimulationIPCClient(simulation_dir)` + `SimulationIPCServer(simulation_dir)`
/// — both pointing at the same directory, which is the only way they communicate).
/// teri replaces the shared directory with a shared mpsc channel + shared
/// `Arc<AtomicBool>`.
///
/// `buffer` controls the mpsc channel capacity (bounded; use e.g. `64` to match
/// the `SimEngine` broadcast buffer for backpressure parity).
///
/// # Returns
///
/// `(client, server)` — client holds the `Sender` (clonable for multiple callers),
/// server holds the `Receiver` (single consumer = the sim loop).
pub fn channel(buffer: usize) -> (SimulationIPCClient, SimulationIPCServer) {
    let (tx, rx) = mpsc::channel::<IpcEnvelope>(buffer);
    let alive = Arc::new(AtomicBool::new(false));
    let client = SimulationIPCClient { tx, alive: Arc::clone(&alive) };
    let server = SimulationIPCServer { rx, running: alive };
    (client, server)
}

// ---------------------------------------------------------------------------
// Default timeout constants (mirror Python per-method defaults)
// ---------------------------------------------------------------------------

/// Default timeout for `send_interview` (Python: `timeout=60.0`).
pub const INTERVIEW_TIMEOUT: Duration = Duration::from_secs(60);
/// Default timeout for `send_batch_interview` (Python: `timeout=120.0`).
pub const BATCH_INTERVIEW_TIMEOUT: Duration = Duration::from_secs(120);
/// Default timeout for `send_close_env` (Python: `timeout=30.0`).
pub const CLOSE_ENV_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Tests — sub-cycle (b)  (S-477..S-492)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod ipc_transport_tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;
    use tokio::task;

    // -----------------------------------------------------------------------
    // Helper: spawn a mock server loop.
    //
    // Starts the server, then loops draining commands:
    //   - By default replies send_success with an empty result map.
    //   - If `fail_next` is set (via a oneshot signal), the next command gets
    //     send_error instead.
    //   - Returns a JoinHandle so the test can abort the loop when done.
    // -----------------------------------------------------------------------
    fn spawn_mock_server(
        mut server: SimulationIPCServer,
        error_message: Option<String>,
    ) -> task::JoinHandle<()> {
        task::spawn(async move {
            server.start();
            loop {
                if let Some(env) = server.poll_commands() {
                    match &error_message {
                        Some(err_msg) => {
                            SimulationIPCServer::send_error(env, err_msg.clone());
                        }
                        None => {
                            SimulationIPCServer::send_success(env, serde_json::Map::new());
                        }
                    }
                } else {
                    // yield to let senders make progress
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        })
    }

    // -----------------------------------------------------------------------
    // Test: send_interview round-trip — args shape + COMPLETED response
    // -----------------------------------------------------------------------

    /// send_interview with platform=None → args has agent_id + prompt, no platform key.
    /// Response is COMPLETED.
    #[tokio::test]
    async fn send_interview_no_platform_round_trip() {
        let (client, server) = channel(64);
        let handle = spawn_mock_server(server, None);

        let resp = client
            .send_interview(42, "What do you think?", None, Duration::from_secs(5))
            .await
            .expect("send_interview must succeed");

        handle.abort();

        assert_eq!(resp.status, CommandStatus::Completed);
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        // command_id round-trip: server echoes the command_id back in the response
        assert!(!resp.command_id.is_empty(), "command_id must be non-empty");
    }

    /// send_interview with platform=Some("twitter") → args includes "platform" key.
    #[tokio::test]
    async fn send_interview_with_platform_includes_platform_key() {
        // We need to inspect the args — use a server that captures the envelope
        // command before replying.
        let (tx_args, rx_args) = tokio::sync::oneshot::channel::<serde_json::Map<String, Value>>();
        let tx_args = std::sync::Arc::new(tokio::sync::Mutex::new(Some(tx_args)));

        let (client, mut server) = channel(64);
        let handle = task::spawn(async move {
            server.start();
            loop {
                if let Some(env) = server.poll_commands() {
                    // Capture the args map
                    let args = env.command.args.clone();
                    if let Some(sender) = tx_args.lock().await.take() {
                        let _ = sender.send(args);
                    }
                    SimulationIPCServer::send_success(env, serde_json::Map::new());
                } else {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        });

        client
            .send_interview(7, "Hello agent", Some("twitter"), Duration::from_secs(5))
            .await
            .expect("send_interview must succeed");

        handle.abort();

        let args = rx_args.await.expect("args channel must receive");
        assert_eq!(args["agent_id"], json!(7));
        assert_eq!(args["prompt"], json!("Hello agent"));
        assert_eq!(args["platform"], json!("twitter"), "platform key must be present");
        assert_eq!(args.len(), 3, "args must have exactly 3 keys when platform is Some");
    }

    /// send_interview without platform → args has exactly 2 keys (no platform).
    #[tokio::test]
    async fn send_interview_without_platform_excludes_platform_key() {
        let (tx_args, rx_args) = tokio::sync::oneshot::channel::<serde_json::Map<String, Value>>();
        let tx_args = std::sync::Arc::new(tokio::sync::Mutex::new(Some(tx_args)));

        let (client, mut server) = channel(64);
        let handle = task::spawn(async move {
            server.start();
            loop {
                if let Some(env) = server.poll_commands() {
                    let args = env.command.args.clone();
                    if let Some(sender) = tx_args.lock().await.take() {
                        let _ = sender.send(args);
                    }
                    SimulationIPCServer::send_success(env, serde_json::Map::new());
                } else {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        });

        client
            .send_interview(99, "No platform", None, Duration::from_secs(5))
            .await
            .expect("send_interview must succeed");

        handle.abort();

        let args = rx_args.await.expect("args channel must receive");
        assert_eq!(args.len(), 2, "args must have exactly 2 keys when platform is None");
        assert!(!args.contains_key("platform"), "platform key must NOT be present");
    }

    // -----------------------------------------------------------------------
    // Test: send_batch_interview args shape
    // -----------------------------------------------------------------------

    /// send_batch_interview → args = {interviews: [...], platform?}.
    #[tokio::test]
    async fn send_batch_interview_args_shape() {
        let (tx_args, rx_args) = tokio::sync::oneshot::channel::<serde_json::Map<String, Value>>();
        let tx_args = std::sync::Arc::new(tokio::sync::Mutex::new(Some(tx_args)));

        let (client, mut server) = channel(64);
        let handle = task::spawn(async move {
            server.start();
            loop {
                if let Some(env) = server.poll_commands() {
                    let args = env.command.args.clone();
                    if let Some(sender) = tx_args.lock().await.take() {
                        let _ = sender.send(args);
                    }
                    SimulationIPCServer::send_success(env, serde_json::Map::new());
                } else {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        });

        let interviews =
            vec![json!({"agent_id": 1, "prompt": "Q1"}), json!({"agent_id": 2, "prompt": "Q2"})];
        client
            .send_batch_interview(interviews.clone(), Some("reddit"), Duration::from_secs(5))
            .await
            .expect("send_batch_interview must succeed");

        handle.abort();

        let args = rx_args.await.expect("args channel must receive");
        assert_eq!(
            args["interviews"],
            json!([{"agent_id": 1, "prompt": "Q1"}, {"agent_id": 2, "prompt": "Q2"}])
        );
        assert_eq!(args["platform"], json!("reddit"));
        assert_eq!(args.len(), 2);
    }

    /// send_batch_interview without platform → no platform key.
    #[tokio::test]
    async fn send_batch_interview_without_platform_excludes_platform_key() {
        let (tx_args, rx_args) = tokio::sync::oneshot::channel::<serde_json::Map<String, Value>>();
        let tx_args = std::sync::Arc::new(tokio::sync::Mutex::new(Some(tx_args)));

        let (client, mut server) = channel(64);
        let handle = task::spawn(async move {
            server.start();
            loop {
                if let Some(env) = server.poll_commands() {
                    let args = env.command.args.clone();
                    if let Some(sender) = tx_args.lock().await.take() {
                        let _ = sender.send(args);
                    }
                    SimulationIPCServer::send_success(env, serde_json::Map::new());
                } else {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        });

        client
            .send_batch_interview(vec![json!({"agent_id": 5})], None, Duration::from_secs(5))
            .await
            .expect("send_batch_interview must succeed");

        handle.abort();

        let args = rx_args.await.expect("args channel must receive");
        assert!(!args.contains_key("platform"), "no platform key when None");
        assert_eq!(args.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Test: send_error → FAILED response with error string
    // -----------------------------------------------------------------------

    /// When the server sends send_error, the client receives FAILED + error string.
    #[tokio::test]
    async fn send_error_propagates_to_client() {
        let (client, server) = channel(64);
        let handle = spawn_mock_server(server, Some("something went wrong".to_string()));

        let resp = client
            .send_interview(1, "test", None, Duration::from_secs(5))
            .await
            .expect("channel must respond even on error");

        handle.abort();

        assert_eq!(resp.status, CommandStatus::Failed);
        assert_eq!(resp.error.as_deref(), Some("something went wrong"));
        assert!(resp.result.is_none());
    }

    // -----------------------------------------------------------------------
    // Test: send_close_env → close_env command type, empty args
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn send_close_env_sends_correct_command_type() {
        let (tx_cmd, rx_cmd) = tokio::sync::oneshot::channel::<CommandType>();
        let tx_cmd = std::sync::Arc::new(tokio::sync::Mutex::new(Some(tx_cmd)));

        let (client, mut server) = channel(64);
        let handle = task::spawn(async move {
            server.start();
            loop {
                if let Some(env) = server.poll_commands() {
                    let ct = env.command.command_type;
                    if let Some(sender) = tx_cmd.lock().await.take() {
                        let _ = sender.send(ct);
                    }
                    SimulationIPCServer::send_success(env, serde_json::Map::new());
                } else {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
        });

        client
            .send_close_env(Duration::from_secs(5))
            .await
            .expect("send_close_env must succeed");

        handle.abort();

        let ct = rx_cmd.await.expect("command type must be received");
        assert_eq!(ct, CommandType::CloseEnv);
    }

    // -----------------------------------------------------------------------
    // Test: check_env_alive reflects start/stop
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn check_env_alive_reflects_start_and_stop() {
        let (client, server) = channel(64);

        // Before start: false
        assert!(!client.check_env_alive(), "alive must be false before start()");

        server.start();
        assert!(client.check_env_alive(), "alive must be true after start()");

        server.stop();
        assert!(!client.check_env_alive(), "alive must be false after stop()");
    }

    // -----------------------------------------------------------------------
    // Test: timeout — command with no server draining → Err(TeriError::Timeout)
    // -----------------------------------------------------------------------

    /// When the server is NOT draining, send_command times out and returns
    /// `TeriError::Timeout` (the variant the API layer maps to 504 / graceful-200,
    /// matching Python's `TimeoutError`).
    #[tokio::test]
    async fn send_command_times_out_when_server_not_draining() {
        // Create a channel but do NOT spawn a server that drains it.
        // We hold the server (and thus the receiver) to keep the channel open
        // (so tx.send doesn't fail), but never call poll_commands.
        let (client, _server) = channel(64);

        // Use a very short timeout so the test is fast.
        let result = client
            .send_command(CommandType::CloseEnv, serde_json::Map::new(), Duration::from_millis(50))
            .await;

        assert!(result.is_err(), "send_command must Err on timeout");
        match result {
            Err(crate::error::TeriError::Timeout(msg)) => {
                // Must mention the timeout duration in the error message
                assert!(
                    msg.to_lowercase().contains("timeout") || msg.contains('s'),
                    "error message should mention timeout, got: {msg}"
                );
            }
            other => panic!("expected TeriError::Timeout, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Test: FIFO ordering — two commands arrive oldest-first at the server
    // -----------------------------------------------------------------------

    /// Two commands sent in order A then B must be received by the server in
    /// the same order (FIFO, matching the mtime-sorted dir scan in Python).
    #[tokio::test]
    async fn poll_commands_fifo_ordering() {
        let (client, mut server) = channel(64);

        // Send two commands WITHOUT a running server task — use try_send (sync)
        // to avoid the borrow issue; the mpsc Sender has try_send.
        let (reply1_tx, reply1_rx) = oneshot::channel::<IPCResponse>();
        let (reply2_tx, reply2_rx) = oneshot::channel::<IPCResponse>();

        let cmd1 = IPCCommand {
            command_id: "first".to_string(),
            command_type: CommandType::Interview,
            args: serde_json::Map::new(),
            timestamp: crate::models::project::python_isoformat_local(),
        };
        let cmd2 = IPCCommand {
            command_id: "second".to_string(),
            command_type: CommandType::BatchInterview,
            args: serde_json::Map::new(),
            timestamp: crate::models::project::python_isoformat_local(),
        };

        client.tx.try_send(IpcEnvelope { command: cmd1, reply: reply1_tx }).unwrap();
        client.tx.try_send(IpcEnvelope { command: cmd2, reply: reply2_tx }).unwrap();

        // Now drain — must come in send order
        let env_a = server.poll_commands().expect("first command must be available");
        let env_b = server.poll_commands().expect("second command must be available");

        assert_eq!(env_a.command.command_id, "first", "first received must be first sent");
        assert_eq!(env_b.command.command_id, "second", "second received must be second sent");

        // Clean up the reply channels
        drop(reply1_rx);
        drop(reply2_rx);
        let _ = env_a.reply.send(IPCResponse {
            command_id: "first".to_string(),
            status: CommandStatus::Completed,
            result: None,
            error: None,
            timestamp: crate::models::project::python_isoformat_local(),
        });
        let _ = env_b.reply.send(IPCResponse {
            command_id: "second".to_string(),
            status: CommandStatus::Completed,
            result: None,
            error: None,
            timestamp: crate::models::project::python_isoformat_local(),
        });
    }

    // -----------------------------------------------------------------------
    // Test: command_id round-trip — response carries the same id as the command
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn command_id_round_trips_in_response() {
        let (client, server) = channel(64);
        let handle = spawn_mock_server(server, None);

        let resp = client
            .send_close_env(Duration::from_secs(5))
            .await
            .expect("send_close_env must succeed");

        handle.abort();

        // The response command_id must be a non-empty UUID string (same one
        // the client generated; we can't know it ahead of time, but we can
        // verify it looks like a UUID and is non-empty).
        assert!(!resp.command_id.is_empty());
        // Must be parseable as a UUID
        assert!(
            uuid::Uuid::parse_str(&resp.command_id).is_ok(),
            "command_id must be a valid UUID, got: {}",
            resp.command_id
        );
    }

    // -----------------------------------------------------------------------
    // Test: client is Clone — two clones can both send successfully
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn client_clone_can_send_concurrently() {
        let (client, server) = channel(64);
        let handle = spawn_mock_server(server, None);

        let client2 = client.clone();
        let (r1, r2) = tokio::join!(
            client.send_close_env(Duration::from_secs(5)),
            client2.send_close_env(Duration::from_secs(5)),
        );

        handle.abort();

        assert!(r1.is_ok(), "first clone send must succeed");
        assert!(r2.is_ok(), "second clone send must succeed");
    }

    // -----------------------------------------------------------------------
    // Test: send_success populates result, send_error populates error
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn send_success_and_send_error_response_fields() {
        // send_success
        {
            let (client, mut server) = channel(64);
            let handle = task::spawn(async move {
                server.start();
                loop {
                    if let Some(env) = server.poll_commands() {
                        let mut result = serde_json::Map::new();
                        result.insert("score".to_string(), json!(99));
                        SimulationIPCServer::send_success(env, result);
                    } else {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                }
            });

            let resp = client
                .send_interview(1, "q", None, Duration::from_secs(5))
                .await
                .expect("must succeed");
            handle.abort();

            assert_eq!(resp.status, CommandStatus::Completed);
            assert_eq!(resp.result.as_ref().and_then(|m| m.get("score")), Some(&json!(99)));
            assert!(resp.error.is_none());
        }

        // send_error
        {
            let (client, server) = channel(64);
            let handle = spawn_mock_server(server, Some("agent error".to_string()));

            let resp = client
                .send_interview(2, "q2", None, Duration::from_secs(5))
                .await
                .expect("channel reply must arrive even on error");
            handle.abort();

            assert_eq!(resp.status, CommandStatus::Failed);
            assert_eq!(resp.error.as_deref(), Some("agent error"));
            assert!(resp.result.is_none());
        }
    }

    // -----------------------------------------------------------------------
    // Test: poll_commands returns None on empty queue
    // -----------------------------------------------------------------------

    #[test]
    fn poll_commands_returns_none_when_queue_empty() {
        // Use a tokio runtime just to construct the channel; poll is sync.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (_client, mut server) = channel(64);
            assert!(
                server.poll_commands().is_none(),
                "poll_commands on empty queue must return None"
            );
        });
    }
}
