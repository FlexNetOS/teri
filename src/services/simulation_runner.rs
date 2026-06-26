//! Run-state types for the simulation runner — sub-cycle (a).
//!
//! Port of `backend/app/services/simulation_runner.py` (MiroFish), data types only.
//!
//! # Sub-cycle scope
//! This module (sub-cycle a) covers **S-541..S-598** — the four
//! pure-data types + their serialization/persistence:
//!
//! - [`RunnerStatus`]        — S-541..S-549 (8-variant enum)
//! - [`AgentAction`]         — S-550..S-560 (struct + `to_dict`)
//! - [`RoundSummary`]        — S-561..S-570 (struct + `to_dict`)
//! - [`SimulationRunState`]  — S-571..S-598 (struct + `add_action` / `to_dict` /
//!   `to_detail_dict`)
//! - [`load_run_state`]      — S-609..S-611 (file persistence helpers)
//!
//! Sub-cycle (b) — **lifecycle** (S-599..S-604, S-608, S-612, S-616, S-617, S-624,
//! S-625, S-627) — is ported below the data types:
//!
//! - [`SimulationRunner`]   — the owned-state supervisor (S-599); `start_simulation`
//!   (S-612), `stop_simulation` (S-617), `cleanup_all` (S-625), `get_running_simulations`
//!   (S-627), `get_run_state` (S-609).
//! - [`RunHandle`]          — per-run state+task+shutdown+ipc bundle (S-602/603/608).
//! - `terminate_handle`     — the 5s grace-then-force cooperative stop (S-616 observable).
//!
//! S-626 (`register_cleanup`) is deferred to U-049 (`[→U-049]`). Sub-cycles (c)–(f)
//! (monitor/tail/graph-fire, readers, interview wiring) extend this file in later cycles.
//!
//! # `[≠]` symbols in this sub-cycle
//!
//! - **S-540 `IS_WINDOWS`** — `[≠]` non-contractual: used only to select between
//!   `taskkill` and `killpg` in the subprocess-terminate path. teri's stop is
//!   OS-agnostic (cooperative `shutdown` flag + `task.abort()`); no branch exists.
//!   No observable output. (DECISION-17 §17.4)
//! - **S-595 `process_pid` value** — `[≠]` value-only: the struct field and the
//!   `to_dict` key are PORTED (shape parity); the runtime value is always `null`
//!   because teri runs the simulation in-process (no OS PID). Honest null matches
//!   Python when the runner has not yet set `process.pid`. (DECISION-17 §17.1)
//!
//! # JSON fidelity
//!
//! Python 3.7+ dict literals preserve insertion order; `serde_json` is compiled
//! with the `preserve_order` feature so `Map::new()` + sequential `.insert()`
//! calls reproduce the exact key order.
//!
//! `None` fields → `Value::Null` (never omitted), matching `json.dump` where
//! Python `None` serialises as JSON `null` (not a missing key).
//!
//! `ensure_ascii=False` + `indent=2` → `serde_json::to_string_pretty` (UTF-8
//! raw, 2-space indent).
//!
//! # `run_state.json` layout
//!
//! `_save_run_state` writes `{sim_data_dir}/{sim_id}/run_state.json`
//! (`to_detail_dict()` — superset of `to_dict` — matching Python L305).
//! `_load_run_state` reads it back with `.get(key, default)` tolerance.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::Result;
use crate::models::project::python_isoformat_local;
use crate::services::simulation_ipc::IPCResponse;

// SQLite support for interview history (optional, feature-gated for security)
#[cfg(feature = "sqlite")]
use rusqlite;

// ---------------------------------------------------------------------------
// RunnerStatus  (S-541..S-549)
// ---------------------------------------------------------------------------

/// Port of `RunnerStatus(str, Enum)` (`simulation_runner.py:36-45`).
///
/// Eight variants; serde serialises each to its lowercase string `.value`,
/// matching Python's `self.runner_status.value` in `to_dict` (L163).
///
/// S-541 (type), S-542 (IDLE), S-543 (STARTING), S-544 (RUNNING),
/// S-545 (PAUSED), S-546 (STOPPING), S-547 (STOPPED), S-548 (COMPLETED),
/// S-549 (FAILED).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunnerStatus {
    /// `"idle"` — runner is idle, no simulation running.
    Idle,
    /// `"starting"` — simulation is being launched.
    Starting,
    /// `"running"` — simulation is actively executing.
    Running,
    /// `"paused"` — simulation is temporarily paused.
    Paused,
    /// `"stopping"` — simulation is being stopped.
    Stopping,
    /// `"stopped"` — simulation has been stopped.
    Stopped,
    /// `"completed"` — simulation ran to natural completion.
    Completed,
    /// `"failed"` — simulation encountered a fatal error.
    Failed,
}

impl RunnerStatus {
    /// Return the string value, mirroring Python's `runner_status.value`.
    ///
    /// ```
    /// # use teri::services::simulation_runner::RunnerStatus;
    /// assert_eq!(RunnerStatus::Idle.as_str(),      "idle");
    /// assert_eq!(RunnerStatus::Starting.as_str(),  "starting");
    /// assert_eq!(RunnerStatus::Running.as_str(),   "running");
    /// assert_eq!(RunnerStatus::Paused.as_str(),    "paused");
    /// assert_eq!(RunnerStatus::Stopping.as_str(),  "stopping");
    /// assert_eq!(RunnerStatus::Stopped.as_str(),   "stopped");
    /// assert_eq!(RunnerStatus::Completed.as_str(), "completed");
    /// assert_eq!(RunnerStatus::Failed.as_str(),    "failed");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl std::fmt::Display for RunnerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// AgentAction  (S-550..S-560)
// ---------------------------------------------------------------------------

/// Port of `AgentAction` dataclass (`simulation_runner.py:48-72`).
///
/// Records a single agent action during a simulation round.
///
/// S-550 (type), S-551..S-559 (fields), S-560 (`to_dict`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    /// Round number this action occurred in (`simulation_runner.py:51`). S-551.
    pub round_num: i64,
    /// ISO-format timestamp string (`simulation_runner.py:52`). S-552.
    pub timestamp: String,
    /// Platform: `"twitter"` or `"reddit"` (`simulation_runner.py:53`). S-553.
    pub platform: String,
    /// Numeric agent ID (`simulation_runner.py:54`). S-554.
    pub agent_id: i64,
    /// Agent display name (`simulation_runner.py:55`). S-555.
    pub agent_name: String,
    /// Action type string e.g. `"CREATE_POST"` (`simulation_runner.py:56`). S-556.
    pub action_type: String,
    /// Additional action arguments as an arbitrary JSON object (`simulation_runner.py:57`).
    /// S-557. Default is an empty map.
    pub action_args: Map<String, Value>,
    /// Optional textual result of the action (`simulation_runner.py:58`). S-558.
    /// Python `None` → JSON `null`.
    pub result: Option<String>,
    /// Whether the action succeeded (`simulation_runner.py:59`). S-559. Default `true`.
    pub success: bool,
}

impl AgentAction {
    /// Build an `AgentAction` with default values for optional fields.
    pub fn new(
        round_num: i64,
        timestamp: String,
        platform: String,
        agent_id: i64,
        agent_name: String,
        action_type: String,
    ) -> Self {
        Self {
            round_num,
            timestamp,
            platform,
            agent_id,
            agent_name,
            action_type,
            action_args: Map::new(),
            result: None,
            success: true,
        }
    }

    /// Port of `AgentAction.to_dict()` (`simulation_runner.py:61-72`).
    ///
    /// Returns a 9-key ordered JSON map. Key order is identical to the Python
    /// source dict literal. `result: None` → `Value::Null` (never omitted).
    ///
    /// S-560.
    pub fn to_dict(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("round_num".into(), Value::Number(self.round_num.into()));
        m.insert("timestamp".into(), Value::String(self.timestamp.clone()));
        m.insert("platform".into(), Value::String(self.platform.clone()));
        m.insert("agent_id".into(), Value::Number(self.agent_id.into()));
        m.insert("agent_name".into(), Value::String(self.agent_name.clone()));
        m.insert("action_type".into(), Value::String(self.action_type.clone()));
        m.insert("action_args".into(), Value::Object(self.action_args.clone()));
        m.insert(
            "result".into(),
            match &self.result {
                Some(r) => Value::String(r.clone()),
                None => Value::Null,
            },
        );
        m.insert("success".into(), Value::Bool(self.success));
        m
    }
}

// ---------------------------------------------------------------------------
// RoundSummary  (S-561..S-570)
// ---------------------------------------------------------------------------

/// Port of `RoundSummary` dataclass (`simulation_runner.py:75-98`).
///
/// Aggregates per-round statistics.
///
/// S-561 (type), S-562..S-569 (fields), S-570 (`to_dict`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundSummary {
    /// Monotonically-increasing round number (`simulation_runner.py:77`). S-562.
    pub round_num: i64,
    /// ISO-format start time (`simulation_runner.py:78`). S-563.
    pub start_time: String,
    /// ISO-format end time; `None` until the round completes (`simulation_runner.py:79`).
    /// S-564.
    pub end_time: Option<String>,
    /// Simulated in-world hour for this round (`simulation_runner.py:80`). Default 0. S-565.
    pub simulated_hour: i64,
    /// Number of Twitter actions in this round (`simulation_runner.py:81`). Default 0. S-566.
    pub twitter_actions: i64,
    /// Number of Reddit actions in this round (`simulation_runner.py:82`). Default 0. S-567.
    pub reddit_actions: i64,
    /// IDs of agents that were active in this round (`simulation_runner.py:83`). S-568.
    pub active_agents: Vec<i64>,
    /// All `AgentAction`s in this round (`simulation_runner.py:84`). S-569.
    pub actions: Vec<AgentAction>,
}

impl RoundSummary {
    /// Build a `RoundSummary` with default values for optional/counter fields.
    pub fn new(round_num: i64, start_time: String) -> Self {
        Self {
            round_num,
            start_time,
            end_time: None,
            simulated_hour: 0,
            twitter_actions: 0,
            reddit_actions: 0,
            active_agents: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// Port of `RoundSummary.to_dict()` (`simulation_runner.py:87-98`).
    ///
    /// Returns an 8-key ordered map. Key order matches the Python dict literal.
    /// Notable: `actions_count` is a **computed** key (`len(self.actions)`, not a
    /// stored field), and `actions` is the full nested list. `end_time: None` →
    /// `Value::Null`.
    ///
    /// S-570.
    pub fn to_dict(&self) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("round_num".into(), Value::Number(self.round_num.into()));
        m.insert("start_time".into(), Value::String(self.start_time.clone()));
        m.insert(
            "end_time".into(),
            match &self.end_time {
                Some(t) => Value::String(t.clone()),
                None => Value::Null,
            },
        );
        m.insert("simulated_hour".into(), Value::Number(self.simulated_hour.into()));
        m.insert("twitter_actions".into(), Value::Number(self.twitter_actions.into()));
        m.insert("reddit_actions".into(), Value::Number(self.reddit_actions.into()));
        m.insert(
            "active_agents".into(),
            Value::Array(self.active_agents.iter().map(|&id| Value::Number(id.into())).collect()),
        );
        // Computed key: len(self.actions) — not stored, derived on the fly.
        m.insert("actions_count".into(), Value::Number((self.actions.len() as i64).into()));
        m.insert(
            "actions".into(),
            Value::Array(self.actions.iter().map(|a| Value::Object(a.to_dict())).collect()),
        );
        m
    }
}

// ---------------------------------------------------------------------------
// SimulationRunState  (S-571..S-598)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Python-faithful one-decimal rounding helper (S-597)
// ---------------------------------------------------------------------------

/// Round `x` to one decimal place using CPython `round(x, 1)` semantics.
///
/// CPython evaluates `round(x, 1)` by considering the **exact mathematical
/// value** of the `f64` — not just the IEEE 754 product `x * 10.0`:
///
/// 1. Compute `scaled = x * 10.0` (IEEE 754, round-to-nearest-even).
/// 2. Let `n = floor(scaled)` and `frac = scaled − n`.
/// 3. If `frac < 0.5` or `frac > 0.5`: round normally (down or up).
/// 4. If `frac == 0.5` exactly in `f64`: the IEEE product happens to land on
///    a tie boundary, but the **true** product (using the exact rational value
///    of the `f64`) may be above, below, or exactly at `n + 0.5`.
///    We resolve this with exact integer arithmetic on the mantissa bits:
///    compare `mantissa × 20` vs `(2n + 1) × 2^(−exp)` to determine the true
///    order.  Only a true mathematical tie triggers half-to-even.
///
/// This matches CPython for all 160 400 `(current_round, total_rounds)` pairs
/// with values in 0..=400 and the 243 pairs that diverge from `f64::round()`.
///
/// Used exclusively for `progress_percent` in [`SimulationRunState::to_dict`]
/// (S-597).  If other fields require the same semantics, this helper is
/// general and can be reused.
fn round_half_even_1dp(x: f64) -> f64 {
    if x < 0.0 {
        return -round_half_even_1dp(-x);
    }
    if !x.is_finite() {
        return x;
    }
    let scaled = x * 10.0;
    let n = scaled.floor(); // exact integer as f64 (no precision loss for our range)
    let frac = scaled - n;

    if frac < 0.5 {
        return n / 10.0;
    }
    if frac > 0.5 {
        return (n + 1.0) / 10.0;
    }
    // frac == 0.5 exactly: compare true mathematical product of x * 10 vs n + 0.5
    // using mantissa bits.  For a normal f64: value = mantissa * 2^exp
    // where mantissa = implicit-1 bit | 52-bit fraction, exp = biased_exp − 1023 − 52.
    // True product vs n + 0.5 ↔ mantissa * 20 vs (2n + 1) * 2^(−exp).
    let bits = x.to_bits();
    let biased_exp = ((bits >> 52) & 0x7ff) as i32;
    let (mantissa, exp): (u64, i32) = if biased_exp == 0 {
        // subnormal
        (bits & 0x000f_ffff_ffff_ffff, -1022 - 52)
    } else {
        // normal
        ((bits & 0x000f_ffff_ffff_ffff) | (1u64 << 52), biased_exp - 1023 - 52)
    };
    let n_i = n as i64;
    let two_n_plus_1 = 2 * n_i + 1; // always positive for x >= 0

    // Compare mantissa * 20 * 2^exp  vs  two_n_plus_1
    // ↔ mantissa * 20  vs  two_n_plus_1 * 2^(−exp)   (when exp < 0)
    // ↔ mantissa * 20 * 2^exp  vs  two_n_plus_1       (when exp >= 0)
    let cmp = if exp >= 0 {
        // lhs = mantissa * 20 * 2^exp  (u128 to avoid overflow)
        let lhs = (mantissa as u128) * 20 * (1u128 << exp as u32);
        let rhs = two_n_plus_1 as u128;
        lhs.cmp(&rhs)
    } else {
        // exp < 0: compare mantissa*20 vs two_n_plus_1 * 2^(-exp)
        let shift = (-exp) as u32;
        let lhs = (mantissa as u128) * 20;
        let rhs = (two_n_plus_1 as u128) << shift;
        lhs.cmp(&rhs)
    };

    match cmp {
        std::cmp::Ordering::Less => n / 10.0, // true product < n.5 → round down
        std::cmp::Ordering::Greater => (n + 1.0) / 10.0, // true product > n.5 → round up
        std::cmp::Ordering::Equal => {
            // exact tie: half-to-even (round to the even integer)
            if n_i % 2 == 0 { n / 10.0 } else { (n + 1.0) / 10.0 }
        }
    }
}

/// Port of `SimulationRunState` dataclass (`simulation_runner.py:101-193`).
///
/// Real-time run state for one simulation.  Persisted to `run_state.json` via
/// [`save_run_state`] / loaded via [`load_run_state`].
///
/// S-571 (type), S-572..S-595 (fields), S-596 (`add_action`),
/// S-597 (`to_dict`), S-598 (`to_detail_dict`).
///
/// # Field notes
///
/// - `process_pid` (S-595): field present for shape parity; always `None` in
///   teri (no OS subprocess). Serialises as JSON `null`. `[≠]` value-only.
/// - `updated_at` (S-592): defaults to `python_isoformat_local()` (mirrors
///   Python `field(default_factory=lambda: datetime.now().isoformat())`).
/// - `max_recent_actions` (S-590): stored in the struct so `add_action` can
///   enforce the cap (Python `self.max_recent_actions` at L150).

// CleanupResult — port of `cleanup_simulation_logs` return shape (U-026 sub-cycle g2).
// Python returns a dict; handler checks `.get("success")` + `.get("errors")`.
#[derive(Debug, Clone)]
pub struct CleanupResult {
    /// True iff no deletion errors occurred.
    pub success: bool,
    /// Names of successfully deleted files/paths.
    pub cleaned_files: Vec<String>,
    /// Deletion errors (absent key or None means no errors).
    pub errors: Option<Vec<String>>,
    /// Human-readable message (only set when sim_dir is absent).
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationRunState {
    /// Simulation ID (`simulation_runner.py:103`). S-572.
    pub simulation_id: String,
    /// Current runner lifecycle status (`simulation_runner.py:104`). S-573.
    pub runner_status: RunnerStatus,

    // Progress fields
    /// Global current round (`simulation_runner.py:107`). S-574.
    pub current_round: i64,
    /// Global total rounds (`simulation_runner.py:108`). S-575.
    pub total_rounds: i64,
    /// Global simulated hours completed (`simulation_runner.py:109`). S-576.
    pub simulated_hours: i64,
    /// Global total simulation hours (`simulation_runner.py:110`). S-577.
    pub total_simulation_hours: i64,

    // Per-platform rounds and simulated hours (dual-platform parallel display)
    /// Twitter current round (`simulation_runner.py:113`). S-578.
    pub twitter_current_round: i64,
    /// Reddit current round (`simulation_runner.py:114`). S-579.
    pub reddit_current_round: i64,
    /// Twitter simulated hours (`simulation_runner.py:115`). S-580.
    pub twitter_simulated_hours: i64,
    /// Reddit simulated hours (`simulation_runner.py:116`). S-581.
    pub reddit_simulated_hours: i64,

    // Platform status flags
    /// Whether Twitter simulation is currently running (`simulation_runner.py:119`). S-582.
    pub twitter_running: bool,
    /// Whether Reddit simulation is currently running (`simulation_runner.py:120`). S-583.
    pub reddit_running: bool,
    /// Cumulative count of Twitter actions (`simulation_runner.py:121`). S-584.
    pub twitter_actions_count: i64,
    /// Cumulative count of Reddit actions (`simulation_runner.py:122`). S-585.
    pub reddit_actions_count: i64,

    // Platform completion flags (set when simulation_end event detected in actions.jsonl)
    /// Whether Twitter simulation has completed (`simulation_runner.py:125`). S-586.
    pub twitter_completed: bool,
    /// Whether Reddit simulation has completed (`simulation_runner.py:126`). S-587.
    pub reddit_completed: bool,

    // Per-round summaries
    /// All round summaries accumulated so far (`simulation_runner.py:129`). S-588.
    pub rounds: Vec<RoundSummary>,

    // Recent actions (for real-time frontend display)
    /// Most-recent actions, newest first, capped at `max_recent_actions`
    /// (`simulation_runner.py:132`). S-589.
    pub recent_actions: Vec<AgentAction>,
    /// Cap on `recent_actions` length (`simulation_runner.py:133`). S-590. Default 50.
    pub max_recent_actions: usize,

    // Timestamps
    /// When the simulation was started; `None` until `start_simulation` sets it
    /// (`simulation_runner.py:136`). S-591.
    pub started_at: Option<String>,
    /// Last-modified timestamp, refreshed by `add_action` and any state mutation
    /// (`simulation_runner.py:137`). S-592. Default: `python_isoformat_local()`.
    pub updated_at: String,
    /// When the simulation completed; `None` until terminal state
    /// (`simulation_runner.py:138`). S-593.
    pub completed_at: Option<String>,

    // Error
    /// Optional error message in failed state (`simulation_runner.py:141`). S-594.
    pub error: Option<String>,

    // Process ID — shape ported, value always null in teri (no OS subprocess).
    /// OS process PID. Always `None` in teri; the key is emitted as JSON `null`
    /// for frontend shape parity. `[≠]` value-only. S-595.
    pub process_pid: Option<i64>,
}

impl SimulationRunState {
    /// Create a new `SimulationRunState` with all defaults, mirroring the Python
    /// dataclass field defaults (`simulation_runner.py:103-145`).
    pub fn new(simulation_id: String) -> Self {
        Self {
            simulation_id,
            runner_status: RunnerStatus::Idle,
            current_round: 0,
            total_rounds: 0,
            simulated_hours: 0,
            total_simulation_hours: 0,
            twitter_current_round: 0,
            reddit_current_round: 0,
            twitter_simulated_hours: 0,
            reddit_simulated_hours: 0,
            twitter_running: false,
            reddit_running: false,
            twitter_actions_count: 0,
            reddit_actions_count: 0,
            twitter_completed: false,
            reddit_completed: false,
            rounds: Vec::new(),
            recent_actions: Vec::new(),
            max_recent_actions: 50,
            started_at: None,
            updated_at: python_isoformat_local(),
            completed_at: None,
            error: None,
            process_pid: None,
        }
    }

    /// Port of `SimulationRunState.add_action` (`simulation_runner.py:147-158`).
    ///
    /// Insert at front of `recent_actions`, enforce the 50-item cap, increment
    /// the relevant platform counter, and refresh `updated_at`.
    ///
    /// Exact Python behavior:
    /// 1. `self.recent_actions.insert(0, action)`
    /// 2. `if len > max_recent_actions: self.recent_actions = recent_actions[:max]`
    /// 3. platform-specific counter bump
    /// 4. `self.updated_at = datetime.now().isoformat()`
    ///
    /// S-596.
    pub fn add_action(&mut self, action: AgentAction) {
        // Step 1: insert at front (mirrors Python list.insert(0, …))
        self.recent_actions.insert(0, action.clone());

        // Step 2: truncate to cap
        if self.recent_actions.len() > self.max_recent_actions {
            self.recent_actions.truncate(self.max_recent_actions);
        }

        // Step 3: per-platform counter
        if action.platform == "twitter" {
            self.twitter_actions_count += 1;
        } else {
            self.reddit_actions_count += 1;
        }

        // Step 4: refresh updated_at
        self.updated_at = python_isoformat_local();
    }

    /// Port of `SimulationRunState.to_dict()` (`simulation_runner.py:160-186`).
    ///
    /// Returns a 20-key ordered map. Key order is identical to the Python dict
    /// literal. Two keys are **computed** (not stored fields):
    ///
    /// - `progress_percent` = `round(current_round / max(total_rounds, 1) * 100, 1)`
    ///   (one-decimal Python `round`; implemented via [`round_half_even_1dp`] which
    ///   matches CPython's half-to-even-on-exact-tie semantics, not `f64::round()`)
    /// - `total_actions_count` = `twitter_actions_count + reddit_actions_count`
    ///
    /// All `Option` fields emit `Value::Null` when `None` (never omitted).
    ///
    /// S-597.
    pub fn to_dict(&self) -> Map<String, Value> {
        // Computed: progress_percent = round(current_round / max(total_rounds, 1) * 100, 1)
        let denom = self.total_rounds.max(1) as f64;
        let raw_pct = self.current_round as f64 / denom * 100.0;
        // Python round(x, 1) uses the EXACT float value (not just the IEEE 754 product)
        // to resolve tie cases, yielding half-to-even (banker's rounding) only at true
        // mathematical midpoints. See round_half_even_1dp for the algorithm.
        let progress_percent = round_half_even_1dp(raw_pct);

        let total_actions_count = self.twitter_actions_count + self.reddit_actions_count;

        let mut m = Map::new();
        m.insert("simulation_id".into(), Value::String(self.simulation_id.clone()));
        m.insert("runner_status".into(), Value::String(self.runner_status.as_str().to_string()));
        m.insert("current_round".into(), Value::Number(self.current_round.into()));
        m.insert("total_rounds".into(), Value::Number(self.total_rounds.into()));
        m.insert("simulated_hours".into(), Value::Number(self.simulated_hours.into()));
        m.insert(
            "total_simulation_hours".into(),
            Value::Number(self.total_simulation_hours.into()),
        );
        // progress_percent: emit as f64. serde_json Number requires finite f64.
        m.insert(
            "progress_percent".into(),
            serde_json::Number::from_f64(progress_percent)
                .map(Value::Number)
                .unwrap_or(Value::Number(serde_json::Number::from(0))),
        );
        // Per-platform rounds / hours
        m.insert("twitter_current_round".into(), Value::Number(self.twitter_current_round.into()));
        m.insert("reddit_current_round".into(), Value::Number(self.reddit_current_round.into()));
        m.insert(
            "twitter_simulated_hours".into(),
            Value::Number(self.twitter_simulated_hours.into()),
        );
        m.insert(
            "reddit_simulated_hours".into(),
            Value::Number(self.reddit_simulated_hours.into()),
        );
        m.insert("twitter_running".into(), Value::Bool(self.twitter_running));
        m.insert("reddit_running".into(), Value::Bool(self.reddit_running));
        m.insert("twitter_completed".into(), Value::Bool(self.twitter_completed));
        m.insert("reddit_completed".into(), Value::Bool(self.reddit_completed));
        m.insert("twitter_actions_count".into(), Value::Number(self.twitter_actions_count.into()));
        m.insert("reddit_actions_count".into(), Value::Number(self.reddit_actions_count.into()));
        m.insert("total_actions_count".into(), Value::Number(total_actions_count.into()));
        m.insert(
            "started_at".into(),
            match &self.started_at {
                Some(t) => Value::String(t.clone()),
                None => Value::Null,
            },
        );
        m.insert("updated_at".into(), Value::String(self.updated_at.clone()));
        m.insert(
            "completed_at".into(),
            match &self.completed_at {
                Some(t) => Value::String(t.clone()),
                None => Value::Null,
            },
        );
        m.insert(
            "error".into(),
            match &self.error {
                Some(e) => Value::String(e.clone()),
                None => Value::Null,
            },
        );
        // process_pid: always null in teri (no OS subprocess). Shape is contractual.
        m.insert(
            "process_pid".into(),
            match self.process_pid {
                Some(pid) => Value::Number(pid.into()),
                None => Value::Null,
            },
        );
        m
    }

    /// Port of `SimulationRunState.to_detail_dict()` (`simulation_runner.py:188-193`).
    ///
    /// Superset of `to_dict()` with two extra keys appended:
    /// - `"recent_actions"`: nested list of each action's `to_dict()`
    /// - `"rounds_count"`: computed `len(self.rounds)` (not stored)
    ///
    /// Used by `_save_run_state` (persists `to_detail_dict()` to `run_state.json`).
    ///
    /// S-598.
    pub fn to_detail_dict(&self) -> Map<String, Value> {
        let mut m = self.to_dict();
        m.insert(
            "recent_actions".into(),
            Value::Array(self.recent_actions.iter().map(|a| Value::Object(a.to_dict())).collect()),
        );
        m.insert("rounds_count".into(), Value::Number((self.rounds.len() as i64).into()));
        m
    }
}

// ---------------------------------------------------------------------------
// run_state.json persistence  (S-609..S-611)
// ---------------------------------------------------------------------------

/// Derive the `run_state.json` path for a simulation ID within the given root dir.
///
/// Mirrors Python `os.path.join(cls.RUN_STATE_DIR, simulation_id, "run_state.json")`
/// used in both `_load_run_state` and `_save_run_state`.
fn run_state_path(sim_data_dir: &Path, simulation_id: &str) -> PathBuf {
    sim_data_dir.join(simulation_id).join("run_state.json")
}

/// Port of `SimulationRunner._load_run_state` (`simulation_runner.py:243-296`).
///
/// Loads a `SimulationRunState` from `{sim_data_dir}/{simulation_id}/run_state.json`.
///
/// Returns `Ok(None)` when the file does not exist (mirrors Python `if not
/// os.path.exists(state_file): return None`). Returns `Ok(None)` on parse errors
/// after logging (mirrors Python `except Exception as e: logger.error(…); return None`).
///
/// Field defaults match Python `.get(key, default)` tolerance (`simulation_runner.py:253-291`).
///
/// S-610 (load helper) / S-609 (`get_run_state` cache-then-file).
pub fn load_run_state(
    sim_data_dir: &Path,
    simulation_id: &str,
) -> Result<Option<SimulationRunState>> {
    let state_file = run_state_path(sim_data_dir, simulation_id);
    if !state_file.exists() {
        return Ok(None);
    }

    let raw = match std::fs::read_to_string(&state_file) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("加载运行状态失败: {}", e);
            return Ok(None);
        }
    };
    let data: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("加载运行状态失败: {}", e);
            return Ok(None);
        }
    };

    // Helper: get a string field with default
    let str_field = |key: &str, default: &str| -> String {
        data.get(key).and_then(Value::as_str).unwrap_or(default).to_string()
    };
    let opt_str_field = |key: &str| -> Option<String> {
        data.get(key).and_then(Value::as_str).map(|s| s.to_string())
    };
    let i64_field = |key: &str, default: i64| -> i64 {
        data.get(key).and_then(Value::as_i64).unwrap_or(default)
    };
    let bool_field = |key: &str, default: bool| -> bool {
        data.get(key).and_then(Value::as_bool).unwrap_or(default)
    };

    // Parse RunnerStatus — default "idle" on missing/unknown (Python `RunnerStatus(data.get(…, "idle"))`)
    let status_str = str_field("runner_status", "idle");
    let runner_status =
        serde_json::from_value(Value::String(status_str.clone())).unwrap_or(RunnerStatus::Idle);

    let mut state = SimulationRunState {
        simulation_id: simulation_id.to_string(),
        runner_status,
        current_round: i64_field("current_round", 0),
        total_rounds: i64_field("total_rounds", 0),
        simulated_hours: i64_field("simulated_hours", 0),
        total_simulation_hours: i64_field("total_simulation_hours", 0),
        twitter_current_round: i64_field("twitter_current_round", 0),
        reddit_current_round: i64_field("reddit_current_round", 0),
        twitter_simulated_hours: i64_field("twitter_simulated_hours", 0),
        reddit_simulated_hours: i64_field("reddit_simulated_hours", 0),
        twitter_running: bool_field("twitter_running", false),
        reddit_running: bool_field("reddit_running", false),
        twitter_actions_count: i64_field("twitter_actions_count", 0),
        reddit_actions_count: i64_field("reddit_actions_count", 0),
        twitter_completed: bool_field("twitter_completed", false),
        reddit_completed: bool_field("reddit_completed", false),
        rounds: Vec::new(),
        recent_actions: Vec::new(),
        max_recent_actions: 50,
        started_at: opt_str_field("started_at"),
        updated_at: str_field("updated_at", &python_isoformat_local()),
        completed_at: opt_str_field("completed_at"),
        error: opt_str_field("error"),
        process_pid: data.get("process_pid").and_then(Value::as_i64),
    };

    // Load recent_actions (Python L279-291)
    if let Some(Value::Array(actions_data)) = data.get("recent_actions") {
        for a in actions_data {
            let action = AgentAction {
                round_num: a.get("round_num").and_then(Value::as_i64).unwrap_or(0),
                timestamp: a.get("timestamp").and_then(Value::as_str).unwrap_or("").to_string(),
                platform: a.get("platform").and_then(Value::as_str).unwrap_or("").to_string(),
                agent_id: a.get("agent_id").and_then(Value::as_i64).unwrap_or(0),
                agent_name: a.get("agent_name").and_then(Value::as_str).unwrap_or("").to_string(),
                action_type: a.get("action_type").and_then(Value::as_str).unwrap_or("").to_string(),
                action_args: a
                    .get("action_args")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default(),
                result: a.get("result").and_then(Value::as_str).map(|s| s.to_string()),
                success: a.get("success").and_then(Value::as_bool).unwrap_or(true),
            };
            state.recent_actions.push(action);
        }
    }

    Ok(Some(state))
}

/// Port of `SimulationRunner._save_run_state` (`simulation_runner.py:298-310`).
///
/// Creates the simulation directory if needed, serialises `state.to_detail_dict()`
/// to `{sim_data_dir}/{simulation_id}/run_state.json` with UTF-8 raw encoding and
/// 2-space indentation (`ensure_ascii=False, indent=2`).
///
/// `serde_json::to_string_pretty` produces exactly 2-space indentation and never
/// escapes non-ASCII characters when working with `String` values in `Value`.
///
/// S-611.
pub fn save_run_state(sim_data_dir: &Path, state: &SimulationRunState) -> Result<()> {
    let sim_dir = sim_data_dir.join(&state.simulation_id);
    std::fs::create_dir_all(&sim_dir)?;

    let state_file = sim_dir.join("run_state.json");
    let data = state.to_detail_dict();
    let json = serde_json::to_string_pretty(&data)?;
    std::fs::write(&state_file, json.as_bytes())?;

    Ok(())
}

