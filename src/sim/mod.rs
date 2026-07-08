pub mod action_logger;
pub mod activation;
pub mod compute_world;
pub mod social_world;

use crate::agent::Platform;
use crate::models::project::python_isoformat_local;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

/// Discriminant for Like/Dislike actions: preserves the post-vs-comment distinction that
/// `to_episode_text` renders as "liked **post** X" vs "liked **comment** Y"
/// (zep_graph_memory_updater.py:_describe_like_post:70-81, _describe_like_comment:153-164).
/// Post and comment IDs belong to separate namespaces; collapsing them erases episode-text fidelity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TargetKind {
    Post,
    Comment,
}

/// MiroFish/OASIS social-media action taxonomy.
///
/// Sourced from:
/// - `backend/app/config.py`: `OASIS_TWITTER_ACTIONS` / `OASIS_REDDIT_ACTIONS`
/// - `backend/app/services/zep_graph_memory_updater.py`: `AgentActivity.to_episode_text` (12 types)
///
/// DO_NOTHING is excluded: the source (`add_activity`) skips it before recording, matching the
/// intentional omission in `to_episode_text`'s dispatch table.
///
/// `TREND` IS in `ACTION_TYPE_MAP` ('trend'→'TREND', run_parallel_simulation.py:627), NOT in
/// `FILTERED_ACTIONS` (only refresh/sign_up are filtered), and IS agent-selectable
/// (agent_action.py:507, OASIS_REDDIT_ACTIONS:197). It passes the filter → becomes an
/// `AgentActivity` → `_describe_generic` renders "performed TREND operation". Added here.
///
/// `REFRESH` IS in `FILTERED_ACTIONS` and is never an agent activity; correctly omitted (`- [≠]`).
///
/// Note: Exact arg naming follows zep_graph_memory_updater.py `action_args.get(...)` key patterns.
/// Like/Dislike carry a `target_kind` discriminant (Post vs Comment) to preserve the distinct
/// episode-text render paths (_describe_like_post vs _describe_like_comment).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SocialAction {
    /// CREATE_POST — args: content
    CreatePost { content: String },
    /// LIKE_POST — target_kind: Post; LIKE_COMMENT — target_kind: Comment
    Like { target_kind: TargetKind, target_id: String },
    /// DISLIKE_POST — target_kind: Post; DISLIKE_COMMENT — target_kind: Comment
    Dislike { target_kind: TargetKind, target_id: String },
    /// REPOST — args: post_id
    Repost { post_id: String },
    /// QUOTE_POST — args: post_id, content (the quote comment)
    Quote { post_id: String, content: String },
    /// FOLLOW — args: user_id
    Follow { user_id: String },
    /// CREATE_COMMENT — args: post_id, content
    Comment { post_id: String, content: String },
    /// SEARCH_POSTS — args: query
    SearchPosts { query: String },
    /// SEARCH_USER — args: query
    SearchUser { query: String },
    /// MUTE — args: user_id
    Mute { user_id: String },
    /// TREND — no args; browse/discovery operation rendered as "performed TREND operation"
    Trend,
    /// DO_NOTHING — no args
    DoNothing,
}

impl std::fmt::Display for SocialAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SocialAction::CreatePost { content } => write!(f, "Posted: {}", content),
            SocialAction::Like { target_kind: TargetKind::Post, target_id } => {
                write!(f, "Liked post: {}", target_id)
            }
            SocialAction::Like { target_kind: TargetKind::Comment, target_id } => {
                write!(f, "Liked comment: {}", target_id)
            }
            SocialAction::Dislike { target_kind: TargetKind::Post, target_id } => {
                write!(f, "Disliked post: {}", target_id)
            }
            SocialAction::Dislike { target_kind: TargetKind::Comment, target_id } => {
                write!(f, "Disliked comment: {}", target_id)
            }
            SocialAction::Repost { post_id } => write!(f, "Reposted: {}", post_id),
            SocialAction::Quote { post_id, content } => {
                write!(f, "Quoted post {}: {}", post_id, content)
            }
            SocialAction::Follow { user_id } => write!(f, "Followed user: {}", user_id),
            SocialAction::Comment { post_id, content } => {
                write!(f, "Commented on {}: {}", post_id, content)
            }
            SocialAction::SearchPosts { query } => write!(f, "Searched posts: {}", query),
            SocialAction::SearchUser { query } => write!(f, "Searched user: {}", query),
            SocialAction::Mute { user_id } => write!(f, "Muted user: {}", user_id),
            SocialAction::Trend => write!(f, "Performed trend operation"),
            SocialAction::DoNothing => write!(f, "Did nothing"),
        }
    }
}

