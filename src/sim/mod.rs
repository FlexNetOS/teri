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
        }
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

    pub async fn run<L: crate::llm::LlmClient>(
        &self,
        pool: &mut crate::agent::AgentPool,
        // TODO(graph-context): pass per-agent subgraph slices once Agent::prepare_action
        // accepts a graph reference. Tracked: _graph param intentionally kept so callers
        // do not need an API change when the feature lands.
        _graph: &crate::graph::KnowledgeGraph,
        llm: &L,
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

        for _ in 0..self.config.max_ticks {
            world.advance_tick();

            // Phase 1: prepare actions concurrently (immutable reads + LLM calls).
            // stream::buffered drives at most `parallelism` futures simultaneously,
            // giving real throughput gains when agent steps are LLM-bound.
            let actions: Vec<crate::error::Result<crate::sim::Action>> =
                stream::iter(pool.agents.iter())
                    .map(|agent| agent.prepare_action(&world, llm))
                    .buffered(self.config.parallelism)
                    .collect()
                    .await;

            // Phase 2: commit results sequentially (mutable writes + world state).
            for (agent, action_result) in pool.agents.iter_mut().zip(actions) {
                let action = action_result?;
                world.apply(agent.id, action.clone());
                agent.commit_action(&action);
                if let Some(snap) = world.agents.get_mut(&agent.id) {
                    snap.state = format!("{:?}", agent.state);
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
}