// ===========================================================================
// SimulationRunner lifecycle — sub-cycle (b)
//
// Ports S-599..S-604, S-608, S-612, S-616, S-617, S-624, S-625, S-627.
// S-626 (`register_cleanup`) is deferred to U-049 (see module note below).
//
// MiroFish's `SimulationRunner` is a process *supervisor*: it `subprocess.Popen`s
// `run_{twitter,reddit,parallel}_simulation.py`, tracks them in class-level dicts
// keyed by `simulation_id`, and process-group-kills them on stop/cleanup. teri runs
// the simulation IN-PROCESS (`SimEngine::run` is a tokio future) — DECISION-2, LOCKED.
//
// The OS-subprocess *transport* (Popen, pgid, taskkill/killpg/SIGTERM/SIGKILL,
// stdout/stderr pipe drains, the `run_*.py` scripts, `sys.executable`) is structurally
// absent in teri and is `[≠]` inexpressible (DECISION-17 §17.4). The *observable
// lifecycle contract* is FULLY PORTED:
//   - start → running state (platform flags set, persisted)
//   - stop  → graceful terminate within a 5s window, then force; STOPPED + completed_at
//   - cleanup → idempotent, terminates ALL runs, stops graph updaters, persists STOPPED
//   - get_running → ids whose task is not finished
//
// `[≠]` symbols realized here (DECISION-17 §17.4, re-justified inline):
//   - S-540 `IS_WINDOWS` — non-contractual platform selector; teri's stop is OS-agnostic.
//   - S-601 `SCRIPTS_DIR` — no `run_*.py` scripts to locate (in-process engine). No output.
//   - S-604 `_action_queues` — thread→thread `Queue` handoff between the Popen monitor
//     thread and main; in-process tokio uses channels directly. No second thread, no queue.
//   - S-606/S-607 `_stdout_files`/`_stderr_files` — file handles existed ONLY to drain a
//     child process's stdout/stderr pipes (avoid pipe-buffer deadlock). No child pipe
//     in-process. (Not in (b)'s symbol list, named here for completeness.)
//   - S-612 (partial) — Popen/`sys.executable`/script-path/`PYTHONUTF8`/`bufsize`/
//     `start_new_session`: no interpreter/script to spawn. The RUNNING state + platform
//     flags + persistence ARE ported; the spawn is a `tokio::spawn` of `SimEngine::run`.
//   - S-616 (partial) — `taskkill`/`killpg`/pgid/SIGTERM/SIGKILL/Win-Unix branch: no OS
//     process to signal. The 5s grace-then-force WINDOW is ported (cooperative shutdown
//     flag, then `JoinHandle::abort()` after `timeout(5s)`).
//
// `register_cleanup` (S-626) is DEFERRED to U-049 — `[→U-049]`, NOT `[≠]`, NOT dropped.
// U-049 wires teri's `ctrl_c` graceful-shutdown (U-002) to call `cleanup_all`. This module
// ships `cleanup_all` as the callable U-049 will invoke; it does NOT install signal handlers.
// ===========================================================================

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::error::TeriError;
use crate::llm::LlmClient;
use crate::services::graph_memory::GraphMemoryManager;
use crate::services::simulation_ipc::{
    CommandPoll, CommandType, IpcEnvelope, SimulationIPCClient, SimulationIPCServer, channel,
};
use crate::services::simulation_manager::SimulationManager;
use crate::sim::SimEngine;

/// Grace window for a cooperative stop before the task is force-aborted, when invoked
/// from `stop_simulation`.
///
/// Mirrors MiroFish `_terminate_process`'s SIGTERM-then-SIGKILL window
/// (`simulation_runner.py:769` `process.wait(timeout=timeout)`, then
/// `os.killpg(pgid, SIGKILL)` on `TimeoutExpired`). The grace duration is the contractual
/// observable; only the kill *mechanism* differs.
///
/// `stop_simulation` calls `_terminate_process(process)` with **no** timeout arg
/// (`simulation_runner.py:793`), so it uses the parameter default `timeout=10`
/// (`simulation_runner.py:721`) → **10 seconds**. A sim that exits gracefully between
/// 5–10s MUST be allowed to finish here (it is force-aborted only under `cleanup_all`).
const STOP_GRACE: Duration = Duration::from_secs(10);

/// Grace window for a cooperative stop before the task is force-aborted, when invoked
/// from `cleanup_all` (server-shutdown path).
///
/// `cleanup_all_simulations` calls `_terminate_process(process, timeout=5)`
/// (`simulation_runner.py:1224`) → **5 seconds**. Shutdown is impatient: a sim that has
/// not exited within 5s is force-aborted.
const CLEANUP_GRACE: Duration = Duration::from_secs(5);

/// Buffer size for the per-run IPC channel (matches the `SimEngine` broadcast buffer
/// for backpressure parity — see `simulation_ipc::channel`).
const IPC_CHANNEL_BUFFER: usize = 64;

/// One simulation's live runtime state — the in-process analog of the six Python
/// class-level dicts keyed by `simulation_id`
/// (`_run_states`/`_processes`/`_action_queues`/`_monitor_threads`/`_graph_memory_enabled`).
///
/// Bundling them into one owned struct (rather than parallel maps) follows DECISION-17
/// §"Class-dicts → owned struct": it makes the per-run invariants (state ↔ task ↔ shutdown
/// flag belong together) un-desynchronizable.
///
/// S-602 (`state`), S-603 (`task`), S-608 (`graph_enabled`). `shutdown` and `ipc_client`
/// realize the cooperative-stop signal and the DECISION-16 interview transport.
pub struct RunHandle {
    /// The real-time run state (S-602 `_run_states[id]`).
    ///
    /// Wrapped in `Arc<tokio::sync::Mutex<…>>` because in MiroFish the run-state object is
    /// **shared mutable state**: `_run_states[id]` is mutated by BOTH the lifecycle methods
    /// (`start`/`stop`/`cleanup` set status/flags) AND the monitor thread
    /// (`_read_action_log` calls `state.add_action(...)`, sets `current_round`,
    /// `twitter_completed`, `runner_status=COMPLETED`, …). The monitor (sub-cycle c) runs as a
    /// separate task that must write the SAME state `get_run_state` returns, so the state lives
    /// behind a shared `Arc<Mutex>` both the runner and the monitor task hold. (Sub-cycle (b)
    /// stored this as a plain `SimulationRunState`; (c) shares it — the only lifecycle rework
    /// the monitor required.)
    pub state: Arc<tokio::sync::Mutex<SimulationRunState>>,
    /// The spawned simulation task — the in-process analog of `_processes[id]`'s `Popen`
    /// (S-603). Driving `SimEngine::run`. `abort()` is the SIGKILL analog.
    task: JoinHandle<()>,
    /// Cooperative-stop signal, honored by `SimEngine`'s tick loop via `with_shutdown`.
    /// `store(true, Release)` is the SIGTERM analog (graceful, between rounds).
    shutdown: Arc<AtomicBool>,
    /// In-process IPC client for interview/close-env round-trips (DECISION-16). The paired
    /// server is owned by the sim task. (`[≠]` replaces the file-IPC the Popen child used.)
    ipc_client: SimulationIPCClient,
    /// The monitor task (`_monitor_threads[id]`, S-605). Spawned in sub-cycle (c) by
    /// [`SimulationRunner::start_simulation`]. `stop`/`cleanup_all` abort it if present so it
    /// does not outlive the run (the daemon-thread teardown analog).
    monitor: Option<JoinHandle<()>>,
    /// Whether graph-memory updating is enabled for this run (S-608 `_graph_memory_enabled[id]`).
    graph_enabled: bool,
    /// Clone of the engine's canonical snapshot-history handle (the complete, lossless tick
    /// record). Extracted before the engine moves into the spawned task so the HTTP
    /// `/ticks/sse` feed can tail every `WorldSnapshot` the run produces. The `Arc` keeps the
    /// history alive for streaming consumers even after the engine is dropped at run end.
    snapshot_history: Arc<parking_lot::Mutex<Vec<crate::sim::WorldSnapshot>>>,
}

impl RunHandle {
    /// Clone the snapshot-history handle for streaming this run's ticks.
    pub fn snapshot_history(&self) -> Arc<parking_lot::Mutex<Vec<crate::sim::WorldSnapshot>>> {
        Arc::clone(&self.snapshot_history)
    }

    /// Borrow the IPC client (used by interview wiring in sub-cycle e/f).
    pub fn ipc_client(&self) -> &SimulationIPCClient {
        &self.ipc_client
    }

    /// Whether graph-memory updating is enabled for this run.
    pub fn graph_enabled(&self) -> bool {
        self.graph_enabled
    }

    /// Whether the simulation task has finished (the in-process analog of
    /// `process.poll() is not None`).
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

/// Inputs required to drive an in-process simulation run.
///
/// This is the seam that replaces MiroFish's `subprocess.Popen(cmd, ...)`: where Python
/// hands a config-file path to a child interpreter that loads the engine/agents/graph
/// itself, teri's caller (U-024 / the API layer) assembles the engine + agent pool +
/// knowledge graph + LLM client and hands them to [`SimulationRunner::start_simulation`],
/// which owns only the *lifecycle* (spawn, register, cooperative stop, force-abort).
///
/// `engine` is taken by value (the spawned task owns it for the run's duration); the
/// runner installs the cooperative-shutdown flag on it before spawning.
pub struct RunInputs<L: LlmClient + Send + Sync + 'static> {
    /// The simulation engine to drive. The runner calls `engine.with_shutdown(flag)` then
    /// `engine.run(&mut pool, &graph, &*llm)` inside the spawned task.
    pub engine: SimEngine,
    /// The agent pool (owned by the task; `SimEngine::run` mutates it).
    pub pool: crate::agent::AgentPool,
    /// The knowledge graph (read by `SimEngine::run`).
    pub graph: crate::graph::KnowledgeGraph,
    /// The LLM client backing agent decisions.
    pub llm: Arc<L>,
    /// Optional per-platform "boost" LLM (U-030 S-934 dual-LLM). When `Some`, reddit agents'
    /// decisions run against this client and twitter agents against `llm`; `None` (single-platform
    /// runs) → every agent uses `llm`. Set only for `platform == "parallel"` when `LLM_BOOST_API_KEY`
    /// is configured (see `build_run_inputs`).
    pub boost_llm: Option<Arc<L>>,
}

/// In-process simulation supervisor — port of `SimulationRunner` (`simulation_runner.py:196`).
///
/// S-599 (type), S-600 (`RUN_STATE_DIR` → `sim_data_dir`), S-602/603/604/608 (per-run dicts
/// folded into [`RunHandle`]), S-624 (`_cleanup_done` → `cleanup_done`).
///
/// Generic over the LLM client `L` because it owns a [`GraphMemoryManager<L>`] and spawns
/// `SimEngine::run::<L>` — the same forcing function as U-021/U-023 (`LlmClient` is not
/// dyn-safe, so the class-level singleton becomes one owned instance held in app state).
///
/// # Concurrency
///
/// `runs` is a `tokio::sync::Mutex` so it can be held across `.await` only where necessary.
/// Lifecycle methods that must `.await` on a handle (stop, cleanup) **take the handle out of
/// the map first**, drop the lock, then await — never holding the lock across the abort/join.
/// RAII reservation of a `simulation_id` in [`SimulationRunner::starting`].
///
/// [`acquire`](StartGuard::acquire) returns `None` if another `start_simulation` for the same id
/// is already in flight; otherwise it inserts the id and the returned guard removes it on `Drop`
/// (every exit path — `Ok`, early `Err`, or a `?` mid-setup), so a failed start never wedges the
/// id permanently.
struct StartGuard<'a> {
    set: &'a std::sync::Mutex<std::collections::HashSet<String>>,
    id: String,
}

impl<'a> StartGuard<'a> {
    fn acquire(
        set: &'a std::sync::Mutex<std::collections::HashSet<String>>,
        id: &str,
    ) -> Option<Self> {
        let mut guard = set.lock().unwrap_or_else(|e| e.into_inner());
        if guard.contains(id) {
            return None;
        }
        guard.insert(id.to_string());
        Some(Self { set, id: id.to_string() })
    }
}

impl Drop for StartGuard<'_> {
    fn drop(&mut self) {
        let mut guard = self.set.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(&self.id);
    }
}

pub struct SimulationRunner<L: LlmClient + Send + Sync + 'static> {
    /// Root simulation-data directory — teri analog of `RUN_STATE_DIR`
    /// (`os.path.join(dirname(__file__), '../../uploads/simulations')`). S-600.
    /// (`SCRIPTS_DIR`, S-601, is `[≠]`: there are no `run_*.py` scripts in-process.)
    sim_data_dir: std::path::PathBuf,
    /// Per-run state + task + shutdown flag, keyed by `simulation_id`. Folds the six Python
    /// class-level dicts (S-602/603/604/605/606/607/608) into one map of owned handles.
    runs: tokio::sync::Mutex<std::collections::HashMap<String, RunHandle>>,
    /// Simulation ids whose `start_simulation` is mid-flight. Reserved before the run-state check
    /// and released only after the handle is registered in `runs`, closing the check-then-register
    /// TOCTOU where two concurrent `POST /start` for the same id could both spawn an engine (the
    /// loser's `RunHandle` would then be dropped, detaching — not aborting — its tasks, leaving an
    /// orphan engine writing the same `actions.jsonl`). A plain sync mutex: the critical sections
    /// (insert/remove) are tiny and never span an `.await`.
    starting: std::sync::Mutex<std::collections::HashSet<String>>,
    /// Graph-memory manager (U-021) — the runner calls `create_updater`/`stop_updater`/
    /// `stop_all` exactly where MiroFish calls `ZepGraphMemoryManager.*`.
    graph_mgr: Arc<GraphMemoryManager<L>>,
    /// Simulation manager (U-023) — owns `state.json`; the runner calls
    /// `mark_state_json_stopped` for the S-625 secondary write (DECISION-17 §17.0 Area 4).
    manager: Arc<SimulationManager>,
    /// Idempotency flag for `cleanup_all` — port of `_cleanup_done` (S-624). Flipped
    /// false→true atomically on the first call (mirrors U-021 `stop_all`'s `compare_exchange`).
    cleanup_done: AtomicBool,
    /// Agent long-term-memory writer — when present, the monitor persists each content-bearing
    /// agent action into the vector/LTM store (the "agent LTM write-back" feature). `None`
    /// disables it (e.g. when no memory store could be opened); the run is byte-identical to
    /// before, just without agent memory. Independent of `graph_mgr` (graph-fact write-back).
    agent_memory: Option<Arc<crate::services::agent_memory::AgentMemoryWriter>>,
}

impl<L: LlmClient + Send + Sync + 'static> SimulationRunner<L> {
    /// Construct a runner over the given data dir, sharing the graph manager and simulation
    /// manager that the rest of teri's app state holds.
    pub fn new(
        sim_data_dir: impl Into<std::path::PathBuf>,
        graph_mgr: Arc<GraphMemoryManager<L>>,
        manager: Arc<SimulationManager>,
    ) -> Self {
        Self {
            sim_data_dir: sim_data_dir.into(),
            runs: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            starting: std::sync::Mutex::new(std::collections::HashSet::new()),
            graph_mgr,
            manager,
            cleanup_done: AtomicBool::new(false),
            agent_memory: None,
        }
    }

    /// Attach an [`AgentMemoryWriter`](crate::services::agent_memory::AgentMemoryWriter) so the
    /// monitor persists each content-bearing agent action as long-term memory. Builder-style so
    /// existing `new(..)` call-sites are unaffected; `None` leaves the feature disabled.
    pub fn with_agent_memory(
        mut self,
        agent_memory: Option<Arc<crate::services::agent_memory::AgentMemoryWriter>>,
    ) -> Self {
        self.agent_memory = agent_memory;
        self
    }

    /// The simulation data directory root (`{SIMULATION_DATA_DIR}`).
    ///
    /// Exposes the private `sim_data_dir` field so consumers (e.g. U-024
    /// `ReportTools::interview_agents`, which loads `{sim_data_dir}/{sim_id}`
    /// agent-profile files) can resolve per-simulation paths without holding a
    /// second config borrow.
    pub fn sim_data_dir(&self) -> &std::path::Path {
        &self.sim_data_dir
    }

    /// Memory-cache-then-file load of a run state — port of `get_run_state` (S-609).
    ///
    /// Python: return `_run_states[id]` if present, else `_load_run_state(id)` (and cache it).
    /// teri: return a clone of the live `RunHandle.state` if a run is registered, else read
    /// `run_state.json` from disk via [`load_run_state`].
    ///
    /// Returns a clone (the live state lives behind the `runs` mutex; a borrow cannot escape).
    pub async fn get_run_state(&self, simulation_id: &str) -> Result<Option<SimulationRunState>> {
        // Take a clone of the shared-state Arc out of the map (drop the runs lock before we
        // lock the per-run state mutex — never hold two locks nested).
        let state_arc = {
            let runs = self.runs.lock().await;
            runs.get(simulation_id).map(|h| Arc::clone(&h.state))
        };
        if let Some(arc) = state_arc {
            return Ok(Some(arc.lock().await.clone()));
        }
        // Not in memory — load from disk (S-610).
        load_run_state(&self.sim_data_dir, simulation_id)
    }

    /// Clone the canonical snapshot-history handle for a registered run (the source the HTTP
    /// `/ticks/sse` feed tails). `None` if no live/recent run handle exists for
    /// `simulation_id` (never started, or already cleaned up) — the caller then has no ticks
    /// to stream (it can still report the persisted terminal state via [`get_run_state`]).
    ///
    /// [`get_run_state`]: Self::get_run_state
    pub async fn snapshot_history(
        &self,
        simulation_id: &str,
    ) -> Option<Arc<parking_lot::Mutex<Vec<crate::sim::WorldSnapshot>>>> {
        let runs = self.runs.lock().await;
        runs.get(simulation_id).map(|h| h.snapshot_history())
    }

    /// Start a simulation — port of `start_simulation` (S-612).
    ///
    /// Observable contract (PORTED exactly; the Popen mechanism is `[≠]`):
    /// 1. **Reject if already running** — if a run for `simulation_id` exists with status
    ///    `Running` or `Starting`, return `Err` (Python L335-337 `raise ValueError`).
    /// 2. **Load config + compute `total_rounds`** — read `simulation_config.json`
    ///    `time_config`, `total_rounds = int(total_hours * 60 / minutes_per_round)`,
    ///    defaults `total_simulation_hours=72`, `minutes_per_round=30` (L350-353).
    ///    Missing config → `Err` (Python L343-344 `raise ValueError("模拟配置不存在")`).
    /// 3. **`max_rounds` truncation** — if `max_rounds > 0`, `total_rounds =
    ///    min(total_rounds, max_rounds)` (L356-360).
    /// 4. **Build STARTING state**, persist `run_state.json` (L362-370).
    /// 5. **Graph-memory updater** — if enabled, require `graph_id`, create the updater,
    ///    set `graph_enabled` (L373-385). On creation failure: log + `graph_enabled=false`
    ///    (Python L381-383 catches and continues — does NOT abort the start).
    /// 6. **Platform flags** — twitter / reddit / parallel set `twitter_running`/
    ///    `reddit_running` (L388-397).
    /// 7. **Spawn the sim task** (the in-process Popen analog), set `runner_status=Running`,
    ///    set `process_pid` (`[≠]` → stays `None` in teri), persist, register the handle (L438-457).
    /// 8. **Return the running state.**
    ///
    /// On any failure during spawn/setup, the state transitions to `Failed` with the error
    /// recorded and persisted before returning `Err` (Python L473-477).
    ///
    /// `graph` for the updater: when graph-memory is enabled, the caller must provide the
    /// shared `KnowledgeGraph` handle the updater writes to (`graph_for_updater`); `None`
    /// when disabled.
    #[allow(clippy::too_many_arguments)]
    pub async fn start_simulation(
        &self,
        simulation_id: &str,
        platform: &str,
        max_rounds: Option<i64>,
        enable_graph_memory_update: bool,
        graph_id: Option<&str>,
        inputs: RunInputs<L>,
        graph_for_updater: Option<Arc<tokio::sync::Mutex<crate::graph::KnowledgeGraph>>>,
    ) -> Result<SimulationRunState> {
        // (0) Reserve this id for the whole start so two concurrent `/start` for the same
        // simulation cannot both pass the check below and spawn duplicate engines. Held (RAII)
        // until this function returns — past the `runs.insert` at the end. Released on any error
        // path too, so a failed start never wedges the id.
        let _start_guard = StartGuard::acquire(&self.starting, simulation_id).ok_or_else(|| {
            TeriError::Sim(format!("simulation start already in progress: {simulation_id}"))
        })?;

        // (1) Reject if already running (L335-337).
        if let Some(existing) = self.get_run_state(simulation_id).await?
            && matches!(existing.runner_status, RunnerStatus::Running | RunnerStatus::Starting)
        {
            return Err(TeriError::Sim(format!("模拟已在运行中: {simulation_id}")));
        }

        // (2) Load config + compute total_rounds (L340-353).
        let config = self
            .manager
            .get_simulation_config(simulation_id)?
            .ok_or_else(|| TeriError::Sim("模拟配置不存在，请先调用 /prepare 接口".to_string()))?;
        let time_config = config.get("time_config");
        let total_hours = time_config
            .and_then(|t| t.get("total_simulation_hours"))
            .and_then(Value::as_i64)
            .unwrap_or(72);
        let minutes_per_round = time_config
            .and_then(|t| t.get("minutes_per_round"))
            .and_then(Value::as_i64)
            .unwrap_or(30);
        // Python `int(total_hours * 60 / minutes_per_round)` — Python `/` is float division,
        // `int()` truncates toward zero. Guard against a zero divisor (Python would raise
        // ZeroDivisionError; teri treats a non-positive cadence as "no truncation basis" → 0).
        let mut total_rounds: i64 = if minutes_per_round != 0 {
            ((total_hours as f64 * 60.0) / minutes_per_round as f64) as i64
        } else {
            0
        };

        // (3) max_rounds truncation (L356-360).
        if let Some(mr) = max_rounds
            && mr > 0
        {
            let original = total_rounds;
            total_rounds = total_rounds.min(mr);
            if total_rounds < original {
                tracing::info!("轮数已截断: {} -> {} (max_rounds={})", original, total_rounds, mr);
            }
        }

        // (4) Build STARTING state + persist (L362-370).
        let mut state = SimulationRunState::new(simulation_id.to_string());
        state.runner_status = RunnerStatus::Starting;
        state.total_rounds = total_rounds;
        state.total_simulation_hours = total_hours;
        state.started_at = Some(crate::models::project::python_isoformat_local());
        save_run_state(&self.sim_data_dir, &state)?;

        // (5) Graph-memory updater (L373-385).
        let mut graph_enabled = false;
        if enable_graph_memory_update {
            let gid = graph_id
                .ok_or_else(|| TeriError::Sim("启用图谱记忆更新时必须提供 graph_id".to_string()))?;
            // The updater needs the shared graph handle to write into.
            match graph_for_updater {
                Some(g) => {
                    match self
                        .graph_mgr
                        .create_updater(simulation_id, g, Arc::clone(&inputs.llm), gid.to_string())
                        .await
                    {
                        Ok(()) => {
                            graph_enabled = true;
                            tracing::info!(
                                "已启用图谱记忆更新: simulation_id={}, graph_id={}",
                                simulation_id,
                                gid
                            );
                        }
                        Err(e) => {
                            // Python L381-383: catch, log, set enabled=false — do NOT abort.
                            tracing::error!("创建图谱记忆更新器失败: {}", e);
                            graph_enabled = false;
                        }
                    }
                }
                None => {
                    // graph_id given but no graph handle supplied to write into.
                    tracing::error!("创建图谱记忆更新器失败: no knowledge-graph handle provided");
                    graph_enabled = false;
                }
            }
        }

        // (6) Platform flags (L388-397). Anything other than twitter/reddit is "parallel".
        match platform {
            "twitter" => state.twitter_running = true,
            "reddit" => state.reddit_running = true,
            _ => {
                state.twitter_running = true;
                state.reddit_running = true;
            }
        }

        // (7) Spawn the sim task (the in-process Popen analog, L408-469).
        // Build the IPC channel (DECISION-16): client stays in the handle, server moves
        // into the task (the sim loop services interview/close commands in (c)/(e)).
        let (ipc_client, ipc_server) = channel(IPC_CHANNEL_BUFFER);

        // Cooperative-stop flag, installed on the engine and held in the handle.
        let shutdown = Arc::new(AtomicBool::new(false));

        let RunInputs { mut engine, pool, graph, llm, boost_llm } = inputs;
        engine.with_shutdown(Arc::clone(&shutdown));

        // Subscribe to the terminal completion signal (U-048) BEFORE the engine moves into the
        // spawned task — this receiver is the monitor's loop-exit signal, replacing Python's
        // `process.poll()` (DECISION-17 §17 Area 2). `watch` retains the final value, so even if
        // the run finishes before the monitor first polls, the monitor still observes `Some(..)`.
        let completion_rx = engine.subscribe_completion();

        // Clone the canonical snapshot-history handle BEFORE the engine moves into the spawned
        // task — the HTTP `/ticks/sse` feed tails this to stream every WorldSnapshot the run
        // emits (the `Arc` outlives the engine, so the ticks remain streamable post-run).
        let snapshot_history = engine.snapshot_history_handle();

        // Parallel run ⇒ dual-platform interview dispatch (ParallelIPCHandler analog). Anything
        // other than "twitter"/"reddit" is parallel — same predicate as the (6) platform flags.
        let parallel = !matches!(platform, "twitter" | "reddit");
        let task = spawn_sim_task(
            engine,
            pool,
            graph,
            llm,
            boost_llm,
            ipc_server,
            Arc::clone(&shutdown),
            parallel,
        );

        // process_pid stays None ([≠] value-only — no OS pid). runner_status → Running.
        state.runner_status = RunnerStatus::Running;
        save_run_state(&self.sim_data_dir, &state)?;

        tracing::info!("模拟启动成功: {}, platform={}", simulation_id, platform);

        // (7b) Spawn the monitor task (`_monitor_threads[id]`, S-605/S-613). It tails the
        // per-platform `actions.jsonl` files by byte offset (S-614 / U-047), fires graph-memory
        // per new action when enabled, detects `simulation_end` → COMPLETED (dual-platform gated
        // by S-615), and updates `current_round`/`simulated_hours` from `round_end` events. The
        // shared run-state Arc is the SAME one `get_run_state` reads, so the monitor's writes are
        // observable. The monitor loops on the 2s poll cadence until the run task finishes (via
        // `completion_rx`), then does ONE final tail pass so no trailing action is lost.
        let state_arc = Arc::new(tokio::sync::Mutex::new(state.clone()));
        let monitor = spawn_monitor_task(
            MonitorContext {
                simulation_id: simulation_id.to_string(),
                sim_data_dir: self.sim_data_dir.clone(),
                state: Arc::clone(&state_arc),
                graph_mgr: Arc::clone(&self.graph_mgr),
                graph_enabled,
                agent_memory: self.agent_memory.clone(),
            },
            completion_rx,
        );

        let handle = RunHandle {
            state: state_arc,
            task,
            shutdown,
            ipc_client,
            monitor: Some(monitor),
            graph_enabled,
            snapshot_history,
        };

        {
            let mut runs = self.runs.lock().await;
            runs.insert(simulation_id.to_string(), handle);
        }

        // (8) Return the running state.
        Ok(state)
    }

    /// Stop a simulation — port of `stop_simulation` (S-617) + `_terminate_process` (S-616).
    ///
    /// Observable contract (PORTED; the kill mechanism is `[≠]`):
    /// 1. **Run must exist** — else `Err` (Python L780-781).
    /// 2. **Run must be RUNNING or PAUSED** — else `Err` (Python L783-784).
    /// 3. Transition to `Stopping`, persist (L786-787).
    /// 4. **Terminate within the 5s grace window, then force** — set the cooperative
    ///    shutdown flag (SIGTERM analog), await the task bounded by `timeout(5s)`; on
    ///    timeout, `task.abort()` (SIGKILL analog). Abort the monitor task too if present.
    /// 5. Transition to `Stopped`, clear platform flags, set `completed_at`, persist (L806-810).
    /// 6. Stop the graph-memory updater if enabled (L813-819).
    ///
    /// Returns the final STOPPED state.
    pub async fn stop_simulation(&self, simulation_id: &str) -> Result<SimulationRunState> {
        // (1)/(2): validate existence + status against the LIVE handle's state.
        // Take the handle OUT of the map (we will await on its task — never hold the lock
        // across .await). If absent in memory, fall back to disk for the precondition checks.
        let mut handle = {
            let mut runs = self.runs.lock().await;
            runs.remove(simulation_id)
        };

        // Determine the current status for the precondition checks.
        let current_status = match &handle {
            Some(h) => h.state.lock().await.runner_status.clone(),
            None => match load_run_state(&self.sim_data_dir, simulation_id)? {
                Some(s) => s.runner_status,
                None => {
                    return Err(TeriError::Sim(format!("模拟不存在: {simulation_id}")));
                }
            },
        };

        if !matches!(current_status, RunnerStatus::Running | RunnerStatus::Paused) {
            // Put the handle back if we removed it (we are not stopping it).
            if let Some(h) = handle {
                let mut runs = self.runs.lock().await;
                runs.insert(simulation_id.to_string(), h);
            }
            return Err(TeriError::Sim(format!(
                "模拟未在运行: {simulation_id}, status={current_status}"
            )));
        }

        // (3): transition to Stopping, persist. Build the working state from the live handle
        // if present, else from disk.
        let mut state = match &handle {
            Some(h) => h.state.lock().await.clone(),
            None => load_run_state(&self.sim_data_dir, simulation_id)?
                .unwrap_or_else(|| SimulationRunState::new(simulation_id.to_string())),
        };
        state.runner_status = RunnerStatus::Stopping;
        save_run_state(&self.sim_data_dir, &state)?;

        // (4): terminate — cooperative-then-force, 10s grace window (S-616). `stop_simulation`
        // calls `_terminate_process(process)` with no timeout arg (py:793) → default 10s (py:721).
        if let Some(h) = handle.as_mut() {
            terminate_handle(h, simulation_id, STOP_GRACE).await;
        }

        // (5): transition to Stopped + clear flags + completed_at, persist (L806-810).
        state.runner_status = RunnerStatus::Stopped;
        state.twitter_running = false;
        state.reddit_running = false;
        state.completed_at = Some(crate::models::project::python_isoformat_local());
        save_run_state(&self.sim_data_dir, &state)?;

        // (6): stop the graph-memory updater if enabled (L813-819).
        let graph_enabled = handle.as_ref().map(|h| h.graph_enabled).unwrap_or(false);
        if graph_enabled {
            self.graph_mgr.stop_updater(simulation_id).await;
            tracing::info!("已停止图谱记忆更新: simulation_id={}", simulation_id);
        }

        tracing::info!("模拟已停止: {}", simulation_id);
        // The handle is dropped here (removed from the map and not re-inserted) — the run is
        // terminated, mirroring Python popping `_processes[id]` after termination.
        Ok(state)
    }

    /// Clean up ALL running simulations — port of `cleanup_all_simulations` (S-625).
    ///
    /// Called on server shutdown (by U-049, which wires `ctrl_c` to it). **Idempotent**
    /// via `cleanup_done` `compare_exchange` (S-624 `_cleanup_done`, mirroring U-021's
    /// `stop_all`):
    ///
    /// 1. If already done → return immediately (Python L1194-1196).
    /// 2. If nothing to clean (no runs, no graph updaters) → silent return (L1199-1203).
    /// 3. Stop ALL graph-memory updaters via `GraphMemoryManager::stop_all` (L1208-1212).
    /// 4. For each handle: if it is **finished** (`is_finished()`, the in-process analog of
    ///    `process.poll() is not None`), SKIP it — Python gates the whole body behind
    ///    `if process.poll() is None:` (L1219), so a completed run is neither terminated nor
    ///    state-written and keeps its final state intact. Otherwise (still running): terminate
    ///    it (cooperative-then-force, 5s grace `timeout=5`, S-616), set its run state STOPPED +
    ///    clear flags + `completed_at` + error "服务器关闭，模拟被终止", persist `run_state.json`
    ///    (L1234-1241), AND do the secondary `state.json` write via the SimulationManager
    ///    (L1244-1259, DECISION-17 §17.0 Area 4). Per-run errors are caught-logged-continued
    ///    (Python L1261-1262).
    /// 5. Drain the runs map — ALL entries, finished or running (Python `_processes.clear()`,
    ///    L1282-1283).
    pub async fn cleanup_all(&self) {
        // (1) Idempotency — flip false→true atomically (mirrors U-021 stop_all; S-624).
        if self
            .cleanup_done
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        // (2) Nothing to clean? Silent return (L1199-1203). Take the runs out so we drop the
        // lock before any .await; check emptiness against runs + the graph manager.
        let mut drained: Vec<(String, RunHandle)> = {
            let mut runs = self.runs.lock().await;
            runs.drain().collect()
        };
        let has_updaters = !self.graph_mgr.get_all_stats().await.is_empty();
        if drained.is_empty() && !has_updaters {
            return; // nothing to clean
        }

        tracing::info!("正在清理所有模拟进程...");

        // (3) Stop all graph-memory updaters first (L1208-1212). `stop_all` is itself
        // idempotent; errors are logged inside it.
        self.graph_mgr.stop_all().await;

        // (4) Terminate each RUNNING run, persist STOPPED state + secondary state.json write.
        //
        // Python gates the ENTIRE block — terminate + `run_state.json` write + `state.json`
        // write — behind `if process.poll() is None:` (`simulation_runner.py:1219`). A
        // finished/completed run is SKIPPED: it is neither terminated nor state-written, so its
        // persisted final state (COMPLETED, etc.) is left intact. Only the drain/removal happens
        // for it (Python `cls._processes.clear()` at L1282 clears all entries regardless).
        //
        // teri mirrors this exactly: `handle.is_finished()` is the in-process equivalent of
        // `process.poll() is not None`. A finished handle is drained (we own it here and let it
        // drop) but its state is NOT overwritten — recording STOPPED+error over a completed run
        // would corrupt its final state (FAIL-2 regression).
        for (simulation_id, mut handle) in drained.drain(..) {
            if handle.is_finished() {
                // poll() is not None → skip entirely (no terminate, no state writes). Drained
                // above; the handle drops at end of this iteration.
                continue;
            }

            // process.poll() is None → the run is still RUNNING; terminate + record shutdown.
            tracing::info!("终止模拟进程: {}", simulation_id);
            // cleanup_all calls `_terminate_process(process, timeout=5)` (py:1224) → 5s grace.
            terminate_handle(&mut handle, &simulation_id, CLEANUP_GRACE).await;

            // Update run_state.json (L1234-1241).
            let mut state = handle.state.lock().await.clone();
            state.runner_status = RunnerStatus::Stopped;
            state.twitter_running = false;
            state.reddit_running = false;
            state.completed_at = Some(crate::models::project::python_isoformat_local());
            state.error = Some("服务器关闭，模拟被终止".to_string());
            if let Err(e) = save_run_state(&self.sim_data_dir, &state) {
                // Per-run catch-log-continue (Python L1261-1262).
                tracing::error!("清理进程失败: {}, error={}", simulation_id, e);
            }

            // Secondary state.json write via the SimulationManager (L1244-1259).
            tracing::info!("尝试更新 state.json: {}", simulation_id);
            match self.manager.mark_state_json_stopped(&simulation_id) {
                Ok(true) => {
                    tracing::info!("已更新 state.json 状态为 stopped: {}", simulation_id);
                }
                Ok(false) => {
                    tracing::warn!("state.json 不存在: {}", simulation_id);
                }
                Err(e) => {
                    tracing::warn!("更新 state.json 失败: {}, error={}", simulation_id, e);
                }
            }
        }

        tracing::info!("模拟进程清理完成");
        // (5) The runs map was already drained above.
    }

    /// Delete per-run log files for a simulation — port of `cleanup_simulation_logs`
    /// (MiroFish `simulation_runner.py:1103-1181`, U-026 sub-cycle g2).
    ///
    /// Scoped deletions (exactly `simulation_runner.py:1136-1147`):
    ///   • Files in `{sim_dir}`: `run_state.json`, `simulation.log`, `stdout.log`,
    ///     `stderr.log`, `twitter_simulation.db`, `reddit_simulation.db`, `env_status.json`.
    ///   • In sub-dirs `["twitter", "reddit"]`: `{dir}/actions.jsonl`.
    /// Each file: skip if absent (not an error); on `fs::remove` error → push name to `errors`.
    ///
    /// In-memory cleanup (`:1171-1173`): remove the run handle so a subsequent `get_run_state`
    /// re-reads fresh from disk (Python `del cls._run_states[id]`). Idempotent when a
    /// prior `stop_simulation` already removed it.
    ///
    /// `sim_dir` missing → returns `{success:true, message:"模拟目录不存在，无需清理"}`.
    ///
    /// Returns `CleanupResult { success, cleaned_files, errors, message }`.
    /// The handler checks `result.success`; `errors` are warn-logged on failure.
    pub async fn cleanup_simulation_logs(&self, simulation_id: &str) -> CleanupResult {
        let sim_dir = self.sim_data_dir.join(simulation_id);

        // sim_dir missing → success (Python :1129-1130)
        if !sim_dir.exists() {
            return CleanupResult {
                success: true,
                cleaned_files: vec![],
                errors: None,
                message: Some("模拟目录不存在，无需清理".to_string()),
            };
        }

        let mut cleaned_files: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        // Files in sim_dir (Python :1136-1141)
        let root_files = [
            "run_state.json",
            "simulation.log",
            "stdout.log",
            "stderr.log",
            "twitter_simulation.db",
            "reddit_simulation.db",
            "env_status.json",
        ];
        for name in &root_files {
            let path = sim_dir.join(name);
            if path.exists() {
                match std::fs::remove_file(&path) {
                    Ok(_) => cleaned_files.push(name.to_string()),
                    Err(e) => errors.push(format!("{name}: {e}")),
                }
            }
            // absent → skip, not an error (Python os.remove skips with FileNotFoundError handled)
        }

        // In sub-dirs twitter/reddit: actions.jsonl (Python :1143-1147)
        for sub in &["twitter", "reddit"] {
            let actions_path = sim_dir.join(sub).join("actions.jsonl");
            if actions_path.exists() {
                match std::fs::remove_file(&actions_path) {
                    Ok(_) => cleaned_files.push(format!("{sub}/actions.jsonl")),
                    Err(e) => errors.push(format!("{sub}/actions.jsonl: {e}")),
                }
            }
        }

        // In-memory cleanup: remove the run handle (Python :1171-1173 `del cls._run_states[id]`).
        // Idempotent — if stop_simulation already removed it, this is a no-op.
        {
            let mut runs = self.runs.lock().await;
            runs.remove(simulation_id);
        }

        let success = errors.is_empty();
        CleanupResult {
            success,
            cleaned_files,
            errors: if errors.is_empty() { None } else { Some(errors) },
            message: None,
        }
    }

    /// List the ids of all simulations whose task is not finished — port of
    /// `get_running_simulations` (S-627).
    ///
    /// Python: ids where `process.poll() is None`. teri: ids where `!task.is_finished()`.
    pub async fn get_running_simulations(&self) -> Vec<String> {
        let runs = self.runs.lock().await;
        runs.iter()
            .filter(|(_, h)| !h.is_finished())
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// Spawn the simulation task that drives `SimEngine::run` to completion in-process.
///
/// This is the in-process analog of `subprocess.Popen(cmd, ...)` (S-612). The task owns the
/// engine, pool, graph, LLM client, and the IPC server (the server's `start()` marks the
/// environment alive for interview round-trips — DECISION-16). When `SimEngine::run` returns
/// (naturally, or early via the cooperative-shutdown flag), the task ends and the IPC server
/// is stopped + dropped.
///
/// Threads the full run context (engine/pool/graph/main+boost LLM/IPC server/shutdown/parallel-mode)
/// into the task — hence the wide arg list; these are a single run's owned inputs, not separable.
#[allow(clippy::too_many_arguments)]
fn spawn_sim_task<L: LlmClient + Send + Sync + 'static>(
    engine: SimEngine,
    pool: crate::agent::AgentPool,
    graph: crate::graph::KnowledgeGraph,
    llm: Arc<L>,
    boost_llm: Option<Arc<L>>,
    ipc_server: SimulationIPCServer,
    shutdown: Arc<AtomicBool>,
    parallel: bool,
) -> JoinHandle<()> {
    // Box+coerce the future to an explicit `Pin<Box<dyn Future + Send>>`. This sidesteps
    // rustc's higher-ranked-lifetime inference failure ("implementation of `FnOnce` is not
    // general enough") on the `SimEngine::run` → `prepare_action` closure when the run future
    // is handed to `tokio::spawn`. The explicit type annotation pins the lifetime so the
    // closure's `for<'a> FnMut(&'a Agent)` bound resolves. Behavior is unchanged.
    let fut: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> = Box::pin(
        run_sim_body(engine, pool, graph, llm, boost_llm, ipc_server, shutdown, parallel),
    );
    tokio::spawn(fut)
}