impl SocialAction {
    /// OASIS `action_type` string written to `actions.jsonl` — the values of
    /// `ACTION_TYPE_MAP` (`run_parallel_simulation.py:614-629`). Deterministic and
    /// golden-testable: this is the byte-faithful half of the producer contract
    /// (the DB-internal `action_args` enrichment is the `[≠]U028-OASIS-INTERNALS` half).
    ///
    /// `Trend`/`DoNothing` map to their `ACTION_TYPE_MAP` entries (`TREND`/`DO_NOTHING`).
    pub fn oasis_action_type(&self) -> &'static str {
        match self {
            SocialAction::CreatePost { .. } => "CREATE_POST",
            SocialAction::Like { target_kind: TargetKind::Post, .. } => "LIKE_POST",
            SocialAction::Like { target_kind: TargetKind::Comment, .. } => "LIKE_COMMENT",
            SocialAction::Dislike { target_kind: TargetKind::Post, .. } => "DISLIKE_POST",
            SocialAction::Dislike { target_kind: TargetKind::Comment, .. } => "DISLIKE_COMMENT",
            SocialAction::Repost { .. } => "REPOST",
            SocialAction::Quote { .. } => "QUOTE_POST",
            SocialAction::Follow { .. } => "FOLLOW",
            SocialAction::Comment { .. } => "CREATE_COMMENT",
            SocialAction::SearchPosts { .. } => "SEARCH_POSTS",
            SocialAction::SearchUser { .. } => "SEARCH_USER",
            SocialAction::Mute { .. } => "MUTE",
            SocialAction::Trend => "TREND",
            SocialAction::DoNothing => "DO_NOTHING",
        }
    }

    /// Structural `action_args` object for the `actions.jsonl` record — teri's native
    /// representation, keyed exactly as `Agent::parse_social_action` parses them (so a record
    /// round-trips through teri's own action parser). This is the no-world path: callers that have
    /// no `SocialWorld` (e.g. unit tests, the round-0 seed) emit exactly these structural fields.
    ///
    /// The richer keys (`post_content`, `author_name`, `comment_content`, `quote_content`,
    /// `target_user_name`) that `run_parallel_simulation.py:_enrich_action_context` resolves out of
    /// the OASIS `post`/`comment`/`user` tables are added by [`SocialAction::oasis_action_args_enriched`],
    /// which resolves them from teri's [`crate::sim::social_world::SocialWorld`] (teri DOES hold the
    /// post/comment/user graph — see `social_world.rs` — so the enrichment is real, not a DB
    /// internal we lack).
    pub fn oasis_action_args(&self) -> serde_json::Value {
        match self {
            SocialAction::CreatePost { content } => serde_json::json!({ "content": content }),
            SocialAction::Like { target_id, .. } => serde_json::json!({ "target_id": target_id }),
            SocialAction::Dislike { target_id, .. } => {
                serde_json::json!({ "target_id": target_id })
            }
            SocialAction::Repost { post_id } => serde_json::json!({ "post_id": post_id }),
            SocialAction::Quote { post_id, content } => {
                serde_json::json!({ "post_id": post_id, "content": content })
            }
            SocialAction::Follow { user_id } => serde_json::json!({ "user_id": user_id }),
            SocialAction::Comment { post_id, content } => {
                serde_json::json!({ "post_id": post_id, "content": content })
            }
            SocialAction::SearchPosts { query } => serde_json::json!({ "query": query }),
            SocialAction::SearchUser { query } => serde_json::json!({ "query": query }),
            SocialAction::Mute { user_id } => serde_json::json!({ "user_id": user_id }),
            SocialAction::Trend | SocialAction::DoNothing => serde_json::json!({}),
        }
    }

    /// Enriched `action_args`: the structural fields from [`SocialAction::oasis_action_args`] PLUS
    /// the context fields `run_parallel_simulation.py:_enrich_action_context` resolves out of the
    /// social tables, looked up from `world`:
    /// - actions targeting a post (`Like`/`Dislike` on a post, `Repost`, `Quote`, `Comment`):
    ///   `post_content` + `author_name` of the targeted post (author resolved via the user
    ///   registry).
    /// - actions targeting a comment (`Like`/`Dislike` on a comment): `comment_content`.
    /// - `Quote`: `quote_content` (the quote's own text — already present as `content`, mirrored
    ///   under the dedicated key MiroFish writes).
    /// - `Follow`/`Mute`: `target_user_name` of the targeted user (resolved via the registry when
    ///   the arg is a numeric id; otherwise the raw handle the agent emitted).
    ///
    /// Fail-soft like MiroFish: an unresolved post/comment/user simply omits the key it could not
    /// resolve (MiroFish's `_get_post_info`/`_get_user_name` return `None`/`''` and skip the
    /// assignment). The structural keys are always identical to [`SocialAction::oasis_action_args`].
    pub fn oasis_action_args_enriched(
        &self,
        world: &crate::sim::social_world::SocialWorld,
    ) -> serde_json::Value {
        use crate::sim::social_world::parse_target_id;
        let mut args = self.oasis_action_args();
        let obj = match args.as_object_mut() {
            Some(obj) => obj,
            None => return args, // {} for Trend/DoNothing — nothing to enrich.
        };

        // Resolve a post's content + author display name into the arg object (no-op if unresolved).
        let enrich_post = |obj: &mut serde_json::Map<String, serde_json::Value>, raw: &str| {
            if let Some(post) = parse_target_id(raw).and_then(|id| world.post_by_id(id)) {
                obj.insert("post_content".into(), post.content.clone().into());
                if let Some(name) = world.user_name(post.author_user_id) {
                    obj.insert("author_name".into(), name.to_string().into());
                }
            }
        };
        // Resolve the target user's display name into `target_user_name`. A numeric arg is looked
        // up in the registry; a non-numeric arg IS the handle the agent emitted, so it is used
        // directly (MiroFish keeps the raw handle when it cannot resolve a numeric id).
        let enrich_user = |obj: &mut serde_json::Map<String, serde_json::Value>, raw: &str| {
            let name = parse_target_id(raw)
                .and_then(|id| world.user_name(id).map(str::to_string))
                .unwrap_or_else(|| raw.to_string());
            if !name.is_empty() {
                obj.insert("target_user_name".into(), name.into());
            }
        };

        match self {
            SocialAction::Like { target_kind: TargetKind::Post, target_id }
            | SocialAction::Dislike { target_kind: TargetKind::Post, target_id } => {
                enrich_post(obj, target_id);
            }
            SocialAction::Like { target_kind: TargetKind::Comment, target_id }
            | SocialAction::Dislike { target_kind: TargetKind::Comment, target_id } => {
                if let Some(comment) =
                    parse_target_id(target_id).and_then(|id| world.comment_by_id(id))
                {
                    obj.insert("comment_content".into(), comment.content.clone().into());
                }
            }
            SocialAction::Repost { post_id } | SocialAction::Comment { post_id, .. } => {
                enrich_post(obj, post_id);
            }
            SocialAction::Quote { post_id, content } => {
                enrich_post(obj, post_id);
                obj.insert("quote_content".into(), content.clone().into());
            }
            SocialAction::Follow { user_id } | SocialAction::Mute { user_id } => {
                enrich_user(obj, user_id);
            }
            // No targeted entity: structural args only.
            SocialAction::CreatePost { .. }
            | SocialAction::SearchPosts { .. }
            | SocialAction::SearchUser { .. }
            | SocialAction::Trend
            | SocialAction::DoNothing => {}
        }
        args
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Action {
    // --- Generic simulation actions (pre-existing; must not be altered) ---
    Speak(String),
    Move(String),
    Interact(String),
    Observe(String),
    Think(String),
    // --- MiroFish/OASIS social-media actions ---
    /// Wraps the OASIS social taxonomy. Using a nested enum keeps all generic match arms
    /// untouched (no churn) and colocalizes the 11 new social arms in `SocialAction`.
    Social(SocialAction),
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Speak(content) => write!(f, "Spoke: {}", content),
            Action::Move(location) => write!(f, "Moved to: {}", location),
            Action::Interact(target) => write!(f, "Interacted with: {}", target),
            Action::Observe(target) => write!(f, "Observed: {}", target),
            Action::Think(content) => write!(f, "Thought: {}", content),
            Action::Social(sa) => write!(f, "Social: {}", sa),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub agent_id: Uuid,
    pub action: Action,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub id: Uuid,
    pub name: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    pub tick: u32,
    pub agents: HashMap<Uuid, AgentSnapshot>,
    pub events: Vec<Event>,
    pub variables: HashMap<String, f32>,
}

impl WorldState {
    pub fn new() -> Self {
        Self {
            tick: 0,
            // Pre-allocate with typical small-pool capacity to avoid early rehashing.
            agents: HashMap::with_capacity(16),
            events: Vec::with_capacity(16),
            variables: HashMap::with_capacity(8),
        }
    }

    pub fn add_agent_snapshot(&mut self, agent_id: Uuid, snapshot: AgentSnapshot) {
        self.agents.insert(agent_id, snapshot);
    }

    pub fn add_event(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn inject_variable(&mut self, key: String, value: f32) {
        self.variables.insert(key, value);
    }

    pub fn get_variable(&self, key: &str) -> Option<f32> {
        self.variables.get(key).copied()
    }

    pub fn apply(&mut self, agent_id: Uuid, action: Action) {
        self.apply_at(agent_id, action, chrono::Utc::now());
    }

    pub fn apply_at(
        &mut self,
        agent_id: Uuid,
        action: Action,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) {
        let event = Event { agent_id, action, timestamp };
        self.events.push(event);
    }

    /// Advance to the next tick, clearing per-tick events.
    ///
    /// Invariant: `events` must contain at most one entry per registered agent.
    /// Callers (SimEngine) are responsible for enforcing this; violations are
    /// caught in debug builds via the assert below.
    pub fn advance_tick(&mut self) {
        debug_assert!(
            self.events.len() <= self.agents.len().max(1) * 2,
            "events ({}) exceeded expected per-tick budget ({}); inject_fn may be over-publishing",
            self.events.len(),
            self.agents.len() * 2,
        );
        self.tick += 1;
        self.events.clear();
    }

    pub fn snapshot(&self) -> WorldSnapshot {
        WorldSnapshot {
            tick: self.tick,
            agents: self.agents.clone(),
            events: self.events.clone(),
            variables: self.variables.clone(),
        }
    }
}

impl Default for WorldState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldSnapshot {
    pub tick: u32,
    pub agents: HashMap<Uuid, AgentSnapshot>,
    pub events: Vec<Event>,
    pub variables: HashMap<String, f32>,
}

impl WorldSnapshot {
    /// Get a variable value from the world snapshot.
    ///
    /// This provides the same interface as `WorldState::get_variable()`.
    ///
    /// # Arguments
    /// * `key` - Variable name to lookup
    ///
    /// # Returns
    /// * `Some(value)` if the variable exists
    /// * `None` if the variable does not exist
    pub fn get_variable(&self, key: &str) -> Option<f32> {
        self.variables.get(key).copied()
    }
}

pub type InjectFn = std::sync::Arc<dyn Fn(u32, &mut WorldState) + Send + Sync>;

/// Configuration for simulation execution.
///
/// Defines tick limits, parallelism level, and an optional injection function
/// for external control of world state (the "God's-eye" mechanism).
///
/// The injection function allows external code to modify the simulation state
/// at each tick, enabling "what-if" scenarios or external control systems.
///
/// # Fields
///
/// * `max_ticks` - Maximum number of simulation ticks to run before stopping
/// * `parallelism` - Max concurrent LLM calls per tick (used by `SimEngine::run`)
/// * `inject_fn` - Optional function called at each tick to modify world state
///
/// # Memory characteristics
///
/// `SimEngine::run` holds all tick snapshots in memory for the full duration
/// of the simulation. Memory usage is approximately
/// `O(max_ticks * agent_count * snapshot_size)`. For large simulations
/// (e.g. `max_ticks > 1000` with large pools), monitor heap usage and
/// consider reducing `max_ticks` or snapshotting to disk.
///
/// # Note on `Clone`
///
/// `SimConfig` implements `Clone` because the injection function is wrapped in
/// `Arc<dyn Fn>`, which is shareable across threads.
///
/// # Example
///
/// ```ignore
/// let config = SimConfig::new(100, 8)
///     .with_inject_fn(|tick, world| {
///         if tick == 50 {
///             world.inject_variable("halfway".to_string(), 1.0);
///         }
///     });
/// ```
#[derive(Clone)]
pub struct SimConfig {
    pub max_ticks: u32,
    /// Maximum number of concurrent LLM calls per tick.
    /// Controls `stream::buffered(parallelism)` in `SimEngine::run`.
    /// Set to 1 to execute agents sequentially; higher values increase
    /// throughput at the cost of additional concurrent HTTP connections.
    pub parallelism: usize,
    pub inject_fn: Option<InjectFn>,
}

impl SimConfig {
    /// Create a new `SimConfig` with the specified tick limit and parallelism.
    ///
    /// The injection function is not set; use `with_inject_fn()` to add one.
    ///
    /// # Arguments
    ///
    /// * `max_ticks` - Maximum number of simulation ticks to run
    /// * `parallelism` - Number of threads for parallel agent execution
    ///
    /// # Example
    ///
    /// ```
    /// # use teri::sim::SimConfig;
    /// let config = SimConfig::new(100, 8);
    /// ```
    pub fn new(max_ticks: u32, parallelism: usize) -> Self {
        Self { max_ticks, parallelism, inject_fn: None }
    }

    /// Derive a runtime [`SimConfig`] from a prepared `simulation_config.json` artifact
    /// (U-019 `SimulationParameters::to_dict()` shape).
    ///
    /// This is the deterministic config→engine mapping that closes the `u026-g` row-1 gap
    /// ("the config→total_rounds mapping does not parametrize the engine"): `max_ticks` is the
    /// simulation's total round count, computed **identically** to
    /// `SimulationRunner::start_simulation` (`simulation_runner.rs:1091-1118`, the status-field
    /// `total_rounds`) so the engine and the run-state derive ticks from the same formula.
    ///
    /// # Mapping (Python source on the left)
    /// - `max_ticks` ← `time_config.total_simulation_hours` (default 72) × 60 ÷
    ///   `time_config.minutes_per_round` (default 30), truncated toward zero. The scripts use
    ///   floor-division `(total_hours*60)//minutes_per_round` (`run_twitter_simulation.py:550`,
    ///   `run_reddit_simulation.py:539`); the service primitive uses `int(total_hours*60/mpr)`
    ///   (`simulation_runner.py:353`). Both are identical over the reachable domain (`hours ≥ 0`,
    ///   `mpr > 0`) — they diverge only for a negative `total_hours`, which the config generator
    ///   never produces. teri mirrors the service primitive's float-div-truncate so there is ONE
    ///   truncation impl. A zero cadence yields 0 rounds (Python would raise `ZeroDivisionError`;
    ///   teri treats a non-positive cadence as "no truncation basis").
    /// - `max_rounds` truncation: when `Some(mr)` with `mr > 0`, `max_ticks = min(total, mr)`
    ///   (`run_twitter_simulation.py:553-557`).
    /// - `parallelism` is caller-supplied (the run's LLM concurrency; OASIS used `semaphore=30`).
    /// - `inject_fn` is `None`; the time-based activation policy (U-028 §4) installs it later.
    ///
    /// The default fallbacks (72 / 30) match the scripts' `.get(key, default)` — they fire only
    /// when a key is absent; a real U-019 artifact always carries both keys explicitly.
    pub fn from_simulation_config(
        config: &serde_json::Value,
        max_rounds: Option<i64>,
        parallelism: usize,
    ) -> Self {
        let time_config = config.get("time_config");
        let total_hours = time_config
            .and_then(|t| t.get("total_simulation_hours"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(72);
        let minutes_per_round = time_config
            .and_then(|t| t.get("minutes_per_round"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(30);

        // Python `int(total_hours * 60 / minutes_per_round)` — float division then truncate
        // toward zero. Guard the zero divisor (Python ZeroDivisionError → teri 0-rounds).
        let mut total_rounds: i64 = if minutes_per_round != 0 {
            ((total_hours as f64 * 60.0) / minutes_per_round as f64) as i64
        } else {
            0
        };

        // max_rounds truncation (only when positive), mirroring start_simulation.
        if let Some(mr) = max_rounds
            && mr > 0
        {
            total_rounds = total_rounds.min(mr);
        }

        // `max_ticks` is u32; Python `range(total_rounds)` with a negative count iterates zero
        // times. Clamp to the u32 domain (a negative or absurd round count cannot panic).
        let max_ticks = total_rounds.clamp(0, u32::MAX as i64) as u32;
        Self { max_ticks, parallelism, inject_fn: None }
    }

    /// Register an injection function to modify world state at each tick.
    ///
    /// The injection function is called by the simulation engine at each tick
    /// with the current tick number and a mutable reference to the `WorldState`.
    /// This allows external code to inject or modify world variables based on
    /// the simulation progress (the "God's-eye" mechanism).
    ///
    /// # Arguments
    ///
    /// * `inject_fn` - A function that takes (tick: u32, world: &mut WorldState)
    ///
    /// # Example
    ///
    /// ```
    /// # use teri::sim::SimConfig;
    /// let config = SimConfig::new(100, 4)
    ///     .with_inject_fn(|tick, world| {
    ///         // Increase temperature every 10 ticks
    ///         if tick % 10 == 0 {
    ///             let current_temp = world.get_variable("temp").unwrap_or(20.0);
    ///             world.inject_variable("temp".to_string(), current_temp + 1.0);
    ///         }
    ///     });
    /// ```
    pub fn with_inject_fn<F>(mut self, inject_fn: F) -> Self
    where
        F: Fn(u32, &mut WorldState) + Send + Sync + 'static,
    {
        self.inject_fn = Some(std::sync::Arc::new(inject_fn));
        self
    }
}

impl std::fmt::Debug for SimConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimConfig")
            .field("max_ticks", &self.max_ticks)
            .field("parallelism", &self.parallelism)
            .field("inject_fn", &self.inject_fn.as_ref().map(|_| "<function>"))
            .finish()
    }
}

impl Default for SimConfig {
    fn default() -> Self {
        Self { max_ticks: 50, parallelism: 8, inject_fn: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub id: Uuid,
    pub history: Vec<WorldSnapshot>,
}

impl SimulationResult {
    /// Returns a reference to the last snapshot in history, i.e. the final world state.
    pub fn final_snapshot(&self) -> Option<&WorldSnapshot> {
        self.history.last()
    }
}

/// Callback type for snapshot hooks registered with `SimEngine`.
/// Each hook is called once per tick with a clone of the tick's snapshot.
pub type SnapshotHook = Arc<dyn Fn(WorldSnapshot) + Send + Sync>;

/// God's-eye runtime injection queue: shared `(variable, value)` entries pushed from outside a
/// live run (REST `POST /:id/inject`) and drained by the engine at each tick boundary.
pub type InjectionQueue = Arc<Mutex<Vec<(String, f32)>>>;

/// Terminal signal emitted once by `SimEngine::run()` when the tick loop completes.
///
/// Mirrors MiroFish `action_logger.log_simulation_end` / `simulation_runner.py` monitor
/// that detects `simulation_end` on the action stream to mark a sim completed.
///
/// `total_ticks` is the count of ticks that were actually executed (== `SimConfig::max_ticks`
/// in the normal case, or fewer if the loop was interrupted by an error — though run() returns
/// Err in that case, so subscribers should treat a Completed signal as always-clean).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimCompletion {
    /// Number of ticks executed before the simulation ended.
    pub total_ticks: u32,
}

/// A per-tick agent-activation gate (U-028 §4).
///
/// The OASIS subprocess selected a stochastic, time-of-day-weighted subset of agents to
/// `env.step` each round (`run_twitter_simulation.py:_get_active_agents_for_round`). teri maps
/// this onto an optional policy consulted by [`SimEngine::run`]: for each tick the engine asks
/// for the active agent ids; only those agents `prepare_action` that tick, mirroring Python's
/// `if not active_agents: continue` (an empty set → no agent acts that round).
///
/// **Additive, opt-in.** When no policy is installed (every pre-existing caller),
/// `SimEngine::run` activates *every* agent each tick exactly as before. The concrete
/// time-based policy is [`crate::sim::activation::TimeActivationPolicy`].
pub trait ActivationPolicy: Send + Sync {
    /// The agent ids (OASIS numeric ids — `SocialProfile.user_id`) active for `tick`.
    /// `SimEngine::run` maps these onto pool agents by `user_id`; agents whose id is absent
    /// skip `prepare_action` that tick. An empty `Vec` means no agent acts this round.
    fn active_agent_ids(&self, tick: u32) -> Vec<i64>;
}

/// The set of per-platform action loggers a [`RunProducer`] fans records out to (U-030 §1,
/// DECISION-U030-1).
///
/// - **Single-platform** (the U-028 case): exactly one entry (twitter-only OR reddit-only). The
///   unified loop routes every record to it — byte-identical to the pre-U030 single-`logger` field.
/// - **Parallel** (U-030): two entries (twitter + reddit). Boundary records (`simulation_start` /
///   `round_start` / `round_end` / `simulation_end`) fan out to ALL loggers; each `log_action`
///   routes to the committing agent's platform logger (so reddit actions land in
///   `reddit/actions.jsonl`, not twitter's — the misroute that forced U-028 to defer parallel).
///
/// Backed by a `Vec` (not a `HashMap`) because there are at most 2 platforms: a linear scan over
/// ≤2 entries is trivial and keeps insertion order (twitter-before-reddit) deterministic.
pub struct PlatformLoggerSet {
    /// Invariant: 1 entry (single-platform) or 2 (parallel); never empty.
    loggers: Vec<(Platform, Arc<action_logger::PlatformActionLogger>)>,
}

impl PlatformLoggerSet {
    /// Single-platform set (the U-028 case). `platform` is the producer's one platform.
    pub fn single(platform: Platform, logger: Arc<action_logger::PlatformActionLogger>) -> Self {
        Self { loggers: vec![(platform, logger)] }
    }

    /// Parallel dual set (twitter + reddit), insertion order twitter-before-reddit.
    pub fn parallel(
        twitter: Arc<action_logger::PlatformActionLogger>,
        reddit: Arc<action_logger::PlatformActionLogger>,
    ) -> Self {
        Self { loggers: vec![(Platform::Twitter, twitter), (Platform::Reddit, reddit)] }
    }

    /// All `(platform, logger)` pairs, for boundary-record fan-out.
    fn iter(&self) -> impl Iterator<Item = &(Platform, Arc<action_logger::PlatformActionLogger>)> {
        self.loggers.iter()
    }

    /// The logger for `platform`, if installed (used to route a `log_action`).
    fn get(&self, platform: Platform) -> Option<&Arc<action_logger::PlatformActionLogger>> {
        self.loggers.iter().find(|(p, _)| *p == platform).map(|(_, l)| l)
    }

    /// The platforms present in this set (used to seed [`PerPlatform`] accumulators).
    fn platforms(&self) -> impl Iterator<Item = Platform> + '_ {
        self.loggers.iter().map(|(p, _)| *p)
    }
}

/// A tiny fixed-size (≤2 platform) accumulator keyed by [`Platform`] (U-030 §2,
/// DECISION-U030-2). Seeded from a [`PlatformLoggerSet`] so a single-platform run holds exactly one
/// slot. Used for per-platform round and total action counts in `round_end` / `simulation_end`.
struct PerPlatform<T> {
    slots: Vec<(Platform, T)>,
}

impl<T: Copy + Default + std::ops::AddAssign> PerPlatform<T> {
    /// One zeroed slot per platform installed in `set`.
    fn zeroed(set: &PlatformLoggerSet) -> Self {
        Self { slots: set.platforms().map(|p| (p, T::default())).collect() }
    }

    /// Add `delta` to `platform`'s slot (no-op if the platform is not installed — unreachable under
    /// the §3 routing invariant, but a safe fail-closed default).
    fn add(&mut self, platform: Platform, delta: T) {
        if let Some(slot) = self.slots.iter_mut().find(|(p, _)| *p == platform) {
            slot.1 += delta;
        }
    }

    /// The accumulated value for `platform` (`T::default()` if not installed).
    fn get(&self, platform: Platform) -> T {
        self.slots
            .iter()
            .find(|(p, _)| *p == platform)
            .map(|(_, v)| *v)
            .unwrap_or_default()
    }
}

/// The `actions.jsonl` producer wiring for a run (U-028 §5 / U-030 §1, DECISION-U028-3 /
/// DECISION-U030-1).
///
/// Holds the per-platform logger set ([`PlatformLoggerSet`]) and the run's config (for
/// `log_simulation_start`'s `total_rounds`/`agents_count` and the `minutes_per_round` used to
/// derive each round's `simulated_hour`). When installed via [`SimEngine::with_producer`],
/// `run()` emits the full Python producer stream
/// (`run_parallel_simulation.py:run_twitter_simulation` structure): one `simulation_start`, then
/// per tick a `round_start` / N× `log_action` / `round_end`, then one `simulation_end`. For a
/// parallel run both platforms' streams are emitted (boundary records fanned out, actions routed).
/// The landed monitor (`spawn_monitor_task`) tails each platform file and marks the run COMPLETED
/// once every enabled platform's `simulation_end` record is seen.
///
/// **Additive, opt-in.** When no producer is installed, `run()` writes nothing (identical to
/// pre-existing behavior).
pub struct RunProducer {
    /// The per-platform JSONL writers (`{sim_dir}/{platform}/actions.jsonl`). One for
    /// single-platform, two for parallel.
    pub loggers: PlatformLoggerSet,
    /// The run config (`simulation_config.json` shape) — read for `log_simulation_start` and
    /// the `time_config.minutes_per_round` used to compute each round's `simulated_hour`.
    pub config: serde_json::Value,
}

impl RunProducer {
    /// `time_config.minutes_per_round` (default 30 — the script fallback,
    /// `run_parallel_simulation.py:1216`).
    fn minutes_per_round(&self) -> i64 {
        self.config
            .get("time_config")
            .and_then(|tc| tc.get("minutes_per_round"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(30)
    }
}

/// Map an `actions.jsonl` write failure into a `TeriError::Sim`. The producer is part of the
/// run, so a failed log write aborts it — matching Python's unguarded `action_logger.*` calls
/// (a write exception there propagates out of the platform coroutine).
fn log_err(record: &str, e: std::io::Error) -> crate::error::TeriError {
    crate::error::TeriError::Sim(format!("actions.jsonl {record} write failed: {e}"))
}

pub struct SimEngine {
    config: SimConfig,
    snapshot_tx: broadcast::Sender<WorldSnapshot>,
    snapshot_history: Arc<Mutex<Vec<WorldSnapshot>>>,
    /// Registered snapshot hooks (e.g. TickBuffer adapters for HTTP streaming).
    snapshot_hooks: Vec<SnapshotHook>,
    /// Watch channel carrying the terminal completion signal.
    ///
    /// Initialized to `None`. `run()` sends `Some(SimCompletion { total_ticks })` exactly
    /// once, after the last snapshot has been broadcast and pushed to history.
    ///
    /// `watch` is chosen over `broadcast` so late subscribers always observe the final
    /// value — a late-arriving SSE handler can call `subscribe_completion()` after `run()`
    /// has already returned and will still see `Some(...)` without a race.
    ///
    /// tokio `watch::Sender::send()` only updates the stored value when at least one
    /// `Receiver` is alive. `_completion_anchor` is that receiver — it keeps the channel
    /// "alive" so that the `send()` in `run()` always persists the `Some(...)` value,
    /// making it observable to any receiver created after `run()` completes.
    completion_tx: watch::Sender<Option<SimCompletion>>,
    _completion_anchor: watch::Receiver<Option<SimCompletion>>,
    /// Optional cooperative-shutdown flag, checked at the top of every tick in `run()`.
    ///
    /// **Additive, opt-in.** Defaults to `None`; when `None`, `run()` behaves exactly as
    /// before (runs the full `max_ticks` loop). When set (via [`SimEngine::with_shutdown`])
    /// and flipped to `true` by an external owner, `run()` breaks out of the tick loop at the
    /// next tick boundary — *gracefully*: the snapshots produced so far are kept, the
    /// completion signal is still emitted with the partial `total_ticks`, and `run()` returns
    /// `Ok(SimulationResult)` with the partial history.
    ///
    /// This is the in-process analog of MiroFish's `os.killpg(pgid, SIGTERM)` cooperative
    /// terminate (`simulation_runner.py:_terminate_process` L759-774): the running simulation
    /// is asked to stop between rounds rather than killed mid-round. The hard-kill analog
    /// (SIGKILL) is the caller's `JoinHandle::abort()` after the 5s grace window
    /// (`SimulationRunner::stop_simulation`).
    ///
    /// Existing callers that never call `with_shutdown` observe identical behavior — this
    /// field is `None` for them and the per-tick check is a no-op.
    shutdown: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Optional per-tick agent-activation gate (U-028 §4). `None` → every agent acts each tick
    /// (pre-existing behavior). `Some` → only `policy.active_agent_ids(tick)` (matched to pool
    /// agents by `SocialProfile.user_id`) `prepare_action` that tick.
    activation: Option<Arc<dyn ActivationPolicy>>,
    /// Optional `actions.jsonl` producer (U-028 §5). `None` → `run()` writes no action log.
    /// `Some` → `run()` emits the full `simulation_start` / per-round / `simulation_end` stream.
    producer: Option<RunProducer>,
    /// Optional social-world substrate (Workstream C). `None` → no post-graph, no
    /// `{platform}_simulation.db`, no feed-back — every pre-existing caller + teri's generic swarm
    /// mode is byte-identical. `Some` → `run_with_boost` seeds round-0 posts, snapshots a feed into
    /// each tick's prompts, applies committed `Action::Social`s into the world ALONGSIDE the
    /// untouched `actions.jsonl` log, and (with the `sqlite` feature) flushes the world to
    /// `{sim_dir}/{platform}_simulation.db` per round so `/posts` + `/comments` return real data.
    ///
    /// Wrapped in a `Mutex` because `run_with_boost` takes `&self` (the engine is shared by ref
    /// into the run future) but the world must be mutated (apply / flush) across the run. The lock
    /// is uncontended — `run_with_boost` is the sole accessor, single-threaded across ticks.
    social: Option<Mutex<social_world::SocialWorldSet>>,
    /// Optional compute-world substrate (execution-effect twin, world-type #2). `None` → the
    /// engine has no compute domain and every pre-existing caller is byte-identical. `Some` →
    /// [`SimEngine::predict_compute_plan`] can forecast a command plan's filesystem effects,
    /// exit, and risk against a cell's twin WITHOUT executing anything (the deductive
    /// real-to-sim-to-real analog of `run_with_boost` for the compute domain).
    ///
    /// Wrapped in a `Mutex` for the same reason as `social`: the forecast mutates the twin
    /// (applying predicted effects into its state oracle) while the engine is shared by `&self`.
    compute: Option<Mutex<compute_world::ComputeWorldSet>>,
    /// God's-eye runtime injection queue (additive, opt-in). `None` → no runtime injection.
    /// `Some` → at each tick boundary `run()` drains the queue and applies every pending
    /// `(key, value)` via `WorldState::inject_variable` BEFORE the optional `inject_fn` runs, so
    /// an operator can push variables into a LIVE simulation from outside (REST `POST /:id/inject`)
    /// and the value persists in `world.variables` (and every subsequent snapshot) thereafter.
    /// Wrapped in a `Mutex<Vec<…>>` shared with the runner's `RunHandle`. Callers that never call
    /// `with_injections` observe byte-identical behavior.
    injections: Option<InjectionQueue>,
}

impl SimEngine {
    pub fn new(config: SimConfig) -> Self {
        // Fixed capacity of 64: gives slow receivers a short grace window before
        // RecvError::Lagged. History replay via subscribe_with_history() covers
        // ticks beyond the 64-slot window.
        let (snapshot_tx, _snapshot_rx) = broadcast::channel(64);
        // Completion watch starts None; run() flips it to Some(SimCompletion) once.
        // The anchor receiver is kept alive in the struct so that send() in run() always
        // updates the stored value (tokio watch: send fails silently if no receivers exist).
        let (completion_tx, completion_anchor) = watch::channel(None);
        Self {
            config,
            snapshot_tx,
            snapshot_history: Arc::new(Mutex::new(Vec::new())),
            snapshot_hooks: Vec::new(),
            completion_tx,
            _completion_anchor: completion_anchor,
            shutdown: None,
            activation: None,
            producer: None,
            social: None,
            compute: None,
            injections: None,
        }
    }

    /// Install a per-tick agent-activation gate (U-028 §4, additive, opt-in).
    ///
    /// When set, `run()` consults `policy.active_agent_ids(tick)` each tick and only the agents
    /// whose `SocialProfile.user_id` is in that set `prepare_action`; the rest are skipped
    /// (mirroring Python's `if not active_agents: continue`). When never called, every agent
    /// acts each tick — identical to the behavior before this method existed.
    pub fn with_activation(&mut self, policy: Arc<dyn ActivationPolicy>) {
        self.activation = Some(policy);
    }

    /// Install the `actions.jsonl` producer (U-028 §5 / DECISION-U028-3, additive, opt-in).
    ///
    /// When set, `run()` emits the full Python producer stream (`simulation_start`, per-round
    /// `round_start`/`log_action`/`round_end`, `simulation_end`) to
    /// `{sim_dir}/{platform}/actions.jsonl` so the landed monitor can tail it and mark the run
    /// COMPLETED. When never called, `run()` writes no action log.
    pub fn with_producer(&mut self, producer: RunProducer) {
        self.producer = Some(producer);
    }

    /// Install the social-world substrate (Workstream C, additive, opt-in).
    ///
    /// When set, `run_with_boost` seeds round-0 initial posts into the world, snapshots a
    /// recency-ranked feed into each tick's social-agent prompts, applies each committed
    /// `Action::Social` into the world (ALONGSIDE the untouched `actions.jsonl` producer log), and
    /// — with the `sqlite` feature — flushes the world to `{sim_dir}/{platform}_simulation.db` each
    /// round so the `/posts` + `/comments` readers return real data. When never called, the run is
    /// byte-identical to before this method existed (no post-graph, no DB file, no feed section).
    pub fn with_social(&mut self, set: social_world::SocialWorldSet) {
        self.social = Some(Mutex::new(set));
    }

    /// Install the compute-world substrate (execution-effect twin, world-type #2; additive,
    /// opt-in — mirrors [`with_social`](Self::with_social)).
    ///
    /// When set, [`predict_compute_plan`](Self::predict_compute_plan) can forecast a command
    /// plan against a cell's twin. When never called, the engine has no compute domain and
    /// every pre-existing run is byte-identical to before this method existed.
    pub fn with_compute(&mut self, set: compute_world::ComputeWorldSet) {
        self.compute = Some(Mutex::new(set));
    }

    /// **Predict a command plan** for `cell` against its installed [`compute_world::ComputeWorld`]
    /// twin, WITHOUT executing anything — the deductive analog of [`run_with_boost`](Self::run_with_boost)
    /// for the compute domain. Each step deduces against the prior steps' predicted mutations
    /// (Holmesian carry-over), and the result aggregates the plan's blast radius, weakest-link
    /// confidence, and first predicted failure into a [`compute_world::ComputeRollout`].
    ///
    /// Returns `None` when no compute substrate is installed (no [`with_compute`](Self::with_compute))
    /// or `cell` is unknown to the set — a missing domain is not an error, exactly as a missing
    /// `social` substrate simply skips the social path.
    pub fn predict_compute_plan(
        &self,
        cell: &str,
        actions: &[compute_world::ComputeAction],
    ) -> Option<compute_world::ComputeRollout> {
        let mut set = self.compute.as_ref()?.lock();
        let world = set.world_mut(cell)?;
        let effects = world.predict_plan(actions);
        Some(compute_world::ComputeRollout::from_effects(cell.to_string(), effects))
    }

    /// Install a cooperative-shutdown flag, checked at the top of every tick in `run()`.
    ///
    /// **Additive, opt-in.** When the shared `AtomicBool` is flipped to `true`, the next
    /// `run()` tick boundary breaks the loop gracefully (partial history kept, completion
    /// signal emitted with the partial tick count, `Ok` returned). Callers that never call
    /// this method observe identical behavior to before this method existed.
    ///
    /// Used by `SimulationRunner::stop_simulation` (U-022 sub-cycle b) to map MiroFish's
    /// `os.killpg(pgid, SIGTERM)` graceful-terminate onto an in-process cooperative stop,
    /// with `JoinHandle::abort()` after a 5s grace window as the SIGKILL analog.
    ///
    /// The flag is read with `Ordering::Acquire` to pair with the owner's `store(true,
    /// Ordering::Release)`.
    pub fn with_shutdown(&mut self, flag: Arc<std::sync::atomic::AtomicBool>) {
        self.shutdown = Some(flag);
    }

    /// Install the God's-eye runtime injection queue (additive, opt-in).
    ///
    /// The shared `Vec<(key, value)>` is drained at every tick boundary and each entry applied via
    /// `WorldState::inject_variable` (the injected value then persists in `world.variables` and in
    /// every subsequent snapshot). An external caller (REST `POST /api/simulation/:id/inject` →
    /// `SimulationRunner::inject_variable`) pushes to this same `Arc`, so variables can be injected
    /// into a LIVE run from a god's-eye view. Callers that never call this method are unaffected
    /// (the field stays `None` and the per-tick drain is skipped).
    pub fn with_injections(&mut self, queue: InjectionQueue) {
        self.injections = Some(queue);
    }

    /// Register a snapshot hook called once per tick during `run()`.
    /// Use `StreamAdapter::as_hook()` to wire a `TickBuffer` for HTTP streaming.
    pub fn register_snapshot_hook(&mut self, hook: SnapshotHook) {
        self.snapshot_hooks.push(hook);
    }

    pub fn config(&self) -> &SimConfig {
        &self.config
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WorldSnapshot> {
        self.snapshot_tx.subscribe()
    }

    /// Subscribe to live tick snapshots and get a handle to all snapshots produced so far.
    ///
    /// The returned `Arc<Mutex<Vec<WorldSnapshot>>>` is populated tick-by-tick during `run()`.
    /// Callers who start listening after `run()` has already begun can drain the vec to replay
    /// any missed ticks, then consume the `Receiver` for subsequent ticks.
    ///
    /// # Deduplication contract
    /// A snapshot for tick `n` may appear in both the history `Vec` (if the caller subscribes
    /// after tick `n` was broadcast) **and** in the `Receiver` (if the caller's receiver was
    /// created before tick `n` was sent). Callers must deduplicate by `WorldSnapshot::tick`
    /// when combining replay history with live receiver output.
    pub fn subscribe_with_history(
        &self,
    ) -> (broadcast::Receiver<WorldSnapshot>, Arc<Mutex<Vec<WorldSnapshot>>>) {
        (self.snapshot_tx.subscribe(), Arc::clone(&self.snapshot_history))
    }

    /// Clone the canonical snapshot-history handle (the complete, lossless tick record).
    ///
    /// This is the source the HTTP `/ticks/sse` feed tails. Unlike a [`TickBuffer`] ring
    /// (bounded — drops old ticks under backpressure) or the broadcast `Receiver` (lossy if
    /// subscribed after a tick was sent), `snapshot_history` is the **single canonical**
    /// in-memory store (every `tick` pushes into it) and grows monotonically for the run's
    /// lifetime — so a consumer polling its length sees every tick exactly once, in order,
    /// no dedup needed. The `Arc` keeps the history alive for the consumer even after the
    /// engine itself is dropped at run end.
    ///
    /// [`TickBuffer`]: crate::api::streaming::TickBuffer
    pub fn snapshot_history_handle(&self) -> Arc<Mutex<Vec<WorldSnapshot>>> {
        Arc::clone(&self.snapshot_history)
    }

    /// Subscribe to the terminal completion signal.
    ///
    /// Returns a `watch::Receiver<Option<SimCompletion>>`. The receiver starts at `None` and
    /// transitions to `Some(SimCompletion { total_ticks })` exactly once when `run()` finishes.
    ///
    /// Because `watch` retains the last value, late subscribers (those who call this method
    /// AFTER `run()` has already returned) will immediately observe `Some(...)` on the first
    /// `borrow()` / `changed()` poll — no timing race for the SSE handler (U-026).
    ///
    /// # Usage pattern (SSE handler)
    /// ```ignore
    /// let mut completion_rx = engine.subscribe_completion();
    /// // ... stream snapshots ...
    /// completion_rx.changed().await?;  // waits until run() sends the signal
    /// let sim_end_event = TickStreamEvent::sim_end(
    ///     completion_rx.borrow().as_ref().unwrap().total_ticks
    /// );
    /// // emit sim_end_event over SSE, then close the stream
    /// ```
    pub fn subscribe_completion(&self) -> watch::Receiver<Option<SimCompletion>> {
        self.completion_tx.subscribe()
    }

    /// Fire the terminal completion signal on an **aborted** run (engine error).
    ///
    /// The normal completion send lives at the success tail of `run_with_boost` and is skipped
    /// when the run returns `Err` (a per-round log write / `flush_final` / extraction failure).
    /// Without a signal the monitor — whose only loop-exit is the completion watch — polls forever
    /// and the run is stuck `Running`. The runner calls this on the engine's error path so the
    /// monitor unblocks, runs its cleanup, and (seeing no `simulation_end` record) marks the run
    /// `Failed`. `total_ticks` is the snapshots committed so far (best-effort; the run is partial).
    pub fn signal_aborted(&self) {
        let total_ticks = self.snapshot_history.lock().len() as u32;
        let _ = self.completion_tx.send(Some(SimCompletion { total_ticks }));
    }

    pub async fn run<L: crate::llm::LlmClient>(
        &self,
        pool: &mut crate::agent::AgentPool,
        graph: &crate::graph::KnowledgeGraph,
        llm: &L,
    ) -> crate::error::Result<SimulationResult> {
        // Thin wrapper preserving the original signature (every pre-existing caller is unaffected):
        // a single-LLM run is a dual-LLM run with no boost client → every agent uses `llm`.
        self.run_with_boost(pool, graph, llm, None).await
    }

    /// Run with an optional per-platform "boost" LLM — port of `run_parallel_simulation`'s
    /// dual-LLM routing (`create_model(use_boost=False)` for twitter at L1130,
    /// `create_model(use_boost=True)` for reddit at L1322). When `boost_llm` is `Some`, REDDIT
    /// agents' decisions run against the boost client and twitter agents against `llm`; when
    /// `None` (single-platform runs + every pre-existing caller via [`SimEngine::run`]), ALL agents
    /// use `llm` — byte-identical to the prior single-LLM behavior (no-downgrade-of-Y).
    pub async fn run_with_boost<L: crate::llm::LlmClient>(
        &self,
        pool: &mut crate::agent::AgentPool,
        // Each agent's `prepare_action` reads this read-only graph to build per-tick "Knowledge
        // Graph Context" from its source entity's neighborhood (an agent without a source entity
        // simply gets no graph section). `&KnowledgeGraph` is `Sync`, shared across the parallel
        // Phase-1 reads.
        graph: &crate::graph::KnowledgeGraph,
        llm: &L,
        boost_llm: Option<&L>,
    ) -> crate::error::Result<SimulationResult> {
        use futures::stream::{self, StreamExt};

        self.snapshot_history.lock().clear();
        let mut world = WorldState::new();

        // Seed agent snapshots into world state
        for agent in pool.iter() {
            world.add_agent_snapshot(
                agent.id,
                AgentSnapshot {
                    id: agent.id,
                    name: agent.persona.name.clone(),
                    state: format!("{:?}", agent.state),
                },
            );
        }

        // Producer wiring (U-028 §5 / U-030 §2): emit the `simulation_start` record before the
        // loop (fanned out to ALL installed loggers) and accumulate a PER-PLATFORM total action
        // count for `simulation_end`. `minutes_per_round` is read once for per-round
        // `simulated_hour` (`(tick * mpr / 60) % 24`, `run_parallel_simulation.py:1235-1236`).
        // No-op when no producer is installed. For a single-platform producer the logger set has
        // one entry, so every fan-out/accumulator below resolves to that one logger — byte-identical
        // to the pre-U030 single-`logger` behavior.
        let mut total_actions: PerPlatform<i64> = match &self.producer {
            Some(producer) => {
                for (_, logger) in producer.loggers.iter() {
                    logger
                        .log_simulation_start(&producer.config)
                        .map_err(|e| log_err("simulation_start", e))?;
                }
                PerPlatform::zeroed(&producer.loggers)
            }
            None => PerPlatform { slots: Vec::new() },
        };

        // Round-0 initial-events injection (U-030 §6, `run_parallel_simulation.py:1171-1211`).
        // Emitted BEFORE the main loop, AFTER `simulation_start`: `round_start(0, 0)` fans out to ALL
        // loggers; each `event_config.initial_posts` entry resolves to the pool agent whose
        // `social.user_id == poster_agent_id` and routes a `CREATE_POST` record (round 0) to that
        // agent's platform logger, counted into that platform's total; an unresolvable
        // `poster_agent_id` is skipped (Python `except Exception: pass`); `round_end(0, count)` fans
        // out. Round-0 CREATE_POSTs count toward `total_actions` per platform (Python increments
        // `total_actions` per initial post, L1199-1201). Python ALSO `env.step`s the posts into the
        // world so later agents react — teri's `WorldState` has no OASIS post-graph, so the
        // world-injection is the known `[≠]U028-OASIS-INTERNALS` substrate gap; the round-0
        // actions.jsonl RECORDS are differentially portable. Always emits round-0 start/end when a
        // producer is installed (Python logs them even with no initial_posts).
        if let Some(ref producer) = self.producer {
            for (_, logger) in producer.loggers.iter() {
                logger.log_round_start(0, 0).map_err(|e| log_err("round_start", e))?;
            }
            let mut round0_counts: PerPlatform<i64> = PerPlatform::zeroed(&producer.loggers);
            let initial_posts = producer
                .config
                .get("event_config")
                .and_then(|ec| ec.get("initial_posts"))
                .and_then(serde_json::Value::as_array);
            if let Some(posts) = initial_posts {
                for post in posts {
                    let poster_id = post
                        .get("poster_agent_id")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0);
                    let content =
                        post.get("content").and_then(serde_json::Value::as_str).unwrap_or("");
                    // Resolve poster_agent_id → the pool agent with that OASIS user_id, then route
                    // to its platform logger. Unresolvable id (or platform with no logger) → skip.
                    let routed = pool.agents.iter().find_map(|a| {
                        a.persona.social.as_ref().and_then(|s| {
                            (s.user_id as i64 == poster_id)
                                .then(|| {
                                    producer.loggers.get(s.platform).map(|l| (s.platform, l, a))
                                })
                                .flatten()
                        })
                    });
                    if let Some((platform, logger, agent)) = routed {
                        let args = serde_json::json!({ "content": content });
                        logger
                            .log_action(
                                0,
                                poster_id,
                                &agent.persona.name,
                                "CREATE_POST",
                                Some(&args),
                                None,
                                true,
                            )
                            .map_err(|e| log_err("log_action", e))?;
                        round0_counts.add(platform, 1);
                        // Workstream C: also seed the social world so round-1 agents SEE these
                        // posts in their feed and can LIKE/COMMENT/REPOST against them. This closes
                        // the [≠]U028-OASIS-INTERNALS "no post-graph" gap the round-0 comment above
                        // anticipated. No-op when no social set is installed.
                        if let Some(social) = &self.social {
                            let mut set = social.lock();
                            if let Some(world) = set.world_mut(platform) {
                                // Register the poster's name so a round-1 LIKE/COMMENT against this
                                // seed post resolves `author_name` (TASK-SIM-2 enrichment).
                                world.register_user(poster_id, &agent.persona.name);
                                world.create_post(poster_id, content, &python_isoformat_local());
                            }
                        }
                    }
                }
            }
            for (platform, logger) in producer.loggers.iter() {
                let count = round0_counts.get(*platform);
                logger.log_round_end(0, count).map_err(|e| log_err("round_end", e))?;
                total_actions.add(*platform, count);
            }
        }
        let minutes_per_round = self.producer.as_ref().map(RunProducer::minutes_per_round);

        for tick_idx in 0..self.config.max_ticks {
            // Cooperative-shutdown check (additive, opt-in). When the owner flips the flag,
            // break gracefully BEFORE advancing/executing this tick: history up to the prior
            // tick is preserved, the completion signal below still fires with the partial
            // count, and `run()` returns Ok. When `shutdown` is None (every pre-existing
            // caller), this is a no-op and the loop runs the full `max_ticks`.
            if let Some(ref flag) = self.shutdown
                && flag.load(std::sync::atomic::Ordering::Acquire)
            {
                break;
            }

            world.advance_tick();

            // Activation gate (U-028 §4): which agents act this tick. With a policy installed,
            // map its active agent ids (OASIS numeric ids) onto pool indices by
            // `SocialProfile.user_id` — agents whose id is absent (or who carry no social
            // profile) skip this round, mirroring Python's `if not active_agents: continue`
            // (an empty set → no agent acts). With no policy (every pre-existing caller), all
            // agents act, in pool order — byte-identical to the prior behavior.
            let active_indices: Vec<usize> = match &self.activation {
                Some(policy) => {
                    let ids: std::collections::HashSet<i64> =
                        policy.active_agent_ids(tick_idx).into_iter().collect();
                    pool.agents
                        .iter()
                        .enumerate()
                        .filter(|(_, a)| {
                            a.persona
                                .social
                                .as_ref()
                                .is_some_and(|s| ids.contains(&(s.user_id as i64)))
                        })
                        .map(|(i, _)| i)
                        .collect()
                }
                None => (0..pool.agents.len()).collect(),
            };

            // Producer: `round_start` is logged EVERY round, even an empty one
            // (`run_parallel_simulation.py:1244-1245`), fanned out to ALL installed loggers
            // (U-030 §2 — each platform's coroutine logs round_start every round). Logged round is
            // 1-based (`round_num+1`); `simulated_hour` is identical across platforms.
            let round = tick_idx as i64 + 1;
            if let (Some(producer), Some(mpr)) = (&self.producer, minutes_per_round) {
                let simulated_hour = (tick_idx as i64 * mpr / 60) % 24;
                for (_, logger) in producer.loggers.iter() {
                    logger
                        .log_round_start(round, simulated_hour)
                        .map_err(|e| log_err("round_start", e))?;
                }
            }

            // Phase 1: prepare actions concurrently (immutable reads + LLM calls), for the
            // ACTIVE agents only. stream::buffered drives at most `parallelism` futures
            // simultaneously, giving real throughput gains when agent steps are LLM-bound.
            //
            // The per-agent futures are collected into a `Vec<Pin<Box<dyn Future + Send>>>`
            // BEFORE streaming, rather than mapped lazily on the agent iterator. This is
            // behavior-identical (the same futures still run concurrently through
            // `.buffered()`), but building the Vec with a plain `for` loop avoids the
            // `stream::iter(...).map(closure)` higher-ranked-lifetime inference failure
            // ("implementation of `FnOnce` is not general enough") that otherwise occurs when
            // `run()`'s future must be `Send` — i.e. when it is handed to `tokio::spawn`
            // (U-022's `SimulationRunner::start_simulation`). Direct `.await` callers are
            // unaffected.
            // Workstream C: snapshot a recency-ranked feed per platform ONCE at the top of the
            // tick — reflecting state through the previous tick — so the concurrent prepare phase
            // borrows an immutable feed (race-free; the world is only mutated in phase 2). Empty /
            // absent when no social set is installed ⇒ `feed_for` returns `None` ⇒ prompts are
            // byte-identical to the no-social path (no-downgrade).
            let feeds: Vec<(Platform, social_world::FeedSnapshot)> = match &self.social {
                Some(social) => {
                    let set = social.lock();
                    let rank = self
                        .producer
                        .as_ref()
                        .map(|p| social_world::FeedRankParams::from_config(&p.config))
                        .unwrap_or_default();
                    set.platforms()
                        .filter_map(|p| {
                            set.world(p).map(|w| (p, w.feed_snapshot(rank.top_n, &rank)))
                        })
                        .collect()
                }
                None => Vec::new(),
            };
            let feed_for = |platform: Option<Platform>| -> Option<&social_world::FeedSnapshot> {
                let platform = platform?;
                feeds.iter().find(|(p, _)| *p == platform).map(|(_, f)| f)
            };

            let actions: Vec<crate::error::Result<crate::sim::Action>> = {
                // Borrow scope: the prepared futures borrow `world`/`llm`; they are all driven
                // to completion by `collect().await` here, releasing those borrows before
                // phase 2's `&mut world`. The boxed-future lifetime `'p` ties to this scope.
                let prepared: Vec<
                    std::pin::Pin<
                        Box<
                            dyn std::future::Future<
                                    Output = crate::error::Result<crate::sim::Action>,
                                > + Send
                                + '_,
                        >,
                    >,
                > = active_indices
                    .iter()
                    .map(|&idx| {
                        // Dual-LLM routing (U-030 S-934): a reddit agent uses the boost client when
                        // one is installed (`create_model(use_boost=True)` for the reddit coroutine);
                        // twitter agents — and EVERY agent when no boost client is installed — use the
                        // general `llm` (`use_boost=False`). Both arms are `&L`, so the boxed future
                        // type is identical.
                        let use_boost = boost_llm.is_some()
                            && matches!(
                                pool.agents[idx].persona.social.as_ref().map(|s| s.platform),
                                Some(crate::agent::Platform::Reddit)
                            );
                        let client: &L = if use_boost { boost_llm.unwrap() } else { llm };
                        let agent_platform =
                            pool.agents[idx].persona.social.as_ref().map(|s| s.platform);
                        let feed = feed_for(agent_platform);
                        Box::pin(pool.agents[idx].prepare_action(&world, feed, Some(graph), client))
                            as std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send + '_>>
                    })
                    .collect();
                stream::iter(prepared).buffered(self.config.parallelism).collect().await
            };

            // Phase 2: commit results sequentially (mutable writes + world state). Iterate the
            // active indices zipped with their prepared actions. For each committed
            // `Action::Social`, emit an `actions.jsonl` record (U-028 §5): round (1-based),
            // agent_id = `SocialProfile.user_id`, agent_name = `Persona.name`, action_type via
            // the `ACTION_TYPE_MAP` value, native action_args. Generic (non-social) actions are
            // not OASIS records and are not logged. The record routes to the committing agent's
            // PLATFORM logger (U-030 §3): in a parallel run a reddit agent's action lands in
            // `reddit/actions.jsonl`, a twitter agent's in `twitter/actions.jsonl`. Counts are
            // accumulated per platform for the per-platform `round_end` / `simulation_end`.
            let mut round_counts: PerPlatform<i64> = match &self.producer {
                Some(producer) => PerPlatform::zeroed(&producer.loggers),
                None => PerPlatform { slots: Vec::new() },
            };
            for (&idx, action_result) in active_indices.iter().zip(actions) {
                let action = action_result?;
                let agent_uuid = pool.agents[idx].id;
                world.apply(agent_uuid, action.clone());
                pool.agents[idx].commit_action(&action);
                if let Some(snap) = world.agents.get_mut(&agent_uuid) {
                    snap.state = format!("{:?}", pool.agents[idx].state);
                }
                if let (Some(producer), Action::Social(sa)) = (&self.producer, &action) {
                    // Route by the committing agent's platform. `social` is always present for a
                    // producer-run pool agent (`load_agent_pool` sets it); a route-miss (no social
                    // profile, or a platform with no logger installed) is a no-op that does NOT
                    // count — never misroute into the wrong file. Under the §3 invariant
                    // (`PlatformLoggerSet` holds a logger for every platform present in the pool)
                    // this is unreachable; it is the fail-closed guard.
                    let social = pool.agents[idx].persona.social.as_ref();
                    if let Some((platform, logger)) = social
                        .and_then(|s| producer.loggers.get(s.platform).map(|l| (s.platform, l)))
                    {
                        let agent_id = social.map(|s| s.user_id as i64).unwrap_or(0);
                        // Enriched args (post_content/author_name/comment_content/quote_content/
                        // target_user_name) when this platform's social world is installed — it
                        // holds the post/comment/user graph the enrichment resolves against
                        // (TASK-SIM-2). Falls back to structural-only when no world is present, so
                        // the no-world record is byte-identical to before.
                        let args = match &self.social {
                            Some(set) => match set.lock().world(platform) {
                                Some(world) => sa.oasis_action_args_enriched(world),
                                None => sa.oasis_action_args(),
                            },
                            None => sa.oasis_action_args(),
                        };
                        logger
                            .log_action(
                                round,
                                agent_id,
                                &pool.agents[idx].persona.name,
                                sa.oasis_action_type(),
                                Some(&args),
                                None,
                                true,
                            )
                            .map_err(|e| log_err("log_action", e))?;
                        round_counts.add(platform, 1);
                    }
                }
                // Workstream C: apply the committed social action into the social world ALONGSIDE
                // the producer's `actions.jsonl` log above (NOT a replacement — the log stays
                // intact). Only for an agent carrying a social profile and only for an
                // `Action::Social`; everything else is untouched (no-downgrade). A bad/unresolved
                // target id is a fail-closed NoOp inside `apply` (never panics, never invents a
                // post). The same `created_at` is used for the DB row so it agrees with the log.
                if let (Some(social), Action::Social(sa)) = (&self.social, &action)
                    && let Some(profile) = pool.agents[idx].persona.social.as_ref()
                {
                    let created_at = python_isoformat_local();
                    let mut set = social.lock();
                    if let Some(world) = set.world_mut(profile.platform) {
                        // Register the actor's display name so a LATER action targeting this
                        // agent's post resolves `author_name` (TASK-SIM-2 enrichment).
                        world.register_user(profile.user_id as i64, &pool.agents[idx].persona.name);
                        world.apply(profile.user_id as i64, sa, &created_at);
                    }
                }
            }

            // Producer: `round_end` with THIS round's per-platform action count, fanned out to ALL
            // loggers (`run_parallel_simulation.py:1274-1275` per coroutine). A platform with no
            // actions this round logs `round_end(round, 0)` — matching Python's
            // `if not active_agents: log_round_end(+1, 0)` and its zero-action branch. Each
            // platform's running total advances by its own round count.
            if let Some(ref producer) = self.producer {
                for (platform, logger) in producer.loggers.iter() {
                    let count = round_counts.get(*platform);
                    logger.log_round_end(round, count).map_err(|e| log_err("round_end", e))?;
                    total_actions.add(*platform, count);
                }
            }

            // Workstream C: flush the social world to its `{platform}_simulation.db` at the end of
            // each round (per-round so `/posts` returns growing data WHILE the sim runs, matching
            // the mid-run observability the monitor + UI expect). No-op without the `sqlite`
            // feature, and no-op when no social set is installed.
            if let Some(social) = &self.social {
                social.lock().flush_round()?;
            }

            // God's-eye runtime injection: drain any variables pushed into a LIVE run from
            // outside (REST /:id/inject) and apply them BEFORE the static inject_fn so a
            // scheduled policy can react to them. Each persists in world.variables thereafter.
            if let Some(ref queue) = self.injections {
                let drained: Vec<(String, f32)> = { queue.lock().drain(..).collect() };
                for (key, value) in drained {
                    world.inject_variable(key, value);
                }
            }

            // Apply God's-eye injection if configured
            if let Some(ref inject) = self.config.inject_fn {
                inject(world.tick, &mut world);
            }

            let snapshot = world.snapshot();
            // Broadcast to live subscribers (RecvError::Lagged signals gap to slow consumers)
            let _ = self.snapshot_tx.send(snapshot.clone());
            // Call registered hooks (e.g. TickBuffer adapters for HTTP streaming — 3A)
            for hook in &self.snapshot_hooks {
                hook(snapshot.clone());
            }
            // snapshot_history is the single canonical in-memory store (6A)
            self.snapshot_history.lock().push(snapshot);
        }

        // Producer: the terminal `simulation_end` record (`run_parallel_simulation.py:1284`),
        // fanned out to ALL loggers with each platform's OWN running `total_actions` (U-030 §2).
        // `total_rounds` is the config-derived count (== `max_ticks`), NOT the executed count —
        // it matches Python even when a cooperative shutdown broke the loop early (both coroutines
        // share the same `total_rounds`). This is the record the landed monitor
        // (`spawn_monitor_task`) detects per platform; for a parallel run BOTH platforms'
        // `simulation_end` records drive the dual-platform completion gate (S-615).
        if let Some(ref producer) = self.producer {
            for (platform, logger) in producer.loggers.iter() {
                logger
                    .log_simulation_end(self.config.max_ticks as i64, total_actions.get(*platform))
                    .map_err(|e| log_err("simulation_end", e))?;
            }
        }

        // Workstream C: final flush to capture the last tick, then the DB connections drop
        // (closing the files). No-op without the `sqlite` feature / no social set.
        if let Some(social) = &self.social {
            social.lock().flush_final()?;
        }

        // Clone history from canonical store; avoids a local Vec running in parallel (6A)
        let history = self.snapshot_history.lock().clone();
        let total_ticks = history.len() as u32;

        // Emit the terminal completion signal AFTER the last snapshot has been committed to
        // history. This ordering guarantees that any SSE handler observing the completion
        // signal can safely drain history up to `total_ticks` without missing the last tick.
        // Mirrors MiroFish action_logger.log_simulation_end / simulation_runner monitor.
        // Ignore the error: if all receivers have been dropped, the signal is irrelevant.
        let _ = self.completion_tx.send(Some(SimCompletion { total_ticks }));

        Ok(SimulationResult { id: Uuid::new_v4(), history })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::social_world::SocialWorld;

    // --- ComputeWorld overlay (world-type #2) wiring ---

    #[test]
    fn compute_overlay_predicts_a_plan_against_a_cell() {
        use crate::sim::compute_world::{ComputeAction, ComputeWorldSet, ExitPrediction, Risk};

        let mut engine = SimEngine::new(SimConfig::new(1, 1));
        engine.with_compute(ComputeWorldSet::new([(
            "cell-a".to_string(),
            std::path::PathBuf::from("/tmp/cell-a"),
        )]));

        let plan = |c: &str| ComputeAction { command: c.to_string(), cwd: ".".to_string() };
        let rollout = engine
            .predict_compute_plan(
                "cell-a",
                &[plan("mkdir build"), plan("rm -rf build"), plan("rm build")],
            )
            .expect("installed cell yields a rollout");

        assert_eq!(rollout.cell, "cell-a");
        assert_eq!(rollout.effects.len(), 3);
        // rm -rf drives blast radius to Irreversible; the trailing rm hits a known-absent path.
        assert_eq!(rollout.max_risk, Risk::Irreversible);
        assert_eq!(rollout.first_failure, Some(2));
        assert!(matches!(
            rollout.effects[2].predicted_exit,
            ExitPrediction::Failure { .. }
        ));
        assert!(!rollout.is_safe());
    }

    #[test]
    fn compute_overlay_is_opt_in_and_cell_scoped() {
        use crate::sim::compute_world::{ComputeAction, ComputeWorldSet};

        // No with_compute → no compute domain → None (a missing substrate is not an error).
        let engine = SimEngine::new(SimConfig::new(1, 1));
        assert!(engine.predict_compute_plan("cell-a", &[]).is_none());

        // Installed, but an unknown cell → None.
        let mut engine = SimEngine::new(SimConfig::new(1, 1));
        engine.with_compute(ComputeWorldSet::new([(
            "cell-a".to_string(),
            std::path::PathBuf::from("/tmp/cell-a"),
        )]));
        let touch = [ComputeAction { command: "touch x".to_string(), cwd: ".".to_string() }];
        assert!(engine.predict_compute_plan("missing", &touch).is_none());
        assert!(engine.predict_compute_plan("cell-a", &touch).is_some());
    }

    // --- TASK-SIM-2 #2: enriched action_args ---

    /// A world seeded with one post (id 1, author 7 "Alice") + one comment (id 1, author 8 "Bob").
    fn enriched_world() -> SocialWorld {
        let mut w = SocialWorld::new(crate::agent::Platform::Reddit);
        w.register_user(7, "Alice");
        w.register_user(8, "Bob");
        let pid = w.create_post(7, "the original post", "2025-12-01T10:00:00");
        assert_eq!(pid, 1);
        let cid = w.apply(
            8,
            &SocialAction::Comment { post_id: "1".into(), content: "a comment".into() },
            "2025-12-01T10:01:00",
        );
        assert_eq!(cid, crate::sim::social_world::ApplyOutcome::CreatedComment(1));
        w
    }

    #[test]
    fn test_enriched_args_like_post_resolves_content_and_author() {
        let w = enriched_world();
        let sa = SocialAction::Like { target_kind: TargetKind::Post, target_id: "post-1".into() };
        let args = sa.oasis_action_args_enriched(&w);
        assert_eq!(args["target_id"], "post-1"); // structural field preserved
        assert_eq!(args["post_content"], "the original post");
        assert_eq!(args["author_name"], "Alice");
    }

    #[test]
    fn test_enriched_args_like_comment_resolves_comment_content() {
        let w = enriched_world();
        let sa = SocialAction::Like { target_kind: TargetKind::Comment, target_id: "1".into() };
        let args = sa.oasis_action_args_enriched(&w);
        assert_eq!(args["comment_content"], "a comment");
        // A comment target has no post_content / author_name keys.
        assert!(args.get("post_content").is_none());
    }

    #[test]
    fn test_enriched_args_comment_resolves_target_post() {
        let w = enriched_world();
        let sa = SocialAction::Comment { post_id: "1".into(), content: "reply".into() };
        let args = sa.oasis_action_args_enriched(&w);
        assert_eq!(args["content"], "reply"); // structural
        assert_eq!(args["post_content"], "the original post");
        assert_eq!(args["author_name"], "Alice");
    }

    #[test]
    fn test_enriched_args_quote_carries_quote_content_and_target() {
        let w = enriched_world();
        let sa = SocialAction::Quote { post_id: "1".into(), content: "I agree!".into() };
        let args = sa.oasis_action_args_enriched(&w);
        assert_eq!(args["post_content"], "the original post");
        assert_eq!(args["author_name"], "Alice");
        assert_eq!(args["quote_content"], "I agree!");
    }

    #[test]
    fn test_enriched_args_follow_resolves_target_user_name_by_id() {
        let w = enriched_world();
        // Numeric id 7 → registered name "Alice".
        let sa = SocialAction::Follow { user_id: "7".into() };
        let args = sa.oasis_action_args_enriched(&w);
        assert_eq!(args["target_user_name"], "Alice");
    }

    #[test]
    fn test_enriched_args_follow_keeps_raw_handle_when_not_numeric() {
        let w = enriched_world();
        // A non-numeric handle is kept verbatim (MiroFish keeps the raw target when unresolved).
        let sa = SocialAction::Follow { user_id: "charlie".into() };
        let args = sa.oasis_action_args_enriched(&w);
        assert_eq!(args["target_user_name"], "charlie");
    }

    #[test]
    fn test_enriched_args_unresolved_post_omits_enrichment_keys() {
        let w = enriched_world();
        // Hallucinated post id 999 — fail-soft: no post_content / author_name added.
        let sa = SocialAction::Like { target_kind: TargetKind::Post, target_id: "999".into() };
        let args = sa.oasis_action_args_enriched(&w);
        assert_eq!(args["target_id"], "999");
        assert!(args.get("post_content").is_none());
        assert!(args.get("author_name").is_none());
    }

    #[test]
    fn test_enriched_author_name_omitted_when_user_unregistered() {
        // Post by an author with no registered name — content resolves, author_name is omitted
        // (mirrors MiroFish's empty-author fallback skipping the assignment).
        let mut w = SocialWorld::new(crate::agent::Platform::Reddit);
        w.create_post(42, "anon post", "2025-12-01T10:00:00");
        let sa = SocialAction::Like { target_kind: TargetKind::Post, target_id: "1".into() };
        let args = sa.oasis_action_args_enriched(&w);
        assert_eq!(args["post_content"], "anon post");
        assert!(args.get("author_name").is_none());
    }

    #[test]
    fn test_structural_args_unchanged_no_world_path() {
        // The no-world structural path must be byte-identical to before (no-downgrade): every
        // variant emits exactly its structural keys and nothing more.
        assert_eq!(
            SocialAction::Like { target_kind: TargetKind::Post, target_id: "1".into() }
                .oasis_action_args(),
            serde_json::json!({ "target_id": "1" })
        );
        assert_eq!(
            SocialAction::Comment { post_id: "1".into(), content: "hi".into() }.oasis_action_args(),
            serde_json::json!({ "post_id": "1", "content": "hi" })
        );
        assert_eq!(
            SocialAction::Quote { post_id: "1".into(), content: "q".into() }.oasis_action_args(),
            serde_json::json!({ "post_id": "1", "content": "q" })
        );
        assert_eq!(
            SocialAction::Follow { user_id: "7".into() }.oasis_action_args(),
            serde_json::json!({ "user_id": "7" })
        );
        assert_eq!(SocialAction::Trend.oasis_action_args(), serde_json::json!({}));
    }

    #[test]
    fn test_enriched_trend_donothing_stay_empty() {
        let w = enriched_world();
        assert_eq!(SocialAction::Trend.oasis_action_args_enriched(&w), serde_json::json!({}));
        assert_eq!(SocialAction::DoNothing.oasis_action_args_enriched(&w), serde_json::json!({}));
    }

    #[test]
    fn test_world_state_creation() {
        let world = WorldState::new();
        assert_eq!(world.tick, 0);
        assert!(world.agents.is_empty());
        assert!(world.events.is_empty());
    }

    #[test]
    fn test_world_state_advance_tick() {
        let mut world = WorldState::new();
        world.advance_tick();
        assert_eq!(world.tick, 1);
    }

    #[test]
    fn test_world_state_variables() {
        let mut world = WorldState::new();
        world.inject_variable("temperature".to_string(), 25.5);
        assert_eq!(world.get_variable("temperature"), Some(25.5));
    }

    #[test]
    fn test_world_snapshot() {
        let world = WorldState::new();
        let snapshot = world.snapshot();
        assert_eq!(snapshot.tick, world.tick);
    }

    #[test]
    fn test_sim_engine_creation() {
        let config = SimConfig { max_ticks: 100, parallelism: 4, inject_fn: None };
        let engine = SimEngine::new(config);
        assert_eq!(engine.config().max_ticks, 100);
    }

    #[test]
    fn test_world_state_apply() {
        let mut world = WorldState::new();
        let agent_id = Uuid::new_v4();
        world.apply(agent_id, Action::Think("pondering".to_string()));
        assert_eq!(world.events.len(), 1);
        assert_eq!(world.events[0].agent_id, agent_id);
    }

    #[test]
    fn test_world_state_apply_at_deterministic() {
        let mut world = WorldState::new();
        let agent_id = Uuid::new_v4();
        let ts = chrono::DateTime::from_timestamp(0, 0).unwrap();
        world.apply_at(agent_id, Action::Speak("hello".to_string()), ts);
        assert_eq!(world.events.len(), 1);
        assert_eq!(world.events[0].timestamp, ts);
    }

    #[test]
    fn test_sim_engine_subscribe() {
        let config = SimConfig::default();
        let engine = SimEngine::new(config);
        let rx = engine.subscribe();
        assert_eq!(rx.len(), 0); // channel is empty
    }

    #[test]
    fn test_subscribe_with_history_returns_shared_arc() {
        let config = SimConfig::default();
        let engine = SimEngine::new(config);
        let (_rx, history) = engine.subscribe_with_history();
        assert_eq!(history.lock().len(), 0);
        // Simulate a tick being pushed (as run() would do)
        let world = WorldState::new();
        history.lock().push(world.snapshot());
        assert_eq!(history.lock().len(), 1);
    }

    #[test]
    fn test_sim_config_with_inject_fn() {
        let inject: InjectFn = std::sync::Arc::new(|tick, world| {
            world.inject_variable("tick".to_string(), tick as f32);
        });
        let config = SimConfig { max_ticks: 10, parallelism: 2, inject_fn: Some(inject) };
        assert_eq!(config.max_ticks, 10);
    }

    #[test]
    fn test_sim_config_new_constructor() {
        let config = SimConfig::new(100, 4);
        assert_eq!(config.max_ticks, 100);
        assert_eq!(config.parallelism, 4);
        assert!(config.inject_fn.is_none());
    }

    #[test]
    fn test_sim_config_with_inject_fn_builder() {
        let config = SimConfig::new(100, 4).with_inject_fn(|tick, world| {
            if tick == 5 {
                world.inject_variable("test_var".to_string(), 42.0);
            }
        });

        assert_eq!(config.max_ticks, 100);
        assert_eq!(config.parallelism, 4);
        assert!(config.inject_fn.is_some());
    }

    #[test]
    fn test_sim_config_builder_chain() {
        let config = SimConfig::new(200, 8).with_inject_fn(|tick, world| {
            world.inject_variable("tick_count".to_string(), tick as f32);
        });

        assert_eq!(config.max_ticks, 200);
        assert_eq!(config.parallelism, 8);
        assert!(config.inject_fn.is_some());
    }

    #[test]
    fn test_world_snapshot_get_variable() {
        let mut world = WorldState::new();
        world.inject_variable("temperature".to_string(), 25.5);
        world.inject_variable("humidity".to_string(), 65.0);

        let snapshot = world.snapshot();

        // Test existing variables
        assert_eq!(snapshot.get_variable("temperature"), Some(25.5));
        assert_eq!(snapshot.get_variable("humidity"), Some(65.0));

        // Test non-existent variable
        assert_eq!(snapshot.get_variable("nonexistent"), None);

        // Test that variables are properly cloned
        world.inject_variable("temperature".to_string(), 30.0); // Modify original
        assert_eq!(snapshot.get_variable("temperature"), Some(25.5)); // Snapshot unchanged
    }

    #[test]
    fn test_world_snapshot_preserves_variables() {
        let mut world = WorldState::new();
        world.inject_variable("test".to_string(), 42.0);

        let snapshot = world.snapshot();

        // Verify snapshot contains variables
        assert_eq!(snapshot.get_variable("test"), Some(42.0));
        assert_eq!(snapshot.variables.len(), 1);

        // Verify variables are accessible via get_variable
        assert_eq!(snapshot.get_variable("test"), world.get_variable("test"));
    }

    #[test]
    fn test_inject_fn_variable_modification() {
        // Test that the injection function can actually modify world variables
        let mut world = WorldState::new();
        world.inject_variable("counter".to_string(), 0.0);

        let config = SimConfig::new(1, 1).with_inject_fn(|tick, world| {
            let current = world.get_variable("counter").unwrap_or(0.0);
            world.inject_variable("counter".to_string(), current + tick as f32);
        });

        // Manually call the injection function
        if let Some(ref inject) = config.inject_fn {
            inject(5, &mut world);
        }

        assert_eq!(world.get_variable("counter"), Some(5.0));
    }

    #[tokio::test]
    async fn test_sim_engine_runs_multiple_agents() {
        // 9A: verify SimEngine::run executes all agents each tick and collects
        // their actions into the world snapshot. Uses a mock LLM.
        use crate::agent::{Agent, AgentPool, Persona};
        use crate::error::Result;
        use crate::llm::LlmClient;
        use async_trait::async_trait;
        use std::pin::Pin;

        struct MockLlm;
        #[async_trait]
        impl LlmClient for MockLlm {
            async fn complete(&self, _: &str) -> Result<String> {
                Ok("Speak(hello from mock)".to_string())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<String> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
        }

        let mut pool = AgentPool::new();
        for i in 0..3 {
            let persona = Persona {
                name: format!("Agent-{}", i),
                background: "Test agent".to_string(),
                traits: vec!["test".to_string()],
                role: "tester".to_string(),
                social: None,
            };
            pool.add_agent(Agent::new(persona));
        }

        let config = SimConfig::new(2, 3); // 2 ticks, 3 concurrent (all agents in parallel)
        let engine = SimEngine::new(config);
        let graph = crate::graph::KnowledgeGraph::new();
        let llm = MockLlm;

        let result = engine.run(&mut pool, &graph, &llm).await.expect("run failed");

        // 2 ticks recorded
        assert_eq!(result.history.len(), 2);
        // Each tick has 3 events (one per agent)
        for snapshot in &result.history {
            assert_eq!(snapshot.events.len(), 3, "expected 3 events at tick {}", snapshot.tick);
        }
        // Tick numbers increment correctly
        assert_eq!(result.history[0].tick, 1);
        assert_eq!(result.history[1].tick, 2);
        // final_snapshot convenience method works
        assert_eq!(result.final_snapshot().unwrap().tick, 2);
    }

    #[tokio::test]
    async fn test_integration_small_agent_pool() {
        // 10A: integration test that actually calls engine.run() with inject_fn,
        // verifies inject_fn variables appear in snapshots and history is complete.
        use crate::agent::{Agent, AgentPool, Persona};
        use crate::error::Result;
        use crate::llm::LlmClient;
        use async_trait::async_trait;
        use std::pin::Pin;

        struct MockLlm;
        #[async_trait]
        impl LlmClient for MockLlm {
            async fn complete(&self, _: &str) -> Result<String> {
                Ok("Think(exploring)".to_string())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<String> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
        }

        let mut pool = AgentPool::new();
        for i in 0..2 {
            let persona = Persona {
                name: format!("TestAgent-{}", i),
                background: format!("Test agent {}", i),
                traits: vec!["curious".to_string()],
                role: "explorer".to_string(),
                social: None,
            };
            pool.add_agent(Agent::new(persona));
        }

        let config = SimConfig::new(3, 2).with_inject_fn(|tick, world| {
            world.inject_variable("sim_tick".to_string(), tick as f32);
            world.inject_variable("pressure".to_string(), 1000.0 + (tick as f32 * 5.0));
        });

        let engine = SimEngine::new(config);
        let graph = crate::graph::KnowledgeGraph::new();
        let llm = MockLlm;

        let result = engine.run(&mut pool, &graph, &llm).await.expect("run failed");

        // 3 ticks in history
        assert_eq!(result.history.len(), 3);

        // inject_fn variables present in each snapshot
        for (i, snapshot) in result.history.iter().enumerate() {
            let expected_tick = (i + 1) as f32;
            assert_eq!(
                snapshot.get_variable("sim_tick"),
                Some(expected_tick),
                "sim_tick wrong at history index {i}"
            );
            assert_eq!(
                snapshot.get_variable("pressure"),
                Some(1000.0 + expected_tick * 5.0),
                "pressure wrong at history index {i}"
            );
        }

        // 2 events per tick (one per agent)
        for snapshot in &result.history {
            assert_eq!(snapshot.events.len(), 2);
        }
    }

    #[tokio::test]
    async fn test_sim_engine_run_basic_with_broadcast() {
        // 11A + 12A: thorough test of engine.run() — history, event count, tick order,
        // and broadcast receiver receives all snapshots in order.
        use crate::agent::{Agent, AgentPool, Persona};
        use crate::error::Result;
        use crate::llm::LlmClient;
        use async_trait::async_trait;
        use std::pin::Pin;

        struct MockLlm;
        #[async_trait]
        impl LlmClient for MockLlm {
            async fn complete(&self, _: &str) -> Result<String> {
                Ok("Observe(the room)".to_string())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<String> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
        }

        const TICKS: u32 = 4;
        const AGENTS: usize = 2;

        let mut pool = AgentPool::new();
        for i in 0..AGENTS {
            pool.add_agent(Agent::new(Persona {
                name: format!("Bot-{i}"),
                background: "test".into(),
                traits: vec!["test".into()],
                role: "observer".into(),
                social: None,
            }));
        }

        let config = SimConfig::new(TICKS, AGENTS);
        let engine = SimEngine::new(config);

        // 12A: subscribe BEFORE run so receiver captures all ticks
        let mut rx = engine.subscribe();

        let graph = crate::graph::KnowledgeGraph::new();
        let llm = MockLlm;
        let result = engine.run(&mut pool, &graph, &llm).await.expect("run failed");

        // History correctness
        assert_eq!(result.history.len(), TICKS as usize);
        for (i, snap) in result.history.iter().enumerate() {
            assert_eq!(snap.tick, (i + 1) as u32);
            assert_eq!(snap.events.len(), AGENTS, "tick {} must have {} events", snap.tick, AGENTS);
        }

        // Broadcast receiver received all TICKS snapshots in order
        let mut received = Vec::new();
        while let Ok(snap) = rx.try_recv() {
            received.push(snap);
        }
        assert_eq!(received.len(), TICKS as usize, "broadcast delivered wrong number of snapshots");
        for (i, snap) in received.iter().enumerate() {
            assert_eq!(snap.tick, (i + 1) as u32, "broadcast tick order wrong at index {i}");
        }
    }

    // ===== Social Action Tests =====

    #[test]
    fn test_social_action_display_create_post() {
        let a = SocialAction::CreatePost { content: "hello world".to_string() };
        assert_eq!(a.to_string(), "Posted: hello world");
        let wrapped = Action::Social(a);
        assert!(wrapped.to_string().starts_with("Social: Posted:"));
    }

    #[test]
    fn test_social_action_display_like_post() {
        let a =
            SocialAction::Like { target_kind: TargetKind::Post, target_id: "post-42".to_string() };
        assert_eq!(a.to_string(), "Liked post: post-42");
    }

    #[test]
    fn test_social_action_display_like_comment() {
        let a = SocialAction::Like {
            target_kind: TargetKind::Comment,
            target_id: "comment-7".to_string(),
        };
        assert_eq!(a.to_string(), "Liked comment: comment-7");
    }

    #[test]
    fn test_social_action_display_dislike_post() {
        let a = SocialAction::Dislike {
            target_kind: TargetKind::Post,
            target_id: "post-7".to_string(),
        };
        assert_eq!(a.to_string(), "Disliked post: post-7");
    }

    #[test]
    fn test_social_action_display_dislike_comment() {
        let a = SocialAction::Dislike {
            target_kind: TargetKind::Comment,
            target_id: "comment-3".to_string(),
        };
        assert_eq!(a.to_string(), "Disliked comment: comment-3");
    }

    #[test]
    fn test_social_action_display_trend() {
        let a = SocialAction::Trend;
        assert_eq!(a.to_string(), "Performed trend operation");
        let wrapped = Action::Social(a);
        assert_eq!(wrapped.to_string(), "Social: Performed trend operation");
    }

    #[test]
    fn test_social_action_display_repost() {
        let a = SocialAction::Repost { post_id: "post-1".to_string() };
        assert_eq!(a.to_string(), "Reposted: post-1");
    }

    #[test]
    fn test_social_action_display_quote() {
        let a = SocialAction::Quote {
            post_id: "post-5".to_string(),
            content: "great take".to_string(),
        };
        assert_eq!(a.to_string(), "Quoted post post-5: great take");
    }

    #[test]
    fn test_social_action_display_follow() {
        let a = SocialAction::Follow { user_id: "user-99".to_string() };
        assert_eq!(a.to_string(), "Followed user: user-99");
    }

    #[test]
    fn test_social_action_display_comment() {
        let a =
            SocialAction::Comment { post_id: "post-3".to_string(), content: "nice!".to_string() };
        assert_eq!(a.to_string(), "Commented on post-3: nice!");
    }

    #[test]
    fn test_social_action_display_search_posts() {
        let a = SocialAction::SearchPosts { query: "climate".to_string() };
        assert_eq!(a.to_string(), "Searched posts: climate");
    }

    #[test]
    fn test_social_action_display_search_user() {
        let a = SocialAction::SearchUser { query: "alice".to_string() };
        assert_eq!(a.to_string(), "Searched user: alice");
    }

    #[test]
    fn test_social_action_display_mute() {
        let a = SocialAction::Mute { user_id: "user-bad".to_string() };
        assert_eq!(a.to_string(), "Muted user: user-bad");
    }

    #[test]
    fn test_social_action_display_do_nothing() {
        let a = SocialAction::DoNothing;
        assert_eq!(a.to_string(), "Did nothing");
    }

    #[test]
    fn test_action_social_apply_no_panic() {
        // GAP-SOCIAL-WORLDSTATE: apply records the event generically; no rich social state yet.
        let mut world = WorldState::new();
        let agent_id = Uuid::new_v4();
        let ts = chrono::DateTime::from_timestamp(0, 0).unwrap();

        let cases = vec![
            Action::Social(SocialAction::CreatePost { content: "test".to_string() }),
            Action::Social(SocialAction::Like {
                target_kind: TargetKind::Post,
                target_id: "p1".to_string(),
            }),
            Action::Social(SocialAction::Like {
                target_kind: TargetKind::Comment,
                target_id: "c1".to_string(),
            }),
            Action::Social(SocialAction::Dislike {
                target_kind: TargetKind::Post,
                target_id: "p2".to_string(),
            }),
            Action::Social(SocialAction::Dislike {
                target_kind: TargetKind::Comment,
                target_id: "c2".to_string(),
            }),
            Action::Social(SocialAction::Repost { post_id: "p3".to_string() }),
            Action::Social(SocialAction::Quote {
                post_id: "p4".to_string(),
                content: "q".to_string(),
            }),
            Action::Social(SocialAction::Follow { user_id: "u1".to_string() }),
            Action::Social(SocialAction::Comment {
                post_id: "p5".to_string(),
                content: "c".to_string(),
            }),
            Action::Social(SocialAction::SearchPosts { query: "q1".to_string() }),
            Action::Social(SocialAction::SearchUser { query: "q2".to_string() }),
            Action::Social(SocialAction::Mute { user_id: "u2".to_string() }),
            Action::Social(SocialAction::Trend),
            Action::Social(SocialAction::DoNothing),
        ];

        for action in cases {
            world.apply_at(agent_id, action, ts);
        }
        assert_eq!(world.events.len(), 14);
    }

    #[test]
    fn test_generic_actions_still_intact() {
        // Confirm all 5 pre-existing generic variants are unaltered.
        let mut world = WorldState::new();
        let id = Uuid::new_v4();
        let ts = chrono::DateTime::from_timestamp(0, 0).unwrap();

        world.apply_at(id, Action::Speak("hi".to_string()), ts);
        world.apply_at(id, Action::Move("park".to_string()), ts);
        world.apply_at(id, Action::Interact("door".to_string()), ts);
        world.apply_at(id, Action::Observe("sky".to_string()), ts);
        world.apply_at(id, Action::Think("plan".to_string()), ts);

        assert_eq!(world.events.len(), 5);
        assert_eq!(world.events[0].action, Action::Speak("hi".to_string()));
        assert_eq!(world.events[1].action, Action::Move("park".to_string()));
        assert_eq!(world.events[2].action, Action::Interact("door".to_string()));
        assert_eq!(world.events[3].action, Action::Observe("sky".to_string()));
        assert_eq!(world.events[4].action, Action::Think("plan".to_string()));
    }

    #[test]
    fn test_social_action_serde_roundtrip() {
        let action = Action::Social(SocialAction::CreatePost { content: "serde test".to_string() });
        let json = serde_json::to_string(&action).expect("serialize failed");
        let back: Action = serde_json::from_str(&json).expect("deserialize failed");
        assert_eq!(action, back);
    }

    // ===== U-048: Completion Signal Tests =====

    #[test]
    fn test_subscribe_completion_initial_value_is_none() {
        // Before run(), the completion channel must carry None.
        let engine = SimEngine::new(SimConfig::default());
        let rx = engine.subscribe_completion();
        assert!(rx.borrow().is_none(), "completion must start as None before run()");
    }

    #[tokio::test]
    async fn test_completion_signal_fires_with_correct_total_ticks() {
        // Core U-048 test: subscribe before run(), run for N ticks, assert completion fires
        // with total_ticks == N (the in-band terminal signal mirrors MiroFish simulation_end).
        use crate::agent::{Agent, AgentPool, Persona};
        use crate::error::Result;
        use crate::llm::LlmClient;
        use async_trait::async_trait;
        use std::pin::Pin;

        struct MockLlm;
        #[async_trait]
        impl LlmClient for MockLlm {
            async fn complete(&self, _: &str) -> Result<String> {
                Ok("Think(idle)".to_string())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<String> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
        }

        const N: u32 = 5;
        let mut pool = AgentPool::new();
        pool.add_agent(Agent::new(Persona {
            name: "A".into(),
            background: "test".into(),
            traits: vec![],
            role: "none".into(),
            social: None,
        }));

        let engine = SimEngine::new(SimConfig::new(N, 1));
        // Subscribe BEFORE run() — the normal SSE handler ordering.
        let completion_rx = engine.subscribe_completion();

        let graph = crate::graph::KnowledgeGraph::new();
        let result = engine.run(&mut pool, &graph, &MockLlm).await.expect("run failed");

        // run() must have sent the completion signal synchronously before returning.
        let completion = completion_rx.borrow().clone();
        assert!(completion.is_some(), "completion must be Some after run()");
        let sc = completion.unwrap();
        assert_eq!(sc.total_ticks, N, "total_ticks must equal max_ticks");
        // Also verify via history length as a cross-check.
        assert_eq!(result.history.len() as u32, N);
    }

    #[tokio::test]
    async fn test_completion_signal_fires_after_last_snapshot() {
        // Ordering guarantee: the completion signal must be sent AFTER the last snapshot
        // has been committed to history. We verify by reading history from the engine's
        // shared Arc just before completion is observed — it must already be fully populated.
        use crate::agent::{Agent, AgentPool, Persona};
        use crate::error::Result;
        use crate::llm::LlmClient;
        use async_trait::async_trait;
        use std::pin::Pin;

        struct MockLlm;
        #[async_trait]
        impl LlmClient for MockLlm {
            async fn complete(&self, _: &str) -> Result<String> {
                Ok("Think(idle)".to_string())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<String> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
        }

        const N: u32 = 3;
        let mut pool = AgentPool::new();
        pool.add_agent(Agent::new(Persona {
            name: "B".into(),
            background: "test".into(),
            traits: vec![],
            role: "none".into(),
            social: None,
        }));

        let engine = SimEngine::new(SimConfig::new(N, 1));
        let completion_rx = engine.subscribe_completion();
        let (_, history_arc) = engine.subscribe_with_history();

        let graph = crate::graph::KnowledgeGraph::new();
        engine.run(&mut pool, &graph, &MockLlm).await.expect("run failed");

        // At the point we observe completion, history must already contain all N snapshots.
        let sc = completion_rx.borrow().clone().expect("completion must be Some after run()");
        assert_eq!(sc.total_ticks, N);
        let history_len = history_arc.lock().len() as u32;
        assert_eq!(
            history_len, N,
            "history must be fully populated when completion signal is observed"
        );
    }

    #[tokio::test]
    async fn test_late_subscriber_sees_completion() {
        // watch holds its last value: a subscriber created AFTER run() must immediately
        // observe Some(SimCompletion) without any timing race.
        use crate::agent::{Agent, AgentPool, Persona};
        use crate::error::Result;
        use crate::llm::LlmClient;
        use async_trait::async_trait;
        use std::pin::Pin;

        struct MockLlm;
        #[async_trait]
        impl LlmClient for MockLlm {
            async fn complete(&self, _: &str) -> Result<String> {
                Ok("Think(idle)".to_string())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<String> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
        }

        const N: u32 = 2;
        let mut pool = AgentPool::new();
        pool.add_agent(Agent::new(Persona {
            name: "C".into(),
            background: "test".into(),
            traits: vec![],
            role: "none".into(),
            social: None,
        }));

        let engine = SimEngine::new(SimConfig::new(N, 1));
        let graph = crate::graph::KnowledgeGraph::new();
        // Run FIRST, subscribe AFTER — tests the watch "holds last value" property.
        engine.run(&mut pool, &graph, &MockLlm).await.expect("run failed");

        // Late subscriber: subscribes after run() has already sent the completion signal.
        let late_rx = engine.subscribe_completion();
        let completion = late_rx.borrow().clone();
        assert!(
            completion.is_some(),
            "late subscriber must see completion immediately via watch"
        );
        assert_eq!(completion.unwrap().total_ticks, N);
    }

    #[tokio::test]
    async fn test_snapshot_broadcast_unaffected_by_completion_channel() {
        // Regression: adding the completion channel must NOT break the existing snapshot
        // broadcast. subscribe() and subscribe_with_history() must still deliver all ticks.
        use crate::agent::{Agent, AgentPool, Persona};
        use crate::error::Result;
        use crate::llm::LlmClient;
        use async_trait::async_trait;
        use std::pin::Pin;

        struct MockLlm;
        #[async_trait]
        impl LlmClient for MockLlm {
            async fn complete(&self, _: &str) -> Result<String> {
                Ok("Observe(sky)".to_string())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<String> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _: &[crate::llm::ChatMessage],
                _: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(crate::error::TeriError::Llm("not used".into()))
            }
        }

        const N: u32 = 4;
        let mut pool = AgentPool::new();
        pool.add_agent(Agent::new(Persona {
            name: "D".into(),
            background: "test".into(),
            traits: vec![],
            role: "none".into(),
            social: None,
        }));

        let engine = SimEngine::new(SimConfig::new(N, 1));
        let mut snap_rx = engine.subscribe();
        let (mut snap_rx2, history_arc) = engine.subscribe_with_history();

        let graph = crate::graph::KnowledgeGraph::new();
        engine.run(&mut pool, &graph, &MockLlm).await.expect("run failed");

        // subscribe() still delivers all N snapshots
        let mut received = Vec::new();
        while let Ok(s) = snap_rx.try_recv() {
            received.push(s);
        }
        assert_eq!(received.len(), N as usize, "snapshot broadcast must deliver all ticks");
        for (i, s) in received.iter().enumerate() {
            assert_eq!(s.tick, (i + 1) as u32);
        }

        // subscribe_with_history() still works
        let mut received2 = Vec::new();
        while let Ok(s) = snap_rx2.try_recv() {
            received2.push(s);
        }
        assert_eq!(received2.len(), N as usize);
        assert_eq!(history_arc.lock().len(), N as usize);
    }

    #[test]
    fn test_sim_completion_serde_roundtrip() {
        let sc = SimCompletion { total_ticks: 42 };
        let json = serde_json::to_string(&sc).expect("serialize");
        let back: SimCompletion = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(sc, back);
    }

    // -----------------------------------------------------------------------
    // Cooperative-shutdown hook (additive, U-022 sub-cycle b)
    // -----------------------------------------------------------------------

    struct IdleLlm;
    #[async_trait::async_trait]
    impl crate::llm::LlmClient for IdleLlm {
        async fn complete(&self, _: &str) -> crate::error::Result<String> {
            Ok("Think(idle)".to_string())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &str,
        ) -> crate::error::Result<T> {
            Err(crate::error::TeriError::Llm("not used".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> crate::error::Result<
            std::pin::Pin<Box<dyn futures::Stream<Item = crate::error::Result<String>> + Send>>,
        > {
            Err(crate::error::TeriError::Llm("not used".into()))
        }
        async fn chat(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> crate::error::Result<String> {
            Err(crate::error::TeriError::Llm("not used".into()))
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> crate::error::Result<T> {
            Err(crate::error::TeriError::Llm("not used".into()))
        }
    }

    fn one_agent_pool(name: &str) -> crate::agent::AgentPool {
        use crate::agent::{Agent, AgentPool, Persona};
        let mut pool = AgentPool::new();
        pool.add_agent(Agent::new(Persona {
            name: name.into(),
            background: "test".into(),
            traits: vec![],
            role: "none".into(),
            social: None,
        }));
        pool
    }

    #[tokio::test]
    async fn test_no_shutdown_flag_runs_full_loop() {
        // Additive-safety: an engine with NO shutdown flag (every pre-existing caller)
        // runs the full max_ticks loop, identical to before with_shutdown existed.
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        const N: u32 = 6;
        let mut pool = one_agent_pool("nosd");
        let engine = SimEngine::new(SimConfig::new(N, 1));
        // Deliberately do NOT call with_shutdown.
        let graph = crate::graph::KnowledgeGraph::new();
        let result = engine.run(&mut pool, &graph, &IdleLlm).await.expect("run failed");
        assert_eq!(result.history.len() as u32, N, "no-shutdown engine must run full loop");

        // And a flag set to false is also a full run.
        let mut pool2 = one_agent_pool("nosd2");
        let mut engine2 = SimEngine::new(SimConfig::new(N, 1));
        engine2.with_shutdown(Arc::new(AtomicBool::new(false)));
        let result2 = engine2.run(&mut pool2, &graph, &IdleLlm).await.expect("run failed");
        assert_eq!(result2.history.len() as u32, N, "shutdown=false must run full loop");
    }

    #[tokio::test]
    async fn test_with_injections_applies_and_persists_across_ticks() {
        // God's-eye injection: a variable pushed into the shared queue is drained at the next
        // tick boundary, applied to the world, and persists in every subsequent snapshot.
        use std::sync::Arc;

        const N: u32 = 4;
        let mut pool = one_agent_pool("inject");
        let mut engine = SimEngine::new(SimConfig::new(N, 1));
        let queue: InjectionQueue = Arc::new(Mutex::new(Vec::new()));
        engine.with_injections(Arc::clone(&queue));
        // Push BEFORE run → drains at tick 0; persists for the remaining ticks.
        queue.lock().push(("crisis".to_string(), 1.0));

        let graph = crate::graph::KnowledgeGraph::new();
        let result = engine.run(&mut pool, &graph, &IdleLlm).await.expect("run failed");

        assert_eq!(result.history.len() as u32, N);
        for snap in &result.history {
            assert_eq!(
                snap.get_variable("crisis"),
                Some(1.0),
                "injected variable persists in every snapshot after it lands"
            );
        }
        assert!(queue.lock().is_empty(), "queue is drained after the tick applies it");
    }

    #[tokio::test]
    async fn test_no_injections_queue_is_noop() {
        // Additive-safety: an engine that never calls with_injections is byte-identical to before.
        const N: u32 = 3;
        let mut pool = one_agent_pool("noinject");
        let engine = SimEngine::new(SimConfig::new(N, 1));
        let graph = crate::graph::KnowledgeGraph::new();
        let result = engine.run(&mut pool, &graph, &IdleLlm).await.expect("run failed");
        assert_eq!(result.history.len() as u32, N);
        assert_eq!(result.history.last().unwrap().get_variable("crisis"), None);
    }

    #[tokio::test]
    async fn test_shutdown_before_run_yields_empty_graceful_completion() {
        // Flag already true before run(): the loop breaks at the FIRST tick boundary,
        // producing zero snapshots, but still emits a (partial) completion signal and
        // returns Ok — the graceful-stop contract.
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        const N: u32 = 50;
        let mut pool = one_agent_pool("sd-pre");
        let mut engine = SimEngine::new(SimConfig::new(N, 1));
        let flag = Arc::new(AtomicBool::new(true));
        engine.with_shutdown(Arc::clone(&flag));
        let completion_rx = engine.subscribe_completion();

        let graph = crate::graph::KnowledgeGraph::new();
        let result = engine.run(&mut pool, &graph, &IdleLlm).await.expect("graceful stop is Ok");

        assert_eq!(result.history.len(), 0, "pre-set shutdown breaks before first tick");
        let sc = completion_rx.borrow().clone().expect("completion still fires on graceful stop");
        assert_eq!(sc.total_ticks, 0, "partial total_ticks reflects the truncated run");
    }

    // -----------------------------------------------------------------------
    // U-028 (c1): SimConfig::from_simulation_config — the deterministic config→engine mapping.
    // Differential vs Python `int(total_hours * 60 / minutes_per_round)` + max_rounds `min`.
    // -----------------------------------------------------------------------

    /// Helper: build a minimal `simulation_config.json`-shaped value with the given time keys.
    fn time_cfg(hours: i64, mpr: i64) -> serde_json::Value {
        serde_json::json!({
            "time_config": { "total_simulation_hours": hours, "minutes_per_round": mpr }
        })
    }

    #[test]
    fn from_simulation_config_default_72h_30min() {
        // 72*60/30 = 144. Python: int(72*60/30) = 144.
        let cfg = SimConfig::from_simulation_config(&time_cfg(72, 30), None, 8);
        assert_eq!(cfg.max_ticks, 144);
        assert_eq!(cfg.parallelism, 8);
        assert!(cfg.inject_fn.is_none());
    }

    #[test]
    fn from_simulation_config_table_matches_python_floor() {
        // (hours, mpr) → int(hours*60/mpr). Each row is the Python `//`-equivalent floor.
        let cases = [
            (24, 60, 24), // 24*60/60 = 24
            (1, 30, 2),   // 60/30 = 2
            (72, 45, 96), // 4320/45 = 96
            (10, 7, 85),  // 600/7 = 85.71 → 85 (truncate toward zero)
            (1, 7, 8),    // 60/7 = 8.57 → 8
            (0, 30, 0),   // 0 rounds
        ];
        for (hours, mpr, expected) in cases {
            let cfg = SimConfig::from_simulation_config(&time_cfg(hours, mpr), None, 4);
            assert_eq!(cfg.max_ticks, expected, "({hours}h, {mpr}min) → {expected} rounds");
        }
    }

    #[test]
    fn from_simulation_config_max_rounds_truncates_only_when_smaller_and_positive() {
        // total = 144. max_rounds=50 → min(144,50)=50.
        let cfg = SimConfig::from_simulation_config(&time_cfg(72, 30), Some(50), 8);
        assert_eq!(cfg.max_ticks, 50);
        // max_rounds larger than total → no truncation (min keeps 144).
        let cfg = SimConfig::from_simulation_config(&time_cfg(72, 30), Some(1000), 8);
        assert_eq!(cfg.max_ticks, 144);
        // max_rounds <= 0 → guard skips truncation (Python `if max_rounds and max_rounds > 0`).
        let cfg = SimConfig::from_simulation_config(&time_cfg(72, 30), Some(0), 8);
        assert_eq!(cfg.max_ticks, 144);
        let cfg = SimConfig::from_simulation_config(&time_cfg(72, 30), Some(-5), 8);
        assert_eq!(cfg.max_ticks, 144);
    }

    #[test]
    fn from_simulation_config_missing_keys_use_script_defaults_72_30() {
        // Absent time_config → defaults total_hours=72, mpr=30 (the scripts' .get fallbacks).
        let empty = serde_json::json!({});
        let cfg = SimConfig::from_simulation_config(&empty, None, 8);
        assert_eq!(cfg.max_ticks, 144);
        // time_config present but partial: only hours → mpr defaults to 30.
        let partial = serde_json::json!({ "time_config": { "total_simulation_hours": 24 } });
        let cfg = SimConfig::from_simulation_config(&partial, None, 8);
        assert_eq!(cfg.max_ticks, 48); // 24*60/30
    }

    #[test]
    fn from_simulation_config_zero_cadence_yields_zero_rounds() {
        // minutes_per_round=0 would be Python ZeroDivisionError; teri → 0 rounds (no basis).
        let cfg = SimConfig::from_simulation_config(&time_cfg(72, 0), None, 8);
        assert_eq!(cfg.max_ticks, 0);
    }
}

// ───────────────────────────────────────────────────────────────────────────────
// U-028 (c3b-i): activation gate + actions.jsonl producer wiring (SimEngine::run)
//
// Differential contract vs `run_parallel_simulation.py:run_twitter_simulation` (the
// authoritative actions.jsonl producer): one `simulation_start`, then per round a
// `round_start` (ALWAYS, even empty) / N× `log_action` / `round_end`, then one
// `simulation_end`. Round numbers are 1-based (`round_num+1`). The `action_type` strings are
// the `ACTION_TYPE_MAP` values (golden-tested here); the DB-internal `action_args` enrichment is
// `[≠]U028-OASIS-INTERNALS`. RNG-free here: the gate is exercised with deterministic policies.
// ───────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod producer_tests {
    use super::*;
    use crate::agent::{Agent, AgentPool, Persona, Platform, SocialProfile};
    use crate::error::Result;
    use crate::llm::LlmClient;
    use crate::sim::action_logger::PlatformActionLogger;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// A test LLM whose `complete` always returns a fixed action string (→ a fixed action).
    struct FixedLlm(&'static str);
    #[async_trait]
    impl LlmClient for FixedLlm {
        async fn complete(&self, _: &str) -> Result<String> {
            Ok(self.0.to_string())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(crate::error::TeriError::Llm("not used".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(crate::error::TeriError::Llm("not used".into()))
        }
        async fn chat(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<String> {
            Err(crate::error::TeriError::Llm("not used".into()))
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<T> {
            Err(crate::error::TeriError::Llm("not used".into()))
        }
    }

    /// A deterministic activation policy returning a fixed id set every tick (no RNG).
    struct FixedActivation(Vec<i64>);
    impl ActivationPolicy for FixedActivation {
        fn active_agent_ids(&self, _tick: u32) -> Vec<i64> {
            self.0.clone()
        }
    }

    /// Build an agent carrying a `SocialProfile` with the given OASIS `user_id` + display name
    /// (Twitter platform).
    fn social_agent(user_id: u64, name: &str) -> Agent {
        social_agent_on(user_id, name, Platform::Twitter)
    }

    /// Like [`social_agent`] but on an explicit platform (for the U-030 parallel dual-sink test).
    fn social_agent_on(user_id: u64, name: &str, platform: Platform) -> Agent {
        let social = SocialProfile {
            user_id,
            user_name: format!("u{user_id}"),
            bio: String::new(),
            persona: String::new(),
            platform,
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
        Agent::new(Persona {
            name: name.to_string(),
            background: String::new(),
            traits: vec![],
            role: "agent".into(),
            social: Some(social),
        })
    }

    fn unique_dir(tag: &str) -> PathBuf {
        let id = uuid::Uuid::new_v4();
        let dir = std::env::temp_dir().join(format!("teri_producer_{tag}_{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Read `{base}/twitter/actions.jsonl` into a Vec of parsed JSON objects.
    fn read_jsonl(log_path: &std::path::Path) -> Vec<Value> {
        let content = std::fs::read_to_string(log_path).unwrap();
        content.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    /// 2 agents, no activation (all act every tick), 2 rounds, each emits CREATE_POST.
    /// Golden-asserts the full producer stream + 1-based rounds + action_type map + sim_end.
    #[tokio::test]
    async fn run_emits_full_actions_jsonl_stream() {
        let base = unique_dir("full_stream");
        let logger = Arc::new(PlatformActionLogger::new("twitter", &base).unwrap());
        let log_path = logger.log_path.clone();

        let mut pool = AgentPool::new();
        pool.add_agent(social_agent(0, "Alice"));
        pool.add_agent(social_agent(1, "Bob"));

        // total_simulation_hours=1, minutes_per_round=30 → from_simulation_config max_ticks=2.
        let config = serde_json::json!({
            "time_config": { "total_simulation_hours": 1, "minutes_per_round": 30 },
            "agent_configs": [{ "agent_id": 0 }, { "agent_id": 1 }]
        });
        let sim_config = SimConfig::from_simulation_config(&config, None, 1);
        assert_eq!(sim_config.max_ticks, 2, "1h / 30min = 2 rounds");

        let mut engine = SimEngine::new(sim_config);
        engine.with_producer(RunProducer {
            loggers: PlatformLoggerSet::single(Platform::Twitter, logger),
            config,
        });
        let graph = crate::graph::KnowledgeGraph::new();
        engine
            .run(&mut pool, &graph, &FixedLlm("CREATE_POST(content=hi)"))
            .await
            .expect("run");

        let recs = read_jsonl(&log_path);
        // 1 sim_start + round-0 (round_start + round_end, no initial_posts) + per round
        // (1 round_start + 2 actions + 1 round_end) ×2 + 1 sim_end = 12.
        assert_eq!(recs.len(), 12, "stream length: {recs:?}");

        // simulation_start first.
        assert_eq!(recs[0]["event_type"], "simulation_start");
        assert_eq!(recs[0]["agents_count"], 2);
        assert_eq!(recs[0]["total_rounds"], 2, "1h*2 = 2 (logger's own formula)");

        // Round 0 (initial-events phase): round_start(0,0) + round_end(0,0); no initial_posts here.
        assert_eq!(recs[1]["event_type"], "round_start");
        assert_eq!(recs[1]["round"], 0, "round-0 is the initial-events phase");
        assert_eq!(recs[1]["simulated_hour"], 0);
        assert_eq!(recs[2]["event_type"], "round_end");
        assert_eq!(recs[2]["round"], 0);
        assert_eq!(recs[2]["actions_count"], 0, "no initial_posts in this config");

        // Round 1 (1-based): round_start, 2× CREATE_POST, round_end(count=2).
        assert_eq!(recs[3]["event_type"], "round_start");
        assert_eq!(recs[3]["round"], 1, "rounds are 1-based (round_num+1)");
        assert_eq!(recs[3]["simulated_hour"], 0, "(0*30/60)%24 = 0");
        assert_eq!(recs[4]["action_type"], "CREATE_POST");
        assert_eq!(recs[4]["round"], 1);
        assert_eq!(recs[4]["action_args"], serde_json::json!({ "content": "hi" }));
        assert_eq!(recs[4]["success"], true);
        assert_eq!(recs[5]["action_type"], "CREATE_POST");
        assert_eq!(recs[6]["event_type"], "round_end");
        assert_eq!(recs[6]["round"], 1);
        assert_eq!(recs[6]["actions_count"], 2);

        // Round 2.
        assert_eq!(recs[7]["event_type"], "round_start");
        assert_eq!(recs[7]["round"], 2);
        assert_eq!(recs[10]["event_type"], "round_end");
        assert_eq!(recs[10]["actions_count"], 2);

        // simulation_end last: total_rounds == max_ticks (config-derived), total_actions == 4.
        assert_eq!(recs[11]["event_type"], "simulation_end");
        assert_eq!(recs[11]["total_rounds"], 2);
        assert_eq!(recs[11]["total_actions"], 4, "2 agents × 2 rounds (round-0 added 0)");

        // Agent ids/names came from the SocialProfile (user_id) + Persona (name).
        let names: Vec<&str> =
            [&recs[4], &recs[5]].iter().map(|r| r["agent_name"].as_str().unwrap()).collect();
        assert!(names.contains(&"Alice") && names.contains(&"Bob"));

        std::fs::remove_dir_all(&base).ok();
    }

    /// An empty activation set → round_start + round_end(0), NO log_action
    /// (Python `if not active_agents: ... log_round_end(.., 0); continue`).
    #[tokio::test]
    async fn run_empty_activation_round_logs_start_end_only() {
        let base = unique_dir("empty_round");
        let logger = Arc::new(PlatformActionLogger::new("twitter", &base).unwrap());
        let log_path = logger.log_path.clone();

        let mut pool = AgentPool::new();
        pool.add_agent(social_agent(0, "Alice"));

        let config = serde_json::json!({
            "time_config": { "total_simulation_hours": 1, "minutes_per_round": 60 },
            "agent_configs": [{ "agent_id": 0 }]
        });
        let mut engine = SimEngine::new(SimConfig::from_simulation_config(&config, None, 1));
        engine.with_activation(Arc::new(FixedActivation(vec![]))); // nobody is active
        engine.with_producer(RunProducer {
            loggers: PlatformLoggerSet::single(Platform::Twitter, logger),
            config,
        });
        let graph = crate::graph::KnowledgeGraph::new();
        engine
            .run(&mut pool, &graph, &FixedLlm("CREATE_POST(content=x)"))
            .await
            .expect("run");

        let recs = read_jsonl(&log_path);
        // sim_start + round-0 (start+end) + round-1 (start + end, empty) + sim_end = 6;
        // NO action records (1h/60min = 1 round, nobody active, no initial_posts).
        assert_eq!(recs.len(), 6, "empty round emits no actions: {recs:?}");
        assert_eq!(recs[1]["event_type"], "round_start");
        assert_eq!(recs[1]["round"], 0, "round-0 initial-events phase");
        assert_eq!(recs[2]["event_type"], "round_end");
        assert_eq!(recs[2]["round"], 0);
        assert_eq!(recs[2]["actions_count"], 0);
        assert_eq!(recs[3]["event_type"], "round_start");
        assert_eq!(recs[3]["round"], 1);
        assert_eq!(recs[4]["event_type"], "round_end");
        assert_eq!(recs[4]["actions_count"], 0);
        assert_eq!(recs[5]["event_type"], "simulation_end");
        assert_eq!(recs[5]["total_actions"], 0);
        assert!(
            recs.iter()
                .all(|r| r["event_type"] != Value::Null || r.get("action_type").is_none())
        );

        std::fs::remove_dir_all(&base).ok();
    }

    /// Activation selects a SUBSET by user_id → only the matched agent acts.
    #[tokio::test]
    async fn run_activation_gates_by_user_id() {
        let base = unique_dir("subset");
        let logger = Arc::new(PlatformActionLogger::new("twitter", &base).unwrap());
        let log_path = logger.log_path.clone();

        let mut pool = AgentPool::new();
        pool.add_agent(social_agent(10, "Ten"));
        pool.add_agent(social_agent(20, "Twenty"));
        pool.add_agent(social_agent(30, "Thirty"));

        let config = serde_json::json!({
            "time_config": { "total_simulation_hours": 1, "minutes_per_round": 60 },
            "agent_configs": []
        });
        let mut engine = SimEngine::new(SimConfig::from_simulation_config(&config, None, 1));
        engine.with_activation(Arc::new(FixedActivation(vec![20]))); // only user_id 20 active
        engine.with_producer(RunProducer {
            loggers: PlatformLoggerSet::single(Platform::Twitter, logger),
            config,
        });
        let graph = crate::graph::KnowledgeGraph::new();
        engine
            .run(&mut pool, &graph, &FixedLlm("FOLLOW(user_id=99)"))
            .await
            .expect("run");

        let recs = read_jsonl(&log_path);
        let actions: Vec<&Value> = recs.iter().filter(|r| r.get("action_type").is_some()).collect();
        assert_eq!(actions.len(), 1, "only the one gated-in agent acts");
        assert_eq!(actions[0]["agent_id"], 20);
        assert_eq!(actions[0]["agent_name"], "Twenty");
        assert_eq!(actions[0]["action_type"], "FOLLOW");
        assert_eq!(actions[0]["action_args"], serde_json::json!({ "user_id": "99" }));

        std::fs::remove_dir_all(&base).ok();
    }

    /// No producer installed → run() writes nothing and behaves exactly as before (history len).
    #[tokio::test]
    async fn run_without_producer_writes_no_log() {
        let base = unique_dir("no_producer");
        let log_path = base.join("twitter").join("actions.jsonl");

        let mut pool = AgentPool::new();
        pool.add_agent(social_agent(0, "Solo"));

        let engine = SimEngine::new(SimConfig::new(3, 1));
        let graph = crate::graph::KnowledgeGraph::new();
        let result = engine.run(&mut pool, &graph, &FixedLlm("Speak(hi)")).await.expect("run");

        assert_eq!(result.history.len(), 3, "all ticks ran (no activation gate)");
        assert!(!log_path.exists(), "no producer → no actions.jsonl written");

        std::fs::remove_dir_all(&base).ok();
    }

    /// The TimeActivationPolicy wires through the trait: an all-active config + the real policy
    /// produces a non-empty, monitor-terminating stream (integration of §4 policy into §5 run).
    #[tokio::test]
    async fn time_activation_policy_drives_run() {
        use crate::sim::activation::TimeActivationPolicy;
        let base = unique_dir("time_policy");
        let logger = Arc::new(PlatformActionLogger::new("twitter", &base).unwrap());
        let log_path = logger.log_path.clone();

        let mut pool = AgentPool::new();
        pool.add_agent(social_agent(0, "A"));
        pool.add_agent(social_agent(1, "B"));

        // activity_level 1.0 + active all hours + huge agents_per_hour → every agent always acts.
        let config = serde_json::json!({
            "time_config": {
                "total_simulation_hours": 1, "minutes_per_round": 30,
                "agents_per_hour_min": 100, "agents_per_hour_max": 100,
                "peak_hours": [], "off_peak_hours": []
            },
            "agent_configs": [
                { "agent_id": 0, "active_hours": (0..24).collect::<Vec<_>>(), "activity_level": 1.0 },
                { "agent_id": 1, "active_hours": (0..24).collect::<Vec<_>>(), "activity_level": 1.0 }
            ]
        });
        let policy = Arc::new(TimeActivationPolicy::from_config(&config, Some(7)));
        let mut engine = SimEngine::new(SimConfig::from_simulation_config(&config, None, 1));
        engine.with_activation(policy);
        engine.with_producer(RunProducer {
            loggers: PlatformLoggerSet::single(Platform::Twitter, logger),
            config,
        });
        let graph = crate::graph::KnowledgeGraph::new();
        engine
            .run(&mut pool, &graph, &FixedLlm("CREATE_POST(content=z)"))
            .await
            .expect("run");

        let recs = read_jsonl(&log_path);
        assert_eq!(recs.last().unwrap()["event_type"], "simulation_end", "monitor-terminating");
        let actions = recs.iter().filter(|r| r.get("action_type").is_some()).count();
        assert!(actions > 0, "all-active config produces real action records");

        std::fs::remove_dir_all(&base).ok();
    }

    /// U-030 parallel dual-sink: a `PlatformLoggerSet::parallel` over a mixed twitter+reddit pool
    /// fans boundary records to BOTH loggers and ROUTES each `log_action` to the committing agent's
    /// platform file — twitter agents' actions land in `twitter/actions.jsonl`, reddit agents' in
    /// `reddit/actions.jsonl`, with correct PER-PLATFORM `round_end`/`simulation_end` counts.
    #[tokio::test]
    async fn run_parallel_routes_actions_to_platform_loggers() {
        let base = unique_dir("parallel");
        let twitter = Arc::new(PlatformActionLogger::new("twitter", &base).unwrap());
        let reddit = Arc::new(PlatformActionLogger::new("reddit", &base).unwrap());
        let twitter_path = twitter.log_path.clone();
        let reddit_path = reddit.log_path.clone();

        // 2 twitter agents + 1 reddit agent, distinct user_ids.
        let mut pool = AgentPool::new();
        pool.add_agent(social_agent_on(10, "Tw1", Platform::Twitter));
        pool.add_agent(social_agent_on(11, "Tw2", Platform::Twitter));
        pool.add_agent(social_agent_on(20, "Rd1", Platform::Reddit));

        // 1h / 30min → 2 rounds; no activation gate → all act every tick.
        let config = serde_json::json!({
            "time_config": { "total_simulation_hours": 1, "minutes_per_round": 30 },
            "agent_configs": [{ "agent_id": 10 }, { "agent_id": 11 }, { "agent_id": 20 }]
        });
        let mut engine = SimEngine::new(SimConfig::from_simulation_config(&config, None, 1));
        engine.with_producer(RunProducer {
            loggers: PlatformLoggerSet::parallel(twitter, reddit),
            config,
        });
        let graph = crate::graph::KnowledgeGraph::new();
        engine
            .run(&mut pool, &graph, &FixedLlm("CREATE_POST(content=hi)"))
            .await
            .expect("run");

        let tw = read_jsonl(&twitter_path);
        let rd = read_jsonl(&reddit_path);

        // Both files get the fanned-out boundary stream: sim_start + round-0 (start+end, no
        // initial_posts) + (round_start+round_end)×2 + sim_end. Twitter additionally has 2
        // actions/round (2 twitter agents), reddit 1/round.
        // Twitter: 1 + 2 + (1 + 2 + 1)×2 + 1 = 12. Reddit: 1 + 2 + (1 + 1 + 1)×2 + 1 = 10.
        assert_eq!(tw.len(), 12, "twitter stream: {tw:?}");
        assert_eq!(rd.len(), 10, "reddit stream: {rd:?}");

        // Every ACTION record in each file belongs to that platform's agents only (routing).
        let tw_action_ids: Vec<i64> = tw
            .iter()
            .filter(|r| r.get("action_type").is_some())
            .map(|r| r["agent_id"].as_i64().unwrap())
            .collect();
        let rd_action_ids: Vec<i64> = rd
            .iter()
            .filter(|r| r.get("action_type").is_some())
            .map(|r| r["agent_id"].as_i64().unwrap())
            .collect();
        assert!(
            tw_action_ids.iter().all(|id| *id == 10 || *id == 11),
            "twitter ids: {tw_action_ids:?}"
        );
        assert!(rd_action_ids.iter().all(|id| *id == 20), "reddit ids: {rd_action_ids:?}");
        assert_eq!(tw_action_ids.len(), 4, "2 twitter agents × 2 rounds");
        assert_eq!(rd_action_ids.len(), 2, "1 reddit agent × 2 rounds");

        // Per-platform round_end counts: round-0 (0) then 2 main rounds — twitter 2/round,
        // reddit 1/round.
        let tw_round_ends: Vec<i64> = tw
            .iter()
            .filter(|r| r["event_type"] == "round_end")
            .map(|r| r["actions_count"].as_i64().unwrap())
            .collect();
        let rd_round_ends: Vec<i64> = rd
            .iter()
            .filter(|r| r["event_type"] == "round_end")
            .map(|r| r["actions_count"].as_i64().unwrap())
            .collect();
        assert_eq!(tw_round_ends, vec![0, 2, 2], "twitter round_end counts (round-0 + 2 rounds)");
        assert_eq!(rd_round_ends, vec![0, 1, 1], "reddit round_end counts (round-0 + 2 rounds)");

        // Per-platform simulation_end totals: twitter 4, reddit 2. Both files terminate on it
        // (drives the monitor dual-gate S-615).
        assert_eq!(tw.last().unwrap()["event_type"], "simulation_end");
        assert_eq!(rd.last().unwrap()["event_type"], "simulation_end");
        assert_eq!(tw.last().unwrap()["total_actions"], 4);
        assert_eq!(rd.last().unwrap()["total_actions"], 2);
        // Both share the same config-derived total_rounds (== max_ticks).
        assert_eq!(tw.last().unwrap()["total_rounds"], 2);
        assert_eq!(rd.last().unwrap()["total_rounds"], 2);

        // simulation_start fanned to both, each stamped with its own platform.
        assert_eq!(tw[0]["event_type"], "simulation_start");
        assert_eq!(tw[0]["platform"], "twitter");
        assert_eq!(rd[0]["event_type"], "simulation_start");
        assert_eq!(rd[0]["platform"], "reddit");

        std::fs::remove_dir_all(&base).ok();
    }

    /// U-030 round-0 initial_posts (run_parallel_simulation.py:1171-1211): each
    /// `event_config.initial_posts` entry is emitted as a round-0 CREATE_POST, ROUTED to the poster
    /// agent's platform logger; an unresolvable `poster_agent_id` is skipped; round-0 counts feed
    /// each platform's `total_actions`. Verified over a parallel (twitter+reddit) pool.
    #[tokio::test]
    async fn run_round0_initial_posts_route_by_platform() {
        let base = unique_dir("round0");
        let twitter = Arc::new(PlatformActionLogger::new("twitter", &base).unwrap());
        let reddit = Arc::new(PlatformActionLogger::new("reddit", &base).unwrap());
        let twitter_path = twitter.log_path.clone();
        let reddit_path = reddit.log_path.clone();

        let mut pool = AgentPool::new();
        pool.add_agent(social_agent_on(10, "Tw", Platform::Twitter));
        pool.add_agent(social_agent_on(20, "Rd", Platform::Reddit));

        // max_ticks 0 → the main loop body never runs; ONLY round-0 + boundary records are emitted,
        // isolating round-0. 3 initial_posts: a twitter agent (10), a reddit agent (20), and an
        // unresolvable id (99 → skipped, Python except:pass).
        let config = serde_json::json!({
            "time_config": { "total_simulation_hours": 0, "minutes_per_round": 30 },
            "event_config": { "initial_posts": [
                { "poster_agent_id": 10, "content": "tw hello" },
                { "poster_agent_id": 20, "content": "rd hello" },
                { "poster_agent_id": 99, "content": "ghost" }
            ]}
        });
        let mut engine = SimEngine::new(SimConfig::from_simulation_config(&config, None, 1));
        assert_eq!(engine.config().max_ticks, 0, "0h → 0 main rounds (round-0 only)");
        engine.with_producer(RunProducer {
            loggers: PlatformLoggerSet::parallel(twitter, reddit),
            config,
        });
        let graph = crate::graph::KnowledgeGraph::new();
        engine.run(&mut pool, &graph, &FixedLlm("DO_NOTHING")).await.expect("run");

        let tw = read_jsonl(&twitter_path);
        let rd = read_jsonl(&reddit_path);

        // Each file: sim_start + round_start(0) + 1 routed CREATE_POST + round_end(0,1) + sim_end = 5.
        assert_eq!(tw.len(), 5, "twitter round-0 stream: {tw:?}");
        assert_eq!(rd.len(), 5, "reddit round-0 stream: {rd:?}");

        // The twitter post (poster 10) landed in the twitter file ONLY; reddit post (20) in reddit.
        assert_eq!(tw[1]["event_type"], "round_start");
        assert_eq!(tw[1]["round"], 0);
        assert_eq!(tw[2]["action_type"], "CREATE_POST");
        assert_eq!(tw[2]["round"], 0);
        assert_eq!(tw[2]["agent_id"], 10);
        assert_eq!(tw[2]["agent_name"], "Tw");
        assert_eq!(tw[2]["action_args"], serde_json::json!({ "content": "tw hello" }));
        assert_eq!(tw[3]["event_type"], "round_end");
        assert_eq!(tw[3]["round"], 0);
        assert_eq!(tw[3]["actions_count"], 1, "1 twitter initial post");

        assert_eq!(rd[2]["action_type"], "CREATE_POST");
        assert_eq!(rd[2]["agent_id"], 20);
        assert_eq!(rd[2]["action_args"], serde_json::json!({ "content": "rd hello" }));
        assert_eq!(rd[3]["actions_count"], 1, "1 reddit initial post (ghost id 99 skipped)");

        // No twitter file holds the reddit post and vice-versa (routing); ghost id 99 nowhere.
        let all_ids: Vec<i64> = tw
            .iter()
            .chain(rd.iter())
            .filter(|r| r.get("action_type").is_some())
            .map(|r| r["agent_id"].as_i64().unwrap())
            .collect();
        assert_eq!(all_ids, vec![10, 20], "only resolvable posters, each in its own file");

        // simulation_end per-platform total_actions counts the round-0 post (1 each).
        assert_eq!(tw.last().unwrap()["event_type"], "simulation_end");
        assert_eq!(tw.last().unwrap()["total_actions"], 1);
        assert_eq!(rd.last().unwrap()["total_actions"], 1);

        std::fs::remove_dir_all(&base).ok();
    }
}
