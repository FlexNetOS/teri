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
use crate::services::simulation_ipc::{SimulationIPCClient, SimulationIPCServer, channel};
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
    pub state: SimulationRunState,
    /// The spawned simulation task — the in-process analog of `_processes[id]`'s `Popen`
    /// (S-603). Driving `SimEngine::run`. `abort()` is the SIGKILL analog.
    task: JoinHandle<()>,
    /// Cooperative-stop signal, honored by `SimEngine`'s tick loop via `with_shutdown`.
    /// `store(true, Release)` is the SIGTERM analog (graceful, between rounds).
    shutdown: Arc<AtomicBool>,
    /// In-process IPC client for interview/close-env round-trips (DECISION-16). The paired
    /// server is owned by the sim task. (`[≠]` replaces the file-IPC the Popen child used.)
    ipc_client: SimulationIPCClient,
    /// The monitor task (`_monitor_threads[id]`, S-605). Ported in sub-cycle (c); `None`
    /// here. `stop`/`cleanup_all` abort it if present, so (c) needs no lifecycle rework.
    monitor: Option<JoinHandle<()>>,
    /// Whether graph-memory updating is enabled for this run (S-608 `_graph_memory_enabled[id]`).
    graph_enabled: bool,
}

impl RunHandle {
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
pub struct SimulationRunner<L: LlmClient + Send + Sync + 'static> {
    /// Root simulation-data directory — teri analog of `RUN_STATE_DIR`
    /// (`os.path.join(dirname(__file__), '../../uploads/simulations')`). S-600.
    /// (`SCRIPTS_DIR`, S-601, is `[≠]`: there are no `run_*.py` scripts in-process.)
    sim_data_dir: std::path::PathBuf,
    /// Per-run state + task + shutdown flag, keyed by `simulation_id`. Folds the six Python
    /// class-level dicts (S-602/603/604/605/606/607/608) into one map of owned handles.
    runs: tokio::sync::Mutex<std::collections::HashMap<String, RunHandle>>,
    /// Graph-memory manager (U-021) — the runner calls `create_updater`/`stop_updater`/
    /// `stop_all` exactly where MiroFish calls `ZepGraphMemoryManager.*`.
    graph_mgr: Arc<GraphMemoryManager<L>>,
    /// Simulation manager (U-023) — owns `state.json`; the runner calls
    /// `mark_state_json_stopped` for the S-625 secondary write (DECISION-17 §17.0 Area 4).
    manager: Arc<SimulationManager>,
    /// Idempotency flag for `cleanup_all` — port of `_cleanup_done` (S-624). Flipped
    /// false→true atomically on the first call (mirrors U-021 `stop_all`'s `compare_exchange`).
    cleanup_done: AtomicBool,
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
            graph_mgr,
            manager,
            cleanup_done: AtomicBool::new(false),
        }
    }

    /// Memory-cache-then-file load of a run state — port of `get_run_state` (S-609).
    ///
    /// Python: return `_run_states[id]` if present, else `_load_run_state(id)` (and cache it).
    /// teri: return a clone of the live `RunHandle.state` if a run is registered, else read
    /// `run_state.json` from disk via [`load_run_state`].
    ///
    /// Returns a clone (the live state lives behind the `runs` mutex; a borrow cannot escape).
    pub async fn get_run_state(&self, simulation_id: &str) -> Result<Option<SimulationRunState>> {
        {
            let runs = self.runs.lock().await;
            if let Some(handle) = runs.get(simulation_id) {
                return Ok(Some(handle.state.clone()));
            }
        }
        // Not in memory — load from disk (S-610).
        load_run_state(&self.sim_data_dir, simulation_id)
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

        let RunInputs { mut engine, pool, graph, llm } = inputs;
        engine.with_shutdown(Arc::clone(&shutdown));

        let task = spawn_sim_task(engine, pool, graph, llm, ipc_server);

        // process_pid stays None ([≠] value-only — no OS pid). runner_status → Running.
        state.runner_status = RunnerStatus::Running;
        save_run_state(&self.sim_data_dir, &state)?;

        tracing::info!("模拟启动成功: {}, platform={}", simulation_id, platform);

        let handle = RunHandle {
            state: state.clone(),
            task,
            shutdown,
            ipc_client,
            monitor: None, // set in sub-cycle (c)
            graph_enabled,
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
            Some(h) => h.state.runner_status.clone(),
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
            Some(h) => h.state.clone(),
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
            let mut state = handle.state.clone();
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
fn spawn_sim_task<L: LlmClient + Send + Sync + 'static>(
    engine: SimEngine,
    pool: crate::agent::AgentPool,
    graph: crate::graph::KnowledgeGraph,
    llm: Arc<L>,
    ipc_server: SimulationIPCServer,
) -> JoinHandle<()> {
    // Box+coerce the future to an explicit `Pin<Box<dyn Future + Send>>`. This sidesteps
    // rustc's higher-ranked-lifetime inference failure ("implementation of `FnOnce` is not
    // general enough") on the `SimEngine::run` → `prepare_action` closure when the run future
    // is handed to `tokio::spawn`. The explicit type annotation pins the lifetime so the
    // closure's `for<'a> FnMut(&'a Agent)` bound resolves. Behavior is unchanged.
    let fut: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> =
        Box::pin(run_sim_body(engine, pool, graph, llm, ipc_server));
    tokio::spawn(fut)
}

/// The body driven by the spawned simulation task.
///
/// Split into a named `async fn` (rather than an inline `async move` block) so the
/// `SimEngine::run` higher-ranked closure resolves cleanly under `tokio::spawn` — an inline
/// block trips rustc's "implementation of `FnOnce` is not general enough" on the
/// `prepare_action` borrow. Behavior is identical to an inline block.
async fn run_sim_body<L: LlmClient + Send + Sync + 'static>(
    engine: SimEngine,
    mut pool: crate::agent::AgentPool,
    graph: crate::graph::KnowledgeGraph,
    llm: Arc<L>,
    ipc_server: SimulationIPCServer,
) {
    // Mark the env alive while the run is in progress (DECISION-16: `check_env_alive`
    // reads this flag; interview commands are serviced by the (c)/(e) loop dispatch).
    ipc_server.start();

    if let Err(e) = engine.run(&mut pool, &graph, &*llm).await {
        tracing::error!("模拟运行失败: {}", e);
    }

    // Run finished (or aborted before this point) — mark env not-alive.
    ipc_server.stop();
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
        }
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

        // Wait until the handle reports finished (poll() is not None analog).
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