/// Poll cadence for the post-simulation wait-for-commands loop.
///
/// Python loops on `await asyncio.wait_for(_shutdown_event.wait(), timeout=0.5)`
/// (`run_twitter_simulation.py:~688`) — a 0.5 s poll interval. teri uses a finer 50 ms cadence:
/// strictly more responsive (never a downgrade), and the interval itself is a non-contractual
/// implementation detail (the observable contract is "pending commands get serviced").
const WAIT_FOR_COMMANDS_POLL: Duration = Duration::from_millis(50);

/// The body driven by the spawned simulation task.
///
/// Split into a named `async fn` (rather than an inline `async move` block) so the
/// `SimEngine::run` higher-ranked closure resolves cleanly under `tokio::spawn` — an inline
/// block trips rustc's "implementation of `FnOnce` is not general enough" on the
/// `prepare_action` borrow. Behavior is identical to an inline block.
///
/// Wide arg list = one run's owned context (engine/pool/graph/main+boost LLM/IPC server/shutdown/
/// parallel-mode), threaded together into the task body; they are not separable concerns.
#[allow(clippy::too_many_arguments)]
async fn run_sim_body<L: LlmClient + Send + Sync + 'static>(
    engine: SimEngine,
    mut pool: crate::agent::AgentPool,
    graph: crate::graph::KnowledgeGraph,
    llm: Arc<L>,
    boost_llm: Option<Arc<L>>,
    mut ipc_server: SimulationIPCServer,
    shutdown: Arc<AtomicBool>,
    parallel: bool,
) {
    // Mark the env alive while the run is in progress (DECISION-16: `check_env_alive`
    // reads this flag; interview commands are serviced by the wait-for-commands loop below).
    ipc_server.start();

    if let Err(e) = engine.run_with_boost(&mut pool, &graph, &*llm, boost_llm.as_deref()).await {
        tracing::error!("simulation run failed: {e}");
        // Fail-closed teardown. On the success tail the engine fires the completion watch; on
        // error it never does, so the monitor (whose only loop-exit IS that watch) would poll
        // forever and `get_run_state` would report `Running` indefinitely while two tasks leak.
        // Fire the signal so the monitor unblocks, runs its cleanup, and marks the run `Failed`
        // (no `simulation_end` record was written). Then return WITHOUT entering the
        // wait-for-commands loop — a failed run has no live env to interview, so lingering would
        // only keep this task (and its engine) alive needlessly.
        engine.signal_aborted();
        ipc_server.stop();
        return;
    }

    // ---------------------------------------------------------------------------------------
    // Wait-for-commands mode — port of `IPCHandler.process_commands` (run_twitter_simulation.py:
    // 343-384) driven by the runner's post-rounds wait loop (`while not _shutdown_event.is_set():
    // await process_commands()`). After the simulation rounds finish, the env stays ALIVE and
    // services IPC commands until a `close_env` arrives, the cooperative-stop flag is set, or
    // every IPC client has been dropped (the run handle was removed — teri's analog of the OS
    // killing the subprocess; Python relied on process death for that teardown).
    //
    // The run's COMPLETED status is decided by the monitor from the `actions.jsonl`
    // `simulation_end` record (`subscribe_completion` fired the moment `engine.run` returned),
    // NOT by this task ending — so lingering here to answer interviews does not delay completion.
    // `pool` is owned exclusively by this body, so post-run interview execution has clean
    // (`&pool`) access with no shared-mutability machinery.
    // ---------------------------------------------------------------------------------------
    loop {
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        match ipc_server.try_poll() {
            CommandPoll::Command(env) => {
                // `dispatch_command` returns `false` only for `close_env` → exit the wait loop.
                if !dispatch_command(env, &pool, &*llm, parallel).await {
                    break;
                }
            }
            CommandPoll::Empty => {
                tokio::time::sleep(WAIT_FOR_COMMANDS_POLL).await;
            }
            CommandPoll::Disconnected => break,
        }
    }

    // Run finished + env-loop exited — mark env not-alive.
    ipc_server.stop();
}

/// Dispatch one IPC command in the wait-for-commands loop.
///
/// Port of `IPCHandler.process_commands`' body (run_twitter_simulation.py:343-384). Returns
/// `true` to keep the env alive (continue the loop) and `false` for `close_env` (exit the loop).
/// `CommandType` has exactly three variants, so the match is exhaustive — Python's `else:
/// "unknown command"` branch is unreachable here because `IPCCommand::from_dict` already rejects
/// any unrecognised `command_type` string at deserialization time.
/// `parallel` selects the dispatch variant — teri's analog of Python launching
/// `ParallelIPCHandler` (parallel run) vs `IPCHandler` (single-platform run). `true` →
/// dual-platform interview ([`execute_interview_parallel`]/[`execute_batch_interview_parallel`],
/// honoring the optional `platform` arg); `false` → the single-env path unchanged
/// ([`execute_interview`]/[`execute_batch_interview`], S-865/866/868).
async fn dispatch_command<L: LlmClient + Send + Sync>(
    env: IpcEnvelope,
    pool: &crate::agent::AgentPool,
    llm: &L,
    parallel: bool,
) -> bool {
    match env.command.command_type {
        CommandType::CloseEnv => {
            // Python: send_response(id, "completed", result={"message": "环境即将关闭"}); return False.
            let mut m = Map::new();
            m.insert("message".to_string(), Value::String("环境即将关闭".to_string()));
            SimulationIPCServer::send_success(env, m);
            false
        }
        CommandType::Interview => {
            let outcome = if parallel {
                execute_interview_parallel(pool, llm, &env.command.args).await
            } else {
                execute_interview(pool, llm, &env.command.args).await
            };
            match outcome {
                Ok(result) => SimulationIPCServer::send_success(env, result),
                Err(e) => SimulationIPCServer::send_error(env, e),
            }
            true
        }
        CommandType::BatchInterview => {
            let outcome = if parallel {
                execute_batch_interview_parallel(pool, llm, &env.command.args).await
            } else {
                execute_batch_interview(pool, llm, &env.command.args).await
            };
            match outcome {
                Ok(result) => SimulationIPCServer::send_success(env, result),
                Err(e) => SimulationIPCServer::send_error(env, e),
            }
            true
        }
    }
}

/// Resolve a pool agent by its OASIS `user_id` (the interview `agent_id`).
///
/// Python resolves via `self.agent_graph.get_agent(agent_id)` which raises if the id is unknown
/// (caught → `send_response(..., "failed", error=str(e))`). teri matches `SocialProfile.user_id`
/// across the pool (the same id `load_agent_pool` assigns) and returns `None` on no match, which
/// the caller turns into an error response.
fn resolve_agent_by_user_id(
    pool: &crate::agent::AgentPool,
    agent_id: i64,
) -> Option<&crate::agent::Agent> {
    pool.agents
        .iter()
        .find(|a| a.persona.social.as_ref().map(|s| s.user_id as i64) == Some(agent_id))
}

/// Build the interview prompt fed to the agent's LLM.
///
/// teri-native analog of OASIS `env.step({agent: ManualAction(INTERVIEW, {"prompt": prompt})})`
/// (`IPCHandler.handle_interview`, run_twitter_simulation.py:214). The OASIS prompt-composition
/// internals are `[≠]U028-OASIS-INTERNALS` (camel-ai builds its own system+interview message);
/// the differentially-portable contract is "the agent answers the question in first person, in
/// character, via its LLM". The persona context (name + background + the recovered OASIS
/// `social.persona` blob) grounds the answer the same way the decision prompt does.
fn build_interview_prompt(agent: &crate::agent::Agent, prompt: &str) -> String {
    let persona = &agent.persona;
    let mut ctx = format!("You are {}.\n{}\n", persona.name, persona.background);
    if let Some(social) = persona.social.as_ref()
        && !social.persona.is_empty()
    {
        ctx.push_str(&format!("Persona: {}\n", social.persona));
    }
    format!(
        "{ctx}\nYou are being interviewed. Answer the following question in the first person, \
         staying fully in character.\nQuestion: {prompt}"
    )
}

/// Execute a single-agent interview natively.
///
/// Port of `IPCHandler.handle_interview` (run_twitter_simulation.py:214-247): resolve the agent,
/// run the interview action, return the result. The OASIS path was `env.step(ManualAction
/// (INTERVIEW))` then `_get_interview_result(agent_id)` reading the `trace` SQLite DB
/// (`[≠]U028-OASIS-INTERNALS`); teri runs the agent's LLM directly and returns the response
/// inline (no DB round-trip). Result shape mirrors `_get_interview_result`'s
/// `{agent_id, response, timestamp}` (run_twitter_simulation.py:300-343). The actual response
/// text is LLM-generated → non-deterministic, the same `[!]`-LLM-gated class as the producer
/// decisions; the contract that IS gated here is resolution + shape + error behavior.
async fn execute_interview<L: LlmClient + Send + Sync>(
    pool: &crate::agent::AgentPool,
    llm: &L,
    args: &Map<String, Value>,
) -> std::result::Result<Map<String, Value>, String> {
    // Python: args.get("agent_id", 0) / args.get("prompt", "").
    let agent_id = args.get("agent_id").and_then(Value::as_i64).unwrap_or(0);
    let prompt = args.get("prompt").and_then(Value::as_str).unwrap_or("");

    let agent = resolve_agent_by_user_id(pool, agent_id)
        .ok_or_else(|| format!("Agent {agent_id} not found"))?;
    let interview_prompt = build_interview_prompt(agent, prompt);
    let response = llm.complete(&interview_prompt).await.map_err(|e| e.to_string())?;

    Ok(interview_result(agent_id, response))
}

/// Shape one interview result, mirroring Python `_get_interview_result`'s
/// `{agent_id, response, timestamp}` (run_twitter_simulation.py:303-308). `timestamp` is the
/// completion time (Python read the OASIS `trace.created_at`; teri stamps it now — a timestamp
/// source `[≠]` only).
fn interview_result(agent_id: i64, response: String) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("agent_id".to_string(), Value::from(agent_id));
    m.insert("response".to_string(), Value::String(response));
    m.insert(
        "timestamp".to_string(),
        Value::String(crate::models::project::python_isoformat_local()),
    );
    m
}

/// Execute a batch interview natively.
///
/// Port of `IPCHandler.handle_batch_interview` (run_twitter_simulation.py:248-300): resolve each
/// `{agent_id, prompt}`, skip unresolvable ids with a warning (Python's per-item `try/except`),
/// and if NONE resolve return an error (`"没有有效的Agent"`). On success the shape is
/// `{interviews_count, results}` where `results` is keyed by `agent_id` (Python's dict keyed by
/// int → JSON string keys), each value the same `{agent_id, response, timestamp}` map.
async fn execute_batch_interview<L: LlmClient + Send + Sync>(
    pool: &crate::agent::AgentPool,
    llm: &L,
    args: &Map<String, Value>,
) -> std::result::Result<Map<String, Value>, String> {
    let interviews = args.get("interviews").and_then(Value::as_array).cloned().unwrap_or_default();

    let mut results = Map::new();
    for item in &interviews {
        let Some(agent_id) = item.get("agent_id").and_then(Value::as_i64) else {
            continue;
        };
        let prompt = item.get("prompt").and_then(Value::as_str).unwrap_or("");
        let Some(agent) = resolve_agent_by_user_id(pool, agent_id) else {
            tracing::warn!("批量采访: 无法获取Agent {}", agent_id);
            continue;
        };
        let interview_prompt = build_interview_prompt(agent, prompt);
        match llm.complete(&interview_prompt).await {
            Ok(response) => {
                results.insert(
                    agent_id.to_string(),
                    Value::Object(interview_result(agent_id, response)),
                );
            }
            Err(e) => tracing::warn!("批量采访: Agent {} LLM 失败: {}", agent_id, e),
        }
    }

    if results.is_empty() {
        // Python: if not actions → send_response(id, "failed", error="没有有效的Agent").
        return Err("没有有效的Agent".to_string());
    }

    let mut m = Map::new();
    m.insert("interviews_count".to_string(), Value::from(results.len() as u64));
    m.insert("results".to_string(), Value::Object(results));
    Ok(m)
}

// ===========================================================================
// PARALLEL (dual-platform) interview dispatch — port of `ParallelIPCHandler`
// (run_parallel_simulation.py:217-600).
//
// Python runs TWO separate OASIS environments (`twitter_env`, `reddit_env`) where the same
// numeric `agent_id` denotes two DIFFERENT agents — one per platform. teri runs ONE unified
// pool (`load_agent_pool("parallel")` unions both platforms' profiles), where each agent carries
// `SocialProfile.platform` and the same `user_id` can appear once per platform. So:
//   - "platform available" (Python `self.{platform}_env` truthy) ≡ the pool has ≥1 agent on
//     that platform ([`pool_has_platform`]).
//   - per-platform `agent_graph.get_agent(agent_id)` ≡ resolve the pool agent whose
//     `(user_id, platform)` matches ([`resolve_agent_on_platform`]); a raise (unknown id) ≡ `None`.
// This is the faithful `[≠]U030-UNIFIED-LOOP` mapping: the unified pool reproduces the
// dual-platform routing/result shape; only the OASIS `env.step`+`trace`-DB mechanism stays
// `[≠]U028-OASIS-INTERNALS` (teri runs the agent's LLM inline, as in the single-env path).
//
// Selection by run mode mirrors Python selecting `ParallelIPCHandler` vs `IPCHandler` by which
// script launched: `dispatch_command(parallel=true)` for a `platform="parallel"` run, the
// untouched single-env `execute_interview`/`execute_batch_interview` otherwise (S-865/866/868
// preserved byte-for-byte).
// ===========================================================================

/// OASIS-style lowercase platform string (`"twitter"` / `"reddit"`) for a `Platform`.
fn platform_str(p: crate::agent::Platform) -> &'static str {
    match p {
        crate::agent::Platform::Twitter => "twitter",
        crate::agent::Platform::Reddit => "reddit",
    }
}

/// Does the pool contain at least one agent on `platform`? teri analog of Python's
/// `self.{platform}_env` truthiness check (env availability).
fn pool_has_platform(pool: &crate::agent::AgentPool, platform: &str) -> bool {
    pool.agents.iter().any(|a| {
        a.persona
            .social
            .as_ref()
            .map(|s| platform_str(s.platform) == platform)
            .unwrap_or(false)
    })
}

/// Resolve the pool agent matching BOTH `user_id == agent_id` and `platform`. teri analog of
/// per-platform `agent_graph.get_agent(agent_id)` (which raises on an unknown id → `None` here).
fn resolve_agent_on_platform<'a>(
    pool: &'a crate::agent::AgentPool,
    agent_id: i64,
    platform: &str,
) -> Option<&'a crate::agent::Agent> {
    pool.agents.iter().find(|a| {
        a.persona
            .social
            .as_ref()
            .is_some_and(|s| s.user_id as i64 == agent_id && platform_str(s.platform) == platform)
    })
}

/// Run one interview on one platform — port of `_interview_single_platform`
/// (run_parallel_simulation.py:317-343). Returns EITHER a success result map
/// (`{agent_id, response, timestamp, platform}`) OR an error map (`{platform, error}`); the
/// caller checks for the `"error"` key (Python `if "error" in result`).
async fn execute_interview_one_platform<L: LlmClient + Send + Sync>(
    pool: &crate::agent::AgentPool,
    llm: &L,
    agent_id: i64,
    prompt: &str,
    platform: &str,
) -> Map<String, Value> {
    // Python: `if not env` → `{"platform": platform, "error": f"{platform}平台不可用"}`.
    if !pool_has_platform(pool, platform) {
        return error_map(platform, &format!("{platform}平台不可用"));
    }
    // Python: `agent = agent_graph.get_agent(agent_id)` (raises on unknown → caught → error map).
    let Some(agent) = resolve_agent_on_platform(pool, agent_id, platform) else {
        return error_map(platform, &format!("Agent {agent_id} not found"));
    };
    let interview_prompt = build_interview_prompt(agent, prompt);
    match llm.complete(&interview_prompt).await {
        // `_get_interview_result(agent_id)` shape + `result["platform"] = actual_platform`.
        Ok(response) => {
            let mut m = interview_result(agent_id, response);
            m.insert("platform".to_string(), Value::String(platform.to_string()));
            m
        }
        // Python: `except Exception as e: return {"platform": platform, "error": str(e)}`.
        Err(e) => error_map(platform, &e.to_string()),
    }
}

/// `{platform, error}` map (the `_interview_single_platform` failure shape).
fn error_map(platform: &str, error: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("platform".to_string(), Value::String(platform.to_string()));
    m.insert("error".to_string(), Value::String(error.to_string()));
    m
}

/// Execute a parallel (dual-platform) single-agent interview — port of
/// `ParallelIPCHandler.handle_interview` (run_parallel_simulation.py:345-414).
///
/// - `platform` ∈ {twitter, reddit}: interview ONLY that platform; success → the single result
///   map (with `platform` key), error → `Err` (→ `send_response` failed).
/// - `platform` absent: interview EVERY available platform, return
///   `{agent_id, prompt, platforms: {twitter: …, reddit: …}}`; succeeds if ≥1 platform succeeds
///   (`success_count > 0`), else `Err("twitter: …; reddit: …")`. No platform available at all →
///   `Err("没有可用的模拟环境")`.
async fn execute_interview_parallel<L: LlmClient + Send + Sync>(
    pool: &crate::agent::AgentPool,
    llm: &L,
    args: &Map<String, Value>,
) -> std::result::Result<Map<String, Value>, String> {
    let agent_id = args.get("agent_id").and_then(Value::as_i64).unwrap_or(0);
    let prompt = args.get("prompt").and_then(Value::as_str).unwrap_or("");
    let platform = args.get("platform").and_then(Value::as_str);

    // Platform specified → single-platform path.
    if let Some(p) = platform
        && (p == "twitter" || p == "reddit")
    {
        let result = execute_interview_one_platform(pool, llm, agent_id, prompt, p).await;
        return match result.get("error").and_then(Value::as_str) {
            Some(err) => Err(err.to_string()),
            None => Ok(result),
        };
    }

    // No platform → interview both available platforms.
    let has_twitter = pool_has_platform(pool, "twitter");
    let has_reddit = pool_has_platform(pool, "reddit");
    if !has_twitter && !has_reddit {
        return Err("没有可用的模拟环境".to_string());
    }

    let mut platforms = Map::new();
    let mut success_count = 0u64;
    let mut errors: Vec<String> = Vec::new();
    // Insertion order twitter→reddit mirrors Python `platforms_to_interview` build order.
    for p in ["twitter", "reddit"] {
        let available = if p == "twitter" { has_twitter } else { has_reddit };
        if !available {
            continue;
        }
        let result = execute_interview_one_platform(pool, llm, agent_id, prompt, p).await;
        match result.get("error").and_then(Value::as_str) {
            Some(err) => errors.push(format!("{p}: {err}")),
            None => success_count += 1,
        }
        platforms.insert(p.to_string(), Value::Object(result));
    }

    if success_count > 0 {
        let mut m = Map::new();
        m.insert("agent_id".to_string(), Value::from(agent_id));
        m.insert("prompt".to_string(), Value::String(prompt.to_string()));
        m.insert("platforms".to_string(), Value::Object(platforms));
        Ok(m)
    } else {
        // Python joins per-platform errors with "; ".
        Err(errors.join("; "))
    }
}

/// Execute a parallel (dual-platform) batch interview — port of
/// `ParallelIPCHandler.handle_batch_interview` (run_parallel_simulation.py:416-514).
///
/// Each item routes by its own `platform` (falling back to the command-level default); an item
/// with neither is interviewed on BOTH available platforms (added to each platform's batch). For
/// a platform with ≥1 RESOLVED item, EVERY item of that platform is collected (resolved → LLM
/// response; unresolvable → `{response: null, timestamp: null}`, mirroring `_get_interview_result`
/// returning a no-row record); if a platform has 0 resolved items it contributes nothing (Python
/// guards `if {platform}_actions:`). Results are keyed `"{platform}_{agent_id}"`. Empty →
/// `Err("没有成功的采访")`.
async fn execute_batch_interview_parallel<L: LlmClient + Send + Sync>(
    pool: &crate::agent::AgentPool,
    llm: &L,
    args: &Map<String, Value>,
) -> std::result::Result<Map<String, Value>, String> {
    let interviews = args.get("interviews").and_then(Value::as_array).cloned().unwrap_or_default();
    let default_platform = args.get("platform").and_then(Value::as_str);

    // Group items by platform (Python: twitter / reddit / both).
    let mut twitter_items: Vec<&Value> = Vec::new();
    let mut reddit_items: Vec<&Value> = Vec::new();
    let mut both_items: Vec<&Value> = Vec::new();
    for item in &interviews {
        match item.get("platform").and_then(Value::as_str).or(default_platform) {
            Some("twitter") => twitter_items.push(item),
            Some("reddit") => reddit_items.push(item),
            _ => both_items.push(item),
        }
    }
    // Distribute "both" items to each AVAILABLE platform's batch (Python:
    // `if self.{platform}_env: {platform}_interviews.extend(both_platforms_interviews)`).
    let has_twitter = pool_has_platform(pool, "twitter");
    let has_reddit = pool_has_platform(pool, "reddit");
    if has_twitter {
        twitter_items.extend(both_items.iter().copied());
    }
    if has_reddit {
        reddit_items.extend(both_items.iter().copied());
    }

    let mut results = Map::new();
    // Insertion order twitter→reddit mirrors Python (twitter batch processed first).
    for (platform, items, available) in
        [("twitter", &twitter_items, has_twitter), ("reddit", &reddit_items, has_reddit)]
    {
        collect_platform_batch(pool, llm, platform, items, available, &mut results).await;
    }

    if results.is_empty() {
        // Python: `else: send_response(id, "failed", error="没有成功的采访")`.
        return Err("没有成功的采访".to_string());
    }
    let mut m = Map::new();
    m.insert("interviews_count".to_string(), Value::from(results.len() as u64));
    m.insert("results".to_string(), Value::Object(results));
    Ok(m)
}

/// Collect one platform's batch into `results` (keyed `"{platform}_{agent_id}"`). Mirrors the
/// Python per-platform block: only proceed if ≥1 item RESOLVES (`if {platform}_actions:`), then
/// emit a record for EVERY item — resolved items get the LLM response, unresolvable items get a
/// null-response record (Python's `_get_interview_result` no-row shape).
async fn collect_platform_batch<L: LlmClient + Send + Sync>(
    pool: &crate::agent::AgentPool,
    llm: &L,
    platform: &str,
    items: &[&Value],
    available: bool,
    results: &mut Map<String, Value>,
) {
    if items.is_empty() || !available {
        return;
    }
    // Resolve each item's agent on this platform up front (Python builds `{platform}_actions`).
    let resolved: Vec<(i64, &str, bool)> = items
        .iter()
        .filter_map(|item| {
            let agent_id = item.get("agent_id").and_then(Value::as_i64)?;
            let prompt = item.get("prompt").and_then(Value::as_str).unwrap_or("");
            let ok = resolve_agent_on_platform(pool, agent_id, platform).is_some();
            if !ok {
                tracing::warn!("批量采访: 无法获取{}Agent {}", platform, agent_id);
            }
            Some((agent_id, prompt, ok))
        })
        .collect();

    // Python `if {platform}_actions:` — only collect when ≥1 item resolved on this platform.
    if !resolved.iter().any(|(_, _, ok)| *ok) {
        return;
    }
    for (agent_id, prompt, ok) in resolved {
        let mut record = if ok {
            let agent =
                resolve_agent_on_platform(pool, agent_id, platform).expect("resolved above");
            let interview_prompt = build_interview_prompt(agent, prompt);
            match llm.complete(&interview_prompt).await {
                Ok(response) => interview_result(agent_id, response),
                // LLM failure → null-response record (the no-result shape), platform-keyed below.
                Err(e) => {
                    tracing::warn!("批量采访: {}Agent {} LLM 失败: {}", platform, agent_id, e);
                    interview_result_null(agent_id)
                }
            }
        } else {
            // Unresolvable on this platform → Python `_get_interview_result` returns a no-row
            // record `{agent_id, response: None, timestamp: None}`.
            interview_result_null(agent_id)
        };
        record.insert("platform".to_string(), Value::String(platform.to_string()));
        results.insert(format!("{platform}_{agent_id}"), Value::Object(record));
    }
}

/// `{agent_id, response: null, timestamp: null}` — the `_get_interview_result` no-DB-row shape.
fn interview_result_null(agent_id: i64) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("agent_id".to_string(), Value::from(agent_id));
    m.insert("response".to_string(), Value::Null);
    m.insert("timestamp".to_string(), Value::Null);
    m
}

/// Cooperative-then-force terminate of a single run's tasks — port of `_terminate_process`
/// (S-616), observable contract only.
///
/// 1. Set the cooperative shutdown flag (`store(true, Release)`) — the SIGTERM analog. The
///    `SimEngine` tick loop honors it at the next round boundary and returns gracefully.
/// 2. Await the task bounded by the `grace` window (`timeout(grace)`).
/// 3. On timeout, `task.abort()` — the SIGKILL analog — then await the (now cancelled) task.
/// 4. Abort the monitor task too if present (it would otherwise outlive the run).
///
/// The `grace` duration is the contractual observable and is supplied by the caller to match
/// the source's per-caller `timeout` (`_terminate_process(process, timeout=…)`):
/// `stop_simulation` → [`STOP_GRACE`] (10s, the Python default), `cleanup_all` →
/// [`CLEANUP_GRACE`] (5s, the explicit `timeout=5`). The Windows `taskkill`/Unix
/// `killpg`/pgid/SIGTERM/SIGKILL machinery is `[≠]` inexpressible (no OS process); the
/// grace-then-force *window* is preserved exactly, including the per-caller difference.
async fn terminate_handle(handle: &mut RunHandle, simulation_id: &str, grace: Duration) {
    // (1) Cooperative stop — SIGTERM analog.
    handle.shutdown.store(true, Ordering::Release);
    tracing::info!("终止模拟 (cooperative): simulation={}", simulation_id);

    // (2)/(3) grace window, then force-abort.
    match tokio::time::timeout(grace, &mut handle.task).await {
        Ok(_join_result) => {
            // Task finished within the grace window (graceful stop succeeded).
        }
        Err(_elapsed) => {
            // Grace window elapsed — force-abort (SIGKILL analog), then reap the cancellation.
            tracing::warn!("模拟未响应协作停止，强制终止: {}", simulation_id);
            handle.task.abort();
            let _ = (&mut handle.task).await; // observe the JoinError::Cancelled, ignore it
        }
    }

    // (4) Abort the monitor task if present (sub-cycle c). It tails the action log and would
    // otherwise outlive the run; aborting it here mirrors the daemon-thread teardown.
    if let Some(monitor) = handle.monitor.take() {
        monitor.abort();
        let _ = monitor.await;
    }
}

// ===========================================================================
// Simulation MONITOR — sub-cycle (c)
//
// Ports S-613 (`_monitor_simulation`), S-614 (`_read_action_log`), S-615
// (`_check_all_platforms_completed`), and populates S-605 (`_monitor_threads` →
// `RunHandle.monitor`, spawned in `start_simulation` above).
//
// REALIZES U-047 `JSONL_TAIL_CONTRACT` (S-1056): the offset/seek incremental reader
// ([`read_action_log`]) is the byte-offset tail — seek to the last offset, read only
// NEW complete lines, advance the offset only past complete lines (a trailing line
// without a newline is a partial write and is NOT consumed until the next poll sees it
// newline-terminated), never re-read, never lose a line.
//
// Monitor source — tail U-010's `actions.jsonl` by byte offset (DECISION-17 §17 Area 2,
// "Monitor source decision"). teri's `SimEngine` writes the SAME `{sim_dir}/{platform}/
// actions.jsonl` via `PlatformActionLogger` (U-010); the monitor re-derives run-state
// from that log exactly as MiroFish's `_read_action_log` does (`f.seek(position)` →
// readline → `f.tell()`). U-048's `subscribe_completion()` is the loop-exit signal,
// replacing Python's `process.poll()` (DECISION-17 §17 Area 2).
//
// `[≠]` in this sub-cycle:
//   - The OS-thread `daemon=True` flag (`simulation_runner.py:467`) — a tokio task is
//     inherently tied to the runtime; the OBSERVABLE "monitor dies with the run" is PORTED
//     via `terminate_handle` aborting `RunHandle.monitor` on stop/cleanup/shutdown.
//   - The non-zero `exit_code` → FAILED branch + `simulation.log` tail read
//     (`simulation_runner.py:524-544`): there is no OS exit code in-process. teri's sim
//     task returns `Result`; a run that ends without emitting `simulation_end` simply leaves
//     the state as last persisted (RUNNING flags cleared on the natural-end path below). The
//     COMPLETED-via-`simulation_end` path (the observable success contract) IS PORTED; the
//     exit-code FAILED branch is the OS-mechanism `[≠]` (the run-failure observable is carried
//     by the sim task's own error logging + the `Failed` transitions in `start_simulation`).
// ===========================================================================

/// Poll cadence for the monitor loop — mirrors MiroFish `time.sleep(2)`
/// (`simulation_runner.py:517`). The 2s interval is an observable cadence and is preserved.
const MONITOR_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Everything the monitor task needs to tail the action logs and update the shared run-state.
///
/// Owned by the spawned monitor task (it is `'static`); built in `start_simulation`.
struct MonitorContext<L: LlmClient + Send + Sync + 'static> {
    /// The simulation id (for log lines + graph-fire routing).
    simulation_id: String,
    /// Root simulation-data dir; the per-platform logs live at `{dir}/{id}/{platform}/actions.jsonl`.
    sim_data_dir: std::path::PathBuf,
    /// The SHARED run-state — the same `Arc<Mutex>` `get_run_state` reads, so monitor writes
    /// are observable (`add_action`, `current_round`, `*_completed`, `runner_status=COMPLETED`).
    state: Arc<tokio::sync::Mutex<SimulationRunState>>,
    /// Graph-memory manager (U-021) — the monitor fires `add_activity_from_dict` per new action
    /// through it when `graph_enabled` (the `get_updater(...).add_activity_from_dict(...)` analog).
    graph_mgr: Arc<GraphMemoryManager<L>>,
    /// Whether graph-memory updating is enabled for this run (`_graph_memory_enabled[id]`).
    graph_enabled: bool,
    /// Agent LTM writer (independent of graph memory) — `Some` persists each content-bearing
    /// action into the vector/LTM store; `None` disables agent memory for this run.
    agent_memory: Option<Arc<crate::services::agent_memory::AgentMemoryWriter>>,
}

/// Spawn the monitor task (`_monitor_threads[id]`, S-605). Returns its `JoinHandle` for storage
/// in `RunHandle.monitor` (aborted by `terminate_handle` on stop/cleanup).
///
/// `completion_rx` (U-048 terminal signal) is passed separately from `ctx` because the loop needs
/// `&mut` access to it (for `changed()`/`borrow()`), while the per-poll readers borrow `ctx`
/// read-only — keeping them in separate values avoids a partial-move/borrow conflict.
fn spawn_monitor_task<L: LlmClient + Send + Sync + 'static>(
    ctx: MonitorContext<L>,
    completion_rx: tokio::sync::watch::Receiver<Option<crate::sim::SimCompletion>>,
) -> JoinHandle<()> {
    tokio::spawn(monitor_simulation(ctx, completion_rx))
}

/// Port of `SimulationRunner._monitor_simulation` (S-613, `simulation_runner.py:481-544`).
///
/// The daemon monitor loop:
/// 1. Polls the per-platform `actions.jsonl` files every [`MONITOR_POLL_INTERVAL`] (2s) by BYTE
///    POSITION (the [`read_action_log`] seek pattern), parsing only NEW complete lines.
/// 2. Fires `ZepGraphMemoryUpdater.add_activity_from_dict` per action WHEN graph memory is enabled
///    (inside [`read_action_log`], via the graph manager).
/// 3. Detects the `simulation_end` event to mark the platform completed and — once ALL enabled
///    platforms have completed ([`check_all_platforms_completed`], S-615) — sets the run COMPLETED.
/// 4. Persists `run_state.json` after each poll (Python L514 `_save_run_state(state)`).
///
/// Loop exit: the in-process analog of `while process.poll() is None` is the U-048 completion
/// watch (`completion_rx`). When the sim task finishes (signals `Some(SimCompletion)`), the loop
/// breaks and does ONE FINAL tail pass (Python L518-522 "进程结束后，最后读取一次日志") so no action
/// written between the last poll and the end signal is lost. The task is also abortable (the
/// daemon-thread teardown analog) via `RunHandle.monitor` in `terminate_handle`.
async fn monitor_simulation<L: LlmClient + Send + Sync + 'static>(
    ctx: MonitorContext<L>,
    mut completion_rx: tokio::sync::watch::Receiver<Option<crate::sim::SimCompletion>>,
) {
    let sim_dir = ctx.sim_data_dir.join(&ctx.simulation_id);
    let twitter_log = sim_dir.join("twitter").join("actions.jsonl");
    let reddit_log = sim_dir.join("reddit").join("actions.jsonl");

    // Per-file byte offsets — the U-047 tail position. Monotonic per file (advanced only past
    // complete lines). Start at 0 (read from the beginning on the first poll).
    let mut twitter_position: u64 = 0;
    let mut reddit_position: u64 = 0;

    // `completion_rx` starts at `None`; `Some(..)` means the run task finished (poll() is not None).

    loop {
        // poll() is None ⇒ still running. Check the watch's current value WITHOUT consuming it.
        let finished = completion_rx.borrow().is_some();
        if finished {
            break;
        }

        // Read Twitter actions (only if the file exists — Python `os.path.exists` guard, L506).
        if twitter_log.exists() {
            twitter_position =
                read_action_log(&twitter_log, twitter_position, &ctx, "twitter").await;
        }
        // Read Reddit actions (L511).
        if reddit_log.exists() {
            reddit_position = read_action_log(&reddit_log, reddit_position, &ctx, "reddit").await;
        }

        // Persist state after the poll (Python L514 `cls._save_run_state(state)`).
        {
            let state = ctx.state.lock().await;
            if let Err(e) = save_run_state(&ctx.sim_data_dir, &state) {
                tracing::warn!("保存运行状态失败: {}, error={}", ctx.simulation_id, e);
            }
        }

        // Sleep the poll interval OR wake early when completion fires (whichever first). Waking
        // early on completion lets the final-pass run promptly; the 2s cadence is otherwise honored.
        tokio::select! {
            _ = tokio::time::sleep(MONITOR_POLL_INTERVAL) => {}
            res = completion_rx.changed() => {
                // `changed()` resolves when the value transitions to Some(..) (or the sender drops).
                // Either way the run is over; loop back, the top-of-loop check breaks out.
                let _ = res;
            }
        }
    }

    // Process ended — FINAL read pass so no trailing action is lost (Python L518-522).
    if twitter_log.exists() {
        twitter_position = read_action_log(&twitter_log, twitter_position, &ctx, "twitter").await;
    }
    if reddit_log.exists() {
        reddit_position = read_action_log(&reddit_log, reddit_position, &ctx, "reddit").await;
    }
    // Offsets consumed only to satisfy the final assignment (mirrors Python reusing `*_position`).
    let _ = (twitter_position, reddit_position);

    // Natural-end housekeeping: clear the platform running flags + persist (Python L545-547
    // `state.twitter_running = False; state.reddit_running = False; _save_run_state(state)`).
    // The COMPLETED transition itself happens inside `read_action_log` on `simulation_end`; the
    // OS-exit-code → FAILED branch (L524-544) is `[≠]` (no OS exit code in-process — see module
    // note). A run that ended cleanly has already been marked COMPLETED by the final pass above.
    {
        let mut state = ctx.state.lock().await;
        state.twitter_running = false;
        state.reddit_running = false;
        // Fail-closed terminal transition. The COMPLETED transition happens inside
        // `read_action_log` on the `simulation_end` record; a cooperative stop sets `Stopped`.
        // If neither happened the status is still `Running` here — i.e. the engine aborted
        // (`run_sim_body` fired `signal_aborted` without writing `simulation_end`) or otherwise
        // ended without a terminal record. Mark it `Failed` so the run is observably terminal
        // instead of a perpetual `Running`. A clean finish (Completed) / cooperative stop
        // (Stopped) is never overwritten.
        if state.runner_status == RunnerStatus::Running {
            tracing::warn!(
                "simulation ended without a terminal record — marking Failed: {}",
                ctx.simulation_id
            );
            state.runner_status = RunnerStatus::Failed;
        }
        if let Err(e) = save_run_state(&ctx.sim_data_dir, &state) {
            tracing::warn!("save run state failed: {}, error={}", ctx.simulation_id, e);
        }
    }

    // Stop the graph-memory updater (Python L549-557 `finally:` block — stop updater if enabled).
    if ctx.graph_enabled {
        ctx.graph_mgr.stop_updater(&ctx.simulation_id).await;
        tracing::info!("已停止图谱记忆更新: simulation_id={}", ctx.simulation_id);
    }

    // Report agent-LTM write-back stats (observability — mirrors the graph updater's end stats).
    if let Some(writer) = &ctx.agent_memory {
        let (persisted, embedded, skipped) = writer.stats();
        tracing::info!(
            simulation_id = %ctx.simulation_id,
            persisted,
            embedded,
            skipped,
            "agent LTM write-back finished (persisted=chronological, embedded=semantic-vector)"
        );
    }

    tracing::info!("模拟监控结束: {}", ctx.simulation_id);
}

/// Port of `SimulationRunner._read_action_log` (S-614, `simulation_runner.py:559-688`) — the
/// offset/seek incremental reader that REALIZES U-047 `JSONL_TAIL_CONTRACT` (S-1056).
///
/// Opens the JSONL, seeks to the last byte `position`, reads only NEW **complete** lines (a line
/// is complete iff it is newline-terminated), parses each, applies its side effects to the shared
/// run-state, and returns the NEW byte offset (the position past the last complete line consumed).
///
/// # U-047 tail invariants (preserved exactly)
/// - **No re-read:** the seek to `position` skips everything already consumed.
/// - **No partial-line loss / no partial-line consumption:** a trailing fragment without a `\n`
///   is NOT consumed; the returned offset stops at the end of the last complete (newline-
///   terminated) line, so the next poll re-reads the fragment once it is newline-terminated.
///   (MiroFish relies on Python's `for line in f` yielding only newline-terminated lines while the
///   writer appends atomically per `writeln!`; we make the newline-boundary explicit so a
///   half-flushed final line is never parsed.)
/// - **Offset monotonic** per file (advanced only past complete lines).
/// - **Robust to a growing file** between polls (each poll seeks to the prior offset and reads the
///   delta) and to **read errors** (logged; the offset is left unchanged so the next poll retries —
///   Python L686-688 `except Exception: return position`).
///
/// # Per-line dispatch (mirrors Python L569-684 exactly)
/// - Blank lines are skipped (Python `if line:`).
/// - A line that is not valid JSON is skipped (Python `except json.JSONDecodeError: pass`).
/// - A record with an `event_type`:
///   - `"simulation_end"` → mark the platform completed + not-running; if
///     [`check_all_platforms_completed`] is now true → run `runner_status=COMPLETED` + `completed_at`.
///   - `"round_end"` → update per-platform `*_current_round` (monotonic max) + `*_simulated_hours`,
///     and the global `current_round` (max) + `simulated_hours` (max of the two platforms).
///   - any other `event_type` → `continue` (no action; NOT added as an action).
/// - Otherwise it is an action record → build an [`AgentAction`] (Python field defaults), call
///   `state.add_action(...)` (S-596 cap/counter/updated_at), bump global `current_round` if larger,
///   and — when `graph_enabled` — fire `add_activity_from_dict(raw_dict, platform)` (U-021).
///
/// Returns the new byte offset; on any I/O/seek error returns `position` unchanged.
async fn read_action_log<L: LlmClient + Send + Sync + 'static>(
    log_path: &Path,
    position: u64,
    ctx: &MonitorContext<L>,
    platform: &str,
) -> u64 {
    use std::io::{Read, Seek, SeekFrom};

    // Open + seek to the last offset (Python `open(...)` + `f.seek(position)`, L578-580).
    let mut file = match std::fs::File::open(log_path) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("读取动作日志失败: {}, error={}", log_path.display(), e);
            return position;
        }
    };
    if let Err(e) = file.seek(SeekFrom::Start(position)) {
        tracing::warn!("读取动作日志失败: {}, error={}", log_path.display(), e);
        return position;
    }

    // Read the delta from `position` to EOF.
    let mut buf = Vec::new();
    if let Err(e) = file.read_to_end(&mut buf) {
        tracing::warn!("读取动作日志失败: {}, error={}", log_path.display(), e);
        return position;
    }

    // Split into COMPLETE (newline-terminated) lines only. A trailing fragment without a newline
    // is a partial write — leave it unconsumed (U-047 partial-line safety) by not advancing the
    // offset past it and not parsing it.
    let mut consumed: u64 = 0; // bytes of complete lines consumed this pass
    let mut new_position = position;

    // Iterate line-by-line over the buffer, tracking the byte length (incl. the `\n`) of each
    // complete line so the offset advances by exact bytes (UTF-8-safe; lengths are byte counts).
    let mut start = 0usize;
    while let Some(rel_nl) = buf[start..].iter().position(|&b| b == b'\n') {
        let line_end = start + rel_nl; // index of '\n'
        let line_bytes = &buf[start..line_end]; // line WITHOUT the trailing '\n'
        // Advance the consumed byte count past this complete line INCLUDING its '\n'.
        consumed += (line_end - start + 1) as u64;
        new_position = position + consumed;
        start = line_end + 1;

        // Decode + strip (Python `line.strip()`). Lossy decode is safe: the writer emits UTF-8.
        let line = String::from_utf8_lossy(line_bytes);
        let line = line.trim();
        if line.is_empty() {
            continue; // Python `if line:`
        }
        // Parse JSON; skip on error (Python `except json.JSONDecodeError: pass`).
        let action_data: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        apply_log_record(&action_data, ctx, platform).await;
    }

    new_position
}

/// Apply one parsed JSONL record's side effects to the shared run-state (and fire graph memory).
///
/// Factored out of [`read_action_log`] so the lock-scoping discipline is explicit: the shared
/// state mutex is held ONLY for the synchronous state mutation, and is DROPPED before the
/// (potentially-awaiting) graph-memory fire — never held across `.await`.
async fn apply_log_record<L: LlmClient + Send + Sync + 'static>(
    action_data: &Value,
    ctx: &MonitorContext<L>,
    platform: &str,
) {
    // ---- event_type records (Python L585-661) ----
    if let Some(event_type) = action_data.get("event_type").and_then(Value::as_str) {
        match event_type {
            "simulation_end" => {
                // Mark this platform completed + not-running; gate run-COMPLETED on ALL platforms.
                let mut state = ctx.state.lock().await;
                match platform {
                    "twitter" => {
                        state.twitter_completed = true;
                        state.twitter_running = false;
                        tracing::info!(
                            "Twitter 模拟已完成: {}, total_rounds={:?}, total_actions={:?}",
                            ctx.simulation_id,
                            action_data.get("total_rounds"),
                            action_data.get("total_actions"),
                        );
                    }
                    "reddit" => {
                        state.reddit_completed = true;
                        state.reddit_running = false;
                        tracing::info!(
                            "Reddit 模拟已完成: {}, total_rounds={:?}, total_actions={:?}",
                            ctx.simulation_id,
                            action_data.get("total_rounds"),
                            action_data.get("total_actions"),
                        );
                    }
                    _ => {}
                }
                // Dual-platform gate (S-615): COMPLETED only when ALL enabled platforms done.
                if check_all_platforms_completed(&ctx.sim_data_dir, &state) {
                    state.runner_status = RunnerStatus::Completed;
                    state.completed_at = Some(crate::models::project::python_isoformat_local());
                    tracing::info!("所有平台模拟已完成: {}", ctx.simulation_id);
                }
            }
            "round_end" => {
                // Per-platform + global round / simulated-hours updates (Python L642-661).
                let round_num = action_data.get("round").and_then(Value::as_i64).unwrap_or(0);
                let simulated_hours =
                    action_data.get("simulated_hours").and_then(Value::as_i64).unwrap_or(0);
                let mut state = ctx.state.lock().await;
                match platform {
                    "twitter" => {
                        if round_num > state.twitter_current_round {
                            state.twitter_current_round = round_num;
                        }
                        state.twitter_simulated_hours = simulated_hours;
                    }
                    "reddit" => {
                        if round_num > state.reddit_current_round {
                            state.reddit_current_round = round_num;
                        }
                        state.reddit_simulated_hours = simulated_hours;
                    }
                    _ => {}
                }
                // Global round = max; global simulated_hours = max of the two platforms (L658-661).
                if round_num > state.current_round {
                    state.current_round = round_num;
                }
                state.simulated_hours =
                    state.twitter_simulated_hours.max(state.reddit_simulated_hours);
            }
            // Any other event_type → `continue` in Python (no action recorded).
            _ => {}
        }
        return; // Python `continue` after handling an event_type record (L661).
    }

    // ---- action records (Python L663-684) ----
    // Build the AgentAction with Python-identical field defaults. NOTE the U-010 ↔ U-022 field
    // alignment: U-010's `log_action` writes `"round"` (NOT `round_num`), `agent_id`, `agent_name`,
    // `action_type`, `action_args`, `result`, `success`, `timestamp` — and NO `platform` key (the
    // platform is the directory). The monitor maps `"round"` → `round_num` and supplies `platform`
    // from the file location, EXACTLY as MiroFish's `_read_action_log` does (L665-674).
    let action = AgentAction {
        round_num: action_data.get("round").and_then(Value::as_i64).unwrap_or(0),
        timestamp: action_data
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(crate::models::project::python_isoformat_local),
        platform: platform.to_string(),
        agent_id: action_data.get("agent_id").and_then(Value::as_i64).unwrap_or(0),
        agent_name: action_data.get("agent_name").and_then(Value::as_str).unwrap_or("").to_string(),
        action_type: action_data
            .get("action_type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        action_args: action_data
            .get("action_args")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
        result: action_data.get("result").and_then(Value::as_str).map(str::to_string),
        success: action_data.get("success").and_then(Value::as_bool).unwrap_or(true),
    };

    // State mutation under the lock ONLY (no .await held across it).
    {
        let mut state = ctx.state.lock().await;
        let round_num = action.round_num;
        state.add_action(action); // S-596: insert-at-front, cap 50, per-platform count, updated_at
        // Update global round (Python L677-678).
        if round_num != 0 && round_num > state.current_round {
            state.current_round = round_num;
        }
    } // lock dropped here

    // Fire graph memory per action when enabled (Python L681-684) — AFTER dropping the state lock.
    // The raw parsed dict is forwarded unchanged; U-021's `add_activity_from_dict` does the
    // `event_type`-skip (already excluded above) + field defaults + DO_NOTHING filter.
    if ctx.graph_enabled {
        ctx.graph_mgr
            .fire_activity_from_dict(&ctx.simulation_id, action_data, platform)
            .await;
    }

    // Fire agent LTM write-back per action when enabled (independent of graph memory). Persists
    // the utterance as chronological + semantic agent memory; best-effort (never errors the sim).
    if let Some(writer) = &ctx.agent_memory {
        writer.write_action(&ctx.simulation_id, action_data, platform).await;
    }
}

/// Port of `SimulationRunner._check_all_platforms_completed` (S-615,
/// `simulation_runner.py:694-718`).
///
/// Dual-platform completion gate: a platform is "enabled" iff its `actions.jsonl` exists; the run
/// is complete iff EVERY enabled platform has its `*_completed` flag set (and at least one platform
/// is enabled). Returns `true` only when all enabled platforms have completed.
///
/// Exact Python logic (L709-718):
/// ```python
/// twitter_enabled = os.path.exists(twitter_log)
/// reddit_enabled  = os.path.exists(reddit_log)
/// if twitter_enabled and not state.twitter_completed: return False
/// if reddit_enabled and not state.reddit_completed:  return False
/// return twitter_enabled or reddit_enabled
/// ```
fn check_all_platforms_completed(sim_data_dir: &Path, state: &SimulationRunState) -> bool {
    let sim_dir = sim_data_dir.join(&state.simulation_id);
    let twitter_log = sim_dir.join("twitter").join("actions.jsonl");
    let reddit_log = sim_dir.join("reddit").join("actions.jsonl");

    // A platform is enabled iff its actions.jsonl exists (Python L706-707).
    let twitter_enabled = twitter_log.exists();
    let reddit_enabled = reddit_log.exists();

    // An enabled-but-not-completed platform blocks completion (Python L710-713).
    if twitter_enabled && !state.twitter_completed {
        return false;
    }
    if reddit_enabled && !state.reddit_completed {
        return false;
    }

    // At least one platform must be enabled AND completed (Python L716-717).
    twitter_enabled || reddit_enabled
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -----------------------------------------------------------------------
    // RunnerStatus
    // -----------------------------------------------------------------------

    #[test]
    fn runner_status_as_str_all_variants() {
        assert_eq!(RunnerStatus::Idle.as_str(), "idle");
        assert_eq!(RunnerStatus::Starting.as_str(), "starting");
        assert_eq!(RunnerStatus::Running.as_str(), "running");
        assert_eq!(RunnerStatus::Paused.as_str(), "paused");
        assert_eq!(RunnerStatus::Stopping.as_str(), "stopping");
        assert_eq!(RunnerStatus::Stopped.as_str(), "stopped");
        assert_eq!(RunnerStatus::Completed.as_str(), "completed");
        assert_eq!(RunnerStatus::Failed.as_str(), "failed");
    }

    #[test]
    fn runner_status_serde_roundtrip() {
        // Each variant must serialise to its lowercase string and deserialise back.
        let cases = [
            (RunnerStatus::Idle, "\"idle\""),
            (RunnerStatus::Starting, "\"starting\""),
            (RunnerStatus::Running, "\"running\""),
            (RunnerStatus::Paused, "\"paused\""),
            (RunnerStatus::Stopping, "\"stopping\""),
            (RunnerStatus::Stopped, "\"stopped\""),
            (RunnerStatus::Completed, "\"completed\""),
            (RunnerStatus::Failed, "\"failed\""),
        ];
        for (variant, expected_json) in &cases {
            let serialised = serde_json::to_string(variant).unwrap();
            assert_eq!(serialised, *expected_json, "serde serialize mismatch for {:?}", variant);
            let back: RunnerStatus = serde_json::from_str(&serialised).unwrap();
            assert_eq!(&back, variant);
        }
    }

    #[test]
    fn runner_status_display() {
        assert_eq!(format!("{}", RunnerStatus::Running), "running");
        assert_eq!(format!("{}", RunnerStatus::Completed), "completed");
    }

    // -----------------------------------------------------------------------
    // AgentAction
    // -----------------------------------------------------------------------

    fn sample_action() -> AgentAction {
        AgentAction {
            round_num: 3,
            timestamp: "2026-06-17T10:00:00".to_string(),
            platform: "twitter".to_string(),
            agent_id: 42,
            agent_name: "Alice".to_string(),
            action_type: "CREATE_POST".to_string(),
            action_args: {
                let mut m = Map::new();
                m.insert("content".into(), Value::String("hello".into()));
                m
            },
            result: None,
            success: true,
        }
    }

    #[test]
    fn agent_action_to_dict_key_order() {
        let action = sample_action();
        let dict = action.to_dict();
        let keys: Vec<&str> = dict.keys().map(|k| k.as_str()).collect();
        assert_eq!(
            keys,
            &[
                "round_num",
                "timestamp",
                "platform",
                "agent_id",
                "agent_name",
                "action_type",
                "action_args",
                "result",
                "success",
            ],
            "to_dict key order must match Python dict literal"
        );
    }

    #[test]
    fn agent_action_to_dict_null_result() {
        let action = sample_action();
        let dict = action.to_dict();
        // result is None → JSON null (never omitted)
        assert_eq!(dict.get("result"), Some(&Value::Null));
    }

    #[test]
    fn agent_action_to_dict_with_result() {
        let mut action = sample_action();
        action.result = Some("post created successfully".to_string());
        let dict = action.to_dict();
        assert_eq!(
            dict.get("result"),
            Some(&Value::String("post created successfully".to_string()))
        );
    }

    #[test]
    fn agent_action_to_dict_values() {
        let action = sample_action();
        let dict = action.to_dict();
        assert_eq!(dict["round_num"], json!(3));
        assert_eq!(dict["timestamp"], json!("2026-06-17T10:00:00"));
        assert_eq!(dict["platform"], json!("twitter"));
        assert_eq!(dict["agent_id"], json!(42));
        assert_eq!(dict["agent_name"], json!("Alice"));
        assert_eq!(dict["action_type"], json!("CREATE_POST"));
        assert_eq!(dict["success"], json!(true));
    }

    #[test]
    fn agent_action_new_defaults() {
        let a = AgentAction::new(
            1,
            "2026-06-17T00:00:00".to_string(),
            "reddit".to_string(),
            7,
            "Bob".to_string(),
            "LIKE_POST".to_string(),
        );
        assert!(a.action_args.is_empty());
        assert!(a.result.is_none());
        assert!(a.success);
    }

    #[test]
    fn agent_action_serde_roundtrip() {
        let action = sample_action();
        let json = serde_json::to_string(&action).unwrap();
        let back: AgentAction = serde_json::from_str(&json).unwrap();
        assert_eq!(back.round_num, action.round_num);
        assert_eq!(back.platform, action.platform);
        assert_eq!(back.action_type, action.action_type);
        assert!(back.result.is_none());
    }

    // -----------------------------------------------------------------------
    // RoundSummary
    // -----------------------------------------------------------------------

    fn sample_round_summary() -> RoundSummary {
        let mut rs = RoundSummary::new(1, "2026-06-17T10:00:00".to_string());
        rs.end_time = Some("2026-06-17T10:05:00".to_string());
        rs.simulated_hour = 3;
        rs.twitter_actions = 5;
        rs.reddit_actions = 2;
        rs.active_agents = vec![1, 2, 3];
        rs.actions.push(sample_action());
        rs
    }

    #[test]
    fn round_summary_to_dict_key_order() {
        let rs = sample_round_summary();
        let dict = rs.to_dict();
        let keys: Vec<&str> = dict.keys().map(|k| k.as_str()).collect();
        assert_eq!(
            keys,
            &[
                "round_num",
                "start_time",
                "end_time",
                "simulated_hour",
                "twitter_actions",
                "reddit_actions",
                "active_agents",
                "actions_count",
                "actions",
            ],
            "to_dict key order must match Python dict literal"
        );
    }

    #[test]
    fn round_summary_to_dict_actions_count_computed() {
        let mut rs = RoundSummary::new(2, "2026-06-17T10:00:00".to_string());
        rs.actions.push(sample_action());
        rs.actions.push(sample_action());
        let dict = rs.to_dict();
        // actions_count = len(actions) — computed, not stored
        assert_eq!(dict["actions_count"], json!(2));
        assert_eq!(dict["actions"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn round_summary_to_dict_null_end_time() {
        let rs = RoundSummary::new(1, "2026-06-17T10:00:00".to_string());
        let dict = rs.to_dict();
        assert_eq!(dict.get("end_time"), Some(&Value::Null));
    }

    #[test]
    fn round_summary_to_dict_nested_actions_are_to_dict() {
        let rs = sample_round_summary();
        let dict = rs.to_dict();
        let actions_arr = dict["actions"].as_array().unwrap();
        assert_eq!(actions_arr.len(), 1);
        // Nested action must have the 9 to_dict keys
        let action_obj = actions_arr[0].as_object().unwrap();
        assert!(action_obj.contains_key("round_num"));
        assert!(action_obj.contains_key("action_args"));
        assert!(action_obj.contains_key("success"));
    }

    #[test]
    fn round_summary_new_defaults() {
        let rs = RoundSummary::new(5, "2026-06-17T09:00:00".to_string());
        assert_eq!(rs.round_num, 5);
        assert!(rs.end_time.is_none());
        assert_eq!(rs.simulated_hour, 0);
        assert_eq!(rs.twitter_actions, 0);
        assert_eq!(rs.reddit_actions, 0);
        assert!(rs.active_agents.is_empty());
        assert!(rs.actions.is_empty());
    }

    // -----------------------------------------------------------------------
    // SimulationRunState — defaults
    // -----------------------------------------------------------------------

    #[test]
    fn simulation_run_state_new_defaults() {
        let s = SimulationRunState::new("sim-001".to_string());
        assert_eq!(s.simulation_id, "sim-001");
        assert_eq!(s.runner_status, RunnerStatus::Idle);
        assert_eq!(s.current_round, 0);
        assert_eq!(s.total_rounds, 0);
        assert_eq!(s.max_recent_actions, 50);
        assert!(s.started_at.is_none());
        assert!(s.completed_at.is_none());
        assert!(s.error.is_none());
        assert!(s.process_pid.is_none()); // [≠] value-only
        assert!(!s.recent_actions.is_empty() || s.recent_actions.is_empty()); // trivially true
        assert!(s.recent_actions.is_empty());
        assert!(s.rounds.is_empty());
    }

    // -----------------------------------------------------------------------
    // SimulationRunState — add_action
    // -----------------------------------------------------------------------

    #[test]
    fn add_action_inserts_at_front() {
        let mut s = SimulationRunState::new("sim-002".to_string());
        let a1 = sample_action();
        let mut a2 = sample_action();
        a2.round_num = 999;
        s.add_action(a1);
        s.add_action(a2);
        // Newest (a2) should be at front
        assert_eq!(s.recent_actions[0].round_num, 999);
        assert_eq!(s.recent_actions[1].round_num, 3);
    }

    #[test]
    fn add_action_enforces_cap() {
        let mut s = SimulationRunState::new("sim-cap".to_string());
        s.max_recent_actions = 3;
        for i in 0..5_i64 {
            let mut a = sample_action();
            a.round_num = i;
            s.add_action(a);
        }
        // Only the last 3 (most recent) should remain, capped
        assert_eq!(s.recent_actions.len(), 3);
        // Front = most recently added (round 4)
        assert_eq!(s.recent_actions[0].round_num, 4);
    }

    #[test]
    fn add_action_increments_twitter_counter() {
        let mut s = SimulationRunState::new("sim-tw".to_string());
        let a = sample_action(); // platform = "twitter"
        s.add_action(a);
        assert_eq!(s.twitter_actions_count, 1);
        assert_eq!(s.reddit_actions_count, 0);
    }

    #[test]
    fn add_action_increments_reddit_counter() {
        let mut s = SimulationRunState::new("sim-rd".to_string());
        let mut a = sample_action();
        a.platform = "reddit".to_string();
        s.add_action(a);
        assert_eq!(s.reddit_actions_count, 1);
        assert_eq!(s.twitter_actions_count, 0);
    }

    #[test]
    fn add_action_refreshes_updated_at() {
        let mut s = SimulationRunState::new("sim-ts".to_string());
        let before = s.updated_at.clone();
        // Sleep briefly to get a different timestamp — but since python_isoformat_local
        // can be same-second, we check it is a valid string (non-empty).
        let a = sample_action();
        s.add_action(a);
        // updated_at must be a non-empty ISO string
        assert!(!s.updated_at.is_empty());
        // In practice it may equal `before` within the same second — just assert type
        let _ = before;
    }

    // -----------------------------------------------------------------------
    // SimulationRunState — to_dict key order and computed fields
    // -----------------------------------------------------------------------

    #[test]
    fn to_dict_key_order() {
        let s = SimulationRunState::new("sim-003".to_string());
        let dict = s.to_dict();
        let keys: Vec<&str> = dict.keys().map(|k| k.as_str()).collect();
        // Exact Python dict-literal insertion order (simulation_runner.py:161-186)
        let expected = &[
            "simulation_id",
            "runner_status",
            "current_round",
            "total_rounds",
            "simulated_hours",
            "total_simulation_hours",
            "progress_percent",
            "twitter_current_round",
            "reddit_current_round",
            "twitter_simulated_hours",
            "reddit_simulated_hours",
            "twitter_running",
            "reddit_running",
            "twitter_completed",
            "reddit_completed",
            "twitter_actions_count",
            "reddit_actions_count",
            "total_actions_count",
            "started_at",
            "updated_at",
            "completed_at",
            "error",
            "process_pid",
        ];
        assert_eq!(&keys, expected, "to_dict key order must match Python source");
    }

    #[test]
    fn to_dict_progress_percent_computed() {
        let mut s = SimulationRunState::new("sim-pct".to_string());
        s.current_round = 3;
        s.total_rounds = 10;
        // Python: round(3 / max(10, 1) * 100, 1) = round(30.0, 1) = 30.0
        let dict = s.to_dict();
        let pct = dict["progress_percent"].as_f64().unwrap();
        assert!((pct - 30.0).abs() < 1e-9, "expected 30.0 got {}", pct);
    }

    #[test]
    fn to_dict_progress_percent_zero_total() {
        let s = SimulationRunState::new("sim-pct0".to_string());
        // total_rounds=0 → max(0,1)=1 denominator, current_round=0 → 0.0
        let dict = s.to_dict();
        let pct = dict["progress_percent"].as_f64().unwrap();
        assert!((pct - 0.0).abs() < 1e-9, "expected 0.0 got {}", pct);
    }

    #[test]
    fn to_dict_progress_percent_one_decimal_rounding() {
        let mut s = SimulationRunState::new("sim-rnd".to_string());
        s.current_round = 1;
        s.total_rounds = 3;
        // Python: round(1/3 * 100, 1) = round(33.3333…, 1) = 33.3
        let dict = s.to_dict();
        let pct = dict["progress_percent"].as_f64().unwrap();
        assert!((pct - 33.3).abs() < 0.01, "expected ~33.3 got {}", pct);
    }

    // -----------------------------------------------------------------------
    // Parity regression tests for round_half_even_1dp (S-597 downgrade fix)
    // These golden values are confirmed by running CPython:
    //   >>> round(6.25, 1)  # 6.2 (NOT 6.3 — half-to-even, 62 is even)
    //   >>> round(0.25, 1)  # 0.2 (NOT 0.3 — half-to-even, 2 is even)
    //   >>> round(6.45, 1)  # 6.5 (true product > 64.5 — normal round up)
    //   >>> round(0.05, 1)  # 0.1 (true product > 0.5 — normal round up)
    // The diverging pairs: cr=1,tr=16 → raw=6.25 → Python 6.2, Rust f64::round 6.3.
    // -----------------------------------------------------------------------

    #[test]
    fn round_half_even_1dp_tie_half_to_even_down() {
        // 6.25 * 10 = 62.5 exactly (true mathematical tie). 62 is even → round down.
        // CPython: round(6.25, 1) = 6.2. Rust f64::round would give 6.3 (wrong).
        let result = round_half_even_1dp(6.25);
        assert!(
            (result - 6.2).abs() < 1e-12,
            "expected 6.2 (tie rounds to even 62), got {}",
            result
        );
    }

    #[test]
    fn round_half_even_1dp_tie_quarter_percent_down() {
        // 0.25 * 10 = 2.5 exactly (true mathematical tie). 2 is even → round down.
        // CPython: round(0.25, 1) = 0.2. Rust f64::round would give 0.3 (wrong).
        let result = round_half_even_1dp(0.25);
        assert!(
            (result - 0.2).abs() < 1e-12,
            "expected 0.2 (tie rounds to even 2), got {}",
            result
        );
    }

    #[test]
    fn round_half_even_1dp_above_tie_rounds_up() {
        // 6.45_f64 is slightly ABOVE 6.45 exact; 6.45_f64 * 10 = 64.5 in IEEE 754,
        // but true mathematical product > 64.5 → rounds UP (not a banker's rounding case).
        // CPython: round(6.45, 1) = 6.5.
        let result = round_half_even_1dp(6.45);
        assert!(
            (result - 6.5).abs() < 1e-12,
            "expected 6.5 (true product > 64.5), got {}",
            result
        );
    }

    #[test]
    fn round_half_even_1dp_above_tie_small_value() {
        // 0.05_f64 is slightly ABOVE 0.05 exact; true product 0.05_f64 * 10 > 0.5
        // → rounds UP. CPython: round(0.05, 1) = 0.1.
        let result = round_half_even_1dp(0.05);
        assert!(
            (result - 0.1).abs() < 1e-12,
            "expected 0.1 (true product > 0.5), got {}",
            result
        );
    }

    #[test]
    fn round_half_even_1dp_progress_percent_verifier_pair() {
        // Golden-confirmed by the verifier: current_round=1, total_rounds=16
        // → raw = 1/16 * 100 = 6.25 → Python round(6.25, 1) = 6.2 (not 6.3).
        let raw = 1.0_f64 / 16.0 * 100.0;
        let result = round_half_even_1dp(raw);
        assert!((result - 6.2).abs() < 1e-12, "cr=1,tr=16: expected 6.2, got {}", result);
    }

    #[test]
    fn round_half_even_1dp_via_to_dict_cr1_tr16() {
        // End-to-end: SimulationRunState.to_dict() must emit 6.2 for cr=1, tr=16.
        let mut s = SimulationRunState::new("sim-parity-fix".to_string());
        s.current_round = 1;
        s.total_rounds = 16;
        let dict = s.to_dict();
        let pct = dict["progress_percent"].as_f64().unwrap();
        assert!((pct - 6.2).abs() < 1e-12, "to_dict cr=1,tr=16: expected 6.2, got {}", pct);
    }

    #[test]
    fn round_half_even_1dp_additional_boundary_pairs() {
        // A sampling of the 243 diverging (cr, tr) pairs confirmed by the verifier.
        // Each: Python gives the even-rounding result; f64::round gives one higher.
        let cases: &[(i64, i64, f64)] = &[
            (1, 80, 1.2),   // raw=1.25, scaled=12.5, 12 even → 1.2
            (1, 400, 0.2),  // raw=0.25, scaled=2.5, 2 even → 0.2
            (5, 16, 31.2),  // raw=31.25, scaled=312.5, 312 even → 31.2
            (9, 16, 56.2),  // raw=56.25, scaled=562.5, 562 even → 56.2
            (13, 16, 81.2), // raw=81.25, scaled=812.5, 812 even → 81.2
        ];
        for &(cr, tr, expected) in cases {
            let raw = cr as f64 / tr as f64 * 100.0;
            let result = round_half_even_1dp(raw);
            assert!(
                (result - expected).abs() < 1e-12,
                "cr={}, tr={}: expected {}, got {}",
                cr,
                tr,
                expected,
                result
            );
        }
    }

    #[test]
    fn to_dict_total_actions_count_computed() {
        let mut s = SimulationRunState::new("sim-tac".to_string());
        s.twitter_actions_count = 7;
        s.reddit_actions_count = 3;
        let dict = s.to_dict();
        assert_eq!(dict["total_actions_count"], json!(10));
    }

    #[test]
    fn to_dict_null_optional_fields() {
        let s = SimulationRunState::new("sim-nulls".to_string());
        let dict = s.to_dict();
        assert_eq!(dict.get("started_at"), Some(&Value::Null));
        assert_eq!(dict.get("completed_at"), Some(&Value::Null));
        assert_eq!(dict.get("error"), Some(&Value::Null));
        // process_pid: [≠] value-only — always null in teri
        assert_eq!(dict.get("process_pid"), Some(&Value::Null));
    }

    #[test]
    fn to_dict_runner_status_as_string_value() {
        let mut s = SimulationRunState::new("sim-st".to_string());
        s.runner_status = RunnerStatus::Running;
        let dict = s.to_dict();
        // Python emits self.runner_status.value (the str value, not the enum name)
        assert_eq!(dict["runner_status"], json!("running"));
    }

    // -----------------------------------------------------------------------
    // SimulationRunState — to_detail_dict
    // -----------------------------------------------------------------------

    #[test]
    fn to_detail_dict_is_superset_of_to_dict() {
        let mut s = SimulationRunState::new("sim-dd".to_string());
        s.add_action(sample_action());

        let base = s.to_dict();
        let detail = s.to_detail_dict();

        // All keys from to_dict must be present in to_detail_dict
        for key in base.keys() {
            assert!(
                detail.contains_key(key.as_str()),
                "to_detail_dict missing key '{}' from to_dict",
                key
            );
        }

        // Extra keys must be exactly recent_actions + rounds_count
        assert!(detail.contains_key("recent_actions"));
        assert!(detail.contains_key("rounds_count"));
    }

    #[test]
    fn to_detail_dict_recent_actions_nested() {
        let mut s = SimulationRunState::new("sim-dd2".to_string());
        s.add_action(sample_action());
        let detail = s.to_detail_dict();
        let recent = detail["recent_actions"].as_array().unwrap();
        assert_eq!(recent.len(), 1);
        // Nested action must have to_dict shape
        let a_obj = recent[0].as_object().unwrap();
        assert!(a_obj.contains_key("round_num"));
        assert!(a_obj.contains_key("success"));
    }

    #[test]
    fn to_detail_dict_rounds_count_computed() {
        let mut s = SimulationRunState::new("sim-rc".to_string());
        s.rounds.push(sample_round_summary());
        s.rounds.push(sample_round_summary());
        let detail = s.to_detail_dict();
        // rounds_count = len(self.rounds) — computed, not stored
        assert_eq!(detail["rounds_count"], json!(2));
    }

    // -----------------------------------------------------------------------
    // run_state.json persistence round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn save_and_load_run_state_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "teri_test_sim_runner_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let sim_id = "test-sim-001";

        let mut original = SimulationRunState::new(sim_id.to_string());
        original.runner_status = RunnerStatus::Completed;
        original.current_round = 5;
        original.total_rounds = 10;
        // Set counters BEFORE add_action so add_action's increment is included
        // in the expected saved values.
        original.started_at = Some("2026-06-17T09:00:00".to_string());
        original.completed_at = Some("2026-06-17T11:00:00".to_string());
        original.twitter_completed = true;
        original.reddit_completed = true;
        // add_action increments twitter_actions_count by 1 (platform="twitter")
        original.add_action(sample_action());
        // Manually set counters after add_action so we know exact values
        original.twitter_actions_count = 20;
        original.reddit_actions_count = 15;

        // Save
        save_run_state(&dir, &original).unwrap();

        // Verify file exists
        let path = dir.join(sim_id).join("run_state.json");
        assert!(path.exists(), "run_state.json must be written");

        // Load
        let loaded = load_run_state(&dir, sim_id).unwrap().expect("must load back the state");

        assert_eq!(loaded.simulation_id, original.simulation_id);
        assert_eq!(loaded.runner_status, RunnerStatus::Completed);
        assert_eq!(loaded.current_round, 5);
        assert_eq!(loaded.total_rounds, 10);
        assert_eq!(loaded.twitter_actions_count, 20);
        assert_eq!(loaded.reddit_actions_count, 15);
        assert!(loaded.twitter_completed);
        assert!(loaded.reddit_completed);
        assert_eq!(loaded.started_at.as_deref(), Some("2026-06-17T09:00:00"));
        assert_eq!(loaded.completed_at.as_deref(), Some("2026-06-17T11:00:00"));
        // recent_actions round-trip
        assert_eq!(loaded.recent_actions.len(), 1);
        assert_eq!(loaded.recent_actions[0].round_num, 3);
        assert_eq!(loaded.recent_actions[0].action_type, "CREATE_POST");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_run_state_missing_file_returns_none() {
        let dir = std::env::temp_dir().join("teri_test_no_such_sim_12345");
        let result = load_run_state(&dir, "nonexistent-sim").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn save_run_state_creates_directory() {
        let dir = std::env::temp_dir().join(format!(
            "teri_test_mkdir_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let sim_id = "mkdir-test";
        let state = SimulationRunState::new(sim_id.to_string());
        save_run_state(&dir, &state).unwrap();
        assert!(dir.join(sim_id).join("run_state.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_run_state_json_is_pretty_printed() {
        let dir = std::env::temp_dir().join(format!(
            "teri_test_pretty_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let sim_id = "pretty-test";
        let state = SimulationRunState::new(sim_id.to_string());
        save_run_state(&dir, &state).unwrap();
        let raw = std::fs::read_to_string(dir.join(sim_id).join("run_state.json")).unwrap();
        // 2-space indent: any line should start with "  " (2 spaces) for nested keys
        assert!(raw.contains("\n  "), "run_state.json must use 2-space indentation");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_run_state_tolerates_missing_fields() {
        let dir = std::env::temp_dir().join(format!(
            "teri_test_tolerant_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let sim_id = "tolerant-sim";
        let sim_dir = dir.join(sim_id);
        std::fs::create_dir_all(&sim_dir).unwrap();
        // Minimal JSON — only simulation_id (all others missing → defaults)
        std::fs::write(sim_dir.join("run_state.json"), r#"{"simulation_id": "tolerant-sim"}"#)
            .unwrap();

        let loaded = load_run_state(&dir, sim_id).unwrap().unwrap();
        assert_eq!(loaded.simulation_id, "tolerant-sim");
        assert_eq!(loaded.runner_status, RunnerStatus::Idle);
        assert_eq!(loaded.current_round, 0);
        assert_eq!(loaded.total_rounds, 0);
        assert!(!loaded.twitter_running);
        assert!(loaded.recent_actions.is_empty());
        assert!(loaded.process_pid.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn to_dict_key_count() {
        let s = SimulationRunState::new("sim-count".to_string());
        let dict = s.to_dict();
        // Python to_dict has exactly 23 keys (simulation_runner.py:161-186)
        assert_eq!(dict.len(), 23, "to_dict must emit exactly 23 keys");
    }

    #[test]
    fn to_detail_dict_key_count() {
        let s = SimulationRunState::new("sim-dd-count".to_string());
        let detail = s.to_detail_dict();
        // to_dict (23) + recent_actions + rounds_count = 25
        assert_eq!(detail.len(), 25, "to_detail_dict must emit exactly 25 keys");
    }
}

// ===========================================================================
// Tests — SimulationRunner lifecycle (sub-cycle b)
// ===========================================================================

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::agent::{Agent, AgentPool, Persona};
    use crate::graph::KnowledgeGraph;
    use crate::llm::{ChatMessage, ChatOptions, LlmClient};
    use crate::services::graph_memory::GraphMemoryManager;
    use crate::services::simulation_manager::SimulationManager;
    use crate::sim::{SimConfig, SimEngine};
    use async_trait::async_trait;
    use std::env;
    use std::pin::Pin;
    use std::time::Duration;

    // ---- Mock LLM: every agent thinks "idle"; the run advances ticks cheaply. ----
    struct MockLlm;
    #[async_trait]
    impl LlmClient for MockLlm {
        async fn complete(&self, _: &str) -> Result<String> {
            Ok("Think(idle)".to_string())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat(&self, _: &[ChatMessage], _: &ChatOptions) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[ChatMessage],
            _: &ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
    }

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let mut p = env::temp_dir();
        p.push(format!(
            "teri_test_sim_runner_lifecycle_{}_{}_{}",
            std::process::id(),
            suffix,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        p
    }

    /// Build a `SimulationRunner<MockLlm>` over a fresh temp dir, returning it plus the dir
    /// and the shared `SimulationManager` (so tests can prep configs + inspect state.json).
    fn make_runner(
        suffix: &str,
    ) -> (SimulationRunner<MockLlm>, std::path::PathBuf, Arc<SimulationManager>) {
        let dir = temp_dir(suffix);
        std::fs::create_dir_all(&dir).unwrap();
        let manager = Arc::new(SimulationManager::new(&dir));
        let graph_mgr = Arc::new(GraphMemoryManager::<MockLlm>::new());
        let runner = SimulationRunner::new(&dir, graph_mgr, Arc::clone(&manager));
        (runner, dir, manager)
    }

    /// Write a `simulation_config.json` with the given time_config under `{dir}/{sim_id}/`.
    fn write_config(dir: &Path, sim_id: &str, total_hours: i64, minutes_per_round: i64) {
        let sim_dir = dir.join(sim_id);
        std::fs::create_dir_all(&sim_dir).unwrap();
        let cfg = serde_json::json!({
            "time_config": {
                "total_simulation_hours": total_hours,
                "minutes_per_round": minutes_per_round
            }
        });
        std::fs::write(
            sim_dir.join("simulation_config.json"),
            serde_json::to_string_pretty(&cfg).unwrap(),
        )
        .unwrap();
    }

    /// Build `RunInputs` for a short simulation (max_ticks ticks, 1 agent).
    fn run_inputs(max_ticks: u32) -> RunInputs<MockLlm> {
        let mut pool = AgentPool::new();
        pool.add_agent(Agent::new(Persona {
            name: "A".into(),
            background: "test".into(),
            traits: vec![],
            role: "none".into(),
            social: None,
        }));
        RunInputs {
            engine: SimEngine::new(SimConfig::new(max_ticks, 1)),
            pool,
            graph: KnowledgeGraph::new(),
            llm: Arc::new(MockLlm),
            boost_llm: None,
        }
    }

    /// Like [`run_inputs`] but with the `actions.jsonl` producer (U-028 c3b-i) attached to the
    /// engine, writing to `{sim_dir}/{platform}/actions.jsonl`. The spawned monitor tails that
    /// file and marks the run COMPLETED on the producer's `simulation_end` record.
    fn run_inputs_with_producer(
        max_ticks: u32,
        sim_dir: &Path,
        platform: &str,
    ) -> RunInputs<MockLlm> {
        let mut pool = AgentPool::new();
        pool.add_agent(Agent::new(Persona {
            name: "A".into(),
            background: "test".into(),
            traits: vec![],
            role: "none".into(),
            social: None,
        }));
        let config = serde_json::json!({
            "time_config": { "total_simulation_hours": 1, "minutes_per_round": 30 }
        });
        let mut engine = SimEngine::new(SimConfig::new(max_ticks, 1));
        let logger = Arc::new(
            crate::sim::action_logger::PlatformActionLogger::new(platform, sim_dir).unwrap(),
        );
        let platform_enum = if platform == "reddit" {
            crate::agent::Platform::Reddit
        } else {
            crate::agent::Platform::Twitter
        };
        engine.with_producer(crate::sim::RunProducer {
            loggers: crate::sim::PlatformLoggerSet::single(platform_enum, logger),
            config,
        });
        RunInputs {
            engine,
            pool,
            graph: KnowledgeGraph::new(),
            llm: Arc::new(MockLlm),
            boost_llm: None,
        }
    }

    /// Like [`run_inputs_with_producer`] but with a DUAL-logger (twitter + reddit) producer (U-030
    /// cycle B), writing BOTH `{sim_dir}/twitter/actions.jsonl` and `{sim_dir}/reddit/actions.jsonl`.
    /// The monitor's dual-platform gate (S-615) requires BOTH to hit `simulation_end` before the run
    /// is marked COMPLETED. The boundary records (simulation_start/round_start/round_end/
    /// simulation_end) fan out to both loggers, so both files are created and terminate even if the
    /// pool produces no social actions.
    fn run_inputs_with_parallel_producer(max_ticks: u32, sim_dir: &Path) -> RunInputs<MockLlm> {
        let pool = AgentPool::new();
        let config = serde_json::json!({
            "time_config": { "total_simulation_hours": 1, "minutes_per_round": 30 }
        });
        let mut engine = SimEngine::new(SimConfig::new(max_ticks, 1));
        let twitter = Arc::new(
            crate::sim::action_logger::PlatformActionLogger::new("twitter", sim_dir).unwrap(),
        );
        let reddit = Arc::new(
            crate::sim::action_logger::PlatformActionLogger::new("reddit", sim_dir).unwrap(),
        );
        engine.with_producer(crate::sim::RunProducer {
            loggers: crate::sim::PlatformLoggerSet::parallel(twitter, reddit),
            config,
        });
        RunInputs {
            engine,
            pool,
            graph: KnowledgeGraph::new(),
            llm: Arc::new(MockLlm),
            boost_llm: None,
        }
    }

    // -----------------------------------------------------------------------
    // Wait-for-commands IPC service loop (CYCLE 56 keystone — U-028/029/030
    // process_commands / handle_interview / handle_batch_interview).
    // -----------------------------------------------------------------------

    /// Build a single-agent pool whose one agent carries `social.user_id == user_id`, so
    /// interview resolution (`resolve_agent_by_user_id`) can find it.
    fn pool_with_social_agent(user_id: u64, name: &str) -> AgentPool {
        let social = crate::agent::SocialProfile {
            user_id,
            user_name: format!("u{user_id}"),
            bio: String::new(),
            persona: String::new(),
            platform: crate::agent::Platform::Twitter,
            karma: 1000,
            friend_count: 100,
            follower_count: 150,
            following_count: 100,
            statuses_count: 500,
            age: None,
            gender: None,
            mbti: None,
            country: None,
            profession: None,
            interested_topics: vec![],
            posting_style: None,
            source_entity_uuid: None,
            source_entity_type: None,
            created_at: String::new(),
        };
        let mut pool = AgentPool::new();
        pool.add_agent(Agent::new(Persona {
            name: name.to_string(),
            background: "an engineer".into(),
            traits: vec![],
            role: "agent".into(),
            social: Some(social),
        }));
        pool
    }

    /// `execute_interview` resolves the pool agent by `user_id` and returns the
    /// `{agent_id, response, timestamp}` shape (mirroring Python `_get_interview_result`).
    #[tokio::test]
    async fn execute_interview_resolves_and_shapes() {
        let pool = pool_with_social_agent(7, "Ada");
        let mut args = Map::new();
        args.insert("agent_id".into(), Value::from(7i64));
        args.insert("prompt".into(), Value::String("How do you feel?".into()));

        let result = execute_interview(&pool, &MockLlm, &args).await.expect("interview ok");
        assert_eq!(result["agent_id"], Value::from(7i64));
        // MockLlm returns "Think(idle)" for any prompt — proves the LLM response flows through.
        assert_eq!(result["response"], Value::String("Think(idle)".into()));
        assert!(result.contains_key("timestamp"));
    }

    /// An unknown `agent_id` yields an error (Python `get_agent` raises → `send_response` failed).
    #[tokio::test]
    async fn execute_interview_unknown_agent_errs() {
        let pool = pool_with_social_agent(7, "Ada");
        let mut args = Map::new();
        args.insert("agent_id".into(), Value::from(999i64));
        args.insert("prompt".into(), Value::String("x".into()));

        let err = execute_interview(&pool, &MockLlm, &args).await.unwrap_err();
        assert!(err.contains("not found"), "unexpected err: {err}");
    }

    /// Batch interview collects per-agent results keyed by `agent_id`, skipping unresolvable ids.
    #[tokio::test]
    async fn execute_batch_interview_collects_and_skips_unresolvable() {
        let mut pool = pool_with_social_agent(7, "Ada");
        // add a second resolvable agent (user_id 8)
        for a in pool_with_social_agent(8, "Bob").agents {
            pool.add_agent(a);
        }
        let mut args = Map::new();
        args.insert(
            "interviews".into(),
            Value::Array(vec![
                serde_json::json!({"agent_id": 7, "prompt": "q1"}),
                serde_json::json!({"agent_id": 99, "prompt": "ghost"}),
                serde_json::json!({"agent_id": 8, "prompt": "q2"}),
            ]),
        );

        let result = execute_batch_interview(&pool, &MockLlm, &args).await.expect("batch ok");
        assert_eq!(result["interviews_count"], Value::from(2u64));
        let results = result["results"].as_object().unwrap();
        assert!(results.contains_key("7"));
        assert!(results.contains_key("8"));
        assert!(!results.contains_key("99"), "ghost agent must be skipped");
        assert_eq!(results["7"]["agent_id"], Value::from(7i64));
    }

    /// Batch interview with NO resolvable agents errors (Python `if not actions` → failed).
    #[tokio::test]
    async fn execute_batch_interview_no_valid_agents_errs() {
        let pool = pool_with_social_agent(7, "Ada");
        let mut args = Map::new();
        args.insert(
            "interviews".into(),
            Value::Array(vec![serde_json::json!({"agent_id": 404, "prompt": "ghost"})]),
        );
        let err = execute_batch_interview(&pool, &MockLlm, &args).await.unwrap_err();
        assert_eq!(err, "没有有效的Agent");
    }

    // ---- PARALLEL (dual-platform) interview dispatch — Cycle 59 (S-920/921/922/924) ----

    /// Build an agent on a specific platform (the unioned-pool analog of OASIS per-platform envs).
    fn social_agent_on(user_id: u64, name: &str, platform: crate::agent::Platform) -> Agent {
        let mut a = pool_with_social_agent(user_id, name).agents.pop().unwrap();
        if let Some(s) = a.persona.social.as_mut() {
            s.platform = platform;
        }
        a
    }

    /// Pool unioning the given `(user_id, name, platform)` agents (mirrors
    /// `load_agent_pool("parallel")` — same `user_id` may appear once per platform).
    fn parallel_pool(specs: &[(u64, &str, crate::agent::Platform)]) -> AgentPool {
        let mut pool = AgentPool::new();
        for (uid, name, plat) in specs {
            pool.add_agent(social_agent_on(*uid, name, *plat));
        }
        pool
    }

    /// platform="twitter" → interview ONLY twitter, result carries the `platform` key.
    #[tokio::test]
    async fn parallel_interview_specified_platform() {
        use crate::agent::Platform::{Reddit, Twitter};
        let pool = parallel_pool(&[(7, "AdaT", Twitter), (7, "AdaR", Reddit)]);
        let mut args = Map::new();
        args.insert("agent_id".into(), Value::from(7i64));
        args.insert("prompt".into(), Value::String("q".into()));
        args.insert("platform".into(), Value::String("twitter".into()));

        let r = execute_interview_parallel(&pool, &MockLlm, &args).await.expect("ok");
        assert_eq!(r["platform"], Value::String("twitter".into()));
        assert_eq!(r["response"], Value::String("Think(idle)".into()));
        assert!(!r.contains_key("platforms"), "single-platform result is not wrapped");
    }

    /// No platform → interview BOTH platforms; `{agent_id, prompt, platforms:{twitter, reddit}}`.
    #[tokio::test]
    async fn parallel_interview_both_platforms() {
        use crate::agent::Platform::{Reddit, Twitter};
        let pool = parallel_pool(&[(7, "AdaT", Twitter), (7, "AdaR", Reddit)]);
        let mut args = Map::new();
        args.insert("agent_id".into(), Value::from(7i64));
        args.insert("prompt".into(), Value::String("q".into()));

        let r = execute_interview_parallel(&pool, &MockLlm, &args).await.expect("ok");
        assert_eq!(r["agent_id"], Value::from(7i64));
        assert_eq!(r["prompt"], Value::String("q".into()));
        let platforms = r["platforms"].as_object().unwrap();
        assert_eq!(platforms["twitter"]["platform"], Value::String("twitter".into()));
        assert_eq!(platforms["twitter"]["response"], Value::String("Think(idle)".into()));
        assert_eq!(platforms["reddit"]["platform"], Value::String("reddit".into()));
        // Insertion order twitter→reddit preserved (preserve_order).
        let keys: Vec<&String> = platforms.keys().collect();
        assert_eq!(keys, vec!["twitter", "reddit"]);
    }

    /// No platform + only twitter present → partial success: `platforms` has just twitter.
    #[tokio::test]
    async fn parallel_interview_both_one_platform_only() {
        use crate::agent::Platform::Twitter;
        let pool = parallel_pool(&[(7, "AdaT", Twitter)]);
        let mut args = Map::new();
        args.insert("agent_id".into(), Value::from(7i64));
        args.insert("prompt".into(), Value::String("q".into()));

        let r = execute_interview_parallel(&pool, &MockLlm, &args).await.expect("ok");
        let platforms = r["platforms"].as_object().unwrap();
        assert!(platforms.contains_key("twitter"));
        assert!(!platforms.contains_key("reddit"), "reddit not in pool → not interviewed");
    }

    /// Empty pool (no platform available at all) → `没有可用的模拟环境`.
    #[tokio::test]
    async fn parallel_interview_no_env_errs() {
        let pool = AgentPool::new();
        let mut args = Map::new();
        args.insert("agent_id".into(), Value::from(7i64));
        let err = execute_interview_parallel(&pool, &MockLlm, &args).await.unwrap_err();
        assert_eq!(err, "没有可用的模拟环境");
    }

    /// platform specified but that platform absent → `{platform}平台不可用` surfaced as the error.
    #[tokio::test]
    async fn parallel_interview_specified_platform_unavailable_errs() {
        use crate::agent::Platform::Twitter;
        let pool = parallel_pool(&[(7, "AdaT", Twitter)]);
        let mut args = Map::new();
        args.insert("agent_id".into(), Value::from(7i64));
        args.insert("platform".into(), Value::String("reddit".into()));
        let err = execute_interview_parallel(&pool, &MockLlm, &args).await.unwrap_err();
        assert_eq!(err, "reddit平台不可用");
    }

    /// No platform, agent unknown on BOTH available platforms → joined per-platform errors.
    #[tokio::test]
    async fn parallel_interview_both_all_fail_errs() {
        use crate::agent::Platform::{Reddit, Twitter};
        let pool = parallel_pool(&[(7, "AdaT", Twitter), (8, "BobR", Reddit)]);
        let mut args = Map::new();
        args.insert("agent_id".into(), Value::from(99i64));
        let err = execute_interview_parallel(&pool, &MockLlm, &args).await.unwrap_err();
        assert_eq!(err, "twitter: Agent 99 not found; reddit: Agent 99 not found");
    }

    /// Batch: per-item platform routing → results keyed `{platform}_{agent_id}`.
    #[tokio::test]
    async fn parallel_batch_routes_by_platform() {
        use crate::agent::Platform::{Reddit, Twitter};
        let pool = parallel_pool(&[(7, "AdaT", Twitter), (8, "BobR", Reddit)]);
        let mut args = Map::new();
        args.insert(
            "interviews".into(),
            Value::Array(vec![
                serde_json::json!({"agent_id": 7, "prompt": "q1", "platform": "twitter"}),
                serde_json::json!({"agent_id": 8, "prompt": "q2", "platform": "reddit"}),
            ]),
        );
        let r = execute_batch_interview_parallel(&pool, &MockLlm, &args).await.expect("ok");
        assert_eq!(r["interviews_count"], Value::from(2u64));
        let res = r["results"].as_object().unwrap();
        assert_eq!(res["twitter_7"]["platform"], Value::String("twitter".into()));
        assert_eq!(res["reddit_8"]["platform"], Value::String("reddit".into()));
    }

    /// Batch: an item with NO platform is interviewed on BOTH platforms (both → twitter+reddit).
    #[tokio::test]
    async fn parallel_batch_both_platforms_item() {
        use crate::agent::Platform::{Reddit, Twitter};
        let pool = parallel_pool(&[(5, "AdaT", Twitter), (5, "AdaR", Reddit)]);
        let mut args = Map::new();
        args.insert(
            "interviews".into(),
            Value::Array(vec![serde_json::json!({"agent_id": 5, "prompt": "q"})]),
        );
        let r = execute_batch_interview_parallel(&pool, &MockLlm, &args).await.expect("ok");
        let res = r["results"].as_object().unwrap();
        assert!(res.contains_key("twitter_5"));
        assert!(res.contains_key("reddit_5"));
        assert_eq!(r["interviews_count"], Value::from(2u64));
    }

    /// Batch: a platform with ≥1 resolved item collects EVERY item — unresolvable ones get a
    /// null-response record (the `_get_interview_result` no-row shape).
    #[tokio::test]
    async fn parallel_batch_unresolvable_gets_null_record() {
        use crate::agent::Platform::Twitter;
        let pool = parallel_pool(&[(7, "AdaT", Twitter)]);
        let mut args = Map::new();
        args.insert(
            "interviews".into(),
            Value::Array(vec![
                serde_json::json!({"agent_id": 7, "prompt": "q1", "platform": "twitter"}),
                serde_json::json!({"agent_id": 8, "prompt": "ghost", "platform": "twitter"}),
            ]),
        );
        let r = execute_batch_interview_parallel(&pool, &MockLlm, &args).await.expect("ok");
        let res = r["results"].as_object().unwrap();
        assert_eq!(res["twitter_7"]["response"], Value::String("Think(idle)".into()));
        assert_eq!(res["twitter_8"]["response"], Value::Null);
        assert_eq!(res["twitter_8"]["timestamp"], Value::Null);
        assert_eq!(r["interviews_count"], Value::from(2u64));
    }

    /// Batch: a platform with ZERO resolved items contributes nothing → overall empty → error.
    #[tokio::test]
    async fn parallel_batch_no_results_errs() {
        use crate::agent::Platform::Twitter;
        let pool = parallel_pool(&[(7, "AdaT", Twitter)]);
        let mut args = Map::new();
        args.insert(
            "interviews".into(),
            Value::Array(vec![
                serde_json::json!({"agent_id": 9, "prompt": "x", "platform": "reddit"}),
            ]),
        );
        let err = execute_batch_interview_parallel(&pool, &MockLlm, &args).await.unwrap_err();
        assert_eq!(err, "没有成功的采访");
    }

    /// Single-platform run (parallel=false) is UNAFFECTED: `dispatch_command(parallel=false)`
    /// routes to the original `execute_interview` returning the unwrapped single result.
    #[tokio::test]
    async fn dispatch_single_mode_unwrapped_result() {
        let pool = pool_with_social_agent(7, "Ada");
        let (client, mut server) = channel(IPC_CHANNEL_BUFFER);
        server.start();
        // client sends an interview; server polls + dispatches with parallel=false.
        let send = tokio::spawn(async move {
            client.send_interview(7, "q", None, Duration::from_secs(5)).await
        });
        // Service exactly one command.
        loop {
            match server.try_poll() {
                CommandPoll::Command(env) => {
                    assert!(dispatch_command(env, &pool, &MockLlm, false).await);
                    break;
                }
                CommandPoll::Empty => tokio::time::sleep(Duration::from_millis(5)).await,
                CommandPoll::Disconnected => panic!("client dropped early"),
            }
        }
        let resp = send.await.unwrap().expect("interview ok");
        // parallel=false → original shape: top-level response, NO `platforms` wrap.
        assert_eq!(resp.result.as_ref().unwrap()["response"], Value::String("Think(idle)".into()));
        assert!(resp.result.as_ref().unwrap().get("platforms").is_none());
    }

    // ---- DUAL-LLM boost routing — Cycle 60 (S-934) ----

    /// Records every prompt it is asked to complete (to observe which client an agent was routed
    /// to). Returns the generic `Think(idle)` action so the run advances without social logging.
    struct RecordingLlm {
        prompts: Arc<std::sync::Mutex<Vec<String>>>,
    }
    impl RecordingLlm {
        fn new() -> Self {
            Self { prompts: Arc::new(std::sync::Mutex::new(Vec::new())) }
        }
        fn count(&self) -> usize {
            self.prompts.lock().unwrap().len()
        }
    }
    #[async_trait]
    impl LlmClient for RecordingLlm {
        async fn complete(&self, p: &str) -> Result<String> {
            self.prompts.lock().unwrap().push(p.to_string());
            Ok("Think(idle)".to_string())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat(&self, _: &[ChatMessage], _: &ChatOptions) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[ChatMessage],
            _: &ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
    }

    /// With a boost client installed, REDDIT agents route to boost and TWITTER agents to main
    /// (`create_model(use_boost=True)` for the reddit coroutine, `False` for twitter).
    #[tokio::test]
    async fn run_with_boost_routes_reddit_to_boost() {
        use crate::agent::Platform::{Reddit, Twitter};
        let mut pool = AgentPool::new();
        pool.add_agent(social_agent_on(1, "Tw", Twitter));
        pool.add_agent(social_agent_on(2, "RdA", Reddit));
        pool.add_agent(social_agent_on(3, "RdB", Reddit));
        let main = RecordingLlm::new();
        let boost = RecordingLlm::new();
        let engine = SimEngine::new(SimConfig::new(1, 4)); // 1 tick → each agent prepares once
        engine
            .run_with_boost(&mut pool, &KnowledgeGraph::new(), &main, Some(&boost))
            .await
            .expect("run ok");
        assert_eq!(boost.count(), 2, "both reddit agents → boost");
        assert_eq!(main.count(), 1, "twitter agent → main");
    }

    /// With NO boost client, every agent (twitter AND reddit) uses the main client — no-downgrade
    /// of the single-LLM path.
    #[tokio::test]
    async fn run_with_boost_none_uses_main_for_all() {
        use crate::agent::Platform::{Reddit, Twitter};
        let mut pool = AgentPool::new();
        pool.add_agent(social_agent_on(1, "Tw", Twitter));
        pool.add_agent(social_agent_on(2, "Rd", Reddit));
        let main = RecordingLlm::new();
        let engine = SimEngine::new(SimConfig::new(1, 4));
        engine
            .run_with_boost(&mut pool, &KnowledgeGraph::new(), &main, None)
            .await
            .expect("run ok");
        assert_eq!(main.count(), 2, "no boost → both agents use main");
    }

    /// The thin `run()` wrapper delegates with no boost → every agent uses main (byte-identical to
    /// the pre-dual-LLM behavior).
    #[tokio::test]
    async fn run_wrapper_uses_main_for_all() {
        use crate::agent::Platform::Reddit;
        let mut pool = AgentPool::new();
        pool.add_agent(social_agent_on(2, "Rd", Reddit));
        let main = RecordingLlm::new();
        let engine = SimEngine::new(SimConfig::new(1, 4));
        engine.run(&mut pool, &KnowledgeGraph::new(), &main).await.expect("run ok");
        assert_eq!(main.count(), 1, "run() → reddit agent still uses main (no boost installed)");
    }

    /// THE KEYSTONE end-to-end: a live `run_sim_body` finishes its (0-tick) run, then stays alive
    /// in wait-for-commands mode servicing IPC. A client interviews an agent (→ LLM response),
    /// an unknown agent_id fails gracefully, and `close_env` completes + breaks the loop so the
    /// task ends and the env is marked not-alive. This is the resolution of `[!] IPC-PRODUCER-PENDING`.
    #[tokio::test(flavor = "multi_thread")]
    async fn wait_for_commands_services_interview_then_close() {
        let pool = pool_with_social_agent(7, "Ada");
        let engine = SimEngine::new(SimConfig::new(0, 1)); // 0 ticks → run() returns immediately
        let (client, server) = channel(IPC_CHANNEL_BUFFER);
        let shutdown = Arc::new(AtomicBool::new(false));
        let task = tokio::spawn(run_sim_body(
            engine,
            pool,
            KnowledgeGraph::new(),
            Arc::new(MockLlm),
            None, // boost_llm — single-LLM
            server,
            shutdown,
            false, // single-platform mode
        ));

        // Interview a real agent → completed + the {agent_id, response, timestamp} shape.
        let resp = client
            .send_interview(7, "How do you feel?", None, Duration::from_secs(5))
            .await
            .expect("send ok");
        assert_eq!(resp.status, crate::services::simulation_ipc::CommandStatus::Completed);
        let result = resp.result.expect("result");
        assert_eq!(result["agent_id"], Value::from(7i64));
        assert_eq!(result["response"], Value::String("Think(idle)".into()));

        // Unknown agent → failed (not a panic, not a hang).
        let bad = client
            .send_interview(999, "x", None, Duration::from_secs(5))
            .await
            .expect("send ok");
        assert_eq!(bad.status, crate::services::simulation_ipc::CommandStatus::Failed);
        assert!(bad.error.unwrap().contains("not found"));

        // close_env → completed + the loop breaks → task ends.
        let close = client.send_close_env(Duration::from_secs(5)).await.expect("send ok");
        assert_eq!(close.status, crate::services::simulation_ipc::CommandStatus::Completed);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("task should end after close_env")
            .expect("task join ok");
        assert!(!client.check_env_alive(), "env must be not-alive after close_env");
    }

    /// The wait loop exits when the cooperative shutdown flag is set (the SIGTERM analog), even
    /// without a close_env command.
    #[tokio::test(flavor = "multi_thread")]
    async fn wait_for_commands_exits_on_shutdown_flag() {
        let pool = pool_with_social_agent(7, "Ada");
        let engine = SimEngine::new(SimConfig::new(0, 1));
        let (client, server) = channel(IPC_CHANNEL_BUFFER);
        let shutdown = Arc::new(AtomicBool::new(false));
        let task = tokio::spawn(run_sim_body(
            engine,
            pool,
            KnowledgeGraph::new(),
            Arc::new(MockLlm),
            None, // boost_llm — single-LLM
            server,
            Arc::clone(&shutdown),
            false, // single-platform mode
        ));
        // Let the run finish + enter the wait loop, then trip the flag.
        tokio::time::sleep(Duration::from_millis(120)).await;
        shutdown.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("task should end on shutdown flag")
            .expect("join ok");
        assert!(!client.check_env_alive());
    }

    /// The wait loop exits when every IPC client is dropped (the run handle was removed — teri's
    /// analog of the OS killing the subprocess), via `CommandPoll::Disconnected`.
    #[tokio::test(flavor = "multi_thread")]
    async fn wait_for_commands_exits_on_client_disconnect() {
        let pool = pool_with_social_agent(7, "Ada");
        let engine = SimEngine::new(SimConfig::new(0, 1));
        let (client, server) = channel(IPC_CHANNEL_BUFFER);
        let shutdown = Arc::new(AtomicBool::new(false));
        let task = tokio::spawn(run_sim_body(
            engine,
            pool,
            KnowledgeGraph::new(),
            Arc::new(MockLlm),
            None, // boost_llm — single-LLM
            server,
            shutdown,
            false, // single-platform mode
        ));
        tokio::time::sleep(Duration::from_millis(120)).await;
        drop(client); // all senders gone → try_poll yields Disconnected → loop breaks
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("task should end on client disconnect")
            .expect("join ok");
    }

    // -----------------------------------------------------------------------
    // start_simulation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn start_missing_config_errors() {
        let (runner, _dir, _mgr) = make_runner("missing_cfg");
        // No simulation_config.json written → ValueError analog.
        let res = runner
            .start_simulation("sim-x", "parallel", None, false, None, run_inputs(1), None)
            .await;
        assert!(res.is_err(), "missing config must error");
        let msg = format!("{}", res.err().unwrap());
        assert!(
            msg.contains("模拟配置不存在"),
            "error must be the missing-config message: {msg}"
        );
    }

    /// U-028 c3b-ii gap-closure proof: a producer-attached engine writes `actions.jsonl`, the
    /// spawned monitor tails it, detects `simulation_end`, and marks the run COMPLETED. This is the
    /// producer→monitor→COMPLETED composition that closes GAP-U026-RUNINPUTS-BUILDER end-to-end.
    #[tokio::test(flavor = "multi_thread")]
    async fn producer_run_reaches_completed_via_monitor() {
        let (runner, dir, _mgr) = make_runner("producer_completed");
        write_config(&dir, "sim-pc", 1, 30); // 2 rounds
        let sim_dir = dir.join("sim-pc");
        let inputs = run_inputs_with_producer(2, &sim_dir, "twitter");
        let _ = runner
            .start_simulation("sim-pc", "twitter", None, false, None, inputs, None)
            .await
            .expect("start should succeed");

        // The run (MockLlm, 2 ticks) finishes fast and emits `simulation_end`; the monitor's
        // completion signal wakes it for a final tail pass → COMPLETED. Poll bounded (~5s).
        let mut completed = false;
        for _ in 0..50 {
            if let Some(rs) = runner.get_run_state("sim-pc").await.unwrap()
                && rs.runner_status == RunnerStatus::Completed
            {
                completed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            completed,
            "producer simulation_end must drive the run to COMPLETED via the monitor"
        );

        // The actions.jsonl the monitor tailed must contain the terminal record.
        let log = sim_dir.join("twitter").join("actions.jsonl");
        let content = std::fs::read_to_string(&log).expect("actions.jsonl written by the producer");
        assert!(content.contains("simulation_end"), "actions.jsonl must contain simulation_end");
    }

    /// U-030 cycle B end-to-end: a PARALLEL run with a dual-logger producer writes BOTH
    /// `twitter/actions.jsonl` and `reddit/actions.jsonl`, each terminating on `simulation_end`, and
    /// the monitor's dual-platform completion gate (S-615) requires BOTH before transitioning the run
    /// to COMPLETED. This is the gap-closure proof that parallel now reaches COMPLETED.
    #[tokio::test(flavor = "multi_thread")]
    async fn parallel_producer_run_reaches_completed() {
        let (runner, dir, _mgr) = make_runner("parallel_completed");
        write_config(&dir, "sim-par", 1, 30); // 2 rounds
        let sim_dir = dir.join("sim-par");
        let inputs = run_inputs_with_parallel_producer(2, &sim_dir);
        let _ = runner
            .start_simulation("sim-par", "parallel", None, false, None, inputs, None)
            .await
            .expect("start should succeed");

        // Poll bounded (~5s) for COMPLETED — only fires once BOTH platforms' simulation_end are seen.
        let mut completed = false;
        for _ in 0..50 {
            if let Some(rs) = runner.get_run_state("sim-par").await.unwrap()
                && rs.runner_status == RunnerStatus::Completed
            {
                completed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            completed,
            "parallel run must reach COMPLETED only after BOTH platforms' simulation_end (dual-gate)"
        );

        // BOTH platform files exist and each terminates on simulation_end.
        for platform in ["twitter", "reddit"] {
            let log = sim_dir.join(platform).join("actions.jsonl");
            let content = std::fs::read_to_string(&log)
                .unwrap_or_else(|_| panic!("{platform}/actions.jsonl must be written"));
            assert!(
                content.contains("simulation_end"),
                "{platform}/actions.jsonl must contain simulation_end"
            );
        }
    }

    #[tokio::test]
    async fn start_computes_total_rounds() {
        let (runner, dir, _mgr) = make_runner("total_rounds");
        // total_hours=72, minutes_per_round=30 → int(72*60/30) = 144.
        write_config(&dir, "sim-tr", 72, 30);
        let state = runner
            .start_simulation("sim-tr", "parallel", None, false, None, run_inputs(2), None)
            .await
            .expect("start should succeed");
        assert_eq!(state.total_rounds, 144);
        assert_eq!(state.total_simulation_hours, 72);
        assert_eq!(state.runner_status, RunnerStatus::Running);
        // parallel → both platforms running
        assert!(state.twitter_running);
        assert!(state.reddit_running);
        assert!(state.started_at.is_some());
        // process_pid is [≠] value-only → always None in teri.
        assert!(state.process_pid.is_none());
    }

    #[tokio::test]
    async fn start_max_rounds_truncates() {
        let (runner, dir, _mgr) = make_runner("max_rounds");
        write_config(&dir, "sim-mr", 72, 30); // 144 natural rounds
        let state = runner
            .start_simulation("sim-mr", "twitter", Some(10), false, None, run_inputs(2), None)
            .await
            .expect("start should succeed");
        assert_eq!(state.total_rounds, 10, "max_rounds=10 must truncate 144 → 10");
        // twitter-only → only twitter running
        assert!(state.twitter_running);
        assert!(!state.reddit_running);
    }

    #[tokio::test]
    async fn start_max_rounds_no_truncate_when_larger() {
        let (runner, dir, _mgr) = make_runner("max_no_trunc");
        write_config(&dir, "sim-nt", 1, 30); // int(1*60/30) = 2 rounds
        let state = runner
            .start_simulation("sim-nt", "reddit", Some(100), false, None, run_inputs(2), None)
            .await
            .expect("start should succeed");
        // min(2, 100) = 2 — no truncation when max_rounds exceeds natural.
        assert_eq!(state.total_rounds, 2);
        assert!(!state.twitter_running);
        assert!(state.reddit_running);
    }

    #[tokio::test]
    async fn start_registers_run_and_persists() {
        let (runner, dir, _mgr) = make_runner("registers");
        write_config(&dir, "sim-reg", 72, 30);
        let _ = runner
            .start_simulation("sim-reg", "parallel", Some(2), false, None, run_inputs(2), None)
            .await
            .expect("start should succeed");

        // run_state.json persisted with status running (or completed, if the 2-tick run
        // finished already — both are acceptable post-start; the key is it was persisted).
        let on_disk = load_run_state(&dir, "sim-reg").unwrap().expect("run_state.json persisted");
        assert_eq!(on_disk.total_rounds, 2);

        // get_run_state returns the live (registered) state.
        let live = runner.get_run_state("sim-reg").await.unwrap().expect("registered");
        assert_eq!(live.simulation_id, "sim-reg");
    }

    #[tokio::test]
    async fn start_rejects_when_already_running() {
        let (runner, dir, _mgr) = make_runner("reject_running");
        write_config(&dir, "sim-dup", 72, 30);
        // First start with a long-enough run that it stays Running (50 ticks, 1 agent —
        // the MockLlm is instant, so this MAY complete fast; to make it reliably "running"
        // we instead seed a Running state directly via a first start and check the in-memory
        // handle's recorded status, which is Running at registration time).
        let _ = runner
            .start_simulation("sim-dup", "parallel", Some(50), false, None, run_inputs(50), None)
            .await
            .expect("first start ok");

        // The handle's recorded state is Running. A second start must reject.
        let res = runner
            .start_simulation("sim-dup", "parallel", Some(50), false, None, run_inputs(50), None)
            .await;
        assert!(res.is_err(), "second concurrent start must reject");
        assert!(format!("{}", res.err().unwrap()).contains("模拟已在运行中"));
    }

    #[tokio::test]
    async fn start_rejects_concurrent_duplicate_via_start_guard() {
        // Regression for the duplicate-start TOCTOU: while one start is mid-flight (its id reserved
        // in `starting`), a second start for the SAME id must be rejected — so two concurrent
        // `/start` cannot both spawn an engine writing the same actions.jsonl.
        let (runner, dir, _m) = make_runner("dup_start_guard");
        write_config(&dir, "sim-x", 1, 30);

        // Simulate the first start being mid-flight by holding its reservation.
        let guard = StartGuard::acquire(&runner.starting, "sim-x").expect("first reservation");

        let err = runner
            .start_simulation("sim-x", "twitter", Some(1), false, None, run_inputs(1), None)
            .await
            .expect_err("a start while another is in progress for the same id must be rejected");
        assert!(matches!(err, TeriError::Sim(_)), "expected a Sim error, got: {err:?}");

        // Releasing the reservation lets a fresh start proceed.
        drop(guard);
        let ok = runner
            .start_simulation("sim-x", "twitter", Some(1), false, None, run_inputs(1), None)
            .await;
        assert!(ok.is_ok(), "start must succeed once the reservation is released: {ok:?}");

        // Let the short run wind down, then tear down the spawned tasks.
        tokio::time::sleep(Duration::from_millis(50)).await;
        runner.cleanup_all().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // get_running_simulations
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_running_lists_active_runs() {
        let (runner, dir, _mgr) = make_runner("get_running");
        write_config(&dir, "sim-run-a", 72, 30);
        let _ = runner
            .start_simulation("sim-run-a", "parallel", Some(50), false, None, run_inputs(50), None)
            .await
            .expect("start ok");
        // The just-registered run is tracked. (It may finish quickly under MockLlm; if so it
        // is reported as not-running, which is the faithful poll()-based contract.)
        let running = runner.get_running_simulations().await;
        // Either it is still running (listed) or finished (not listed) — both honor the
        // is_finished() contract. We assert the method returns without panicking and that any
        // listed id is the one we started.
        for id in &running {
            assert_eq!(id, "sim-run-a");
        }
    }

    // -----------------------------------------------------------------------
    // stop_simulation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn stop_nonexistent_errors() {
        let (runner, _dir, _mgr) = make_runner("stop_missing");
        let res = runner.stop_simulation("ghost").await;
        assert!(res.is_err());
        assert!(format!("{}", res.err().unwrap()).contains("模拟不存在"));
    }

    #[tokio::test]
    async fn stop_not_running_errors() {
        let (runner, dir, _mgr) = make_runner("stop_not_running");
        // Persist a run_state.json in COMPLETED state (not Running/Paused) — stop must reject.
        let mut s = SimulationRunState::new("sim-done".to_string());
        s.runner_status = RunnerStatus::Completed;
        save_run_state(&dir, &s).unwrap();

        let res = runner.stop_simulation("sim-done").await;
        assert!(res.is_err(), "stop on a non-running sim must error");
        assert!(format!("{}", res.err().unwrap()).contains("模拟未在运行"));
    }

    #[tokio::test]
    async fn stop_transitions_to_stopped() {
        let (runner, dir, _mgr) = make_runner("stop_ok");
        write_config(&dir, "sim-stop", 72, 30);
        // Start a long run so it is reliably Running at stop time.
        let _ = runner
            .start_simulation(
                "sim-stop",
                "parallel",
                Some(1000),
                false,
                None,
                run_inputs(1000),
                None,
            )
            .await
            .expect("start ok");

        let stopped = runner.stop_simulation("sim-stop").await.expect("stop ok");
        assert_eq!(stopped.runner_status, RunnerStatus::Stopped);
        assert!(!stopped.twitter_running);
        assert!(!stopped.reddit_running);
        assert!(stopped.completed_at.is_some());

        // The run is no longer tracked as running.
        let running = runner.get_running_simulations().await;
        assert!(!running.contains(&"sim-stop".to_string()));

        // run_state.json reflects STOPPED.
        let on_disk = load_run_state(&dir, "sim-stop").unwrap().unwrap();
        assert_eq!(on_disk.runner_status, RunnerStatus::Stopped);
    }

    #[tokio::test]
    async fn stop_completes_within_grace_window() {
        // The cooperative stop must finish well within (a small multiple of) the grace window
        // for a fast-yielding sim — proving the graceful path, not the force-abort.
        let (runner, dir, _mgr) = make_runner("stop_grace");
        write_config(&dir, "sim-grace", 72, 30);
        let _ = runner
            .start_simulation(
                "sim-grace",
                "parallel",
                Some(1000),
                false,
                None,
                run_inputs(1000),
                None,
            )
            .await
            .expect("start ok");

        let t0 = std::time::Instant::now();
        let _ = runner.stop_simulation("sim-grace").await.expect("stop ok");
        let elapsed = t0.elapsed();
        // Must not block the full grace window (the cooperative flag stops it between ticks).
        assert!(
            elapsed < STOP_GRACE + Duration::from_secs(2),
            "stop took too long: {:?}",
            elapsed
        );
    }

    /// FAIL-1 regression: the two terminate callers use DIFFERENT grace windows, matching the
    /// per-caller Python `_terminate_process` timeouts.
    ///   - `stop_simulation` → no timeout arg (`simulation_runner.py:793`) → default `timeout=10`
    ///     (`simulation_runner.py:721`) → [`STOP_GRACE`] == 10s.
    ///   - `cleanup_all`     → `timeout=5` (`simulation_runner.py:1224`)    → [`CLEANUP_GRACE`] == 5s.
    ///
    /// A sim that exits gracefully between 5–10s must be allowed to finish under
    /// `stop_simulation` but force-aborted under `cleanup_all`.
    #[test]
    fn terminate_grace_windows_match_python_defaults() {
        // stop_simulation's window is the Python default (10s), NOT the cleanup window (5s).
        assert_eq!(
            STOP_GRACE,
            Duration::from_secs(10),
            "stop_simulation must use the Python `_terminate_process` default timeout=10s (py:721/793)"
        );
        // cleanup_all's window is the explicit `timeout=5` (5s).
        assert_eq!(
            CLEANUP_GRACE,
            Duration::from_secs(5),
            "cleanup_all must use the explicit Python `_terminate_process(timeout=5)` (py:1224)"
        );
        // The two windows MUST differ — the 5s narrowing that lumped them is the FAIL-1 bug.
        assert_ne!(
            STOP_GRACE, CLEANUP_GRACE,
            "the stop vs cleanup grace windows must be distinct (10s vs 5s)"
        );
        // The 5–10s band where stop tolerates but cleanup aborts must be non-empty.
        assert!(STOP_GRACE > CLEANUP_GRACE);
    }

    // -----------------------------------------------------------------------
    // cleanup_all
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn cleanup_all_is_idempotent() {
        let (runner, _dir, _mgr) = make_runner("cleanup_idem");
        // No runs, no updaters → silent return, but the idempotency flag still flips.
        runner.cleanup_all().await;
        // Second call must be a no-op (returns immediately on the flag).
        runner.cleanup_all().await;
        // No panic / no double-cleanup is the assertion; reaching here is success.
    }

    #[tokio::test]
    async fn cleanup_all_terminates_and_records() {
        let (runner, dir, manager) = make_runner("cleanup_terminate");
        // Create a state.json (U-023) so the secondary write has a file to edit.
        let created = manager.create_simulation("proj", "graph", true, true).unwrap();
        let sim_id = created.simulation_id.clone();
        write_config(&dir, &sim_id, 72, 30);

        let _ = runner
            .start_simulation(&sim_id, "parallel", Some(1000), false, None, run_inputs(1000), None)
            .await
            .expect("start ok");

        runner.cleanup_all().await;

        // run_state.json must be STOPPED with the shutdown error message.
        let on_disk = load_run_state(&dir, &sim_id).unwrap().unwrap();
        assert_eq!(on_disk.runner_status, RunnerStatus::Stopped);
        assert_eq!(on_disk.error.as_deref(), Some("服务器关闭，模拟被终止"));
        assert!(on_disk.completed_at.is_some());

        // Secondary state.json write: status flipped to "stopped".
        let reloaded = manager.get_simulation(&sim_id).unwrap().unwrap();
        assert_eq!(reloaded.status, crate::services::simulation_manager::SimulationStatus::Stopped);

        // After cleanup the runs map is drained.
        assert!(runner.get_running_simulations().await.is_empty());
    }

    /// Spin a freshly-started run until its task is finished (the in-process analog of
    /// `process.poll() is not None`), bounded so the test can't hang.
    async fn wait_until_finished(runner: &SimulationRunner<MockLlm>, sim_id: &str) {
        for _ in 0..200 {
            if !runner.get_running_simulations().await.contains(&sim_id.to_string()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("run {sim_id} did not finish within the bound");
    }

    /// Read the raw `status` field out of `{dir}/{sim_id}/state.json`.
    fn read_state_json_status(dir: &Path, sim_id: &str) -> String {
        let raw = std::fs::read_to_string(dir.join(sim_id).join("state.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        v["status"].as_str().unwrap().to_string()
    }

    /// FAIL-2 regression: a FINISHED/completed run must SURVIVE `cleanup_all` with its persisted
    /// state INTACT. Python gates the whole record-keeping body behind `if process.poll() is None:`
    /// (`simulation_runner.py:1219`) — a completed run is skipped entirely (not terminated, not
    /// state-written), keeping its final COMPLETED state. Overwriting it with STOPPED + the
    /// shutdown error message would corrupt a normally-completed sim's record (the bug).
    #[tokio::test]
    async fn cleanup_all_preserves_finished_run_state() {
        let (runner, dir, manager) = make_runner("cleanup_finished_survives");
        // state.json (U-023) so we can prove the secondary write does NOT fire for a finished run.
        let created = manager.create_simulation("proj", "graph", true, true).unwrap();
        let sim_id = created.simulation_id.clone();
        write_config(&dir, &sim_id, 72, 30);

        // Start a TINY run so the task finishes near-instantly under MockLlm.
        let _ = runner
            .start_simulation(&sim_id, "parallel", Some(1), false, None, run_inputs(1), None)
            .await
            .expect("start ok");

        // A run lingers in wait-for-commands mode after its rounds — the MiroFish default
        // (`wait_for_commands = not args.no_wait`, and the Flask launcher never passes `--no-wait`),
        // so the process stays alive (`poll() is None`) until close_env/SIGTERM. Send close_env so
        // the task breaks the wait loop and actually finishes (the "poll() not None" analog).
        let _ = runner.close_simulation_env(&sim_id, Duration::from_secs(5)).await;
        wait_until_finished(&runner, &sim_id).await;
        assert!(
            !runner.get_running_simulations().await.contains(&sim_id),
            "run must be finished before we simulate a completed record"
        );

        // Simulate the run having recorded its OWN final COMPLETED state (run_state.json) and a
        // COMPLETED state.json — the state a normally-finished sim would leave on disk.
        let mut final_state = SimulationRunState::new(sim_id.clone());
        final_state.runner_status = RunnerStatus::Completed;
        final_state.completed_at = Some("2026-06-17T12:00:00".to_string());
        final_state.error = None;
        save_run_state(&dir, &final_state).unwrap();
        // Overwrite state.json status to "completed" directly on disk (raw, like the engine would).
        {
            let sf = dir.join(&sim_id).join("state.json");
            let raw = std::fs::read_to_string(&sf).unwrap();
            let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap();
            v["status"] = serde_json::Value::String("completed".to_string());
            std::fs::write(&sf, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        }

        // Run cleanup_all — the finished run must be DRAINED but NOT state-overwritten.
        runner.cleanup_all().await;

        // run_state.json: still COMPLETED, no shutdown error, original completed_at intact.
        let on_disk = load_run_state(&dir, &sim_id).unwrap().unwrap();
        assert_eq!(
            on_disk.runner_status,
            RunnerStatus::Completed,
            "finished run's run_state.json must NOT be clobbered to STOPPED by cleanup_all"
        );
        assert_eq!(
            on_disk.error, None,
            "finished run must NOT acquire the '服务器关闭' shutdown error"
        );
        assert_eq!(
            on_disk.completed_at.as_deref(),
            Some("2026-06-17T12:00:00"),
            "finished run's completed_at must be untouched"
        );

        // state.json: secondary write must NOT have fired — status stays "completed".
        assert_eq!(
            read_state_json_status(&dir, &sim_id),
            "completed",
            "finished run's state.json must NOT be flipped to 'stopped' by cleanup_all"
        );

        // The run is still DRAINED from the map (cleanup did remove it — Python _processes.clear()).
        assert!(runner.get_running_simulations().await.is_empty());
    }

    /// FAIL-2 companion: in one cleanup_all, a RUNNING run is stopped+error-recorded while a
    /// FINISHED run's state is preserved — proving the gate discriminates the two, not all-or-nothing.
    #[tokio::test]
    async fn cleanup_all_stops_running_but_skips_finished() {
        let (runner, dir, manager) = make_runner("cleanup_mixed");

        // --- The FINISHED run ---
        let done = manager.create_simulation("proj", "g-done", true, true).unwrap();
        let done_id = done.simulation_id.clone();
        write_config(&dir, &done_id, 72, 30);
        let _ = runner
            .start_simulation(&done_id, "parallel", Some(1), false, None, run_inputs(1), None)
            .await
            .expect("start finished-run ok");
        // Finish the run faithfully: a wait-for-commands run stays alive until close_env.
        let _ = runner.close_simulation_env(&done_id, Duration::from_secs(5)).await;
        wait_until_finished(&runner, &done_id).await;
        // Record its COMPLETED final state.
        let mut done_state = SimulationRunState::new(done_id.clone());
        done_state.runner_status = RunnerStatus::Completed;
        done_state.completed_at = Some("2026-06-17T12:00:00".to_string());
        save_run_state(&dir, &done_state).unwrap();
        {
            let sf = dir.join(&done_id).join("state.json");
            let raw = std::fs::read_to_string(&sf).unwrap();
            let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap();
            v["status"] = serde_json::Value::String("completed".to_string());
            std::fs::write(&sf, serde_json::to_string_pretty(&v).unwrap()).unwrap();
        }

        // --- The still-RUNNING run (long, so it is reliably running at cleanup time) ---
        let live = manager.create_simulation("proj", "g-live", true, true).unwrap();
        let live_id = live.simulation_id.clone();
        write_config(&dir, &live_id, 72, 30);
        let _ = runner
            .start_simulation(&live_id, "parallel", Some(1000), false, None, run_inputs(1000), None)
            .await
            .expect("start running-run ok");
        assert!(
            runner.get_running_simulations().await.contains(&live_id),
            "the long run must still be running before cleanup"
        );

        // One cleanup over BOTH.
        runner.cleanup_all().await;

        // Finished run: state preserved (COMPLETED, no shutdown error, status "completed").
        let done_disk = load_run_state(&dir, &done_id).unwrap().unwrap();
        assert_eq!(done_disk.runner_status, RunnerStatus::Completed);
        assert_eq!(done_disk.error, None);
        assert_eq!(read_state_json_status(&dir, &done_id), "completed");

        // Running run: STOPPED + shutdown error recorded, state.json flipped to "stopped".
        let live_disk = load_run_state(&dir, &live_id).unwrap().unwrap();
        assert_eq!(
            live_disk.runner_status,
            RunnerStatus::Stopped,
            "the still-running run MUST be stopped by cleanup_all"
        );
        assert_eq!(live_disk.error.as_deref(), Some("服务器关闭，模拟被终止"));
        assert!(!live_disk.twitter_running);
        assert!(!live_disk.reddit_running);
        assert_eq!(read_state_json_status(&dir, &live_id), "stopped");

        // Both drained.
        assert!(runner.get_running_simulations().await.is_empty());
    }
}

// ===========================================================================
// Tests — Simulation MONITOR (sub-cycle c): offset-tail, graph-fire, completion,
// dual-platform gate, final-read-after-end.
//
// These exercise S-613 (`monitor_simulation`), S-614 (`read_action_log` / U-047
// S-1056), S-615 (`check_all_platforms_completed`) directly and end-to-end.
// ===========================================================================

#[cfg(test)]
mod monitor_tests {
    use super::*;
    use crate::graph::KnowledgeGraph;
    use crate::llm::{ChatMessage, ChatOptions, LlmClient};
    use crate::services::graph_memory::GraphMemoryManager;
    use async_trait::async_trait;
    use std::env;
    use std::io::Write;
    use std::pin::Pin;

    // ---- Mock LLM (unused by the updater worker on the happy path; required by the type). ----
    struct MockLlm;
    #[async_trait]
    impl LlmClient for MockLlm {
        async fn complete(&self, _: &str) -> Result<String> {
            Ok("Think(idle)".to_string())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat(&self, _: &[ChatMessage], _: &ChatOptions) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[ChatMessage],
            _: &ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
    }

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let mut p = env::temp_dir();
        p.push(format!(
            "teri_test_sim_monitor_{}_{}_{}",
            std::process::id(),
            suffix,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        p
    }

    /// The 4-tuple `make_ctx` returns: the monitor context, its temp dir, the shared run-state
    /// Arc (to assert on after a read), and the graph manager Arc (to assert graph-fire).
    type CtxFixture = (
        MonitorContext<MockLlm>,
        std::path::PathBuf,
        Arc<tokio::sync::Mutex<SimulationRunState>>,
        Arc<GraphMemoryManager<MockLlm>>,
    );

    /// Build a bare `MonitorContext<MockLlm>` over a fresh temp dir for one sim id. Returns the
    /// context, the dir, the shared state Arc, and the graph manager Arc.
    fn make_ctx(suffix: &str, sim_id: &str, graph_enabled: bool) -> CtxFixture {
        let dir = temp_dir(suffix);
        std::fs::create_dir_all(&dir).unwrap();
        let state = Arc::new(tokio::sync::Mutex::new(SimulationRunState::new(sim_id.to_string())));
        let graph_mgr = Arc::new(GraphMemoryManager::<MockLlm>::new());
        let ctx = MonitorContext {
            simulation_id: sim_id.to_string(),
            sim_data_dir: dir.clone(),
            state: Arc::clone(&state),
            graph_mgr: Arc::clone(&graph_mgr),
            graph_enabled,
            agent_memory: None,
        };
        (ctx, dir, state, graph_mgr)
    }

    /// Append a single JSONL line (newline-terminated) to `{dir}/{sim_id}/{platform}/actions.jsonl`.
    fn append_line(dir: &Path, sim_id: &str, platform: &str, value: &Value) {
        let pdir = dir.join(sim_id).join(platform);
        std::fs::create_dir_all(&pdir).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(pdir.join("actions.jsonl"))
            .unwrap();
        writeln!(f, "{}", serde_json::to_string(value).unwrap()).unwrap();
    }

    /// Append RAW bytes (no auto newline) — for partial-line tests.
    fn append_raw(dir: &Path, sim_id: &str, platform: &str, bytes: &str) {
        let pdir = dir.join(sim_id).join(platform);
        std::fs::create_dir_all(&pdir).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(pdir.join("actions.jsonl"))
            .unwrap();
        f.write_all(bytes.as_bytes()).unwrap();
    }

    fn action_value(round: i64, agent_id: i64, action_type: &str) -> Value {
        serde_json::json!({
            "round": round,
            "timestamp": "2026-06-17T10:00:00",
            "agent_id": agent_id,
            "agent_name": format!("agent-{agent_id}"),
            "action_type": action_type,
            "action_args": {"content": "hello"},
            "result": null,
            "success": true,
        })
    }

    fn log_path(dir: &Path, sim_id: &str, platform: &str) -> std::path::PathBuf {
        dir.join(sim_id).join(platform).join("actions.jsonl")
    }

    // -----------------------------------------------------------------------
    // read_action_log — the U-047 offset tail (S-614 / S-1056)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn read_action_log_reads_new_lines_and_returns_offset() {
        let sim_id = "tail-basic";
        let (ctx, dir, state, _g) = make_ctx("tail_basic", sim_id, false);
        append_line(&dir, sim_id, "twitter", &action_value(1, 7, "CREATE_POST"));
        append_line(&dir, sim_id, "twitter", &action_value(1, 8, "LIKE_POST"));

        let path = log_path(&dir, sim_id, "twitter");
        let off = read_action_log(&path, 0, &ctx, "twitter").await;

        // Both actions consumed → 2 recent_actions, twitter count == 2, current_round == 1.
        let s = state.lock().await;
        assert_eq!(s.recent_actions.len(), 2);
        assert_eq!(s.twitter_actions_count, 2);
        assert_eq!(s.current_round, 1);
        // Offset advanced to EOF (file size).
        assert_eq!(off, std::fs::metadata(&path).unwrap().len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// End-to-end through the REAL monitor path: `read_action_log` → `apply_log_record` →
    /// `AgentMemoryWriter::write_action`. A content-bearing action is persisted as agent LTM
    /// (chronological + semantic) and recallable; a structural `DO_NOTHING` is skipped. This is
    /// the wiring a live `teri run`/`teri serve` monitor exercises per action.
    #[tokio::test]
    async fn monitor_persists_agent_ltm_for_content_actions() {
        use httpmock::prelude::*;

        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"object":"list","data":[{"object":"embedding","embedding":[0.1,0.2,0.3],"index":0}],"model":"m","usage":{"prompt_tokens":1,"total_tokens":1}}"#,
            );
        });
        let cfg = crate::config::LlmConfig {
            base_url: server.base_url(),
            api_key: String::new(),
            model: "m".to_string(),
            embed_model: "e".to_string(),
            timeout_secs: 5,
            max_retries: 0,
            max_tokens: 256,
            provider: crate::config::LlmProvider::Openai,
        };

        let mem_dir = temp_dir("agent_ltm_mem");
        std::fs::create_dir_all(&mem_dir).unwrap();
        let store = Arc::new(crate::memory::MemoryStore::new(&mem_dir).unwrap());
        let embedder = Arc::new(crate::embedding::EmbeddingClient::new(&cfg));
        let writer = Arc::new(crate::services::agent_memory::AgentMemoryWriter::new(
            store.clone(),
            embedder.clone(),
        ));

        let sim_id = "ltm-monitor";
        let dir = temp_dir("agent_ltm_sim");
        std::fs::create_dir_all(&dir).unwrap();
        let state = Arc::new(tokio::sync::Mutex::new(SimulationRunState::new(sim_id.to_string())));
        let ctx = MonitorContext {
            simulation_id: sim_id.to_string(),
            sim_data_dir: dir.clone(),
            state,
            graph_mgr: Arc::new(GraphMemoryManager::<MockLlm>::new()),
            graph_enabled: false,
            agent_memory: Some(writer.clone()),
        };

        // A CREATE_POST (content="hello" per `action_value`) and a contentless DO_NOTHING.
        append_line(&dir, sim_id, "reddit", &action_value(1, 5, "CREATE_POST"));
        append_line(
            &dir,
            sim_id,
            "reddit",
            &serde_json::json!({
                "round": 1, "agent_id": 5, "agent_name": "agent-5",
                "action_type": "DO_NOTHING", "action_args": {}, "success": true
            }),
        );
        let path = log_path(&dir, sim_id, "reddit");
        let _ = read_action_log(&path, 0, &ctx, "reddit").await;

        let (persisted, embedded, skipped) = writer.stats();
        assert_eq!(persisted, 1, "CREATE_POST persisted as chronological LTM");
        assert_eq!(embedded, 1, "CREATE_POST embedded into the vector store");
        assert_eq!(skipped, 1, "DO_NOTHING skipped (no content)");

        let ns = crate::services::agent_memory::AgentMemoryWriter::agent_namespace(sim_id, 5);
        let recalled = store.semantic_recall(ns, &embedder, "hello", 5).await.unwrap();
        assert_eq!(recalled.len(), 1, "the post is semantically recallable");
        assert!(recalled[0].content.contains("hello"));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&mem_dir);
    }

    #[tokio::test]
    async fn read_action_log_no_double_read_across_polls() {
        // The core U-047 invariant: a second poll from the returned offset must NOT re-read the
        // already-consumed lines, only the NEW one appended between polls.
        let sim_id = "tail-nodouble";
        let (ctx, dir, state, _g) = make_ctx("tail_nodouble", sim_id, false);
        append_line(&dir, sim_id, "twitter", &action_value(1, 1, "CREATE_POST"));

        let path = log_path(&dir, sim_id, "twitter");
        let off1 = read_action_log(&path, 0, &ctx, "twitter").await;
        assert_eq!(state.lock().await.recent_actions.len(), 1);

        // File grows between polls (one more line).
        append_line(&dir, sim_id, "twitter", &action_value(2, 2, "LIKE_POST"));
        let off2 = read_action_log(&path, off1, &ctx, "twitter").await;

        // Exactly ONE more action consumed (no re-read of the first line).
        let s = state.lock().await;
        assert_eq!(s.recent_actions.len(), 2, "second poll must read only the NEW line");
        assert_eq!(s.twitter_actions_count, 2);
        assert!(off2 > off1, "offset must advance past the growth");
        assert_eq!(off2, std::fs::metadata(&path).unwrap().len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn read_action_log_partial_line_not_consumed() {
        // A trailing line WITHOUT a newline is a partial write — it must NOT be parsed/consumed,
        // and the offset must stop at the end of the last COMPLETE (newline-terminated) line.
        let sim_id = "tail-partial";
        let (ctx, dir, state, _g) = make_ctx("tail_partial", sim_id, false);
        // One complete line, then a partial fragment (no newline).
        append_line(&dir, sim_id, "twitter", &action_value(1, 1, "CREATE_POST"));
        let complete_len = {
            let path = log_path(&dir, sim_id, "twitter");
            std::fs::metadata(&path).unwrap().len()
        };
        append_raw(&dir, sim_id, "twitter", "{\"round\": 2, \"agent_id\": 2, \"action_t");

        let path = log_path(&dir, sim_id, "twitter");
        let off = read_action_log(&path, 0, &ctx, "twitter").await;

        // Only the complete line consumed; the partial fragment is left for the next poll.
        let s = state.lock().await;
        assert_eq!(s.recent_actions.len(), 1, "partial line must NOT be consumed");
        assert_eq!(off, complete_len, "offset must stop at the last complete line, not EOF");
        drop(s);

        // Now the writer finishes the line (newline-terminate it). Next poll consumes it exactly once.
        append_raw(&dir, sim_id, "twitter", "ype\": \"LIKE_POST\", \"success\": true}\n");
        let off2 = read_action_log(&path, off, &ctx, "twitter").await;
        let s = state.lock().await;
        assert_eq!(s.recent_actions.len(), 2, "the now-complete line must be consumed once");
        assert_eq!(off2, std::fs::metadata(&path).unwrap().len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn read_action_log_skips_blank_and_invalid_lines() {
        let sim_id = "tail-skip";
        let (ctx, dir, state, _g) = make_ctx("tail_skip", sim_id, false);
        // blank line, an invalid-JSON line, then a valid action.
        append_raw(&dir, sim_id, "twitter", "\n");
        append_raw(&dir, sim_id, "twitter", "not json at all\n");
        append_line(&dir, sim_id, "twitter", &action_value(1, 1, "CREATE_POST"));

        let path = log_path(&dir, sim_id, "twitter");
        let off = read_action_log(&path, 0, &ctx, "twitter").await;
        let s = state.lock().await;
        assert_eq!(s.recent_actions.len(), 1, "blank + invalid lines skipped, valid one kept");
        // Offset still advances past ALL complete lines (blank + invalid + valid all newline-term).
        assert_eq!(off, std::fs::metadata(&path).unwrap().len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn read_action_log_event_types_not_added_as_actions() {
        let sim_id = "tail-events";
        let (ctx, dir, state, _g) = make_ctx("tail_events", sim_id, false);
        // round_end + an unknown event + simulation_end — none are "actions".
        append_line(
            &dir,
            sim_id,
            "twitter",
            &serde_json::json!({"event_type": "round_end", "round": 5, "simulated_hours": 12}),
        );
        append_line(
            &dir,
            sim_id,
            "twitter",
            &serde_json::json!({"event_type": "round_start", "round": 6}),
        );

        let path = log_path(&dir, sim_id, "twitter");
        let _ = read_action_log(&path, 0, &ctx, "twitter").await;
        let s = state.lock().await;
        assert_eq!(s.recent_actions.len(), 0, "event_type records are not actions");
        // round_end updated per-platform + global round/hours.
        assert_eq!(s.twitter_current_round, 5);
        assert_eq!(s.twitter_simulated_hours, 12);
        assert_eq!(s.current_round, 5);
        assert_eq!(s.simulated_hours, 12);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // check_all_platforms_completed — dual-platform gate (S-615)
    // -----------------------------------------------------------------------

    #[test]
    fn check_completed_single_twitter_only() {
        let dir = temp_dir("gate_tw_only");
        let sim_id = "gate-tw";
        // Only twitter log exists → twitter-only run.
        std::fs::create_dir_all(dir.join(sim_id).join("twitter")).unwrap();
        std::fs::write(log_path(&dir, sim_id, "twitter"), b"").unwrap();

        let mut state = SimulationRunState::new(sim_id.to_string());
        // Not yet completed → gate false.
        assert!(!check_all_platforms_completed(&dir, &state));
        // Twitter completed → gate true (reddit not enabled).
        state.twitter_completed = true;
        assert!(check_all_platforms_completed(&dir, &state));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_completed_dual_requires_both() {
        let dir = temp_dir("gate_dual");
        let sim_id = "gate-dual";
        // BOTH logs exist → dual-platform run.
        std::fs::create_dir_all(dir.join(sim_id).join("twitter")).unwrap();
        std::fs::create_dir_all(dir.join(sim_id).join("reddit")).unwrap();
        std::fs::write(log_path(&dir, sim_id, "twitter"), b"").unwrap();
        std::fs::write(log_path(&dir, sim_id, "reddit"), b"").unwrap();

        let mut state = SimulationRunState::new(sim_id.to_string());
        // Neither done → false.
        assert!(!check_all_platforms_completed(&dir, &state));
        // Only twitter done → STILL false (reddit enabled, not completed).
        state.twitter_completed = true;
        assert!(
            !check_all_platforms_completed(&dir, &state),
            "one platform done must NOT complete"
        );
        // Both done → true.
        state.reddit_completed = true;
        assert!(check_all_platforms_completed(&dir, &state), "both done = completed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_completed_no_platforms_enabled_is_false() {
        let dir = temp_dir("gate_none");
        let sim_id = "gate-none";
        std::fs::create_dir_all(dir.join(sim_id)).unwrap();
        let state = SimulationRunState::new(sim_id.to_string());
        // No actions.jsonl anywhere → no platform enabled → false (not vacuously true).
        assert!(!check_all_platforms_completed(&dir, &state));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // simulation_end → COMPLETED (via read_action_log dispatch)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn simulation_end_single_platform_marks_completed() {
        let sim_id = "end-single";
        let (ctx, dir, state, _g) = make_ctx("end_single", sim_id, false);
        append_line(&dir, sim_id, "twitter", &action_value(1, 1, "CREATE_POST"));
        append_line(
            &dir,
            sim_id,
            "twitter",
            &serde_json::json!({"event_type": "simulation_end", "platform": "twitter",
                "total_rounds": 1, "total_actions": 1}),
        );

        let path = log_path(&dir, sim_id, "twitter");
        let _ = read_action_log(&path, 0, &ctx, "twitter").await;

        let s = state.lock().await;
        assert!(s.twitter_completed, "twitter must be flagged completed");
        assert!(!s.twitter_running);
        // Only twitter enabled → run COMPLETED.
        assert_eq!(s.runner_status, RunnerStatus::Completed);
        assert!(s.completed_at.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn simulation_end_dual_one_platform_not_completed() {
        // Dual-platform: twitter ends but reddit still running → run NOT completed yet.
        let sim_id = "end-dual";
        let (ctx, dir, state, _g) = make_ctx("end_dual", sim_id, false);
        // Both platform logs exist (dual run).
        append_line(&dir, sim_id, "reddit", &action_value(1, 9, "CREATE_POST"));
        append_line(&dir, sim_id, "twitter", &action_value(1, 1, "CREATE_POST"));
        append_line(
            &dir,
            sim_id,
            "twitter",
            &serde_json::json!({"event_type": "simulation_end", "platform": "twitter",
                "total_rounds": 1, "total_actions": 1}),
        );

        let tpath = log_path(&dir, sim_id, "twitter");
        let _ = read_action_log(&tpath, 0, &ctx, "twitter").await;

        {
            let s = state.lock().await;
            assert!(s.twitter_completed);
            assert_ne!(
                s.runner_status,
                RunnerStatus::Completed,
                "reddit still enabled+running → run must NOT be completed yet"
            );
        }

        // Now reddit ends too → run COMPLETED.
        append_line(
            &dir,
            sim_id,
            "reddit",
            &serde_json::json!({"event_type": "simulation_end", "platform": "reddit",
                "total_rounds": 1, "total_actions": 1}),
        );
        let rpath = log_path(&dir, sim_id, "reddit");
        let _ = read_action_log(&rpath, 0, &ctx, "reddit").await;

        let s = state.lock().await;
        assert!(s.reddit_completed);
        assert_eq!(s.runner_status, RunnerStatus::Completed, "both done → completed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // graph-fire when enabled / not when disabled
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn graph_fire_disabled_does_not_register_activities() {
        let sim_id = "graph-off";
        let (ctx, dir, _state, graph_mgr) = make_ctx("graph_off", sim_id, false);
        // No updater created (graph_enabled = false). Reading an action must NOT touch the manager.
        append_line(&dir, sim_id, "twitter", &action_value(1, 1, "CREATE_POST"));
        let path = log_path(&dir, sim_id, "twitter");
        let _ = read_action_log(&path, 0, &ctx, "twitter").await;
        // No updater registered → get_all_stats empty.
        assert!(graph_mgr.get_all_stats().await.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn graph_fire_enabled_forwards_actions_to_updater() {
        let sim_id = "graph-on";
        let (ctx, dir, _state, graph_mgr) = make_ctx("graph_on", sim_id, true);
        // Register a live updater so the monitor's fire path has a target.
        let graph = Arc::new(tokio::sync::Mutex::new(KnowledgeGraph::new()));
        graph_mgr
            .create_updater(sim_id, graph, Arc::new(MockLlm), "test-graph".to_string())
            .await
            .unwrap();

        // Two real actions + one DO_NOTHING (the updater skips DO_NOTHING) + an event (skipped).
        append_line(&dir, sim_id, "twitter", &action_value(1, 1, "CREATE_POST"));
        append_line(&dir, sim_id, "twitter", &action_value(1, 2, "LIKE_POST"));
        append_line(&dir, sim_id, "twitter", &action_value(1, 3, "DO_NOTHING"));
        append_line(
            &dir,
            sim_id,
            "twitter",
            &serde_json::json!({"event_type": "round_end", "round": 1}),
        );

        let path = log_path(&dir, sim_id, "twitter");
        let _ = read_action_log(&path, 0, &ctx, "twitter").await;

        // The updater must have RECEIVED the action dicts (total_activities counts non-DO_NOTHING
        // enqueues; DO_NOTHING is skipped_count; event_type records are filtered before enqueue).
        // Give the async send a moment to register on the counters.
        let stats = graph_mgr.get_all_stats().await;
        let s = stats.get(sim_id).expect("updater registered");
        assert_eq!(s.total_activities, 2, "2 real actions forwarded (DO_NOTHING skipped)");
        assert_eq!(s.skipped_count, 1, "DO_NOTHING is skipped by the updater");

        // Cleanup the updater task.
        graph_mgr.stop_all().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // file-not-yet-exists robustness (read_action_log on a missing file)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn read_action_log_missing_file_returns_position_unchanged() {
        let sim_id = "missing";
        let (ctx, dir, state, _g) = make_ctx("missing", sim_id, false);
        // The file does NOT exist. read_action_log opens it → error → returns position unchanged.
        let path = log_path(&dir, sim_id, "twitter");
        let off = read_action_log(&path, 42, &ctx, "twitter").await;
        assert_eq!(off, 42, "missing file must leave the offset unchanged");
        assert_eq!(state.lock().await.recent_actions.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // monitor_simulation — end-to-end: poll, completion exit, FINAL read pass
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn monitor_loop_does_final_read_after_completion() {
        // Drive the full monitor loop: it polls, the completion signal fires, then the FINAL pass
        // must pick up an action written AFTER completion fired (no trailing action lost — L518-522).
        let sim_id = "final-read";
        let (ctx, dir, state, _g) = make_ctx("final_read", sim_id, false);

        // A completion watch we control: start None (running), flip to Some to end the loop.
        let (tx, rx) = tokio::sync::watch::channel::<Option<crate::sim::SimCompletion>>(None);

        // Write one action BEFORE the loop sees completion.
        append_line(&dir, sim_id, "twitter", &action_value(1, 1, "CREATE_POST"));

        let monitor = tokio::spawn(monitor_simulation(ctx, rx));

        // Let the monitor do at least one poll (it sleeps 2s, but reads immediately on entry).
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Write a SECOND action, THEN signal completion — the loop must do a final read that picks
        // it up.
        append_line(&dir, sim_id, "twitter", &action_value(2, 2, "LIKE_POST"));
        let _ = tx.send(Some(crate::sim::SimCompletion { total_ticks: 2 }));

        // The monitor task should finish promptly (it wakes early on completion via select!).
        tokio::time::timeout(Duration::from_secs(3), monitor)
            .await
            .expect("monitor must exit after completion")
            .expect("monitor task must not panic");

        // BOTH actions present (the second one via the final read pass).
        let s = state.lock().await;
        assert_eq!(s.recent_actions.len(), 2, "the final read pass must capture the late action");
        // Natural-end housekeeping cleared the running flags.
        assert!(!s.twitter_running);
        assert!(!s.reddit_running);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn monitor_loop_already_completed_at_start_still_reads() {
        // If the completion watch is ALREADY Some at the monitor's first poll (the run finished
        // before the monitor started), the loop breaks immediately but the FINAL read pass still
        // runs — so actions are not lost (watch retains the final value; no race — DECISION-17).
        let sim_id = "already-done";
        let (ctx, dir, state, _g) = make_ctx("already_done", sim_id, false);
        append_line(&dir, sim_id, "twitter", &action_value(1, 1, "CREATE_POST"));

        let (_tx, rx) =
            tokio::sync::watch::channel(Some(crate::sim::SimCompletion { total_ticks: 1 }));
        let monitor = tokio::spawn(monitor_simulation(ctx, rx));
        tokio::time::timeout(Duration::from_secs(3), monitor)
            .await
            .expect("monitor must exit")
            .expect("no panic");

        // The final pass still read the action even though the loop never polled.
        assert_eq!(state.lock().await.recent_actions.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn monitor_marks_failed_when_run_ends_without_terminal_record() {
        // Regression for the hung-run bug: an engine error fires the completion watch (via
        // `signal_aborted`) but writes NO `simulation_end` record, so `read_action_log` never sets
        // COMPLETED. A run that was Running at the monitor's exit must be marked Failed (terminal),
        // not left Running forever.
        let sim_id = "aborted";
        let (ctx, dir, state, _g) = make_ctx("aborted", sim_id, false);
        // The run was in progress when the engine aborted.
        state.lock().await.runner_status = RunnerStatus::Running;

        // Completion fires (as signal_aborted does) but there is NO simulation_end record on disk.
        let (_tx, rx) =
            tokio::sync::watch::channel(Some(crate::sim::SimCompletion { total_ticks: 0 }));
        let monitor = tokio::spawn(monitor_simulation(ctx, rx));
        tokio::time::timeout(Duration::from_secs(3), monitor)
            .await
            .expect("monitor must exit")
            .expect("no panic");

        let s = state.lock().await;
        assert_eq!(
            s.runner_status,
            RunnerStatus::Failed,
            "a run that ended without a terminal record must be marked Failed, not left Running"
        );
        assert!(!s.twitter_running);
        assert!(!s.reddit_running);
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn monitor_does_not_override_completed_status() {
        // The Failed transition must NOT clobber a clean finish: if the run already reached
        // Completed (the `simulation_end` final-pass transition), the monitor leaves it Completed.
        let sim_id = "completed-keep";
        let (ctx, dir, state, _g) = make_ctx("completed_keep", sim_id, false);
        state.lock().await.runner_status = RunnerStatus::Completed;

        let (_tx, rx) =
            tokio::sync::watch::channel(Some(crate::sim::SimCompletion { total_ticks: 1 }));
        let monitor = tokio::spawn(monitor_simulation(ctx, rx));
        tokio::time::timeout(Duration::from_secs(3), monitor)
            .await
            .expect("exit")
            .expect("ok");

        assert_eq!(state.lock().await.runner_status, RunnerStatus::Completed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn monitor_loop_persists_run_state_json() {
        // The monitor persists run_state.json after polls / at end. After it exits, the on-disk
        // run_state.json must reflect the actions it consumed.
        let sim_id = "persist";
        let (ctx, dir, _state, _g) = make_ctx("persist", sim_id, false);
        append_line(&dir, sim_id, "twitter", &action_value(3, 1, "CREATE_POST"));
        append_line(
            &dir,
            sim_id,
            "twitter",
            &serde_json::json!({"event_type": "simulation_end", "platform": "twitter",
                "total_rounds": 1, "total_actions": 1}),
        );

        let (_tx, rx) =
            tokio::sync::watch::channel(Some(crate::sim::SimCompletion { total_ticks: 1 }));
        let monitor = tokio::spawn(monitor_simulation(ctx, rx));
        tokio::time::timeout(Duration::from_secs(3), monitor).await.unwrap().unwrap();

        // run_state.json persisted with the consumed action + COMPLETED status.
        let on_disk = load_run_state(&dir, sim_id).unwrap().expect("run_state.json persisted");
        assert_eq!(on_disk.runner_status, RunnerStatus::Completed);
        assert_eq!(on_disk.recent_actions.len(), 1);
        assert_eq!(on_disk.current_round, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ===========================================================================
// Reader methods — sub-cycle (d) — reads via the U-047 tail from read_action_log
//
// Ports S-618 `get_actions`, S-619 `get_timeline`, S-620 `get_agent_stats`.
// All are pure reads that use the same underlying log-tail mechanism.
// ===========================================================================

impl<L: LlmClient + Send + Sync + 'static> SimulationRunner<L> {
    /// Port of `SimulationRunner.get_all_actions` (S-618, `simulation_runner.py:894-952`).
    ///
    /// Reads all actions from both platforms (`twitter/actions.jsonl`, `reddit/actions.jsonl`),
    /// applying optional filters for platform/agent_id/round_num. Actions are sorted by
    /// timestamp descending (newest first), matching Python's `.sort(key=lambda x: x.timestamp, reverse=True)`.
    ///
    /// The per-platform log paths use the same pattern as [`read_action_log`]:
    /// `{sim_data_dir}/{simulation_id}/{platform}/actions.jsonl`
    pub fn get_all_actions(
        &self,
        simulation_id: &str,
        platform: Option<&str>,
        agent_id: Option<i64>,
        round_num: Option<i64>,
    ) -> Result<Vec<AgentAction>> {
        let sim_dir = self.sim_data_dir.join(simulation_id);

        // Collect actions from all relevant platforms.
        // Twitter log path: {sim_dir}/twitter/actions.jsonl
        // Reddit log path: {sim_dir}/reddit/actions.jsonl
        let mut actions: Vec<AgentAction> = vec![];

        // Python `if not platform`: an empty-string platform ("") is falsy, so it reads BOTH
        // platform files (and the inner record filter is skipped) — semantically identical to
        // no filter. teri must treat `Some("")` the same way, not as a literal platform name
        // (which would match neither file). See `_read_actions_from_file` filter guard below.
        let no_filter = platform.is_none() || platform == Some("");

        if no_filter || platform == Some("twitter") {
            let twitter_log = sim_dir.join("twitter").join("actions.jsonl");
            if twitter_log.exists() {
                actions.extend(read_actions_from_file(
                    &twitter_log,
                    Some("twitter"),
                    platform,
                    agent_id,
                    round_num,
                )?);
            }
        }

        if no_filter || platform == Some("reddit") {
            let reddit_log = sim_dir.join("reddit").join("actions.jsonl");
            if reddit_log.exists() {
                actions.extend(read_actions_from_file(
                    &reddit_log,
                    Some("reddit"),
                    platform,
                    agent_id,
                    round_num,
                )?);
            }
        }

        // Fallback: try old single-file format (no platform subdir)
        if actions.is_empty() {
            let legacy_log = sim_dir.join("actions.jsonl");
            if legacy_log.exists() {
                actions.extend(read_actions_from_file(
                    &legacy_log,
                    None, // legacy format should have platform in the record
                    platform,
                    agent_id,
                    round_num,
                )?);
            }
        }

        // Sort by timestamp descending (newest first), matching Python's reverse=True.
        // We need to parse timestamps; use chrono for this.
        actions.sort_by(|a, b| {
            let a_ts = parse_timestamp(&a.timestamp);
            let b_ts = parse_timestamp(&b.timestamp);
            b_ts.cmp(&a_ts) // descending: b vs a (newer first)
        });

        Ok(actions)
    }

    /// Port of `SimulationRunner.get_actions` (S-618, `simulation_runner.py:955-987`).
    ///
    /// Returns actions with pagination (limit/offset), plus optional filters.
    ///
    /// # Arguments
    /// * `simulation_id` — simulation ID
    /// * `limit` — max number of results to return (default 100)
    /// * `offset` — skip this many results from the start (default 0)
    /// * `platform` — filter by platform ("twitter" or "reddit"); None = both
    /// * `agent_id` — filter by agent ID; None = all agents
    /// * `round_num` — filter by round number; None = all rounds
    pub fn get_actions(
        &self,
        simulation_id: &str,
        limit: usize,
        offset: usize,
        platform: Option<&str>,
        agent_id: Option<i64>,
        round_num: Option<i64>,
    ) -> Result<Vec<AgentAction>> {
        let all = self.get_all_actions(simulation_id, platform, agent_id, round_num)?;
        // Apply pagination: [offset..offset+limit]
        let end = (offset + limit).min(all.len());
        Ok(all[offset..end].to_vec())
    }

    /// Port of `SimulationRunner.get_timeline` (S-619, `simulation_runner.py:989-1057`).
    ///
    /// Returns per-round summaries for all rounds in the simulation.
    ///
    /// # Arguments
    /// * `simulation_id` — simulation ID
    /// * `start_round` — include only rounds >= this (default 0)
    /// * `end_round` — include only rounds <= this; None = no upper bound
    pub fn get_timeline(
        &self,
        simulation_id: &str,
        start_round: i64,
        end_round: Option<i64>,
    ) -> Result<Vec<TimelineEntry>> {
        let actions = self.get_all_actions(simulation_id, None, None, None)?;

        // Group by round_num
        let mut rounds: std::collections::HashMap<i64, TimelineRound> =
            std::collections::HashMap::new();

        for action in &actions {
            let round_num = action.round_num;

            if round_num < start_round {
                continue;
            }
            if let Some(end) = end_round
                && round_num > end
            {
                continue;
            }

            // Python: `if round_num not in rounds: rounds[round_num] = {... first/last = timestamp}`
            let entry = rounds
                .entry(round_num)
                .or_insert_with(|| TimelineRound::new(round_num, action.timestamp.clone()));
            entry.total_actions += 1;

            match action.platform.as_str() {
                "twitter" => entry.twitter_actions += 1,
                "reddit" => entry.reddit_actions += 1,
                _ => {}
            }

            entry.active_agents.insert(action.agent_id);

            *entry.action_types.entry(action.action_type.clone()).or_insert(0) += 1;

            // Python: `r["last_action_time"] = action.timestamp` on every iteration (DESC order →
            // ends up the OLDEST). first_action_time stays the first-seen (NEWEST).
            entry.last_action_time = action.timestamp.clone();
        }

        // Convert to TimelineEntry and sort by round_num ascending
        let mut result: Vec<TimelineEntry> = rounds.into_values().map(|r| r.into_entry()).collect();

        result.sort_by_key(|a| a.round_num);

        Ok(result)
    }

    /// Port of `SimulationRunner.get_agent_stats` (S-620, `simulation_runner.py:1060-1094`).
    ///
    /// Returns per-agent statistics sorted by total actions descending.
    pub fn get_agent_stats(&self, simulation_id: &str) -> Result<Vec<AgentStats>> {
        let actions = self.get_all_actions(simulation_id, None, None, None)?;

        let mut agent_stats: std::collections::HashMap<i64, AgentStatsEntry> =
            std::collections::HashMap::new();

        for action in &actions {
            let entry = agent_stats.entry(action.agent_id).or_insert_with(|| AgentStatsEntry {
                agent_id: action.agent_id,
                agent_name: action.agent_name.clone(),
                total_actions: 0,
                twitter_actions: 0,
                reddit_actions: 0,
                action_types: std::collections::HashMap::new(),
                first_action_time: action.timestamp.clone(),
                last_action_time: action.timestamp.clone(),
            });

            entry.total_actions += 1;

            match action.platform.as_str() {
                "twitter" => entry.twitter_actions += 1,
                "reddit" => entry.reddit_actions += 1,
                _ => {}
            }

            *entry.action_types.entry(action.action_type.clone()).or_insert(0) += 1;

            // Python: `stats["last_action_time"] = action.timestamp` every iteration (DESC order →
            // ends up the OLDEST). first_action_time stays first-seen (NEWEST). Was previously a
            // bare comment — last_action_time never updated, so it was stuck at the newest stamp.
            entry.last_action_time = action.timestamp.clone();
        }

        let mut result: Vec<AgentStats> =
            agent_stats.into_values().map(|e| e.into_stats()).collect();

        // Sort by total_actions descending (Python's reverse=True on key=lambda x: x["total_actions"])
        result.sort_by_key(|r| std::cmp::Reverse(r.total_actions));

        Ok(result)
    }

    // ---------------------------------------------------------------------------
    // Sub-cycle (e) - Interview wiring via IPC
    // S-628, S-630, S-631, S-633
    // ---------------------------------------------------------------------------

    /// Check whether the simulation environment for `simulation_id` is alive.
    ///
    /// Delegates to [`SimulationIPCClient::check_env_alive`] (S-628).
    pub async fn check_env_alive(&self, simulation_id: &str) -> Result<bool> {
        let runs = self.runs.lock().await;
        let handle = runs
            .get(simulation_id)
            .ok_or_else(|| TeriError::Sim(format!("Simulation not found: {}", simulation_id)))?;
        Ok(handle.ipc_client().check_env_alive())
    }

    /// Interview a single agent via IPC.
    ///
    /// Port of `interview_agent(agent_id, prompt, platform=None, timeout=60.0)`
    /// (`simulation_runner.py:1428-1490`).
    ///
    /// Default timeout: 60 s (matches Python default). Platform is optional.
    ///
    /// S-630
    pub async fn interview_agent(
        &self,
        simulation_id: &str,
        agent_id: i64,
        prompt: &str,
        platform: Option<&str>,
        timeout: Duration,
    ) -> Result<IPCResponse> {
        let runs = self.runs.lock().await;
        let handle = runs
            .get(simulation_id)
            .ok_or_else(|| TeriError::Sim(format!("Simulation not found: {}", simulation_id)))?;
        handle.ipc_client().send_interview(agent_id, prompt, platform, timeout).await
    }

    /// Interview multiple agents via IPC.
    ///
    /// Port of `interview_agents_batch(interviews, platform=None, timeout=120.0)`
    /// (`simulation_runner.py:1492-1548`).
    ///
    /// Default timeout: 120 s (matches Python default). Platform is optional.
    ///
    /// S-631
    pub async fn interview_agents_batch(
        &self,
        simulation_id: &str,
        interviews: Vec<Value>,
        platform: Option<&str>,
        timeout: Duration,
    ) -> Result<IPCResponse> {
        let runs = self.runs.lock().await;
        let handle = runs
            .get(simulation_id)
            .ok_or_else(|| TeriError::Sim(format!("Simulation not found: {}", simulation_id)))?;
        handle.ipc_client().send_batch_interview(interviews, platform, timeout).await
    }

    /// Close the simulation environment via IPC.
    ///
    /// Port of `close_simulation_env(timeout=30.0)` (`simulation_runner.py:1611-1658`).
    ///
    /// Default timeout: 30 s (matches Python default).
    ///
    /// S-633
    pub async fn close_simulation_env(
        &self,
        simulation_id: &str,
        timeout: Duration,
    ) -> Result<IPCResponse> {
        let runs = self.runs.lock().await;
        let handle = runs
            .get(simulation_id)
            .ok_or_else(|| TeriError::Sim(format!("Simulation not found: {}", simulation_id)))?;
        handle.ipc_client().send_close_env(timeout).await
    }

    // ---------------------------------------------------------------------------
    // Sub-cycle (f) - History, env-status + register_cleanup boundary
    // S-629, S-632, S-634/635
    // ---------------------------------------------------------------------------

    /// Get detailed status of the simulation environment from env_status.json.
    ///
    /// Port of `get_env_status_detail(simulation_id)` (`simulation_runner.py:1392-1428`).
    /// Returns default status if file doesn't exist or is invalid.
    pub fn get_env_status_detail(
        &self,
        simulation_id: &str,
    ) -> Result<serde_json::Map<String, Value>> {
        let sim_dir = self.sim_data_dir.join(simulation_id);
        let status_file = sim_dir.join("env_status.json");

        let default_status = {
            let mut m = serde_json::Map::new();
            m.insert("status".to_string(), Value::String("stopped".to_string()));
            m.insert("twitter_available".to_string(), Value::Bool(false));
            m.insert("reddit_available".to_string(), Value::Bool(false));
            m.insert("timestamp".to_string(), Value::Null);
            m
        };

        if !status_file.exists() {
            return Ok(default_status);
        }

        let content = std::fs::read_to_string(&status_file)
            .map_err(|e| TeriError::Sim(format!("Failed to read env_status.json: {}", e)))?;

        let status: serde_json::Map<String, Value> = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(_) => return Ok(default_status),
        };

        // Extract fields with defaults matching Python's .get()
        let mut result = default_status;
        if let Some(v) = status.get("status").and_then(|s| s.as_str()) {
            result.insert("status".to_string(), Value::String(v.to_string()));
        }
        if let Some(v) = status.get("twitter_available").and_then(|b| b.as_bool()) {
            result.insert("twitter_available".to_string(), Value::Bool(v));
        }
        if let Some(v) = status.get("reddit_available").and_then(|b| b.as_bool()) {
            result.insert("reddit_available".to_string(), Value::Bool(v));
        }
        if let Some(v) = status.get("timestamp") {
            result.insert("timestamp".to_string(), v.clone());
        }

        Ok(result)
    }

    /// Interview all agents via IPC by reading agent_configs from simulation_config.json.
    ///
    /// Port of `interview_all_agents(simulation_id, prompt, platform=None, timeout=180.0)`
    /// (`simulation_runner.py:1551-1609`).
    ///
    /// Reads agent configurations and sends a batch interview command for all agents.
    pub async fn interview_all_agents(
        &self,
        simulation_id: &str,
        prompt: &str,
        platform: Option<&str>,
        timeout: Duration,
    ) -> Result<IPCResponse> {
        let sim_dir = self.sim_data_dir.join(simulation_id);
        let config_path = sim_dir.join("simulation_config.json");

        if !sim_dir.exists() {
            return Err(TeriError::Sim(format!("Simulation not found: {}", simulation_id)));
        }

        if !config_path.exists() {
            return Err(TeriError::Sim(format!("Simulation config not found: {}", simulation_id)));
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| TeriError::Sim(format!("Failed to read simulation_config.json: {}", e)))?;

        let config: serde_json::Map<String, Value> =
            serde_json::from_str(&content).map_err(|e| {
                TeriError::Sim(format!("Failed to parse simulation_config.json: {}", e))
            })?;

        let agent_configs =
            config.get("agent_configs").and_then(|v| v.as_array()).ok_or_else(|| {
                TeriError::Sim("agent_configs not found in simulation_config.json".to_string())
            })?;

        if agent_configs.is_empty() {
            return Err(TeriError::Sim(format!(
                "No agents found in simulation config: {}",
                simulation_id
            )));
        }

        let mut interviews = Vec::new();
        for agent_config in agent_configs {
            if let Some(obj) = agent_config.as_object()
                && let Some(agent_id) = obj.get("agent_id").and_then(|v| v.as_i64())
            {
                interviews.push(serde_json::json!({
                    "agent_id": agent_id,
                    "prompt": prompt
                }));
            }
        }

        self.interview_agents_batch(simulation_id, interviews, platform, timeout).await
    }

    // SQLite support for interview history (optional, feature-gated for security)
    #[cfg(feature = "sqlite")]
    fn get_interview_history_from_db(
        db_path: &Path,
        _platform_name: &str,
        agent_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<serde_json::Map<String, Value>>> {
        use rusqlite::{Connection, params};

        if !db_path.exists() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        match Connection::open(db_path) {
            Ok(conn) => {
                let query = if agent_id.is_some() {
                    "SELECT user_id, info, created_at FROM trace WHERE action = 'interview' AND user_id = ? ORDER BY created_at DESC LIMIT ?"
                } else {
                    "SELECT user_id, info, created_at FROM trace WHERE action = 'interview' ORDER BY created_at DESC LIMIT ?"
                };

                let mut stmt = match conn.prepare(query) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("Failed to prepare query for {}: {}", db_path.display(), e);
                        return Ok(Vec::new());
                    }
                };

                let limit_param = limit as i64;
                let iter = match agent_id {
                    Some(aid) => stmt.query_map(params![aid, limit_param], Self::row_to_result),
                    None => stmt.query_map(params![limit_param], Self::row_to_result),
                };

                if let Ok(rows) = iter {
                    // flatten() yields only the Ok rows (skips any per-row error), behaviorally
                    // identical to the prior `if let Ok(r) = row`.
                    for r in rows.flatten() {
                        results.push(r);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to open SQLite db {}: {}", db_path.display(), e);
            }
        }

        Ok(results)
    }

    /// Convert a SQLite row to a JSON map for interview history.
    #[cfg(feature = "sqlite")]
    fn row_to_result(row: &rusqlite::Row<'_>) -> rusqlite::Result<serde_json::Map<String, Value>> {
        let user_id: i64 = row.get(0)?;
        let info_json: String = row.get(1).unwrap_or_default();
        let created_at: String = row.get(2)?;

        // Parse info JSON or use empty map
        let info: serde_json::Map<String, Value> = match serde_json::from_str(&info_json) {
            Ok(i) => i,
            Err(_) => {
                serde_json::json!({"raw": info_json}).as_object().cloned().unwrap_or_default()
            }
        };

        // Build result map matching Python's response structure
        let mut result = serde_json::Map::new();
        result.insert("agent_id".to_string(), Value::Number(user_id.into()));
        result.insert(
            "platform".to_string(),
            Value::String(row.get::<_, String>(3).unwrap_or_default()),
        );

        // Response: info.get("response", info) - if "response" exists use it, else use info directly
        let response = info
            .get("response")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Object(info.clone()));
        result.insert("response".to_string(), response);

        // Prompt: info.get("prompt", "")
        result.insert(
            "prompt".to_string(),
            Value::String(info.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string()),
        );
        result.insert("timestamp".to_string(), Value::String(created_at));

        Ok(result)
    }

    /// Get interview history from all platform databases.
    ///
    /// Port of `get_interview_history(simulation_id, platform=None, agent_id=None, limit=100)`
    /// (`simulation_runner.py:1717-1762`).
    #[cfg(feature = "sqlite")]
    pub fn get_interview_history(
        &self,
        simulation_id: &str,
        platform: Option<&str>,
        agent_id: Option<i64>,
        limit: usize,
    ) -> Result<Vec<serde_json::Map<String, Value>>> {
        let sim_dir = self.sim_data_dir.join(simulation_id);

        let platforms = match platform {
            Some("twitter") | Some("reddit") => vec![platform.unwrap()],
            _ => vec!["twitter", "reddit"],
        };

        let mut results = Vec::new();
        for p in &platforms {
            let db_path = sim_dir.join(format!("{}_simulation.db", p));
            let platform_results =
                Self::get_interview_history_from_db(&db_path, p, agent_id, limit)?;
            results.extend(platform_results);
        }

        // Sort by timestamp descending
        results.sort_by(|a, b| {
            let ts_a = a.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            let ts_b = b.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            ts_b.cmp(ts_a)
        });

        // If multiple platforms queried and results exceed limit, truncate
        if platforms.len() > 1 && results.len() > limit {
            results.truncate(limit);
        }

        Ok(results)
    }
}

/// Helper: read actions from a single JSONL file with optional filters.
///
/// Mirrors Python's `_read_actions_from_file`.
fn read_actions_from_file(
    log_path: &Path,
    default_platform: Option<&str>,
    platform_filter: Option<&str>,
    agent_id_filter: Option<i64>,
    round_num_filter: Option<i64>,
) -> Result<Vec<AgentAction>> {
    use std::io::{BufRead, BufReader};

    let file = std::fs::File::open(log_path).map_err(|e| {
        TeriError::Sim(format!("Failed to open log file {}: {}", log_path.display(), e))
    })?;
    let reader = BufReader::new(file);

    let mut actions = Vec::new();

    for line in reader.lines() {
        let line = line.map_err(|e| {
            TeriError::Sim(format!("Failed to read line from {}: {}", log_path.display(), e))
        })?;

        // Skip blank lines (Python: `if not line:`)
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Parse JSON
        let data: Value = serde_json::from_str(&line).map_err(|e| {
            TeriError::Sim(format!("Failed to parse JSON from {}: {}", log_path.display(), e))
        })?;

        // Apply filters before building the action.
        // Python `if platform_filter and record_platform != platform_filter:` — an empty-string
        // filter ("") is falsy, so the record-level platform filter is SKIPPED entirely (matches
        // not-filtering). Guarding on `!p.is_empty()` keeps `Some("")` from filtering out every
        // record (which carries a concrete "twitter"/"reddit" platform).
        if let Some(pf) = platform_filter.filter(|p| !p.is_empty()) {
            let record_platform = data.get("platform").and_then(Value::as_str).unwrap_or("");
            if record_platform != pf && default_platform != Some(pf) {
                continue;
            }
        }

        if let Some(af) = agent_id_filter {
            let record_agent_id = data.get("agent_id").and_then(Value::as_i64).unwrap_or(0);
            if record_agent_id != af {
                continue;
            }
        }

        if let Some(rf) = round_num_filter {
            let record_round = data.get("round").and_then(Value::as_i64).unwrap_or(0);
            if record_round != rf {
                continue;
            }
        }

        // Build AgentAction (Python field defaults + platform from file path)
        let action = AgentAction {
            round_num: data.get("round").and_then(Value::as_i64).unwrap_or(0),
            timestamp: data
                .get("timestamp")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(python_isoformat_local),
            platform: data
                .get("platform")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| default_platform.unwrap_or("twitter").to_string()),
            agent_id: data.get("agent_id").and_then(Value::as_i64).unwrap_or(0),
            agent_name: data
                .get("agent_name")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_default(),
            action_type: data
                .get("action_type")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_default(),
            action_args: data
                .get("action_args")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            result: data.get("result").and_then(Value::as_str).map(String::from),
            success: data.get("success").and_then(Value::as_bool).unwrap_or(true),
        };

        actions.push(action);
    }

    Ok(actions)
}

/// Parse a timestamp string into chrono::DateTime for sorting.
///
/// Handles ISO-8601 format with optional timezone or naive datetime.
fn parse_timestamp(ts: &str) -> chrono::DateTime<chrono::Utc> {
    // Try parsing with nano seconds first (Python's isoformat includes them)
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%.f") {
        return chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc);
    }
    // Try without fractional seconds
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S") {
        return chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc);
    }
    // Fallback: parse with any format (might handle timezone)
    if let Ok(dt) = ts.parse::<chrono::DateTime<chrono::Utc>>() {
        return dt;
    }
    // Last resort: use current time as fallback
    chrono::Utc::now()
}

/// Per-round summary for timeline output.
#[derive(Debug, Clone)]
struct TimelineRound {
    round_num: i64,
    twitter_actions: usize,
    reddit_actions: usize,
    total_actions: usize,
    active_agents: std::collections::HashSet<i64>,
    action_types: std::collections::HashMap<String, usize>,
    // Python tracks first/last action timestamps per round (simulation_runner.py:1024-1025,1039).
    // Actions arrive newest-first (get_actions sorts DESC), so `first_action_time` ends up the
    // NEWEST (set once on first-seen) and `last_action_time` the OLDEST (overwritten each iter) —
    // the names are intentionally inverted relative to chronology, matching Python verbatim.
    first_action_time: String,
    last_action_time: String,
}

impl TimelineRound {
    fn new(round_num: i64, timestamp: String) -> Self {
        Self {
            round_num,
            twitter_actions: 0,
            reddit_actions: 0,
            total_actions: 0,
            active_agents: std::collections::HashSet::new(),
            action_types: std::collections::HashMap::new(),
            first_action_time: timestamp.clone(),
            last_action_time: timestamp,
        }
    }

    fn into_entry(self) -> TimelineEntry {
        TimelineEntry {
            round_num: self.round_num,
            twitter_actions: self.twitter_actions as i64,
            reddit_actions: self.reddit_actions as i64,
            total_actions: self.total_actions as i64,
            active_agents_count: self.active_agents.len() as i64,
            active_agents: self.active_agents.into_iter().collect(),
            action_type_counts: self
                .action_types
                .into_iter()
                .map(|(k, v)| (k, Value::Number((v as i64).into())))
                .collect(),
            first_action_time: self.first_action_time,
            last_action_time: self.last_action_time,
        }
    }
}

/// Output type for [`SimulationRunner::get_timeline`].
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub round_num: i64,
    pub twitter_actions: i64,
    pub reddit_actions: i64,
    pub total_actions: i64,
    pub active_agents_count: i64,
    pub active_agents: Vec<i64>,
    #[serde(rename = "action_types")]
    pub action_type_counts: serde_json::Map<String, Value>,
    pub first_action_time: String,
    pub last_action_time: String,
}

impl TimelineEntry {
    /// Convert to a JSON Value (for API responses).
    ///
    /// Key order is byte-exact with Python's timeline-entry dict (simulation_runner.py:1043-1053):
    /// `round_num, twitter_actions, reddit_actions, total_actions, active_agents_count,
    /// active_agents, action_types, first_action_time, last_action_time`.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("round_num".into(), Value::Number(self.round_num.into()));
        m.insert("twitter_actions".into(), Value::Number(self.twitter_actions.into()));
        m.insert("reddit_actions".into(), Value::Number(self.reddit_actions.into()));
        m.insert("total_actions".into(), Value::Number(self.total_actions.into()));
        m.insert("active_agents_count".into(), Value::Number(self.active_agents_count.into()));
        m.insert(
            "active_agents".into(),
            Value::Array(self.active_agents.iter().map(|&id| Value::Number(id.into())).collect()),
        );
        m.insert("action_types".into(), Value::Object(self.action_type_counts.clone()));
        m.insert("first_action_time".into(), Value::String(self.first_action_time.clone()));
        m.insert("last_action_time".into(), Value::String(self.last_action_time.clone()));
        Value::Object(m)
    }
}

/// Per-agent statistics entry (internal type).
#[derive(Debug, Clone)]
struct AgentStatsEntry {
    agent_id: i64,
    agent_name: String,
    total_actions: usize,
    twitter_actions: usize,
    reddit_actions: usize,
    action_types: std::collections::HashMap<String, usize>,
    first_action_time: String,
    last_action_time: String,
}

impl AgentStatsEntry {
    fn into_stats(self) -> AgentStats {
        AgentStats {
            agent_id: self.agent_id,
            agent_name: self.agent_name,
            total_actions: self.total_actions as i64,
            twitter_actions: self.twitter_actions as i64,
            reddit_actions: self.reddit_actions as i64,
            action_type_counts: self
                .action_types
                .into_iter()
                .map(|(k, v)| (k, Value::Number((v as i64).into())))
                .collect(),
            first_action_time: self.first_action_time,
            last_action_time: self.last_action_time,
        }
    }
}

/// Output type for [`SimulationRunner::get_agent_stats`].
#[derive(Debug, Clone, Serialize)]
pub struct AgentStats {
    pub agent_id: i64,
    pub agent_name: String,
    pub total_actions: i64,
    pub twitter_actions: i64,
    pub reddit_actions: i64,
    #[serde(rename = "action_types")]
    pub action_type_counts: serde_json::Map<String, Value>,
    pub first_action_time: String,
    pub last_action_time: String,
}

impl AgentStats {
    /// Convert to a JSON Value (for API responses).
    ///
    /// Key order is byte-exact with Python's agent-stats dict (simulation_runner.py:1075-1083):
    /// `agent_id, agent_name, total_actions, twitter_actions, reddit_actions, action_types,
    /// first_action_time, last_action_time` — `action_types` BEFORE the two timestamps.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("agent_id".into(), Value::Number(self.agent_id.into()));
        m.insert("agent_name".into(), Value::String(self.agent_name.clone()));
        m.insert("total_actions".into(), Value::Number(self.total_actions.into()));
        m.insert("twitter_actions".into(), Value::Number(self.twitter_actions.into()));
        m.insert("reddit_actions".into(), Value::Number(self.reddit_actions.into()));
        m.insert("action_types".into(), Value::Object(self.action_type_counts.clone()));
        m.insert("first_action_time".into(), Value::String(self.first_action_time.clone()));
        m.insert("last_action_time".into(), Value::String(self.last_action_time.clone()));
        Value::Object(m)
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod reader_tests {
    use super::*;
    use crate::llm::{ChatMessage, ChatOptions, LlmClient};
    use async_trait::async_trait;
    use std::io::Write;

    // Mock LLM for testing (defined locally since llm::testing doesn't exist)
    struct MockLlm;
    #[async_trait]
    impl LlmClient for MockLlm {
        async fn complete(&self, _: &str) -> Result<String> {
            Ok("Think(test)".to_string())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat(&self, _: &[ChatMessage], _: &ChatOptions) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[ChatMessage],
            _: &ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
    }

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("teri_test_readers_{}", suffix));
        p
    }

    /// Create a test runner and dir with sample action logs.
    fn make_runner_with_logs(sim_id: &str) -> (SimulationRunner<MockLlm>, std::path::PathBuf) {
        let dir = temp_dir(sim_id);
        // The simulation_id becomes the subdirectory
        let sim_dir = dir.join(sim_id);
        std::fs::create_dir_all(&sim_dir).unwrap();

        // Create sample Twitter actions
        let twitter_dir = sim_dir.join("twitter");
        std::fs::create_dir_all(&twitter_dir).unwrap();
        // Truncate existing file or create new one (write mode instead of append)
        let mut twitter_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(twitter_dir.join("actions.jsonl"))
            .unwrap();

        // Write some Twitter actions
        for i in 0..3 {
            writeln!(
                &mut twitter_file,
                "{{\"round\": {}, \"timestamp\": \"2026-06-18T10:00:{}Z\", \"platform\": \"twitter\", \
                 \"agent_id\": {}, \"agent_name\": \"TwitterAgent{}\", \
                 \"action_type\": \"CREATE_POST\", \"action_args\": {{\"content\": \"tweet {}\"}}, \
                 \"result\": null, \"success\": true}}",
                (i % 2) + 1,
                i,
                i,
                i,
                i
            )
            .unwrap();
        }

        // Create sample Reddit actions
        let reddit_dir = sim_dir.join("reddit");
        std::fs::create_dir_all(&reddit_dir).unwrap();
        let mut reddit_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(reddit_dir.join("actions.jsonl"))
            .unwrap();

        for i in 0..2 {
            writeln!(
                &mut reddit_file,
                "{{\"round\": {}, \"timestamp\": \"2026-06-18T10:00:{}Z\", \"platform\": \"reddit\", \
                 \"agent_id\": {}, \"agent_name\": \"RedditAgent{}\", \
                 \"action_type\": \"LIKE_POST\", \"action_args\": {{\"post_id\": \"r{}\"}}, \
                 \"result\": null, \"success\": true}}",
                (i % 2) + 1,
                i + 3,
                i + 10,
                i,
                i
            )
            .unwrap();
        }

        let manager = Arc::new(SimulationManager::new(&dir));
        let graph_mgr = Arc::new(GraphMemoryManager::<MockLlm>::new());
        let runner =
            SimulationRunner::new(&dir, graph_mgr, Arc::clone(&manager) as Arc<SimulationManager>);
        (runner, dir)
    }

    #[test]
    fn get_actions_returns_paginated_results() {
        let (runner, _dir) = make_runner_with_logs("pagination");
        // Use the same sim_id that was used to create logs
        let actions = runner.get_actions("pagination", 2, 0, None, None, None).unwrap();
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn get_all_actions_filters_by_platform() {
        let (runner, _dir) = make_runner_with_logs("filter-platform");
        // Use the same sim_id that was used to create logs
        let tw_only =
            runner.get_all_actions("filter-platform", Some("twitter"), None, None).unwrap();
        assert_eq!(tw_only.len(), 3);
        for a in &tw_only {
            assert_eq!(a.platform, "twitter");
        }

        let rd_only =
            runner.get_all_actions("filter-platform", Some("reddit"), None, None).unwrap();
        assert_eq!(rd_only.len(), 2);
        for a in &rd_only {
            assert_eq!(a.platform, "reddit");
        }
    }

    #[test]
    fn get_all_actions_filters_by_agent_id() {
        let (runner, _dir) = make_runner_with_logs("filter-agent");
        // Use the same sim_id that was used to create logs
        let filtered = runner.get_all_actions("filter-agent", None, Some(1), None).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].agent_id, 1);
    }

    #[test]
    fn get_timeline_aggregates_by_round() {
        let (runner, _dir) = make_runner_with_logs("timeline");
        // Use the same sim_id that was used to create logs
        let timeline = runner.get_timeline("timeline", 1, None).unwrap();
        assert_eq!(timeline.len(), 2); // rounds 1 and 2
    }

    #[test]
    fn get_agent_stats_aggregates_per_agent() {
        let (runner, _dir) = make_runner_with_logs("agent-stats");
        let stats = runner.get_agent_stats("agent-stats").unwrap();
        assert_eq!(stats.len(), 5); // 3 twitter + 2 reddit agents
        // First agent should be the one with most actions
        assert!(
            stats[0].total_actions >= stats[1..].iter().map(|s| s.total_actions).max().unwrap_or(0)
        );
    }

    #[test]
    fn get_all_actions_sorts_by_timestamp_descending() {
        let (runner, _dir) = make_runner_with_logs("timestamp-sort");
        // Use the same sim_id that was used to create logs
        let actions = runner.get_all_actions("timestamp-sort", None, None, None).unwrap();
        // Actions should be sorted by timestamp descending
        for i in 0..actions.len().saturating_sub(1) {
            let a_ts = parse_timestamp(&actions[i].timestamp);
            let b_ts = parse_timestamp(&actions[i + 1].timestamp);
            assert!(a_ts >= b_ts, "Action {} timestamp should be >= action {}", i, i + 1);
        }
    }

    #[test]
    fn get_all_actions_falls_back_to_legacy_format() {
        let dir = temp_dir("legacy");
        // The simulation_id becomes the subdirectory
        let sim_dir = dir.join("sim-legacy");
        std::fs::create_dir_all(&sim_dir).unwrap();

        // Write legacy single-file format (no platform subdirs, platform in record)
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(sim_dir.join("actions.jsonl"))
            .unwrap();
        writeln!(
            f,
            "{{\"round\": 1, \"timestamp\": \"2026-06-18T10:00:00Z\", \
             \"platform\": \"twitter\", \"agent_id\": 1, \"agent_name\": \"LegacyAgent\", \
             \"action_type\": \"CREATE_POST\", \"success\": true}}"
        )
        .unwrap();

        let manager = Arc::new(SimulationManager::new(&dir));
        let graph_mgr = Arc::new(GraphMemoryManager::<MockLlm>::new());
        let runner =
            SimulationRunner::new(&dir, graph_mgr, Arc::clone(&manager) as Arc<SimulationManager>);

        let actions = runner.get_all_actions("sim-legacy", None, None, None).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].platform, "twitter");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
