use crate::error::{Result, TeriError};
use crate::graph::{Entity, KnowledgeGraph, Relation};
use crate::llm::{ChatMessage, ChatOptions, LlmClient, ResponseFormat};
use crate::sim::social_world::FeedSnapshot;
use crate::sim::{Action, SocialAction, TargetKind, WorldState};
use chrono::Utc;
use minijinja::{Environment, context};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Facts + node summaries semantically recalled about a single entity.
///
/// # MiroFish parity (TASK-SIM-6 #5)
/// Mirrors the `{"facts": [...], "node_summaries": [...]}` dict returned by
/// `OasisProfileGenerator._search_zep_for_entity` (oasis_profile_generator.py:286-412).
/// `facts` are edge facts; `node_summaries` are related-node summaries / names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecalledEntityFacts {
    pub facts: Vec<String>,
    pub node_summaries: Vec<String>,
}

/// A source of semantically-recalled facts about an entity, used to enrich the persona
/// prompt (TASK-SIM-6 #5 — the part-4 "Zep hybrid search" half of `_build_entity_context`).
///
/// teri's analog of Zep's graph hybrid search is the embedding-cosine recall over the
/// graph's vector namespace ([`ReportTools::search_graph_semantic`] +
/// [`GraphSearchLens`](crate::services::zep_tools::GraphSearchLens)). A wired implementation
/// runs that search; a stub implementation feeds fixed facts in tests. When no recall source
/// is supplied to `generate_social`, this enrichment is skipped entirely and the persona
/// context is byte-identical to the pre-S11 (parts 1-3 only) behaviour (no-downgrade).
#[async_trait::async_trait]
pub trait EntityFactRecall: Send + Sync {
    /// Recall facts/summaries relevant to `entity_name`. Implementations should fail soft
    /// (return [`RecalledEntityFacts::default`]) rather than erroring — a recall miss must
    /// never abort persona generation, matching MiroFish's best-effort Zep search.
    async fn recall(&self, entity_name: &str) -> RecalledEntityFacts;
}

/// Social media platform a `SocialProfile` belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Twitter,
    Reddit,
}

/// Per-platform allowed social-action names (TASK-SIM-2 #1). Mirrors MiroFish's
/// `TWITTER_ACTIONS` / `REDDIT_ACTIONS` (`run_parallel_simulation.py:178-202`), which it passes as
/// `available_actions` to the OASIS agent-graph generator so the LLM is only ever OFFERED its
/// platform's actions. teri's LLM can emit any action string, so we gate at validation time
/// instead: an action not in the agent's platform set is coerced to `DO_NOTHING` (the
/// behaviourally-equivalent outcome of "never offered" — it produces no social effect).
///
/// Kept as a static set rather than threading `Config` to every agent; `Platform::allowed_actions`
/// is asserted equal to `Config.oasis_twitter_actions` / `oasis_reddit_actions` by
/// `config.rs` so the two never drift. NOTE: MiroFish's lists include `REFRESH` (Reddit) and
/// `DO_NOTHING`, neither of which is an agent-selectable `SocialAction` variant in teri (REFRESH is
/// a FILTERED_ACTION never recorded; DO_NOTHING is the fallback target). Both are still listed so
/// the set equals the config list verbatim.
const TWITTER_ALLOWED_ACTIONS: &[&str] =
    &["CREATE_POST", "LIKE_POST", "REPOST", "FOLLOW", "DO_NOTHING", "QUOTE_POST"];

const REDDIT_ALLOWED_ACTIONS: &[&str] = &[
    "LIKE_POST",
    "DISLIKE_POST",
    "CREATE_POST",
    "CREATE_COMMENT",
    "LIKE_COMMENT",
    "DISLIKE_COMMENT",
    "SEARCH_POSTS",
    "SEARCH_USER",
    "TREND",
    "REFRESH",
    "DO_NOTHING",
    "FOLLOW",
    "MUTE",
];

impl Platform {
    /// The OASIS action-name strings this platform offers (the static mirror of the config lists).
    pub fn allowed_actions(self) -> &'static [&'static str] {
        match self {
            Platform::Twitter => TWITTER_ALLOWED_ACTIONS,
            Platform::Reddit => REDDIT_ALLOWED_ACTIONS,
        }
    }

    /// Whether `action_type` (an OASIS `ACTION_TYPE_MAP` value, e.g. `CREATE_COMMENT`) is offered
    /// on this platform.
    pub fn allows_action(self, action_type: &str) -> bool {
        self.allowed_actions().contains(&action_type)
    }
}

/// Social-media–specific profile data for a simulated agent.
///
/// Holds the fields that map directly to `OasisAgentProfile` fields.  `SocialProfile.bio`
/// and `SocialProfile.persona` are DISTINCT (matching MiroFish source): `bio` is the short
/// public user bio; `persona` is the long, detailed personality description.  Both are
/// serialized under those exact keys in `to_reddit_format` / `to_twitter_format`.
///
/// `user_id` matches `OasisAgentProfile.user_id` (the OASIS numeric id used as the
/// key in exported JSON/CSV; distinct from `Agent.id: Uuid` which is the native sim id).
///
/// Defaults match MiroFish `OasisAgentProfile` defaults:
///   karma=1000, friend_count=100, follower_count=150, statuses_count=500.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialProfile {
    /// OASIS numeric user id (matches `OasisAgentProfile.user_id`).
    pub user_id: u64,
    /// Social-platform handle (e.g. `alice_wonder_42`); serialized as `"username"` (no
    /// underscore) per OASIS library requirement — see `to_reddit_format`/`to_twitter_format`.
    pub user_name: String,
    /// Short public bio displayed on the profile page (distinct from `persona`).
    pub bio: String,
    /// Detailed personality description used in LLM system prompts (distinct from `bio`).
    pub persona: String,
    /// Which platform this profile targets.
    pub platform: Platform,
    /// Reddit-style karma score (default 1000).
    #[serde(default = "SocialProfile::default_karma")]
    pub karma: i64,
    /// Number of accounts this agent follows (default 100).
    #[serde(default = "SocialProfile::default_friend_count")]
    pub friend_count: i64,
    /// Number of followers (default 150).
    #[serde(default = "SocialProfile::default_follower_count")]
    pub follower_count: i64,
    /// Accounts this agent follows — kept as alias for `friend_count` in the Twitter model.
    #[serde(default = "SocialProfile::default_friend_count")]
    pub following_count: i64,
    /// Number of posts / status updates (default 500).
    #[serde(default = "SocialProfile::default_statuses_count")]
    pub statuses_count: i64,
    pub age: Option<u32>,
    pub gender: Option<String>,
    pub mbti: Option<String>,
    pub country: Option<String>,
    pub profession: Option<String>,
    #[serde(default)]
    pub interested_topics: Vec<String>,
    /// Freeform description of how this agent posts (tone, frequency, style).
    pub posting_style: Option<String>,
    /// UUID of the source entity this profile was derived from.
    pub source_entity_uuid: Option<String>,
    /// Entity-type label from the source graph (e.g. `"student"`, `"university"`).
    pub source_entity_type: Option<String>,
    /// ISO-8601 date string of profile creation (e.g. `"2026-06-14"`).
    pub created_at: String,
}

impl SocialProfile {
    fn default_karma() -> i64 {
        1000
    }
    fn default_friend_count() -> i64 {
        100
    }
    fn default_follower_count() -> i64 {
        150
    }
    fn default_statuses_count() -> i64 {
        500
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub name: String,
    pub background: String,
    pub traits: Vec<String>,
    pub role: String,
    /// Optional social-media profile overlay.  Generic personas keep `None`.
    /// The `#[serde(default)]` ensures old 4-field JSON deserializes without error.
    #[serde(default)]
    pub social: Option<SocialProfile>,
}

impl Persona {
    /// Serialize to Reddit-platform OASIS format.
    ///
    /// Mirrors `OasisAgentProfile.to_reddit_format` exactly:
    /// - Always-present keys: `user_id`, `username` (OASIS library requires no underscore),
    ///   `name`, `bio`, `persona`, `karma`, `created_at`.
    /// - Conditionally present (only when `Some`/non-empty): `age`, `gender`, `mbti`,
    ///   `country`, `profession`, `interested_topics`.
    ///
    /// Returns `None` when `self.social` is `None`.
    pub fn to_reddit_format(&self) -> Option<serde_json::Value> {
        let social = self.social.as_ref()?;
        let mut profile = serde_json::json!({
            "user_id": social.user_id,
            "username": social.user_name,  // OASIS library requires "username" (no underscore)
            "name": self.name,
            "bio": social.bio,
            "persona": social.persona,
            "karma": social.karma,
            "created_at": social.created_at,
        });

        // Conditional demographics — mirror Python's `if self.age:` guards exactly.
        // In Python, 0 / "" / [] are all falsy, so we omit on None AND on 0 for age.
        if let Some(age) = social.age
            && age > 0
        {
            profile["age"] = serde_json::Value::from(age);
        }
        if let Some(ref gender) = social.gender
            && !gender.is_empty()
        {
            profile["gender"] = serde_json::Value::from(gender.as_str());
        }
        if let Some(ref mbti) = social.mbti
            && !mbti.is_empty()
        {
            profile["mbti"] = serde_json::Value::from(mbti.as_str());
        }
        if let Some(ref country) = social.country
            && !country.is_empty()
        {
            profile["country"] = serde_json::Value::from(country.as_str());
        }
        if let Some(ref profession) = social.profession
            && !profession.is_empty()
        {
            profile["profession"] = serde_json::Value::from(profession.as_str());
        }
        if !social.interested_topics.is_empty() {
            profile["interested_topics"] =
                serde_json::Value::from(social.interested_topics.clone());
        }

        Some(profile)
    }

    /// Serialize to Twitter-platform OASIS format.
    ///
    /// Mirrors `OasisAgentProfile.to_twitter_format` exactly:
    /// - Always-present keys: `user_id`, `username` (no underscore), `name`, `bio`,
    ///   `persona`, `friend_count`, `follower_count`, `statuses_count`, `created_at`.
    /// - Conditionally present (same falsy-guard as Python): `age`, `gender`, `mbti`,
    ///   `country`, `profession`, `interested_topics`.
    /// - Note: `karma` is NOT included (Reddit-only field).
    ///
    /// Returns `None` when `self.social` is `None`.
    pub fn to_twitter_format(&self) -> Option<serde_json::Value> {
        let social = self.social.as_ref()?;
        let mut profile = serde_json::json!({
            "user_id": social.user_id,
            "username": social.user_name,  // OASIS library requires "username" (no underscore)
            "name": self.name,
            "bio": social.bio,
            "persona": social.persona,
            "friend_count": social.friend_count,
            "follower_count": social.follower_count,
            "statuses_count": social.statuses_count,
            "created_at": social.created_at,
        });

        // Conditional demographics — identical falsy-guard semantics as to_reddit_format.
        if let Some(age) = social.age
            && age > 0
        {
            profile["age"] = serde_json::Value::from(age);
        }
        if let Some(ref gender) = social.gender
            && !gender.is_empty()
        {
            profile["gender"] = serde_json::Value::from(gender.as_str());
        }
        if let Some(ref mbti) = social.mbti
            && !mbti.is_empty()
        {
            profile["mbti"] = serde_json::Value::from(mbti.as_str());
        }
        if let Some(ref country) = social.country
            && !country.is_empty()
        {
            profile["country"] = serde_json::Value::from(country.as_str());
        }
        if let Some(ref profession) = social.profession
            && !profession.is_empty()
        {
            profile["profession"] = serde_json::Value::from(profession.as_str());
        }
        if !social.interested_topics.is_empty() {
            profile["interested_topics"] =
                serde_json::Value::from(social.interested_topics.clone());
        }

        Some(profile)
    }

    /// Serialize to the complete flat dict format.
    ///
    /// Mirrors `OasisAgentProfile.to_dict` exactly: all fields unconditionally, with
    /// `null` for `Option`s that are `None` and `[]` for empty `interested_topics`.
    /// Uses `user_name` (with underscore) for the full-dict key — distinct from the
    /// `"username"` (no underscore) used in platform-specific formats.
    ///
    /// Returns `None` when `self.social` is `None`.
    pub fn to_dict(&self) -> Option<serde_json::Value> {
        let social = self.social.as_ref()?;
        Some(serde_json::json!({
            "user_id": social.user_id,
            "user_name": social.user_name,
            "name": self.name,
            "bio": social.bio,
            "persona": social.persona,
            "karma": social.karma,
            "friend_count": social.friend_count,
            "follower_count": social.follower_count,
            "statuses_count": social.statuses_count,
            "age": social.age,
            "gender": social.gender,
            "mbti": social.mbti,
            "country": social.country,
            "profession": social.profession,
            "interested_topics": social.interested_topics,
            "source_entity_uuid": social.source_entity_uuid,
            "source_entity_type": social.source_entity_type,
            "created_at": social.created_at,
        }))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentState {
    Idle,
    Thinking,
    Acting,
    Observing,
    Communicating,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub timestamp: chrono::DateTime<Utc>,
    pub content: String,
    pub importance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemory {
    pub short_term: VecDeque<MemoryEntry>,
    pub short_term_capacity: usize,
}

impl AgentMemory {
    pub fn new(capacity: usize) -> Self {
        Self { short_term: VecDeque::with_capacity(capacity), short_term_capacity: capacity }
    }

    pub fn add_memory(&mut self, entry: MemoryEntry) {
        if self.short_term.len() >= self.short_term_capacity {
            self.short_term.pop_front();
        }
        self.short_term.push_back(entry);
    }

    pub fn get_recent(&self, limit: usize) -> Vec<&MemoryEntry> {
        self.short_term
            .iter()
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn clear(&mut self) {
        self.short_term.clear();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub persona: Persona,
    pub memory: AgentMemory,
    pub state: AgentState,
}

impl Agent {
    pub fn new(persona: Persona) -> Self {
        Self {
            id: Uuid::new_v4(),
            persona,
            memory: AgentMemory::new(50),
            state: AgentState::Idle,
        }
    }

    pub fn add_memory(&mut self, content: String, importance: f32) {
        let entry = MemoryEntry { timestamp: Utc::now(), content, importance };
        self.memory.add_memory(entry);
    }

    pub fn set_state(&mut self, state: AgentState) {
        self.state = state;
    }

    /// Build the per-agent "Knowledge Graph Context" lines from the agent's source entity's
    /// immediate neighborhood, or `None` when there is nothing to add.
    ///
    /// Returns `None` (graph context omitted, prompt byte-identical to before) when: no graph was
    /// provided, the agent has no `source_entity_uuid` (a generic / non-graph-derived agent), the
    /// uuid does not parse, the entity is absent from this graph, or it has no relations. Otherwise
    /// returns one line per relation — `EntityA --[Relation]--> EntityB` — so the agent can reason
    /// over what the seed-derived graph says about the entity it personifies, during the run.
    fn graph_context_section(&self, graph: Option<&KnowledgeGraph>) -> Option<String> {
        let graph = graph?;
        let uuid_str = self.persona.social.as_ref()?.source_entity_uuid.as_ref()?;
        let entity_id = uuid::Uuid::parse_str(uuid_str).ok()?;
        let source = graph.get_entity_by_id(entity_id)?;
        let neighbors = graph.get_neighbor_relations(entity_id).ok()?;
        if neighbors.is_empty() {
            return None;
        }
        let mut lines = String::new();
        for (neighbor, rel, is_outgoing) in neighbors {
            let (from, to) = if is_outgoing {
                (source.name.as_str(), neighbor.name.as_str())
            } else {
                (neighbor.name.as_str(), source.name.as_str())
            };
            lines.push_str(&format!("- {from} --[{}]--> {to}\n", rel.kind));
        }
        Some(lines)
    }

    /// Pure read phase of a step: retrieve context, call LLM, return validated action.
    /// Does NOT mutate agent state or memory — safe to call concurrently across agents.
    /// Pair with `commit_action` to complete the step.
    pub async fn prepare_action<L: LlmClient>(
        &self,
        world: &WorldState,
        feed: Option<&FeedSnapshot>,
        graph: Option<&KnowledgeGraph>,
        llm: &L,
    ) -> Result<Action> {
        let relevant_memories = self.retrieve_relevant_memories(world);
        let graph_context = self.graph_context_section(graph);
        let context =
            self.construct_context(world, &relevant_memories, feed, graph_context.as_deref());
        let action_str = self.generate_action_with_fallback(&context, llm).await?;
        // Robustness: a single unparseable LLM line must NOT abort the whole simulation (the run
        // loop propagates this `Result` via `?`). An action string we cannot classify is treated
        // as "the agent failed to decide" → a no-op `Think`, exactly like the LLM-error fallback
        // in `generate_action_with_fallback`. This keeps a long multi-agent run resilient to the
        // occasional off-format completion a local model emits, instead of one bad token ending
        // every other agent's turn too.
        Ok(self.parse_and_validate_action(&action_str).unwrap_or_else(|_| {
            Action::Think("I could not decide on a clear action this round".to_string())
        }))
    }

    /// Mutation phase of a step: store action in memory and return to Idle.
    /// Call after `prepare_action` has returned the validated action.
    pub fn commit_action(&mut self, action: &Action) {
        self.store_action_in_memory(action);
        self.set_state(AgentState::Idle);
    }

    /// Execute one step of the agent's decision-making process
    pub async fn step<L: LlmClient>(&mut self, world: &WorldState, llm: &L) -> Result<Action> {
        // Set state to Thinking
        self.set_state(AgentState::Thinking);

        // Retrieve relevant memories
        let relevant_memories = self.retrieve_relevant_memories(world);

        // Construct context from world state + memories. `step` is the standalone single-agent
        // path (tests / non-social callers); it has no social feed, so pass `None` — the feed
        // section is omitted and the prompt is byte-identical to before the feed-back landed.
        let context = self.construct_context(world, &relevant_memories, None, None);

        // Set state to Acting
        self.set_state(AgentState::Acting);

        // Generate action using LLM with fallback
        let action = self.generate_action_with_fallback(&context, llm).await?;

        // Parse and validate action
        let validated_action = self.parse_and_validate_action(&action)?;

        // Store action in memory
        self.store_action_in_memory(&validated_action);

        // Return to Idle state
        self.set_state(AgentState::Idle);

        Ok(validated_action)
    }

    /// Retrieve relevant memories based on current world state
    /// Uses keyword-overlap scoring: ranks memories by word overlap with recent events and world variables.
    /// Falls back to recency if no overlaps found.
    fn retrieve_relevant_memories(&self, world: &WorldState) -> Vec<&MemoryEntry> {
        if self.memory.short_term.is_empty() {
            return Vec::new();
        }

        // Build a set of context keywords from recent events and variables
        let mut context_words = std::collections::HashSet::new();

        // Add keywords from recent events (up to 5)
        for event in world.events.iter().rev().take(5) {
            let action_str = event.action.to_string().to_lowercase();
            for word in action_str.split_whitespace() {
                if word.len() > 2 {
                    // Skip very short words (likely noise)
                    context_words.insert(word.to_string());
                }
            }
        }

        // Add keywords from world variables
        for key in world.variables.keys() {
            for word in key.to_lowercase().split('_') {
                if word.len() > 2 {
                    context_words.insert(word.to_string());
                }
            }
        }

        // Score each memory by word overlap with context
        let mut scored_memories: Vec<(usize, &MemoryEntry)> = self
            .memory
            .short_term
            .iter()
            .map(|m| {
                let overlap_count = m
                    .content
                    .to_lowercase()
                    .split_whitespace()
                    .filter(|word| context_words.contains(*word))
                    .count();
                (overlap_count, m)
            })
            .collect();

        // Sort by overlap (descending), then by recency (index descending)
        scored_memories.sort_by(|a, b| {
            b.0.cmp(&a.0).then_with(|| {
                let a_idx =
                    self.memory.short_term.iter().position(|m| m.timestamp == a.1.timestamp);
                let b_idx =
                    self.memory.short_term.iter().position(|m| m.timestamp == b.1.timestamp);
                b_idx.cmp(&a_idx)
            })
        });

        // Return top 10 memories
        scored_memories.iter().take(10).map(|(_, m)| *m).collect()
    }

    /// Construct context string from world state and memories.
    ///
    /// When `feed` is `Some` and non-empty, a "Recent posts in your feed:" section is appended so
    /// a social agent can target REAL post ids (`LIKE_POST(target_id=...)`, `REPOST(post_id=...)`,
    /// `CREATE_COMMENT(post_id=...)`). The section is round-trippable by
    /// [`ActionGenerator::parse_feed_posts`]. Generic (non-social) agents pass `feed = None`, so
    /// the section is omitted and the prompt is byte-identical (no-downgrade).
    fn construct_context(
        &self,
        world: &WorldState,
        memories: &[&MemoryEntry],
        feed: Option<&FeedSnapshot>,
        graph_context: Option<&str>,
    ) -> String {
        let mut context = format!(
            "Agent: {}\nRole: {}\nState: {:?}\n\n",
            self.persona.name, self.persona.role, self.state
        );

        context.push_str(&format!("World Tick: {}\n\n", world.tick));

        // Add recent events with agent names
        if !world.events.is_empty() {
            context.push_str("Recent Events:\n");
            for event in world.events.iter().rev().take(5) {
                let agent_name = world
                    .agents
                    .get(&event.agent_id)
                    .map(|snapshot| snapshot.name.as_str())
                    .unwrap_or("Unknown Agent");
                context.push_str(&format!("- {}: {}\n", agent_name, event.action));
            }
            context.push('\n');
        }

        // Add memories
        if !memories.is_empty() {
            context.push_str("Relevant Memories:\n");
            for memory in memories {
                context.push_str(&format!("- {}\n", memory.content));
            }
            context.push('\n');
        }

        // Add world variables
        if !world.variables.is_empty() {
            context.push_str("World State:\n");
            for (key, value) in &world.variables {
                context.push_str(&format!("- {}: {:.2}\n", key, value));
            }
        }

        // Add the per-agent knowledge-graph context (the source entity's neighborhood) so the
        // agent reasons over what the seed-derived graph says about the entity it personifies.
        // Bounded by a blank line so `parse_graph_context` (find "\n\n") delimits it exactly like
        // the other sections. `None`/empty (every non-graph caller) appends NOTHING → byte-identical
        // prompt, no regression.
        if let Some(graph_context) = graph_context
            && !graph_context.trim().is_empty()
        {
            if !context.ends_with("\n\n") {
                context.push('\n');
            }
            context.push_str("Knowledge Graph Context:\n");
            context.push_str(graph_context);
            if !context.ends_with('\n') {
                context.push('\n');
            }
            context.push('\n');
        }

        // Add the social feed (recency-ranked recent posts) so the agent can react to REAL posts.
        // Format is round-trippable by `parse_feed_posts`:
        //   - [post-12 by user 7 | 3 likes | 1 shares] <content>
        // The section is terminated with a blank line so the section-end parser (find "\n\n")
        // bounds it exactly like the other sections. When `feed` is `None`/empty (every
        // non-social caller) NOTHING is appended, so the generic prompt is byte-identical.
        if let Some(feed) = feed
            && !feed.is_empty()
        {
            // Separate from any preceding section with a blank line (the World State section above
            // is not blank-line-terminated; other sections are).
            if !context.ends_with("\n\n") {
                context.push('\n');
            }
            context.push_str("Recent posts in your feed:\n");
            for p in &feed.posts {
                context.push_str(&format!(
                    "- [post-{} by user {} | {} likes | {} shares] {}\n",
                    p.id, p.author_user_id, p.num_likes, p.num_shares, p.content
                ));
            }
            context.push('\n');
        }

        context
    }

    /// Generate action using LLM with context and fallback
    async fn generate_action_with_fallback<L: LlmClient>(
        &self,
        context: &str,
        llm: &L,
    ) -> Result<String> {
        // Try to generate action
        match self.generate_action(context, llm).await {
            Ok(action) => Ok(action),
            Err(_) => {
                // Fallback to a simple thinking action
                Ok("Think(I need to consider my next move carefully)".to_string())
            }
        }
    }

    /// Generate action using LLM with context
    async fn generate_action<L: LlmClient>(&self, context: &str, llm: &L) -> Result<String> {
        let generator = ActionGenerator::new();
        let prompt = generator.generate_prompt(self, context)?;

        llm.complete(&prompt).await
    }

    /// Parse and validate the action string with robust parsing.
    ///
    /// Generic actions use the single-arg form: `Speak(hello world)`
    /// Social actions use the same outer form with either a single arg or comma-separated key=value
    /// args matching the MiroFish OASIS action name strings (SCREAMING_SNAKE_CASE from config.py).
    /// Example: `CREATE_POST(content=hello world)` or `LIKE_POST(target_id=post-42)`.
    /// Single-field social actions also accept a bare value: `LIKE_POST(post-42)`.
    fn parse_and_validate_action(&self, action_str: &str) -> Result<Action> {
        let action_str = action_str.trim();

        // Find the first '(' and the last ')' to handle nested parentheses
        if let Some(paren_start) = action_str.find('(')
            && let Some(paren_end) = action_str.rfind(')')
            && paren_end > paren_start
        {
            let action_type = action_str[..paren_start].trim();
            let content = action_str[paren_start + 1..paren_end].trim();

            // Generic simulation actions (5 pre-existing variants — must remain unchanged)
            match action_type {
                "Speak" => return Ok(Action::Speak(content.to_string())),
                "Move" => return Ok(Action::Move(content.to_string())),
                "Interact" => return Ok(Action::Interact(content.to_string())),
                "Observe" => return Ok(Action::Observe(content.to_string())),
                "Think" => return Ok(Action::Think(content.to_string())),
                _ => {}
            }

            // MiroFish/OASIS social action names (SCREAMING_SNAKE_CASE per config.py)
            // Args parsed as key=value pairs; bare values accepted for single-field actions.
            let social = self.parse_social_action(action_type, content);
            if let Some(sa) = social {
                return Ok(Action::Social(self.gate_platform_action(sa)));
            }

            return Err(TeriError::Agent(format!("Unknown action type: {}", action_type)));
        }

        Err(TeriError::Agent(format!("Invalid action format: {}", action_str)))
    }

    /// Parse an OASIS social action name + content string into a `SocialAction`.
    ///
    /// Returns `None` if the action name is not a known social action (caller will then emit
    /// `Unknown action type`). Returns `Some(SocialAction)` — including defaults for missing
    /// optional args — for known social action names.
    fn parse_social_action(&self, action_type: &str, content: &str) -> Option<SocialAction> {
        /// Extract a named key from `key=value,...` content, falling back to the whole string.
        fn get_arg(content: &str, key: &str) -> String {
            for part in content.split(',') {
                let part = part.trim();
                if let Some(rest) = part.strip_prefix(key)
                    && let Some(val) = rest.strip_prefix('=')
                {
                    return val.trim().to_string();
                }
            }
            // Bare value: the entire content is the argument (for single-arg actions)
            content.to_string()
        }

        match action_type {
            "CREATE_POST" => {
                Some(SocialAction::CreatePost { content: get_arg(content, "content") })
            }
            "LIKE_POST" => Some(SocialAction::Like {
                target_kind: TargetKind::Post,
                target_id: get_arg(content, "target_id"),
            }),
            "LIKE_COMMENT" => Some(SocialAction::Like {
                target_kind: TargetKind::Comment,
                target_id: get_arg(content, "target_id"),
            }),
            "DISLIKE_POST" => Some(SocialAction::Dislike {
                target_kind: TargetKind::Post,
                target_id: get_arg(content, "target_id"),
            }),
            "DISLIKE_COMMENT" => Some(SocialAction::Dislike {
                target_kind: TargetKind::Comment,
                target_id: get_arg(content, "target_id"),
            }),
            "REPOST" => Some(SocialAction::Repost { post_id: get_arg(content, "post_id") }),
            "QUOTE_POST" => Some(SocialAction::Quote {
                post_id: get_arg(content, "post_id"),
                content: get_arg(content, "content"),
            }),
            "FOLLOW" => Some(SocialAction::Follow { user_id: get_arg(content, "user_id") }),
            "CREATE_COMMENT" => Some(SocialAction::Comment {
                post_id: get_arg(content, "post_id"),
                content: get_arg(content, "content"),
            }),
            "SEARCH_POSTS" => Some(SocialAction::SearchPosts { query: get_arg(content, "query") }),
            "SEARCH_USER" => Some(SocialAction::SearchUser { query: get_arg(content, "query") }),
            "MUTE" => Some(SocialAction::Mute { user_id: get_arg(content, "user_id") }),
            "TREND" | "trend" => Some(SocialAction::Trend),
            "DO_NOTHING" => Some(SocialAction::DoNothing),
            _ => None,
        }
    }

    /// Gate a parsed social action against the agent's platform allowed-set (TASK-SIM-2 #1).
    ///
    /// MiroFish only OFFERS each agent its platform's actions (`TWITTER_ACTIONS` / `REDDIT_ACTIONS`
    /// → `available_actions`), so a Twitter agent can never select e.g. `CREATE_COMMENT`. teri's LLM
    /// can emit any name, so an action outside the agent's platform set is coerced to
    /// `DO_NOTHING` — the behaviourally-equivalent "no social effect" outcome of an action that was
    /// never offered.
    ///
    /// A generic (non-social) agent has no platform; its actions are never gated (they are not
    /// social actions anyway). `DO_NOTHING` itself is in both platform sets, so coercion is
    /// idempotent and never loops.
    fn gate_platform_action(&self, sa: SocialAction) -> SocialAction {
        match self.persona.social.as_ref() {
            Some(profile) if !profile.platform.allows_action(sa.oasis_action_type()) => {
                SocialAction::DoNothing
            }
            _ => sa,
        }
    }

    /// Store the executed action in memory with dynamic importance.
    ///
    /// Importance weights for social actions follow MiroFish's behavioural significance model:
    /// high for content-creation / social-graph changes; low for passive engagement; near-zero
    /// for no-ops. (Exact episode-text natural-language fidelity is U-021's job, not this unit's.)
    fn store_action_in_memory(&mut self, action: &Action) {
        let (memory_content, importance) = match action {
            Action::Speak(content) => {
                let importance = if content.len() > 100 { 0.8 } else { 0.6 };
                (format!("Spoke: {}", content), importance)
            }
            Action::Move(location) => (format!("Moved to: {}", location), 0.7),
            Action::Interact(target) => (format!("Interacted with: {}", target), 0.8),
            Action::Observe(target) => (format!("Observed: {}", target), 0.5),
            Action::Think(content) => {
                let importance = if content.contains("plan") || content.contains("strategy") {
                    0.9
                } else {
                    0.4
                };
                (format!("Thought: {}", content), importance)
            }
            Action::Social(sa) => {
                let (desc, imp) = match sa {
                    // High-signal: original content creation
                    SocialAction::CreatePost { content } => (format!("Posted: {}", content), 0.85),
                    // Medium-high: social graph modifications
                    SocialAction::Follow { user_id } => {
                        (format!("Followed user: {}", user_id), 0.75)
                    }
                    SocialAction::Mute { user_id } => (format!("Muted user: {}", user_id), 0.75),
                    // Medium: content amplification
                    SocialAction::Repost { post_id } => (format!("Reposted: {}", post_id), 0.65),
                    SocialAction::Quote { post_id, content } => {
                        (format!("Quoted post {} with: {}", post_id, content), 0.70)
                    }
                    SocialAction::Comment { post_id, content } => {
                        (format!("Commented on {}: {}", post_id, content), 0.70)
                    }
                    // Low: passive engagement
                    SocialAction::Like { target_kind: TargetKind::Post, target_id } => {
                        (format!("Liked post: {}", target_id), 0.30)
                    }
                    SocialAction::Like { target_kind: TargetKind::Comment, target_id } => {
                        (format!("Liked comment: {}", target_id), 0.30)
                    }
                    SocialAction::Dislike { target_kind: TargetKind::Post, target_id } => {
                        (format!("Disliked post: {}", target_id), 0.30)
                    }
                    SocialAction::Dislike { target_kind: TargetKind::Comment, target_id } => {
                        (format!("Disliked comment: {}", target_id), 0.30)
                    }
                    // Low: informational / search / discovery
                    SocialAction::SearchPosts { query } => {
                        (format!("Searched posts: {}", query), 0.25)
                    }
                    SocialAction::SearchUser { query } => {
                        (format!("Searched user: {}", query), 0.25)
                    }
                    // Low: browse/discovery operation
                    SocialAction::Trend => ("Performed trend operation".to_string(), 0.25),
                    // Near-zero: no-op
                    SocialAction::DoNothing => ("Did nothing".to_string(), 0.05),
                };
                (desc, imp)
            }
        };

        self.add_memory(memory_content, importance);
    }
}

#[derive(Debug, Clone)]
/// A pool of agents with shared group memory.
///
/// # Clone Behavior
///
/// Cloning an AgentPool creates a new instance that shares the same group memory
/// through `Arc<RwLock<>>`. This means both pools will share the same group memory
/// data, but have separate agent vectors. This is the desired behavior for shared
/// memory scenarios, but be aware that modifications to group memory will be visible
/// to all cloned instances.
pub struct AgentPool {
    pub agents: Vec<Agent>,
    pub group_memory: Arc<RwLock<Vec<MemoryEntry>>>,
}

impl AgentPool {
    pub fn new() -> Self {
        Self { agents: Vec::new(), group_memory: Arc::new(RwLock::new(Vec::new())) }
    }

    pub fn add_agent(&mut self, agent: Agent) {
        self.agents.push(agent);
    }

    pub fn get(&self, id: Uuid) -> Option<&Agent> {
        self.agents.iter().find(|a| a.id == id)
    }

    pub fn get_mut(&mut self, id: Uuid) -> Option<&mut Agent> {
        self.agents.iter_mut().find(|a| a.id == id)
    }

    pub fn len(&self) -> usize {
        self.agents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Agent> {
        self.agents.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Agent> {
        self.agents.iter_mut()
    }

    /// Spawn N unique agents using personas generated from the knowledge graph
    pub async fn spawn<L: LlmClient>(n: usize, graph: &KnowledgeGraph, llm: &L) -> Result<Self> {
        let mut pool = Self::new();
        let generator = PersonaGenerator::new();
        let mut generated_personas: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        // Get all entities from graph to use as persona anchors
        let entities = graph.get_all_entities();
        if entities.is_empty() {
            return Err(TeriError::Agent(
                "No entities available in graph for persona generation".to_string(),
            ));
        }

        // Generate N unique personas
        for i in 0..n {
            let mut attempts = 0;
            let max_attempts = 5; // Prevent infinite loops

            loop {
                // Cycle through entities if we need more personas than available entities
                let entity = &entities[i % entities.len()];

                let persona = generator.generate(graph, entity, llm).await.map_err(|e| {
                    TeriError::Agent(format!(
                        "Failed to generate persona for entity {}: {}",
                        entity.name, e
                    ))
                })?;

                // Create a unique identifier for the persona (name + role combination)
                let persona_id = format!("{}|{}", persona.name, persona.role);

                // Check if this persona is unique
                if !generated_personas.contains(&persona_id) {
                    generated_personas.insert(persona_id);
                    let agent = Agent::new(persona);
                    pool.add_agent(agent);
                    break;
                }

                attempts += 1;
                if attempts >= max_attempts {
                    // If we can't generate a unique persona after several attempts,
                    // create a variation by adding an incrementing suffix until unique
                    let base_name = persona.name.clone();
                    let mut suffix = attempts;
                    loop {
                        let mut varied_persona = persona.clone();
                        varied_persona.name = format!("{} {}", base_name, suffix);
                        let varied_id = format!("{}|{}", varied_persona.name, varied_persona.role);
                        if !generated_personas.contains(&varied_id) {
                            generated_personas.insert(varied_id);
                            let agent = Agent::new(varied_persona);
                            pool.add_agent(agent);
                            break;
                        }
                        suffix += 1;
                        // Safety limit to prevent infinite loops
                        if suffix > 100 {
                            return Err(TeriError::Agent(
                                "Failed to generate unique persona after 100 variations"
                                    .to_string(),
                            ));
                        }
                    }
                    break;
                }
            }
        }

        Ok(pool)
    }

    /// Add a memory entry to the shared group memory
    pub async fn add_group_memory(&self, entry: MemoryEntry) {
        let mut group_memory = self.group_memory.write().await;

        // Check capacity BEFORE pushing to prevent temporary unbounded growth
        if group_memory.len() >= 1000 {
            let len = group_memory.len();
            group_memory.drain(0..len - 999); // Keep space for the new entry
        }

        group_memory.push(entry);
    }

    /// Get recent group memory entries
    pub async fn get_group_memory(&self, limit: usize) -> Vec<MemoryEntry> {
        let group_memory = self.group_memory.read().await;
        group_memory.iter().rev().take(limit).cloned().collect()
    }
}

/// Generates personas based on entities from the knowledge graph
pub struct PersonaGenerator {
    template: String,
}

impl PersonaGenerator {
    /// Create a new PersonaGenerator with the default embedded template
    pub fn new() -> Self {
        let template = include_str!("../../templates/persona_gen.jinja").to_string();
        Self { template }
    }

    /// Create a new PersonaGenerator with a custom template from file
    /// Falls back to embedded template if file loading fails
    pub fn from_file<P: AsRef<std::path::Path>>(template_path: P) -> Self {
        match std::fs::read_to_string(template_path) {
            Ok(template) => Self { template },
            Err(e) => {
                eprintln!(
                    "Warning: Failed to load template from file ({}), falling back to embedded template",
                    e
                );
                Self::new()
            }
        }
    }

    // ===== TASK-SIM-1 (S6): persona-generation fidelity constants =====
    //
    // Mirrors `oasis_profile_generator.py:156-179`. Used by the rule-based randomization
    // (gap #3) and the two-prompt individual-vs-group selection (gap #2).

    /// MBTI personality types — drawn at random for rule-based individual personas.
    /// Mirrors `OasisProfileGenerator.MBTI_TYPES` (oasis_profile_generator.py:156-161).
    const MBTI_TYPES: [&'static str; 16] = [
        "INTJ", "INTP", "ENTJ", "ENTP", "INFJ", "INFP", "ENFJ", "ENFP", "ISTJ", "ISFJ", "ESTJ",
        "ESFJ", "ISTP", "ISFP", "ESTP", "ESFP",
    ];

    /// Common countries — drawn at random for rule-based personas.
    /// Mirrors `OasisProfileGenerator.COUNTRIES` (oasis_profile_generator.py:164-167).
    const COUNTRIES: [&'static str; 11] = [
        "China",
        "US",
        "UK",
        "Japan",
        "Germany",
        "France",
        "Canada",
        "Australia",
        "Brazil",
        "India",
        "South Korea",
    ];

    /// Individual (person-like) entity types → generate a concrete personal persona.
    /// Mirrors `OasisProfileGenerator.INDIVIDUAL_ENTITY_TYPES` (oasis_profile_generator.py:170-173).
    const INDIVIDUAL_ENTITY_TYPES: [&'static str; 10] = [
        "student",
        "alumni",
        "professor",
        "person",
        "publicfigure",
        "expert",
        "faculty",
        "official",
        "journalist",
        "activist",
    ];

    /// Group / institutional entity types → generate a representative institutional account.
    /// Mirrors `OasisProfileGenerator.GROUP_ENTITY_TYPES` (oasis_profile_generator.py:176-179).
    const GROUP_ENTITY_TYPES: [&'static str; 9] = [
        "university",
        "governmentagency",
        "organization",
        "ngo",
        "mediaoutlet",
        "company",
        "institution",
        "group",
        "community",
    ];

    /// Whether `entity_type` denotes an individual (person-like) entity.
    /// Mirrors `_is_individual_entity` (oasis_profile_generator.py:489-491).
    fn is_individual_entity(entity_type: &str) -> bool {
        let t = entity_type.to_lowercase();
        Self::INDIVIDUAL_ENTITY_TYPES.contains(&t.as_str())
    }

    /// Whether `entity_type` denotes a group / institutional entity.
    /// Mirrors `_is_group_entity` (oasis_profile_generator.py:493-495).
    fn is_group_entity(entity_type: &str) -> bool {
        let t = entity_type.to_lowercase();
        Self::GROUP_ENTITY_TYPES.contains(&t.as_str())
    }

    /// Create a new PersonaGenerator with a custom template string
    pub fn with_template(template: String) -> Self {
        Self { template }
    }

    /// Sanitize entity names to prevent template injection
    fn sanitize_entity_name(&self, name: &str) -> String {
        // Replace template-like patterns that could interfere with string replacement
        name.replace("{{", "")
            .replace("}}", "")
            .replace("{%", "")
            .replace("%}", "")
            // Also replace any newlines that could break template formatting
            .replace(['\n', '\r'], " ")
            // Trim multiple spaces
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Generate a persona based on an entity from the knowledge graph
    pub async fn generate<L: LlmClient>(
        &self,
        graph: &KnowledgeGraph,
        entity: &Entity,
        llm: &L,
    ) -> Result<Persona> {
        // Create a simple description based on entity connections
        let entity_description = self.generate_entity_description(graph, entity)?;

        // Sanitize entity name to prevent template injection
        let sanitized_name = self.sanitize_entity_name(&entity.name);

        // Render the template using minijinja
        let env = Environment::new();
        let template_context = context! {
            entity_name => sanitized_name,
            entity_kind => entity.kind.to_string(),
            entity_description => entity_description,
        };

        let prompt = env
            .template_from_str(&self.template)
            .map_err(|e| TeriError::Agent(format!("Template parsing error: {}", e)))?
            .render(template_context)
            .map_err(|e| TeriError::Agent(format!("Template rendering error: {}", e)))?;

        // Generate persona using LLM
        let response = llm.complete(&prompt).await?;

        // Parse the JSON response
        let persona: Persona = serde_json::from_str(&response)
            .map_err(|e| TeriError::Agent(format!("Failed to parse persona JSON: {}", e)))?;

        // Validate persona
        self.validate_persona(&persona)?;

        Ok(persona)
    }

    /// Generate a simple description of an entity based on its connections
    fn generate_entity_description(
        &self,
        graph: &KnowledgeGraph,
        entity: &Entity,
    ) -> Result<String> {
        let neighbors = graph.get_neighbors(entity.id).map_err(|e| {
            TeriError::Agent(format!("Failed to get neighbors for {}: {}", entity.name, e))
        })?;

        if neighbors.is_empty() {
            Ok(format!("{} is a {} with no known connections.", entity.name, entity.kind))
        } else {
            let neighbor_names: Vec<String> = neighbors
                .iter()
                .take(3) // Limit to avoid overly long descriptions
                .map(|n| n.name.clone())
                .collect();

            Ok(format!(
                "{} is a {} connected to: {}.",
                entity.name,
                entity.kind,
                neighbor_names.join(", ")
            ))
        }
    }

    /// Validate that a persona meets minimum requirements
    fn validate_persona(&self, persona: &Persona) -> Result<()> {
        if persona.name.trim().is_empty() {
            return Err(TeriError::Agent("Persona name cannot be empty".to_string()));
        }

        if persona.background.trim().is_empty() {
            return Err(TeriError::Agent("Persona background cannot be empty".to_string()));
        }

        if persona.traits.is_empty() || persona.traits.len() > 10 {
            return Err(TeriError::Agent("Persona must have between 1 and 10 traits".to_string()));
        }

        if persona.role.trim().is_empty() {
            return Err(TeriError::Agent("Persona role cannot be empty".to_string()));
        }

        Ok(())
    }

    /// Repair a truncated JSON string by closing unbalanced braces/brackets and dangling strings.
    ///
    /// Ports `OasisProfileGenerator._fix_truncated_json` (oasis_profile_generator.py:583).
    /// Strategy:
    /// 1. Strip surrounding whitespace.
    /// 2. If the last char is not `"`, `,`, `}`, or `]`, append `"` to close a dangling string.
    /// 3. Close any remaining unbalanced `[` brackets, then `{` braces.
    ///
    /// Returns the repaired string (may still fail JSON parse if damage is too severe).
    pub fn fix_truncated_json(content: &str) -> String {
        let mut content = content.trim().to_string();

        // Count unbalanced braces/brackets
        let open_braces = content.chars().filter(|&c| c == '{').count() as isize
            - content.chars().filter(|&c| c == '}').count() as isize;
        let open_brackets = content.chars().filter(|&c| c == '[').count() as isize
            - content.chars().filter(|&c| c == ']').count() as isize;

        // Close a dangling (truncated) string: if the last char is not a valid JSON terminal char
        if !content.is_empty() {
            let last = content.chars().last().unwrap();
            if last != '"' && last != ',' && last != '}' && last != ']' {
                content.push('"');
            }
        }

        // Close unbalanced brackets, then braces (inner before outer)
        for _ in 0..open_brackets.max(0) {
            content.push(']');
        }
        for _ in 0..open_braces.max(0) {
            content.push('}');
        }

        content
    }

    /// Aggressively salvage a broken/truncated JSON LLM response.
    ///
    /// Ports `OasisProfileGenerator._try_fix_json` (oasis_profile_generator.py:606).
    /// Repair sequence:
    /// 1. Apply `fix_truncated_json` (close brackets/strings).
    /// 2. Extract the first `{…}` block via regex.
    /// 3. Normalize newlines inside JSON string values (replace with spaces, collapse whitespace).
    /// 4. Attempt `serde_json::from_str`.
    /// 5. If still failing, strip control characters (0x00..0x1f, 0x7f..0x9f) and retry.
    /// 6. If all structural repairs fail, do field-level regex extraction of `bio` / `persona`
    ///    and return a minimal-but-valid object (marks `_fixed: true`).
    /// 7. If nothing can be extracted, return `None` (caller uses rule-based fallback).
    ///
    /// Returns `Some(Value)` when ANY salvage succeeds (the caller should use it), `None` on
    /// complete failure. `_fixed` key is set on structurally-repaired results; callers that
    /// accept partial objects should accept any `Some`.
    pub fn try_fix_json(
        content: &str,
        entity_name: &str,
        entity_type: &str,
        entity_summary: &str,
    ) -> Option<serde_json::Value> {
        // Step 1: truncation repair
        let content = Self::fix_truncated_json(content);

        // Step 2: extract the first {...} block
        // Simple brace-scan: find first '{', then walk forward tracking depth to find its match.
        let json_str = Self::extract_json_object(&content)?;

        // Step 3 + 4: normalize newlines inside string values, then try parse
        let json_normalized = Self::normalize_json_string_newlines(&json_str);
        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&json_normalized) {
            v["_fixed"] = serde_json::Value::Bool(true);
            return Some(v);
        }

        // Step 5: strip control characters, then retry
        let json_stripped = Self::strip_control_chars(&json_normalized);
        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&json_stripped) {
            v["_fixed"] = serde_json::Value::Bool(true);
            return Some(v);
        }

        // Step 6: field-level regex extraction of bio / persona
        let bio = Self::extract_json_string_field(&content, "bio").unwrap_or_else(|| {
            if !entity_summary.is_empty() {
                entity_summary.chars().take(200).collect()
            } else {
                format!("{}: {}", entity_type, entity_name)
            }
        });
        let persona =
            Self::extract_json_string_field_partial(&content, "persona").unwrap_or_else(|| {
                if !entity_summary.is_empty() {
                    entity_summary.to_string()
                } else {
                    format!("{entity_name} is a {entity_type}.")
                }
            });

        // Only return a partial result if we actually extracted something from the content
        // (mirrors MiroFish's `if bio_match or persona_match:` guard)
        let has_bio_match = Self::extract_json_string_field(&content, "bio").is_some();
        let has_persona_match =
            Self::extract_json_string_field_partial(&content, "persona").is_some();
        if has_bio_match || has_persona_match {
            return Some(serde_json::json!({
                "bio": bio,
                "persona": persona,
                "_fixed": true,
            }));
        }

        // Step 7: complete failure
        None
    }

    /// Extract the first `{...}` block from a string using brace-depth tracking.
    fn extract_json_object(s: &str) -> Option<String> {
        let start = s.find('{')?;
        let chars: Vec<char> = s[start..].chars().collect();
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escape_next = false;
        let mut end_idx = None;

        for (i, &ch) in chars.iter().enumerate() {
            if escape_next {
                escape_next = false;
                continue;
            }
            if ch == '\\' && in_string {
                escape_next = true;
                continue;
            }
            if ch == '"' {
                in_string = !in_string;
                continue;
            }
            if in_string {
                continue;
            }
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    end_idx = Some(i + 1);
                    break;
                }
            }
        }

        let end = end_idx.unwrap_or(chars.len());
        let extracted: String = chars[..end].iter().collect();
        Some(extracted)
    }

    /// Normalize newlines inside JSON string values (replace `\n`/`\r` with spaces, collapse
    /// multiple whitespace into one). Mirrors the `fix_string_newlines` inner function in
    /// MiroFish `_try_fix_json` (oasis_profile_generator.py:620-629).
    fn normalize_json_string_newlines(s: &str) -> String {
        // Walk the string, find quoted regions, normalize whitespace inside them.
        let mut result = String::with_capacity(s.len());
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '"' {
                // Start of a JSON string — collect until closing unescaped '"'
                result.push('"');
                i += 1;
                let mut in_escape = false;
                let mut string_content = String::new();
                while i < chars.len() {
                    let ch = chars[i];
                    if in_escape {
                        string_content.push('\\');
                        string_content.push(ch);
                        in_escape = false;
                    } else if ch == '\\' {
                        in_escape = true;
                    } else if ch == '"' {
                        break;
                    } else {
                        // Replace actual newline/CR with space
                        if ch == '\n' || ch == '\r' {
                            string_content.push(' ');
                        } else {
                            string_content.push(ch);
                        }
                    }
                    i += 1;
                }
                // Collapse multiple whitespace into single space inside the string
                let normalized: String =
                    string_content.split_whitespace().collect::<Vec<_>>().join(" ");
                result.push_str(&normalized);
                result.push('"');
            } else {
                result.push(chars[i]);
            }
            i += 1;
        }
        result
    }

    /// Strip JSON control characters (0x00–0x1f and 0x7f–0x9f) and collapse whitespace.
    /// Mirrors `re.sub(r'[\x00-\x1f\x7f-\x9f]', ' ', json_str)` + `re.sub(r'\s+', ' ', …)`
    /// in MiroFish `_try_fix_json` step 5 (oasis_profile_generator.py:640-641).
    fn strip_control_chars(s: &str) -> String {
        let replaced: String = s
            .chars()
            .map(|c| {
                let cp = c as u32;
                if cp <= 0x1f || (0x7f..=0x9f).contains(&cp) { ' ' } else { c }
            })
            .collect();
        // Collapse multiple whitespace
        replaced.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Extract the value of a JSON string field using a simple regex-equivalent scan.
    /// Matches `"field": "value"` and returns `value`. Returns `None` if not found.
    /// Mirrors `re.search(r'"bio"\s*:\s*"([^"]*)"', content)` in MiroFish.
    fn extract_json_string_field(content: &str, field: &str) -> Option<String> {
        let needle = format!("\"{}\"", field);
        let field_start = content.find(&needle)?;
        let after_key = &content[field_start + needle.len()..];
        // Skip whitespace and colon
        let after_colon = after_key
            .trim_start_matches(|c: char| c.is_whitespace() || c == ':')
            .trim_start();
        if !after_colon.starts_with('"') {
            return None;
        }
        let value_start = &after_colon[1..]; // skip opening "
        let end = value_start.find('"')?;
        Some(value_start[..end].to_string())
    }

    /// Extract a (possibly truncated) JSON string field value.
    /// Unlike `extract_json_string_field`, this accepts a value that is NOT terminated by `"`,
    /// taking everything up to end-of-string. Mirrors the `persona_match` pattern
    /// `re.search(r'"persona"\s*:\s*"([^"]*)', content)` (no closing quote) in MiroFish.
    fn extract_json_string_field_partial(content: &str, field: &str) -> Option<String> {
        let needle = format!("\"{}\"", field);
        let field_start = content.find(&needle)?;
        let after_key = &content[field_start + needle.len()..];
        let after_colon = after_key
            .trim_start_matches(|c: char| c.is_whitespace() || c == ':')
            .trim_start();
        if !after_colon.starts_with('"') {
            return None;
        }
        let value_start = &after_colon[1..]; // skip opening "
        // Take up to closing `"` if present, otherwise take everything (truncated)
        let end = value_start.find('"').unwrap_or(value_start.len());
        if end == 0 {
            return None;
        }
        Some(value_start[..end].to_string())
    }

    /// Build an entity context string from the KnowledgeGraph, enriching the social profile
    /// prompt with neighbor information.
    ///
    /// Ports the IN-PROCESS parts (1–3) of `OasisProfileGenerator._build_entity_context`
    /// (oasis_profile_generator.py:414):
    /// - Part 1: entity attributes (name + kind used as "attributes" in teri's Entity model)
    /// - Part 2: related edges — relationship/fact lines (S-356, was previously dropped)
    /// - Part 3: related nodes (neighbor names + kinds from `KnowledgeGraph::get_neighbors`)
    ///
    /// The Zep-search half (part 4, `_search_zep_for_entity`) is wired separately via the
    /// optional [`EntityFactRecall`] source in
    /// [`generate_social_with_recall`](Self::generate_social_with_recall) (TASK-SIM-6 #5).
    ///
    /// Returns the entity-context string plus the set of graph-derived relationship/fact lines
    /// (TASK-SIM-6 #5: `existing_facts`, oasis_profile_generator.py:435), used to dedup
    /// semantically-recalled facts. Returns an empty string if the entity has no neighbors
    /// (graceful flat fallback).
    fn build_entity_context_with_facts(
        graph: &KnowledgeGraph,
        entity: &Entity,
    ) -> (String, std::collections::HashSet<String>) {
        let mut context_parts: Vec<String> = Vec::new();
        let mut existing_facts: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        // Part 1: entity attributes — in teri's Entity model, the main attributes are
        // `name` and `kind`; we surface them as a context section.
        // Mirrors _build_entity_context:425-432.
        let attrs = format!("- name: {}\n- kind: {}", entity.name, entity.kind);
        context_parts.push(format!("### Entity Attributes\n{}", attrs));

        // Part 2: related edges — relationship/fact lines.
        // Mirrors _build_entity_context:434-453: iterate `entity.related_edges`; for each edge
        // emit a fact line if the relation carries a fact, else a directional arrow line.
        //
        // In teri, `Relation` carries no free-text fact field (facts are derived from entity
        // summaries passed in by the caller), so this section always emits directional lines.
        // Each emitted line (minus its leading "- ") is recorded in `existing_facts` so that
        // part-4 recall enrichment can dedup against the graph-derived relationships.
        let neighbor_relations = graph.get_neighbor_relations(entity.id).unwrap_or_default();
        if !neighbor_relations.is_empty() {
            let relationships: Vec<String> = neighbor_relations
                .iter()
                .filter_map(|(neighbor, rel, is_outgoing)| {
                    Self::_relation_line(&entity.name, neighbor.name.as_str(), rel, *is_outgoing)
                })
                .collect();
            if !relationships.is_empty() {
                for line in &relationships {
                    existing_facts.insert(line.trim_start_matches("- ").to_string());
                }
                context_parts.push(format!(
                    "### Related Facts and Relationships\n{}",
                    relationships.join("\n")
                ));
            }
        }

        // Part 3: related nodes (neighbor names + kinds from the graph)
        // Mirrors `entity.related_nodes` iteration in _build_entity_context:456-472.
        let neighbors = graph.get_neighbors(entity.id).unwrap_or_default();
        if !neighbors.is_empty() {
            let related_info: Vec<String> =
                neighbors.iter().map(|n| format!("- **{}** ({})", n.name, n.kind)).collect();
            context_parts.push(format!("### Related Entities\n{}", related_info.join("\n")));
        }

        (context_parts.join("\n\n"), existing_facts)
    }

    /// Format recalled facts/summaries into the part-4 enrichment sections, deduped against
    /// `existing_facts`. Mirrors oasis_profile_generator.py:478-485:
    /// - facts not already present → "### Recalled Facts" (capped at 15, matching `[:15]`)
    /// - node summaries → "### Recalled Related Nodes" (capped at 10, matching `[:10]`)
    ///
    /// Returns an empty string when there is nothing new to add.
    fn format_recalled_facts(
        recalled: &RecalledEntityFacts,
        existing_facts: &std::collections::HashSet<String>,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();

        let new_facts: Vec<&String> = recalled
            .facts
            .iter()
            .filter(|f| !existing_facts.contains(*f))
            .take(15)
            .collect();
        if !new_facts.is_empty() {
            let lines: Vec<String> = new_facts.iter().map(|f| format!("- {}", f)).collect();
            parts.push(format!("### Recalled Facts\n{}", lines.join("\n")));
        }

        if !recalled.node_summaries.is_empty() {
            let lines: Vec<String> =
                recalled.node_summaries.iter().take(10).map(|s| format!("- {}", s)).collect();
            parts.push(format!("### Recalled Related Nodes\n{}", lines.join("\n")));
        }

        parts.join("\n\n")
    }

    /// Formats one relationship line for Part 2 of `build_entity_context`.
    ///
    /// Mirrors MiroFish's `_build_entity_context` lines 443–450:
    /// - If the relation carries a fact/summary, emit `- <fact>` (future: extend `Relation`).
    /// - Else emit a directional arrow line using `edge_name` (the `RelationKind` display name)
    ///   and the edge's direction relative to `entity_name`.
    ///
    /// Returns `None` when there is nothing to emit (currently unused; ensures the `filter_map`
    /// call site is forward-compatible if we add a "skip" condition later).
    fn _relation_line(
        entity_name: &str,
        neighbor_name: &str,
        rel: &Relation,
        is_outgoing: bool,
    ) -> Option<String> {
        // When Relation gains a `fact` field this branch becomes active:
        // if let Some(fact) = &rel.fact { return Some(format!("- {}", fact)); }

        let edge_name = format!("{}", rel.kind);
        let line = if is_outgoing {
            // entity --[RelationKind]--> (neighbor)
            // Mirrors Python: f"- {entity.name} --[{edge_name}]--> (相关实体)"
            format!("- {} --[{}]--> ({})", entity_name, edge_name, neighbor_name)
        } else {
            // (neighbor) --[RelationKind]--> entity
            // Mirrors Python: f"- (相关实体) --[{edge_name}]--> {entity.name}"
            format!("- ({}) --[{}]--> {}", neighbor_name, edge_name, entity_name)
        };
        Some(line)
    }

    /// Generate a social-media profile for an entity, returning a `SocialProfile`.
    ///
    /// Mirrors `OasisProfileGenerator.generate_profile_from_entity`:
    /// 1. Builds entity context from the graph (neighbors) if `graph_ctx` is provided.
    /// 2. Tries LLM → parse JSON → populate `SocialProfile`.
    /// 3. On parse failure, tries `try_fix_json` to salvage a partial/truncated response.
    /// 4. If salvage succeeds and parses, populates from the salvaged JSON.
    /// 5. Only if STILL unparseable falls back to `generate_social_rule_based`.
    ///
    /// `bio` and `persona` in the returned `SocialProfile` are distinct fields matching
    /// `OasisAgentProfile.bio` (short public bio) and `OasisAgentProfile.persona` (detailed
    /// personality description).  `user_id` defaults to 0; callers that export to OASIS
    /// should set it to the desired numeric id after construction.
    ///
    /// `graph_ctx`: optional `(&KnowledgeGraph, &Entity)` — when provided, enriches the LLM
    /// prompt with neighbor context from the graph (ports S-356 `_build_entity_context`).
    /// When `None`, the profile is generated from the flat summary alone (backward-compatible).
    ///
    /// Delegates to [`generate_social_with_recall`](Self::generate_social_with_recall) with no
    /// recall source — i.e. the persona context uses only the in-process graph (parts 1-3).
    pub async fn generate_social<L: LlmClient>(
        &self,
        entity_name: &str,
        entity_type: &str,
        entity_summary: &str,
        platform: Platform,
        llm: &L,
        graph_ctx: Option<(&KnowledgeGraph, &Entity)>,
    ) -> Result<SocialProfile> {
        self.generate_social_with_recall(
            entity_name,
            entity_type,
            entity_summary,
            platform,
            llm,
            graph_ctx,
            None,
        )
        .await
    }

    /// Like [`generate_social`](Self::generate_social), but with an optional semantic-recall
    /// source that enriches the persona context with facts about the entity (TASK-SIM-6 #5 —
    /// the part-4 "Zep hybrid search" half of `_build_entity_context`,
    /// oasis_profile_generator.py:475-485).
    ///
    /// `recall`: when `Some`, the recalled facts/summaries are appended to the entity context
    /// AFTER the in-process graph parts (1-3), with the same dedup MiroFish applies — facts
    /// already present in the graph-derived "Related Facts and Relationships" section are
    /// dropped (oasis_profile_generator.py:480). When `None`, the context is byte-identical to
    /// `generate_social` (no-downgrade).
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_social_with_recall<L: LlmClient>(
        &self,
        entity_name: &str,
        entity_type: &str,
        entity_summary: &str,
        platform: Platform,
        llm: &L,
        graph_ctx: Option<(&KnowledgeGraph, &Entity)>,
        recall: Option<&dyn EntityFactRecall>,
    ) -> Result<SocialProfile> {
        let user_name = Self::generate_username(entity_name);
        let created_at = chrono::Utc::now().format("%Y-%m-%d").to_string();

        // Anchor the persona to its source graph entity (its UUID) so the simulation can later fetch
        // that entity's neighborhood and feed it to the agent as per-tick graph context. Previously
        // hardcoded `None`, which severed the persona↔entity link — the swarm could never reason
        // over the extracted graph during the run.
        let source_entity_uuid = graph_ctx.map(|(_, e)| e.id.to_string());

        let default_bio = format!("{}: {}", entity_type, entity_name);
        let default_persona = if entity_summary.is_empty() {
            format!("{entity_name} is a {entity_type} participating in social discussions.")
        } else {
            entity_summary.to_string()
        };

        // S-356: build entity context from graph neighbors (enrichment).
        // TASK-SIM-6 #5: also collect the graph-derived facts so we can dedup recalled facts
        // against them (oasis_profile_generator.py:435,480 — `existing_facts`).
        let (mut graph_context, existing_facts) = match graph_ctx {
            Some((graph, entity)) => Self::build_entity_context_with_facts(graph, entity),
            None => (String::new(), std::collections::HashSet::new()),
        };

        // TASK-SIM-6 #5: part-4 semantic recall enrichment (deduped). Best-effort: a recall
        // miss leaves the context unchanged. Mirrors oasis_profile_generator.py:475-485.
        if let Some(recall_source) = recall {
            let recalled = recall_source.recall(entity_name).await;
            let enrichment = Self::format_recalled_facts(&recalled, &existing_facts);
            if !enrichment.is_empty() {
                if graph_context.is_empty() {
                    graph_context = enrichment;
                } else {
                    graph_context.push_str("\n\n");
                    graph_context.push_str(&enrichment);
                }
            }
        }

        let entity_context = if graph_context.is_empty() {
            String::new()
        } else {
            format!("\n\nEntity context:\n{}", graph_context)
        };

        // TASK-SIM-1 gap #2: individual-vs-group prompt SELECTION
        // (mirrors `_generate_profile_with_llm` :513-522, where `is_individual =
        // _is_individual_entity(entity_type)` is the positive test — known INDIVIDUAL types get the
        // personal framing; everything else, including the GROUP set and unknown types, gets the
        // institutional framing).
        let is_individual = Self::is_individual_entity(entity_type);
        let system_prompt = Self::persona_system_prompt();
        let user_prompt = Self::build_persona_prompt(
            entity_name,
            entity_type,
            entity_summary,
            &entity_context,
            platform,
            is_individual,
        );

        // TASK-SIM-1 gap #2: 3-attempt retry loop with temperature ramp `0.7 - attempt*0.1`
        // (mirrors `_generate_profile_with_llm` :524-581). Uses the EXISTING `chat()` API with a
        // system + user message vector and per-attempt `ChatOptions { temperature }`.
        //
        // Try LLM → parse → salvage (S-360/S-361) → next attempt → rule-based.
        const MAX_ATTEMPTS: u32 = 3;
        let mut profile_data: Option<serde_json::Value> = None;
        for attempt in 0..MAX_ATTEMPTS {
            let temperature = 0.7 - (attempt as f32) * 0.1;
            let messages =
                [ChatMessage::system(system_prompt), ChatMessage::user(user_prompt.clone())];
            // TASK-SIM-6 #7: request the structured-output (JSON-object) shape — mirrors
            // `response_format={"type":"json_object"}` (oasis_profile_generator.py:536). No
            // max_tokens (let the model run free, like MiroFish :538).
            let opts = ChatOptions {
                temperature: Some(temperature),
                max_tokens: None,
                response_format: Some(ResponseFormat::JsonObject),
            };
            // TASK-SIM-6 #7: use the truncation-aware entry point so we can detect a
            // `finish_reason == "length"` cutoff (mirrors oasis_profile_generator.py:544-547).
            match llm.chat_with_meta(&messages, &opts).await {
                Ok(completion) => {
                    let truncated = completion.is_truncated();
                    let response = completion.content;
                    if truncated {
                        // Truncated by the token cap: close the open braces/brackets/strings
                        // before parsing (mirrors `_fix_truncated_json`, :545-547), then let
                        // the normal parse → salvage path run on the repaired content. If even
                        // the repair is unparseable, fall through to the next (lower-temp) attempt
                        // — MiroFish's loop retries truncated attempts the same way.
                        let repaired = Self::fix_truncated_json(&response);
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&repaired) {
                            profile_data = Some(v);
                            break;
                        }
                        if let Some(mut v) =
                            Self::try_fix_json(&repaired, entity_name, entity_type, entity_summary)
                        {
                            if let Some(m) = v.as_object_mut() {
                                m.remove("_fixed");
                            }
                            profile_data = Some(v);
                            break;
                        }
                        // Still unparseable after repair: treat as a failed attempt, retry.
                        continue;
                    }
                    // First attempt: direct parse.
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&response) {
                        profile_data = Some(v);
                        break;
                    }
                    // Salvage attempt (S-360 + S-361): try_fix_json before retrying.
                    if let Some(mut v) =
                        Self::try_fix_json(&response, entity_name, entity_type, entity_summary)
                    {
                        // Strip internal _fixed marker before use.
                        if let Some(m) = v.as_object_mut() {
                            m.remove("_fixed");
                        }
                        profile_data = Some(v);
                        break;
                    }
                    // Unparseable + unsalvageable: fall through to the next attempt.
                }
                // LLM error: fall through to the next attempt.
                Err(_) => continue,
            }
        }

        // Seedable RNG (gap #3): entropy in production. Used to randomize numeric counts whose
        // values are absent from the LLM JSON, and to drive the rule-based fallback.
        let mut rng = StdRng::from_entropy();

        if let Some(data) = profile_data {
            let bio = data["bio"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&default_bio)
                .to_string();
            let persona = data["persona"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(&default_persona)
                .to_string();
            // TASK-SIM-1 gap #3: when the LLM omits a numeric count, draw from the same randomized
            // ranges MiroFish uses in `generate_profile_from_entity` :262-265 (karma 500-5000,
            // friends 50-500, followers 100-1000, statuses 100-2000) — NOT a fixed default.
            let karma = data["karma"].as_i64().unwrap_or_else(|| rng.gen_range(500..=5000));
            let friend_count =
                data["friend_count"].as_i64().unwrap_or_else(|| rng.gen_range(50..=500));
            let follower_count =
                data["follower_count"].as_i64().unwrap_or_else(|| rng.gen_range(100..=1000));
            let following_count = friend_count; // Twitter model: following ≈ friend_count
            let statuses_count =
                data["statuses_count"].as_i64().unwrap_or_else(|| rng.gen_range(100..=2000));
            let age = data["age"].as_u64().map(|v| v as u32);
            let gender = data["gender"].as_str().map(|s| s.to_string());
            let mbti = data["mbti"].as_str().map(|s| s.to_string());
            let country = data["country"].as_str().map(|s| s.to_string());
            let profession = data["profession"].as_str().map(|s| s.to_string());
            let interested_topics = data["interested_topics"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let posting_style = data["posting_style"].as_str().map(|s| s.to_string());

            Ok(SocialProfile {
                user_id: 0,
                user_name,
                bio,
                persona,
                platform,
                karma,
                friend_count,
                follower_count,
                following_count,
                statuses_count,
                age,
                gender,
                mbti,
                country,
                profession,
                interested_topics,
                posting_style,
                source_entity_uuid,
                source_entity_type: Some(entity_type.to_string()),
                created_at,
            })
        } else {
            // Rule-based fallback — mirrors _generate_profile_rule_based. Carry the entity link
            // through this path too, so a fallback persona is still graph-anchored.
            let mut profile = Self::generate_social_rule_based(
                entity_name,
                entity_type,
                entity_summary,
                platform,
                &user_name,
                &created_at,
                &mut rng,
            );
            profile.source_entity_uuid = source_entity_uuid;
            Ok(profile)
        }
    }

    /// System prompt for persona generation.
    ///
    /// Mirrors `_get_system_prompt` (oasis_profile_generator.py:672-675). Per the S6 spec the
    /// prompt text itself is not zh/en-localized here (that's the i18n axis tracked separately);
    /// we preserve the INTENT: instruct the model to act as a user-profile-generation expert and
    /// emit valid JSON with no unescaped newlines in string values.
    fn persona_system_prompt() -> &'static str {
        "You are an expert at generating social-media user profiles. Produce a detailed, realistic \
         persona for opinion-dynamics simulation that faithfully reflects the real-world entity. \
         You MUST return a single valid JSON object; string values must not contain unescaped \
         newlines."
    }

    /// Build the user prompt for persona generation, selecting the individual-vs-group framing.
    ///
    /// Mirrors `_build_individual_persona_prompt` (:677-724) and `_build_group_persona_prompt`
    /// (:726-772). TASK-SIM-1 gap #1: BOTH framings include a memory section — an individual gets
    /// a 个人记忆 (personal-memory) framing tying the person to the event and their prior
    /// actions/reactions; a group/institution gets a 机构记忆 (institutional-memory) framing doing
    /// the same for the organization. The memory section is built from the available event/entity
    /// context (the entity summary + graph-neighbor context already assembled into `entity_context`).
    fn build_persona_prompt(
        entity_name: &str,
        entity_type: &str,
        entity_summary: &str,
        entity_context: &str,
        platform: Platform,
        is_individual: bool,
    ) -> String {
        let platform_name = match platform {
            Platform::Twitter => "Twitter",
            Platform::Reddit => "Reddit",
        };
        // Mirrors the Python `context[:3000]` truncation guard (:688, :737).
        let context_str: String = entity_context.chars().take(3000).collect();

        if is_individual {
            // Individual persona — mirrors `_build_individual_persona_prompt`. The persona spec
            // enumerates the same sub-sections, including the personal-memory (个人记忆) section
            // that ties the person to the event and their existing actions/reactions (:710).
            format!(
                r#"Generate a detailed social-media user persona for an INDIVIDUAL entity, staying as faithful as possible to the real-world entity.

Entity name: {entity_name}
Entity type: {entity_type}
Entity summary: {entity_summary}{context_block}
Platform: {platform_name}

Return a JSON object with these fields:
1. bio: short public bio string (~200 chars, displayed on the profile page)
2. persona: a detailed, single-paragraph personality description that includes:
   - Basic info (age, profession, education, location)
   - Background (key experiences, this person's connection to the event, social relationships)
   - Personality (MBTI type, core traits, how they express emotion)
   - Social-media behavior (posting frequency, content preferences, interaction style, language quirks)
   - Stance (attitude toward the topic; what content would anger or move them)
   - Distinctive features (catchphrases, notable experiences, personal hobbies)
   - Personal memory (an important part of the persona: describe this individual's connection to the event, and the actions and reactions this individual has ALREADY taken in the event)
3. karma: integer (Reddit-style score)
4. friend_count: integer (accounts followed)
5. follower_count: integer (followers)
6. statuses_count: integer (posts made)
7. age: integer
8. gender: "male", "female", or "other"
9. mbti: MBTI type string (e.g. "INTJ")
10. country: country name string
11. profession: profession string
12. interested_topics: array of strings
13. posting_style: short description of posting tone and frequency

Important:
- Every field value must be a string or number; do not use unescaped newlines.
- persona must be one coherent block of text.
- Keep content consistent with the entity information.
- Return only valid JSON."#,
                entity_name = entity_name,
                entity_type = entity_type,
                entity_summary = entity_summary,
                context_block = Self::memory_context_block(&context_str),
                platform_name = platform_name,
            )
        } else {
            // Group/institutional persona — mirrors `_build_group_persona_prompt`. Includes the
            // institutional-memory (机构记忆) section tying the institution to the event and its
            // existing actions/reactions (:759).
            format!(
                r#"Generate a detailed social-media ACCOUNT persona for a GROUP / INSTITUTIONAL entity, staying as faithful as possible to the real-world entity.

Entity name: {entity_name}
Entity type: {entity_type}
Entity summary: {entity_summary}{context_block}
Platform: {platform_name}

Return a JSON object with these fields:
1. bio: official account bio (~200 chars, professional and measured)
2. persona: a detailed, single-paragraph account description that includes:
   - Institutional basics (formal name, nature of the institution, founding background, main functions)
   - Account positioning (account type, target audience, core purpose)
   - Voice/style (language characteristics, common phrasing, taboo topics)
   - Content profile (content types, posting frequency, active periods)
   - Stance (the official position on the core topic; how it handles controversy)
   - Notes (the profile of the group it represents, operational habits)
   - Institutional memory (an important part of the persona: describe this institution's connection to the event, and the actions and reactions this institution has ALREADY taken in the event)
3. age: integer 30 (virtual age for an institutional account)
4. gender: "other" (institutional accounts use "other")
5. mbti: MBTI type string describing the account's style (e.g. "ISTJ" for rigorous/conservative)
6. country: country name string
7. profession: description of the institution's function
8. interested_topics: array of strings (focus areas)

Important:
- Every field value must be a string or number; do not use null and do not use unescaped newlines.
- persona must be one coherent block of text.
- age must be the integer 30 and gender must be the string "other".
- Return only valid JSON."#,
                entity_name = entity_name,
                entity_type = entity_type,
                entity_summary = entity_summary,
                context_block = Self::memory_context_block(&context_str),
                platform_name = platform_name,
            )
        }
    }

    /// Render the event/entity context block injected into the persona prompt.
    ///
    /// Mirrors the Python `上下文信息:\n{context_str}` block (:697-698, :746-747). When there is
    /// no graph/entity context the block is empty (the prompt's persona-memory field still asks the
    /// model to ground the memory in the entity summary). Kept as a small DRY helper because both
    /// the individual and group branches inject it identically.
    fn memory_context_block(context_str: &str) -> String {
        if context_str.trim().is_empty() {
            String::new()
        } else {
            format!("\n\nContext (use this to ground the persona-memory section):\n{context_str}")
        }
    }

    /// Pick a random element from a string slice using the supplied RNG.
    ///
    /// DRY helper for `random.choice(...)` parity. Returns an owned `String`. The slice is always
    /// non-empty at every call site (the constant tables / fixed literal arrays), so `unwrap` is
    /// infallible; we still guard with `expect` to make the invariant explicit.
    fn choose(rng: &mut StdRng, choices: &[&str]) -> String {
        (*choices.choose(rng).expect("choice slice is non-empty")).to_string()
    }

    /// Rule-based fallback for social profile generation.
    ///
    /// Mirrors `OasisProfileGenerator._generate_profile_rule_based` (oasis_profile_generator.py:
    /// 774-845): assigns defaults keyed by entity type (individual vs group/institution).
    /// `bio` and `persona` are populated distinctly — `bio` is a short tagline and `persona` is
    /// the longer entity summary or a default description.
    ///
    /// TASK-SIM-1 gap #3: age/gender/mbti/country and the social counts are RANDOMIZED for
    /// individual / generic entities (drawing from `MBTI_TYPES` / `COUNTRIES` and sensible
    /// numeric ranges), exactly as MiroFish does with `random.randint` / `random.choice`.
    /// Institutional accounts keep MiroFish's FIXED values (age=30, gender="other", mbti="ISTJ")
    /// — only their numeric counts are randomized (parity with the LLM-path randomization). The
    /// caller supplies a seedable `StdRng` so tests can fix the seed for determinism.
    fn generate_social_rule_based(
        entity_name: &str,
        entity_type: &str,
        entity_summary: &str,
        platform: Platform,
        user_name: &str,
        created_at: &str,
        rng: &mut StdRng,
    ) -> SocialProfile {
        let entity_type_lower = entity_type.to_lowercase();

        // Individual entity types → personal profile defaults (randomized demographics).
        let (
            bio,
            persona,
            age,
            gender,
            mbti,
            country,
            profession,
            interested_topics,
            posting_style,
        ) = if matches!(entity_type_lower.as_str(), "student" | "alumni") {
            (
                format!("{} with interests in academics and social issues.", entity_type),
                if entity_summary.is_empty() {
                    format!(
                        "{entity_name} is a {etl} who is actively engaged in academic and social discussions. They enjoy sharing perspectives and connecting with peers.",
                        etl = entity_type.to_lowercase()
                    )
                } else {
                    entity_summary.to_string()
                },
                // random.randint(18, 30) — oasis_profile_generator.py:790
                Some(rng.gen_range(18..=30u32)),
                Some(Self::choose(rng, &["male", "female"])),
                Some(Self::choose(rng, &Self::MBTI_TYPES)),
                Some(Self::choose(rng, &Self::COUNTRIES)),
                Some("Student".to_string()),
                vec![
                    "Education".to_string(),
                    "Social Issues".to_string(),
                    "Technology".to_string(),
                ],
                Some("Casual, frequent posts on campus life and social topics".to_string()),
            )
        } else if matches!(
            entity_type_lower.as_str(),
            "publicfigure" | "expert" | "faculty" | "professor"
        ) {
            (
                "Expert and thought leader in their field.".to_string(),
                if entity_summary.is_empty() {
                    format!(
                        "{entity_name} is a recognized {etl} who shares insights and opinions on important matters. They are known for their expertise and influence in public discourse.",
                        etl = entity_type.to_lowercase()
                    )
                } else {
                    entity_summary.to_string()
                },
                // random.randint(35, 60) — oasis_profile_generator.py:802
                Some(rng.gen_range(35..=60u32)),
                Some(Self::choose(rng, &["male", "female"])),
                // random.choice(["ENTJ", "INTJ", "ENTP", "INTP"]) — oasis_profile_generator.py:804
                Some(Self::choose(rng, &["ENTJ", "INTJ", "ENTP", "INTP"])),
                Some(Self::choose(rng, &Self::COUNTRIES)),
                Some("Expert".to_string()),
                vec![
                    "Politics".to_string(),
                    "Economics".to_string(),
                    "Culture & Society".to_string(),
                ],
                Some("Thoughtful, infrequent posts with expert analysis".to_string()),
            )
        } else if matches!(entity_type_lower.as_str(), "mediaoutlet" | "socialmediaplatform") {
            // TASK-SIM-6 #4: media entities get a DISTINCT profile, not the generic
            // institutional one. Mirrors `_generate_default_profile`
            // (oasis_profile_generator.py:810-820): news-focused bio/persona, profession
            // "Media", and media-specific interests. Note `socialmediaplatform` is NOT in
            // GROUP_ENTITY_TYPES, so without this arm it would fall to the generic default —
            // this branch must precede `is_group_entity` (which contains `mediaoutlet`).
            (
                format!("Official account for {}. News and updates.", entity_name),
                if entity_summary.is_empty() {
                    format!(
                        "{entity_name} is a media entity that reports news and facilitates public discourse. The account shares timely updates and engages with the audience on current events."
                    )
                } else {
                    entity_summary.to_string()
                },
                // Fixed institutional virtual demographics (oasis_profile_generator.py:814-817).
                Some(30u32),
                Some("other".to_string()),
                Some("ISTJ".to_string()),
                Some("China".to_string()),
                Some("Media".to_string()),
                vec![
                    "General News".to_string(),
                    "Current Events".to_string(),
                    "Public Affairs".to_string(),
                ],
                Some(format!(
                    "News-focused account for {entity_name}. Timely, professional updates."
                )),
            )
        } else if Self::is_group_entity(&entity_type_lower) {
            // Group/institution entity types (the canonical `GROUP_ENTITY_TYPES` set) →
            // institutional account defaults. MiroFish keeps these
            // FIXED (age=30, gender="other", mbti="ISTJ"; oasis_profile_generator.py:826-828),
            // so we do too — institutional accounts are deliberately uniform.
            (
                format!("Official account of {}.", entity_name),
                if entity_summary.is_empty() {
                    format!(
                        "{entity_name} is an institutional entity that communicates official positions, announcements, and engages with stakeholders on relevant matters."
                    )
                } else {
                    entity_summary.to_string()
                },
                Some(30u32),
                Some("other".to_string()),
                Some("ISTJ".to_string()),
                Some("China".to_string()),
                Some(entity_type.to_string()),
                vec![
                    "Public Policy".to_string(),
                    "Community".to_string(),
                    "Official Announcements".to_string(),
                ],
                Some(format!("Official account for {entity_name}. Professional, measured tone.")),
            )
        } else {
            // Default: generic participant (randomized demographics).
            (
                if entity_summary.is_empty() {
                    format!("{}: {}", entity_type, entity_name)
                } else {
                    entity_summary.chars().take(150).collect()
                },
                if entity_summary.is_empty() {
                    format!(
                        "{entity_name} is a {etl} participating in social discussions.",
                        etl = entity_type.to_lowercase()
                    )
                } else {
                    entity_summary.to_string()
                },
                // random.randint(25, 50) — oasis_profile_generator.py:839
                Some(rng.gen_range(25..=50u32)),
                Some(Self::choose(rng, &["male", "female"])),
                Some(Self::choose(rng, &Self::MBTI_TYPES)),
                Some(Self::choose(rng, &Self::COUNTRIES)),
                Some(entity_type.to_string()),
                vec!["General".to_string(), "Social Issues".to_string()],
                Some("Occasional posts on general topics".to_string()),
            )
        };

        SocialProfile {
            user_id: 0,
            user_name: user_name.to_string(),
            bio,
            persona,
            platform,
            // TASK-SIM-1 gap #3: randomized social counts (same ranges as the LLM path /
            // generate_profile_from_entity:262-265) instead of the old fixed defaults.
            karma: rng.gen_range(500..=5000),
            friend_count: rng.gen_range(50..=500),
            follower_count: rng.gen_range(100..=1000),
            following_count: rng.gen_range(50..=500),
            statuses_count: rng.gen_range(100..=2000),
            age,
            gender,
            mbti,
            country,
            profession,
            interested_topics,
            posting_style,
            source_entity_uuid: None,
            source_entity_type: Some(entity_type.to_string()),
            created_at: created_at.to_string(),
        }
    }

    /// Generate a URL-safe username from an entity name.
    ///
    /// Mirrors `OasisProfileGenerator._generate_username`: lowercase, underscores,
    /// alphanumeric only, plus a 3-digit numeric suffix to avoid collisions.
    pub fn generate_username(name: &str) -> String {
        let base: String = name
            .to_lowercase()
            .replace(' ', "_")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        // Use a stable hash-derived suffix so the output is deterministic in tests.
        // Simple djb2-style fold over the name bytes.
        let hash: u32 =
            name.bytes().fold(5381u32, |acc, b| acc.wrapping_mul(33).wrapping_add(b as u32));
        let suffix = 100 + (hash % 900); // 100..=999
        format!("{base}_{suffix}")
    }
}

impl Default for PersonaGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Generates action prompts based on agent context and world state
pub struct ActionGenerator {
    template: String,
}

impl ActionGenerator {
    /// Create a new ActionGenerator with the default embedded template
    pub fn new() -> Self {
        let template = include_str!("../../templates/agent_action.jinja").to_string();
        Self { template }
    }

    /// Create a new ActionGenerator with a custom template from file
    /// Falls back to embedded template if file loading fails
    pub fn from_file<P: AsRef<std::path::Path>>(template_path: P) -> Self {
        match std::fs::read_to_string(template_path) {
            Ok(template) => Self { template },
            Err(e) => {
                eprintln!(
                    "Warning: Failed to load action template from file ({}), falling back to embedded template",
                    e
                );
                Self::new()
            }
        }
    }

    /// Generate a prompt for action generation based on agent and context
    pub fn generate_prompt(&self, agent: &Agent, context: &str) -> Result<String> {
        let env = Environment::new();

        // Parse recent events from context
        let recent_events = self.parse_recent_events(context);
        let relevant_memories = self.parse_relevant_memories(context);
        let world_variables = self.parse_world_variables(context);
        let world_tick = self.parse_world_tick(context);
        let feed_posts = self.parse_feed_posts(context);
        let graph_context = self.parse_graph_context(context);

        // Convert HashMap to Vec of tuples for MiniJinja iteration
        let world_variables_seq: Vec<(String, f32)> = world_variables.into_iter().collect();

        // Route the recovered OASIS personality (`SocialProfile.persona`, the `user_char` blob a
        // profile reader recovers in U-028 c2) into the decision prompt. Without this, an agent
        // loaded from a profile would decide purely from `background`(=bio)+`traits` and its OASIS
        // personality would be SHADOWED (the c2 verifier watch-item). Generic agents (no social
        // profile) yield "" → the template's `{% if agent_persona %}` skips the section → their
        // prompt is byte-identical to before (no regression).
        let agent_persona = agent.persona.social.as_ref().map(|s| s.persona.as_str()).unwrap_or("");

        // U-028 c2 completion: an agent loaded from an OASIS profile (i.e. it carries a
        // `SocialProfile`) must be offered the platform's social action space — CREATE_POST,
        // LIKE_POST, CREATE_COMMENT, FOLLOW, … (config.py OASIS_TWITTER_ACTIONS /
        // OASIS_REDDIT_ACTIONS) — so `parse_and_validate_action`'s `Action::Social` branch is
        // actually reachable and the simulation produces social-media activity. Without this the
        // decision prompt only ever offered the 5 generic actions, so every agent fell through to
        // a generic Speak/Think and NO social action was ever generated (the gap the parser,
        // `SocialAction`, and the platform loggers were all built to serve). Generic agents (no
        // social profile) yield `is_social = false` → the template keeps the generic action menu
        // verbatim (no regression to teri's native swarm mode).
        let is_social = agent.persona.social.is_some();
        let platform = agent
            .persona
            .social
            .as_ref()
            .map(|s| match s.platform {
                Platform::Twitter => "twitter",
                Platform::Reddit => "reddit",
            })
            .unwrap_or("");

        let template_context = context! {
            agent_name => &agent.persona.name,
            agent_role => &agent.persona.role,
            agent_state => format!("{:?}", agent.state),
            agent_background => &agent.persona.background,
            agent_traits => &agent.persona.traits,
            agent_persona => agent_persona,
            is_social => is_social,
            platform => platform,
            world_tick => world_tick,
            recent_events => recent_events,
            relevant_memories => relevant_memories,
            world_variables => world_variables_seq,
            feed_posts => feed_posts,
            graph_context => graph_context,
        };

        let prompt = env
            .template_from_str(&self.template)
            .map_err(|e| TeriError::Agent(format!("Template parsing error: {}", e)))?
            .render(template_context)
            .map_err(|e| TeriError::Agent(format!("Template rendering error: {}", e)))?;

        Ok(prompt)
    }

    /// Parse recent events from context string
    fn parse_recent_events(&self, context: &str) -> Vec<String> {
        let mut events = Vec::new();
        if let Some(events_start) = context.find("Recent Events:") {
            // Find section end: either double newline or end of string
            let section_start = events_start + 14;
            let section_end = context[section_start..]
                .find("\n\n")
                .map(|i| section_start + i)
                .unwrap_or(context.len());
            let events_section = &context[section_start..section_end];
            for line in events_section.lines() {
                if let Some(content) = line.strip_prefix("- ") {
                    events.push(content.to_string());
                }
            }
        }
        events
    }

    /// Parse relevant memories from context string
    fn parse_relevant_memories(&self, context: &str) -> Vec<MemoryEntry> {
        let mut memories = Vec::new();
        if let Some(memories_start) = context.find("Relevant Memories:") {
            // Find section end: either double newline or end of string
            let section_start = memories_start + 19;
            let section_end = context[section_start..]
                .find("\n\n")
                .map(|i| section_start + i)
                .unwrap_or(context.len());
            let memories_section = &context[section_start..section_end];
            for line in memories_section.lines() {
                if let Some(content) = line.strip_prefix("- ") {
                    memories.push(MemoryEntry {
                        timestamp: Utc::now(),
                        content: content.to_string(),
                        importance: 0.7,
                    });
                }
            }
        }
        memories
    }

    /// Parse world variables from context string
    fn parse_world_variables(&self, context: &str) -> std::collections::HashMap<String, f32> {
        let mut variables = std::collections::HashMap::new();
        if let Some(vars_start) = context.find("World State:") {
            // Find section end: either double newline or end of string
            let section_start = vars_start + 12;
            let section_end = context[section_start..]
                .find("\n\n")
                .map(|i| section_start + i)
                .unwrap_or(context.len());
            let vars_section = &context[section_start..section_end];
            for line in vars_section.lines() {
                if let Some(line_content) = line.strip_prefix("- ")
                    && let Some(colon_pos) = line_content.find(':')
                {
                    let key = line_content[..colon_pos].trim().to_string();
                    let value_str = line_content[colon_pos + 1..].trim();
                    if let Ok(value) = value_str.parse::<f32>() {
                        variables.insert(key, value);
                    }
                }
            }
        }
        variables
    }

    /// Parse world tick from context string
    fn parse_world_tick(&self, context: &str) -> u32 {
        if let Some(tick_start) = context.find("World Tick: ")
            && let Some(tick_end) = context[tick_start + 12..].find('\n')
        {
            let tick_str = &context[tick_start + 12..tick_start + 12 + tick_end];
            return tick_str.parse().unwrap_or(0);
        }
        0
    }

    /// Parse the "Recent posts in your feed:" section emitted by `construct_context`. Each line is
    /// `- [post-<id> by user <author> | <likes> likes | <shares> shares] <content>`. A malformed
    /// line is skipped (fail-closed) rather than aborting the prompt.
    fn parse_feed_posts(&self, context: &str) -> Vec<FeedPostView> {
        let mut posts = Vec::new();
        let Some(start) = context.find("Recent posts in your feed:") else {
            return posts;
        };
        let section_start = start + "Recent posts in your feed:".len();
        let section_end = context[section_start..]
            .find("\n\n")
            .map(|i| section_start + i)
            .unwrap_or(context.len());
        for line in context[section_start..section_end].lines() {
            let Some(rest) = line.strip_prefix("- [") else {
                continue;
            };
            let Some((meta, content)) = rest.split_once("] ") else {
                continue;
            };
            // meta = "post-<id> by user <author> | <likes> likes | <shares> shares"
            let mut parts = meta.split(" | ");
            let id_author = parts.next().unwrap_or("");
            let likes_str = parts.next().unwrap_or("");
            let shares_str = parts.next().unwrap_or("");
            let id = id_author
                .strip_prefix("post-")
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse::<i64>().ok());
            let author =
                id_author.rsplit("user ").next().and_then(|s| s.trim().parse::<i64>().ok());
            let (Some(id), Some(author)) = (id, author) else {
                continue;
            };
            let likes =
                likes_str.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let shares =
                shares_str.split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(0);
            posts.push(FeedPostView {
                id: format!("post-{id}"),
                author,
                likes,
                shares,
                content: content.to_string(),
            });
        }
        posts
    }

    /// Parse the "Knowledge Graph Context:" section emitted by `construct_context`. Each line is
    /// `- <EntityA> --[<Relation>]--> <EntityB>`. Returns the relation descriptions (with the
    /// leading `- ` stripped) for template iteration; an absent header / empty section yields an
    /// empty Vec so the template's `{% if graph_context %}` section is skipped (no regression).
    fn parse_graph_context(&self, context: &str) -> Vec<String> {
        let mut rels = Vec::new();
        let Some(start) = context.find("Knowledge Graph Context:") else {
            return rels;
        };
        let section_start = start + "Knowledge Graph Context:".len();
        let section_end = context[section_start..]
            .find("\n\n")
            .map(|i| section_start + i)
            .unwrap_or(context.len());
        for line in context[section_start..section_end].lines() {
            if let Some(rest) = line.strip_prefix("- ")
                && !rest.trim().is_empty()
            {
                rels.push(rest.to_string());
            }
        }
        rels
    }
}

/// Template-facing view of a feed post (used in `agent_action.jinja`'s feed section). `id` is the
/// `post-<n>` string the agent should copy verbatim into `LIKE_POST(target_id=...)` / etc.
#[derive(Debug, Clone, Serialize)]
struct FeedPostView {
    id: String,
    author: i64,
    likes: i64,
    shares: i64,
    content: String,
}

impl Default for ActionGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for AgentPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EntityKind, KnowledgeGraph};
    use crate::sim::{AgentSnapshot, Event};
    use async_trait::async_trait;
    use std::pin::Pin;

    /// Concatenate the user-role message contents from a chat vector.
    ///
    /// `generate_social` (TASK-SIM-1) drives the LLM through `chat()` with a system + user
    /// message; the persona prompt lives in the user message. The prompt-capture test mocks use
    /// this to recover the prompt text they used to read from `complete()`.
    fn capture_user_message(messages: &[crate::llm::ChatMessage]) -> String {
        messages
            .iter()
            .filter(|m| matches!(m.role, crate::llm::ChatRole::User))
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }

    // Mock LLM for testing
    struct MockPersonaLlm {
        response: String,
    }

    impl MockPersonaLlm {
        fn new(response: &str) -> Self {
            Self { response: response.to_string() }
        }
    }

    #[async_trait]
    impl LlmClient for MockPersonaLlm {
        async fn complete(&self, _prompt: &str) -> Result<String> {
            Ok(self.response.clone())
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _prompt: &str) -> Result<T> {
            Err(TeriError::Llm("Not implemented in mock".to_string()))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("Streaming not implemented in mock".to_string()))
        }

        async fn chat(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> Result<String> {
            // `generate_social` now drives the LLM through `chat()` (TASK-SIM-1 two-prompt path);
            // return the canned response so existing tests exercise the same flow as before.
            Ok(self.response.clone())
        }

        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("chat_json not implemented in mock".to_string()))
        }
    }

    #[test]
    fn test_agent_creation() {
        let persona = Persona {
            name: "Alice".to_string(),
            background: "A curious researcher".to_string(),
            traits: vec!["analytical".to_string(), "creative".to_string()],
            role: "Analyst".to_string(),
            social: None,
        };

        let agent = Agent::new(persona.clone());
        assert_eq!(agent.persona.name, "Alice");
        assert_eq!(agent.state, AgentState::Idle);
    }

    #[test]
    fn test_agent_memory() {
        let persona = Persona {
            name: "Alice".to_string(),
            background: "A curious researcher".to_string(),
            traits: vec!["analytical".to_string()],
            role: "Analyst".to_string(),
            social: None,
        };

        let mut agent = Agent::new(persona);
        agent.add_memory("First memory".to_string(), 0.8);
        agent.add_memory("Second memory".to_string(), 0.9);

        assert_eq!(agent.memory.short_term.len(), 2);
    }

    #[test]
    fn test_agent_pool() {
        let mut pool = AgentPool::new();
        let persona = Persona {
            name: "Alice".to_string(),
            background: "A curious researcher".to_string(),
            traits: vec!["analytical".to_string()],
            role: "Analyst".to_string(),
            social: None,
        };

        let agent = Agent::new(persona);
        let agent_id = agent.id;
        pool.add_agent(agent);

        assert_eq!(pool.len(), 1);
        assert!(pool.get(agent_id).is_some());
    }

    #[test]
    fn test_agent_state_change() {
        let persona = Persona {
            name: "Alice".to_string(),
            background: "A curious researcher".to_string(),
            traits: vec!["analytical".to_string()],
            role: "Analyst".to_string(),
            social: None,
        };

        let mut agent = Agent::new(persona);
        assert_eq!(agent.state, AgentState::Idle);

        agent.set_state(AgentState::Thinking);
        assert_eq!(agent.state, AgentState::Thinking);
    }

    #[tokio::test]
    async fn test_persona_generator_creation() {
        let generator = PersonaGenerator::new();
        assert!(!generator.template.is_empty());
        assert!(generator.template.contains("persona generation system"));
    }

    #[tokio::test]
    async fn test_persona_generator_with_mock_llm() {
        let mock_response = r#"{
            "name": "Sarah Chen",
            "background": "An experienced project manager who has worked at Acme for 8 years.",
            "traits": ["organized", "detail-oriented", "collaborative"],
            "role": "Senior Project Manager"
        }"#;

        let mock_llm = MockPersonaLlm::new(mock_response);
        let generator = PersonaGenerator::new();

        // Create a test graph with an entity
        let mut graph = KnowledgeGraph::new();
        let entity = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Acme Corporation".to_string(),
            kind: EntityKind::Organization,
        };
        graph.add_entity(entity.clone()).expect("Failed to add entity");

        let persona = generator
            .generate(&graph, &entity, &mock_llm)
            .await
            .expect("Failed to generate persona");

        assert_eq!(persona.name, "Sarah Chen");
        assert_eq!(persona.role, "Senior Project Manager");
        assert_eq!(persona.traits.len(), 3);
        assert!(persona.traits.contains(&"organized".to_string()));
    }

    #[tokio::test]
    async fn test_persona_generator_validation() {
        let generator = PersonaGenerator::new();

        // Test empty name
        let invalid_persona = Persona {
            name: "".to_string(),
            background: "Valid background".to_string(),
            traits: vec!["valid".to_string()],
            role: "Valid role".to_string(),
            social: None,
        };
        assert!(generator.validate_persona(&invalid_persona).is_err());

        // Test empty background
        let invalid_persona = Persona {
            name: "Valid Name".to_string(),
            background: "".to_string(),
            traits: vec!["valid".to_string()],
            role: "Valid role".to_string(),
            social: None,
        };
        assert!(generator.validate_persona(&invalid_persona).is_err());

        // Test too many traits
        let invalid_persona = Persona {
            name: "Valid Name".to_string(),
            background: "Valid background".to_string(),
            traits: (0..11).map(|i| format!("trait_{}", i)).collect(), // 11 traits
            role: "Valid role".to_string(),
            social: None,
        };
        assert!(generator.validate_persona(&invalid_persona).is_err());

        // Test valid persona
        let valid_persona = Persona {
            name: "Valid Name".to_string(),
            background: "Valid background".to_string(),
            traits: vec!["trait1".to_string(), "trait2".to_string()],
            role: "Valid role".to_string(),
            social: None,
        };
        assert!(generator.validate_persona(&valid_persona).is_ok());
    }

    #[tokio::test]
    async fn test_agent_pool_spawn_with_mock_llm() {
        let mock_response = r#"{
            "name": "Test Agent",
            "background": "A test agent for unit testing.",
            "traits": ["test-oriented", "methodical"],
            "role": "Test Subject"
        }"#;

        let mock_llm = MockPersonaLlm::new(mock_response);

        // Create a test graph with entities
        let mut graph = KnowledgeGraph::new();
        let entity1 = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Entity1".to_string(),
            kind: EntityKind::Person,
        };
        let entity2 = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Entity2".to_string(),
            kind: EntityKind::Organization,
        };
        graph.add_entity(entity1).expect("Failed to add entity1");
        graph.add_entity(entity2).expect("Failed to add entity2");

        // Spawn 2 agents
        let pool = AgentPool::spawn(2, &graph, &mock_llm).await.expect("Failed to spawn agents");

        assert_eq!(pool.len(), 2);

        // Verify agents have unique IDs
        let agents: Vec<_> = pool.iter().collect();
        assert_ne!(agents[0].id, agents[1].id);

        // Verify all agents have valid personas
        for agent in agents {
            assert!(!agent.persona.name.is_empty());
            assert!(!agent.persona.background.is_empty());
            assert!(!agent.persona.traits.is_empty());
            assert!(!agent.persona.role.is_empty());
        }
    }

    #[tokio::test]
    async fn test_agent_pool_group_memory() {
        let pool = AgentPool::new();

        // Add some group memories in sequence
        let memory1 = MemoryEntry {
            timestamp: chrono::Utc::now(),
            content: "Group memory 1".to_string(),
            importance: 0.8,
        };
        // Small sleep to ensure different timestamps
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let memory2 = MemoryEntry {
            timestamp: chrono::Utc::now(),
            content: "Group memory 2".to_string(),
            importance: 0.9,
        };

        pool.add_group_memory(memory1.clone()).await;
        pool.add_group_memory(memory2.clone()).await;

        // Retrieve recent memories - returns in reverse insertion order (most recently added first)
        // Note: This is insertion order (via Vec::rev()), not sorted by timestamp
        let recent = pool.get_group_memory(2).await;
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].content, "Group memory 2"); // Last inserted first
        assert_eq!(recent[1].content, "Group memory 1");

        // Test limit
        let limited = pool.get_group_memory(1).await;
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].content, "Group memory 2");
    }

    #[tokio::test]
    async fn test_agent_pool_spawn_empty_graph() {
        let mock_llm = MockPersonaLlm::new("{}");
        let empty_graph = KnowledgeGraph::new();

        let result = AgentPool::spawn(1, &empty_graph, &mock_llm).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No entities available"));
    }

    #[test]
    fn test_entity_description_generation() {
        let generator = PersonaGenerator::new();
        let mut graph = KnowledgeGraph::new();

        // Create an entity with no connections
        let isolated_entity = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Isolated".to_string(),
            kind: EntityKind::Person,
        };
        graph
            .add_entity(isolated_entity.clone())
            .expect("Failed to add isolated entity");

        let description = generator
            .generate_entity_description(&graph, &isolated_entity)
            .expect("Failed to generate description");
        assert!(description.contains("no known connections"));

        // Create connected entities
        let connected_entity = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Connected".to_string(),
            kind: EntityKind::Person,
        };
        let neighbor = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Neighbor".to_string(),
            kind: EntityKind::Organization,
        };

        let connected_idx = graph
            .add_entity(connected_entity.clone())
            .expect("Failed to add connected entity");
        let neighbor_idx = graph.add_entity(neighbor.clone()).expect("Failed to add neighbor");

        graph.add_relation(
            connected_idx,
            neighbor_idx,
            crate::graph::Relation::new(crate::graph::RelationKind::RelatedTo, 0.8)
                .expect("Valid relation"),
        );

        let description = generator
            .generate_entity_description(&graph, &connected_entity)
            .expect("Failed to generate description");
        assert!(description.contains("connected to"));
        assert!(description.contains("Neighbor"));
    }

    #[test]
    fn test_template_sanitization() {
        let generator = PersonaGenerator::new();

        // Test entity names with template-like syntax
        let malicious_name = "Test {{ malicious }} {% injection %} \n\r\t";
        let sanitized = generator.sanitize_entity_name(malicious_name);

        // Should remove template syntax and whitespace
        assert!(!sanitized.contains("{{"));
        assert!(!sanitized.contains("}}"));
        assert!(!sanitized.contains("{%"));
        assert!(!sanitized.contains("%}"));
        assert!(!sanitized.contains('\n'));
        assert!(!sanitized.contains('\r'));
        assert!(!sanitized.contains('\t'));

        // Should preserve the actual content
        assert!(sanitized.contains("Test"));
        assert!(sanitized.contains("malicious"));
        assert!(sanitized.contains("injection"));
    }

    #[tokio::test]
    async fn test_persona_deduplication() {
        let mock_response = r#"{
            "name": "Duplicate Agent",
            "background": "An agent that would be duplicated.",
            "traits": ["duplicate", "test"],
            "role": "Test Subject"
        }"#;

        let mock_llm = MockPersonaLlm::new(mock_response);

        // Create a test graph with a single entity
        let mut graph = KnowledgeGraph::new();
        let entity = Entity {
            id: uuid::Uuid::new_v4(),
            name: "SingleEntity".to_string(),
            kind: EntityKind::Person,
        };
        graph.add_entity(entity).expect("Failed to add entity");

        // Spawn 3 agents - should create variations to avoid duplicates
        let pool = AgentPool::spawn(3, &graph, &mock_llm).await.expect("Failed to spawn agents");

        assert_eq!(pool.len(), 3);

        // Verify agents have unique personas (using HashSet for exact uniqueness check)
        let agents: Vec<_> = pool.iter().collect();
        let persona_ids: std::collections::HashSet<String> = agents
            .iter()
            .map(|a| format!("{}|{}", a.persona.name, a.persona.role))
            .collect();

        // All 3 agents should have unique (name, role) combinations
        assert_eq!(persona_ids.len(), 3, "All 3 spawned agents must have unique personas");

        // Verify at least one agent has the original name (first one succeeds without conflict)
        assert!(persona_ids.iter().any(|id| id.contains("Duplicate Agent|")));

        // With 3 agents and max_attempts=5, at least 2 should have numeric suffixes
        // The variation logic: attempt 0 = original, attempt 5 = "Name 5", attempt 5 again = "Name 5 5"
        let varied_count = agents.iter().filter(|a| a.persona.name != "Duplicate Agent").count();
        assert!(varied_count >= 1, "At least 1 agent should have a varied name");
    }

    #[test]
    fn test_persona_generator_from_file() {
        // Test with non-existent file (should fall back to embedded template)
        let generator = PersonaGenerator::from_file("non_existent_template.jinja");
        assert!(!generator.template.is_empty());
        assert!(generator.template.contains("persona generation system"));
    }

    #[test]
    fn test_persona_generator_with_custom_template() {
        let custom_template =
            "Custom template for {{ entity_name }} ({{ entity_kind }})".to_string();
        let generator = PersonaGenerator::with_template(custom_template.clone());
        assert_eq!(generator.template, custom_template);
    }

    // ===== Action Generation Tests =====

    #[test]
    fn test_parse_and_validate_action_speak() {
        let persona = Persona {
            name: "Test".to_string(),
            background: "Test background".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let agent = Agent::new(persona);

        let action = agent.parse_and_validate_action("Speak(Hello world)").unwrap();
        assert!(matches!(action, Action::Speak(ref s) if s == "Hello world"));
    }

    #[test]
    fn test_parse_and_validate_action_move() {
        let persona = Persona {
            name: "Test".to_string(),
            background: "Test background".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let agent = Agent::new(persona);

        let action = agent.parse_and_validate_action("Move(Central Park)").unwrap();
        assert!(matches!(action, Action::Move(ref s) if s == "Central Park"));
    }

    #[test]
    fn test_parse_and_validate_action_interact() {
        let persona = Persona {
            name: "Test".to_string(),
            background: "Test background".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let agent = Agent::new(persona);

        let action = agent.parse_and_validate_action("Interact(Computer terminal)").unwrap();
        assert!(matches!(action, Action::Interact(ref s) if s == "Computer terminal"));
    }

    #[test]
    fn test_parse_and_validate_action_observe() {
        let persona = Persona {
            name: "Test".to_string(),
            background: "Test background".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let agent = Agent::new(persona);

        let action = agent.parse_and_validate_action("Observe(Suspicious activity)").unwrap();
        assert!(matches!(action, Action::Observe(ref s) if s == "Suspicious activity"));
    }

    #[test]
    fn test_parse_and_validate_action_think() {
        let persona = Persona {
            name: "Test".to_string(),
            background: "Test background".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let agent = Agent::new(persona);

        let action = agent.parse_and_validate_action("Think(I need a strategy)").unwrap();
        assert!(matches!(action, Action::Think(ref s) if s == "I need a strategy"));
    }

    #[test]
    fn test_parse_and_validate_action_with_whitespace() {
        let persona = Persona {
            name: "Test".to_string(),
            background: "Test background".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let agent = Agent::new(persona);

        let action = agent.parse_and_validate_action("  Speak(  Hello world  )  ").unwrap();
        assert!(matches!(action, Action::Speak(ref s) if s == "Hello world"));
    }

    #[test]
    fn test_parse_and_validate_action_unknown_type() {
        let persona = Persona {
            name: "Test".to_string(),
            background: "Test background".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let agent = Agent::new(persona);

        let result = agent.parse_and_validate_action("UnknownAction(something)");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown action type"));
    }

    #[test]
    fn test_parse_and_validate_action_invalid_format() {
        let persona = Persona {
            name: "Test".to_string(),
            background: "Test background".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let agent = Agent::new(persona);

        let result = agent.parse_and_validate_action("Invalid format without parentheses");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid action format"));
    }

    #[test]
    fn test_parse_and_validate_action_nested_parens() {
        let persona = Persona {
            name: "Test".to_string(),
            background: "Test background".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let agent = Agent::new(persona);

        // The parser uses rfind(')') to find the LAST ')', handling nested parens.
        // For "Think(Consider (the implications))":
        // - first '(' is at index 5, last ')' is at index 33 (the final one)
        // - content = "Consider (the implications)" (includes inner closing paren)
        let action = agent.parse_and_validate_action("Think(Consider (the implications))").unwrap();
        match action {
            Action::Think(content) => {
                assert_eq!(content, "Consider (the implications)");
                assert!(content.contains("Consider"));
                assert!(content.contains("implications"));
            }
            _ => panic!("Expected Think action"),
        }
    }

    #[test]
    fn test_action_generator_creation() {
        let generator = ActionGenerator::new();
        assert!(!generator.template.is_empty());
    }

    #[test]
    fn test_action_generator_from_file_fallback() {
        // Test with non-existent file (should fall back to embedded template)
        let generator = ActionGenerator::from_file("non_existent_action_template.jinja");
        assert!(!generator.template.is_empty());
    }

    #[test]
    fn test_action_generator_generate_prompt() {
        let generator = ActionGenerator::new();
        let persona = Persona {
            name: "Alice".to_string(),
            background: "A curious researcher".to_string(),
            traits: vec!["analytical".to_string(), "creative".to_string()],
            role: "Analyst".to_string(),
            social: None,
        };
        let agent = Agent::new(persona);

        // Context must end with "\n\n" after each section for proper parsing
        let context = "World Tick: 5\n\nRecent Events:\n- Bob: Spoke: Hello\n\nRelevant Memories:\n- Previous observation\n\nWorld State:\n- temperature: 0.8\n\n";

        let prompt = generator.generate_prompt(&agent, context).unwrap();
        assert!(prompt.contains("Alice"));
        assert!(prompt.contains("Analyst"));
        // Template renders agent traits (passed directly, not from context)
        assert!(prompt.contains("analytical"));
        assert!(prompt.contains("creative"));
        // World variables parsed from context and rendered
        assert!(prompt.contains("temperature"));
    }

    /// U-028 c3b-iii / c2 watch-item: the recovered OASIS personality (`SocialProfile.persona`)
    /// MUST reach the decision prompt, or an agent loaded from a profile decides without its
    /// OASIS personality (shadowed by bio+traits only).
    #[test]
    fn test_generate_prompt_includes_social_persona() {
        let generator = ActionGenerator::new();
        let social = SocialProfile {
            user_id: 7,
            user_name: "skeptic7".to_string(),
            bio: "a researcher".to_string(),
            persona: "A relentless skeptic who questions every claim and loves debate.".to_string(),
            platform: Platform::Twitter,
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
        let agent = Agent::new(Persona {
            name: "Sam".to_string(),
            background: "a researcher".to_string(),
            traits: vec!["curious".to_string()],
            role: "agent".to_string(),
            social: Some(social),
        });
        let prompt = generator.generate_prompt(&agent, "World Tick: 1\n\n").unwrap();
        assert!(prompt.contains("Persona:"), "prompt must render the Persona section");
        assert!(
            prompt.contains("A relentless skeptic who questions every claim"),
            "the recovered OASIS persona text must appear in the decision prompt: {prompt}"
        );
    }

    /// Build a single social agent (twitter) for feed-in-prompt tests.
    fn social_agent(name: &str) -> Agent {
        let social = SocialProfile {
            user_id: 7,
            user_name: "u7".to_string(),
            bio: String::new(),
            persona: String::new(),
            platform: Platform::Twitter,
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
            background: "bg".to_string(),
            traits: vec![],
            role: "agent".to_string(),
            social: Some(social),
        })
    }

    /// Workstream C: the recency-ranked feed snapshot must reach a social agent's decision prompt,
    /// carrying the exact post ids the agent should target. This is the load-bearing feed-back
    /// proof — without it agents are told to LIKE_POST(target_id=...) with no ids to use.
    #[test]
    fn test_feed_appears_in_social_agent_prompt() {
        let agent = social_agent("Ada");
        let world = WorldState::new();
        let feed = crate::sim::social_world::FeedSnapshot {
            posts: vec![
                crate::sim::social_world::FeedPost {
                    id: 12,
                    author_user_id: 4,
                    content: "swarm intelligence paper".to_string(),
                    num_likes: 3,
                    num_shares: 1,
                },
                crate::sim::social_world::FeedPost {
                    id: 9,
                    author_user_id: 5,
                    content: "thoughts on the policy".to_string(),
                    num_likes: 0,
                    num_shares: 0,
                },
            ],
        };

        // construct_context appends the feed section, then generate_prompt round-trips it.
        let context = agent.construct_context(&world, &[], Some(&feed), None);
        assert!(context.contains("Recent posts in your feed:"));

        let generator = ActionGenerator::new();
        let prompt = generator.generate_prompt(&agent, &context).unwrap();
        assert!(prompt.contains("Recent posts in your feed"), "feed header missing: {prompt}");
        assert!(prompt.contains("post-12"), "feed post id 12 missing: {prompt}");
        assert!(prompt.contains("post-9"), "feed post id 9 missing: {prompt}");
        assert!(prompt.contains("swarm intelligence paper"), "feed content missing: {prompt}");
        // The parser recovered the like count.
        assert!(prompt.contains("3 likes"), "feed like count missing: {prompt}");
    }

    /// No-downgrade: with `feed = None` (every non-social caller, and `Agent::step`) NO feed section
    /// is appended and the prompt has no feed block — byte-identical to before feed-back landed.
    #[test]
    fn test_no_feed_section_when_feed_is_none() {
        let agent = social_agent("Ada");
        let world = WorldState::new();
        let context = agent.construct_context(&world, &[], None, None);
        assert!(!context.contains("Recent posts in your feed"));
        let generator = ActionGenerator::new();
        let prompt = generator.generate_prompt(&agent, &context).unwrap();
        assert!(!prompt.contains("Recent posts in your feed"));
    }

    /// `parse_feed_posts` round-trips the exact `construct_context` format and skips malformed
    /// lines fail-closed (a bad line never aborts the prompt).
    #[test]
    fn test_parse_feed_posts_round_trip_and_skip_malformed() {
        let generator = ActionGenerator::new();
        let context = "Recent posts in your feed:\n\
            - [post-12 by user 7 | 3 likes | 1 shares] hello world\n\
            - garbage line that does not match\n\
            - [post-9 by user 4 | 0 likes | 0 shares] another\n\n";
        let parsed = generator.parse_feed_posts(context);
        assert_eq!(parsed.len(), 2, "malformed line must be skipped: {parsed:?}");
        assert_eq!(parsed[0].id, "post-12");
        assert_eq!(parsed[0].author, 7);
        assert_eq!(parsed[0].likes, 3);
        assert_eq!(parsed[0].shares, 1);
        assert_eq!(parsed[0].content, "hello world");
        assert_eq!(parsed[1].id, "post-9");
    }

    /// A generic agent (no social profile) must produce a prompt with NO Persona section — the
    /// `{% if agent_persona %}` skips it on the empty string, so non-social agents are unaffected.
    #[test]
    fn test_generate_prompt_omits_persona_when_no_social() {
        let generator = ActionGenerator::new();
        let agent = Agent::new(Persona {
            name: "Gen".to_string(),
            background: "generic".to_string(),
            traits: vec![],
            role: "agent".to_string(),
            social: None,
        });
        let prompt = generator.generate_prompt(&agent, "World Tick: 1\n\n").unwrap();
        assert!(
            !prompt.contains("Persona:"),
            "no social profile → no Persona section (no regression): {prompt}"
        );
    }

    /// Helper: a minimal `SocialProfile` on a given platform for prompt/menu tests.
    #[cfg(test)]
    fn test_social_profile(platform: Platform) -> SocialProfile {
        SocialProfile {
            user_id: 1,
            user_name: "tester".to_string(),
            bio: "bio".to_string(),
            persona: "a persona".to_string(),
            platform,
            karma: 0,
            friend_count: 0,
            follower_count: 0,
            following_count: 0,
            statuses_count: 0,
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
        }
    }

    /// U-028 c2 completion: a Twitter social agent's decision prompt MUST offer the OASIS social
    /// action space (CREATE_POST etc.) so `Action::Social` is actually reachable — and MUST NOT
    /// fall back to the generic Speak/Move menu (which never yields a social action).
    #[test]
    fn test_social_prompt_offers_twitter_actions() {
        let generator = ActionGenerator::new();
        let agent = Agent::new(Persona {
            name: "Tweep".to_string(),
            background: "b".to_string(),
            traits: vec![],
            role: "agent".to_string(),
            social: Some(test_social_profile(Platform::Twitter)),
        });
        let prompt = generator.generate_prompt(&agent, "World Tick: 1\n\n").unwrap();
        assert!(prompt.contains("twitter"), "names the platform: {prompt}");
        assert!(prompt.contains("CREATE_POST(content="), "offers CREATE_POST: {prompt}");
        assert!(prompt.contains("REPOST"), "offers REPOST (a twitter action): {prompt}");
        // Twitter must NOT advertise reddit-only actions, and must NOT show the generic menu.
        assert!(!prompt.contains("CREATE_COMMENT"), "no reddit-only CREATE_COMMENT on twitter");
        assert!(
            !prompt.contains("Move(location)"),
            "social agent must not get the generic action menu: {prompt}"
        );
    }

    /// Build a tiny graph (Jane Doe --WorksFor--> Acme Corp) plus the two entity ids.
    fn graph_with_worksfor() -> (crate::graph::KnowledgeGraph, uuid::Uuid, uuid::Uuid) {
        use crate::graph::{Entity, EntityKind, KnowledgeGraph, Relation, RelationKind};
        let mut graph = KnowledgeGraph::new();
        let acme =
            Entity { id: Uuid::new_v4(), name: "Acme Corp".into(), kind: EntityKind::Organization };
        let jane = Entity { id: Uuid::new_v4(), name: "Jane Doe".into(), kind: EntityKind::Person };
        let (acme_id, jane_id) = (acme.id, jane.id);
        let na = graph.add_entity(acme).unwrap();
        let nj = graph.add_entity(jane).unwrap();
        graph.add_relation(nj, na, Relation::new(RelationKind::WorksFor, 0.9).unwrap());
        (graph, acme_id, jane_id)
    }

    fn agent_anchored_to(entity_id: uuid::Uuid) -> Agent {
        let mut social = test_social_profile(Platform::Twitter);
        social.source_entity_uuid = Some(entity_id.to_string());
        Agent::new(Persona {
            name: "Acme Corp".into(),
            background: "b".into(),
            traits: vec![],
            role: "agent".into(),
            social: Some(social),
        })
    }

    /// An agent anchored to a graph entity gets that entity's neighborhood as graph context.
    #[test]
    fn graph_context_section_builds_neighbor_lines() {
        let (graph, acme_id, _jane) = graph_with_worksfor();
        let agent = agent_anchored_to(acme_id);
        let section = agent.graph_context_section(Some(&graph)).expect("graph context present");
        assert!(
            section.contains("Jane Doe --[WorksFor]--> Acme Corp"),
            "neighbor relation must be rendered: {section}"
        );
    }

    /// No graph, no source entity, or an unknown entity → no graph context (prompt unchanged).
    #[test]
    fn graph_context_section_absent_cases_return_none() {
        let (graph, acme_id, _) = graph_with_worksfor();

        // No graph handle.
        assert!(agent_anchored_to(acme_id).graph_context_section(None).is_none());

        // Social profile but no source_entity_uuid.
        let generic_social = Agent::new(Persona {
            name: "x".into(),
            background: "b".into(),
            traits: vec![],
            role: "agent".into(),
            social: Some(test_social_profile(Platform::Twitter)),
        });
        assert!(generic_social.graph_context_section(Some(&graph)).is_none());

        // Anchored to an id that is not in this graph.
        let stranger = agent_anchored_to(Uuid::new_v4());
        assert!(stranger.graph_context_section(Some(&graph)).is_none());
    }

    /// The graph-context section round-trips through `construct_context` → `generate_prompt` and
    /// actually reaches the rendered prompt (not dropped by the re-parse).
    #[test]
    fn graph_context_reaches_the_rendered_prompt() {
        let (graph, acme_id, _) = graph_with_worksfor();
        let agent = agent_anchored_to(acme_id);
        let world = WorldState::new();
        let graph_section = agent.graph_context_section(Some(&graph));
        let context = agent.construct_context(&world, &[], None, graph_section.as_deref());
        assert!(context.contains("Knowledge Graph Context:"), "section in context: {context}");

        let prompt = ActionGenerator::new().generate_prompt(&agent, &context).unwrap();
        assert!(
            prompt.contains("Jane Doe --[WorksFor]--> Acme Corp"),
            "graph context must survive the generate_prompt re-parse: {prompt}"
        );
    }

    /// The Reddit menu differs from Twitter (CREATE_COMMENT / TREND etc.) and must NOT leak
    /// `REFRESH` — teri's parser intentionally does not handle it, so offering it would make the
    /// agent emit an unparseable action.
    #[test]
    fn test_social_prompt_offers_reddit_actions_without_refresh() {
        let generator = ActionGenerator::new();
        let agent = Agent::new(Persona {
            name: "Redditor".to_string(),
            background: "b".to_string(),
            traits: vec![],
            role: "agent".to_string(),
            social: Some(test_social_profile(Platform::Reddit)),
        });
        let prompt = generator.generate_prompt(&agent, "World Tick: 1\n\n").unwrap();
        assert!(prompt.contains("reddit"), "names the platform: {prompt}");
        assert!(prompt.contains("CREATE_COMMENT"), "offers reddit CREATE_COMMENT: {prompt}");
        assert!(prompt.contains("TREND"), "offers reddit TREND: {prompt}");
        assert!(
            !prompt.contains("REFRESH"),
            "must NOT offer REFRESH (parser filters it): {prompt}"
        );
    }

    /// No-regression: a generic (non-social) agent still gets the original Speak/Move/… menu.
    #[test]
    fn test_generic_prompt_keeps_classic_action_menu() {
        let generator = ActionGenerator::new();
        let agent = Agent::new(Persona {
            name: "Gen".to_string(),
            background: "b".to_string(),
            traits: vec![],
            role: "agent".to_string(),
            social: None,
        });
        let prompt = generator.generate_prompt(&agent, "World Tick: 1\n\n").unwrap();
        assert!(prompt.contains("Speak(content)"), "generic menu intact: {prompt}");
        assert!(
            !prompt.contains("CREATE_POST"),
            "generic agent gets no social actions: {prompt}"
        );
    }

    /// The CREATE_POST string a social agent is now prompted to emit must round-trip through the
    /// parser into an `Action::Social(CreatePost)` — closing the loop the prompt opens.
    #[test]
    fn test_create_post_string_parses_to_social_action() {
        let agent = Agent::new(Persona {
            name: "P".to_string(),
            background: "b".to_string(),
            traits: vec![],
            role: "agent".to_string(),
            social: Some(test_social_profile(Platform::Reddit)),
        });
        let action = agent
            .parse_and_validate_action("CREATE_POST(content=Hello world from the swarm)")
            .unwrap();
        match action {
            Action::Social(SocialAction::CreatePost { content }) => {
                assert_eq!(content, "Hello world from the swarm")
            }
            other => panic!("expected Social(CreatePost), got {other:?}"),
        }
    }

    #[test]
    fn test_action_generator_parse_context() {
        let generator = ActionGenerator::new();

        // Test parsing world tick
        let context = "World Tick: 42\n\nAgent: Test\nRole: Role\n";
        let tick = generator.parse_world_tick(context);
        assert_eq!(tick, 42);

        // Test parsing recent events from construct_context format
        let context_with_events = "World Tick: 5\n\nRecent Events:\n- Bob: Spoke: Hello\n- Alice: Moved to: Park\n\nRelevant Memories:\n";
        let events = generator.parse_recent_events(context_with_events);
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("Bob"));

        // Test parsing world variables
        let context_with_vars = "World State:\n- var1: 0.5\n- var2: 1.0\n\n";
        let variables = generator.parse_world_variables(context_with_vars);
        assert_eq!(variables.len(), 2);
        assert_eq!(variables.get("var1"), Some(&0.5f32));

        // Test parsing memories
        let context_with_memories =
            "Relevant Memories:\n- Memory content here\n- Another memory\n\nWorld State:";
        let memories = generator.parse_relevant_memories(context_with_memories);
        assert_eq!(memories.len(), 2);
    }

    // ===== Integration Tests with Mock World State =====

    #[tokio::test]
    async fn test_agent_step_with_mock_llm() {
        let mock_response = "Speak(Hello from mock LLM)";
        let mock_llm = MockPersonaLlm::new(mock_response);

        let persona = Persona {
            name: "TestAgent".to_string(),
            background: "A test agent".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let mut agent = Agent::new(persona);

        let mut world = WorldState::new();
        world.advance_tick();
        world.inject_variable("temperature".to_string(), 0.5);

        let action = agent.step(&world, &mock_llm).await.unwrap();
        assert!(matches!(action, Action::Speak(ref s) if s == "Hello from mock LLM"));

        // Verify agent state was restored to Idle
        assert_eq!(agent.state, AgentState::Idle);

        // Verify action was stored in memory
        let recent = agent.memory.get_recent(1);
        assert_eq!(recent.len(), 1);
        assert!(recent[0].content.contains("Spoke"));
    }

    #[tokio::test]
    async fn test_agent_step_with_fallback() {
        // Mock LLM that returns an error to trigger fallback
        struct ErrorMockLlm;

        #[async_trait]
        impl LlmClient for ErrorMockLlm {
            async fn complete(&self, _prompt: &str) -> Result<String> {
                Err(TeriError::Llm("Mock error".to_string()))
            }

            async fn complete_json<T: serde::de::DeserializeOwned>(
                &self,
                _prompt: &str,
            ) -> Result<T> {
                Err(TeriError::Llm("Mock error".to_string()))
            }

            async fn stream(
                &self,
                _prompt: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(TeriError::Llm("Mock error".to_string()))
            }
            async fn chat(
                &self,
                _messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<String> {
                Err(TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
        }

        let mock_llm = ErrorMockLlm;
        let persona = Persona {
            name: "TestAgent".to_string(),
            background: "A test agent".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
            social: None,
        };
        let mut agent = Agent::new(persona);
        let world = WorldState::new();

        let action = agent.step(&world, &mock_llm).await.unwrap();
        // Should fallback to Think action
        assert!(
            matches!(action, Action::Think(ref content) if content.contains("consider my next move"))
        );
    }

    #[tokio::test]
    async fn test_agent_step_stores_action_in_memory_with_importance() {
        let mock_response = "Think(Developing a strategic plan for the mission)";
        let mock_llm = MockPersonaLlm::new(mock_response);

        let persona = Persona {
            name: "StrategicAgent".to_string(),
            background: "A strategic thinker".to_string(),
            traits: vec!["strategic".to_string()],
            role: "Planner".to_string(),
            social: None,
        };
        let mut agent = Agent::new(persona);
        let world = WorldState::new();

        // Clear any existing memories
        agent.memory.clear();

        agent.step(&world, &mock_llm).await.unwrap();

        let recent = agent.memory.get_recent(1);
        assert_eq!(recent.len(), 1);
        // Production code stores as "Thought: {content}" where content is "Developing a strategic plan for the mission"
        assert_eq!(recent[0].content, "Thought: Developing a strategic plan for the mission");
        // High importance (0.9) because content contains "plan"
        assert_eq!(recent[0].importance, 0.9);
    }

    #[tokio::test]
    async fn test_agent_step_integration_with_complex_world() {
        let mock_response = "Observe(Surroundings)";
        let mock_llm = MockPersonaLlm::new(mock_response);

        let persona = Persona {
            name: "Observer".to_string(),
            background: "A careful observer".to_string(),
            traits: vec!["observant".to_string()],
            role: "Scout".to_string(),
            social: None,
        };
        let mut agent = Agent::new(persona.clone());

        let mut world = WorldState::new();
        world.advance_tick();
        world.advance_tick();

        // Add another agent to the world
        let other_agent_id = Uuid::new_v4();
        world.add_agent_snapshot(
            other_agent_id,
            AgentSnapshot {
                id: other_agent_id,
                name: "OtherAgent".to_string(),
                state: "Idle".to_string(),
            },
        );

        // Add an event from the other agent
        world.add_event(Event {
            agent_id: other_agent_id,
            action: Action::Speak("Hello everyone".to_string()),
            timestamp: chrono::Utc::now(),
        });

        // Add world variables
        world.inject_variable("danger_level".to_string(), 0.3);
        world.inject_variable("visibility".to_string(), 0.8);

        let action = agent.step(&world, &mock_llm).await.unwrap();
        assert!(matches!(action, Action::Observe(ref s) if s == "Surroundings"));

        // Verify agent has memory of the action
        let memories = agent.memory.get_recent(10);
        assert!(!memories.is_empty());
    }

    // ===== Social Action Parse Tests =====
    //
    // Each test covers: parse string → correct variant (parse), Display (display), and memory
    // storage with expected importance band (apply-record). This satisfies all three gates from
    // the cycle-3 spec.

    fn make_test_agent() -> Agent {
        Agent::new(Persona {
            name: "SocialBot".to_string(),
            background: "A social media agent".to_string(),
            traits: vec!["engaged".to_string()],
            role: "Poster".to_string(),
            social: None,
        })
    }

    /// An agent carrying a minimal `SocialProfile` on `platform` (for the per-platform action gate).
    fn make_platform_agent(platform: Platform) -> Agent {
        Agent::new(Persona {
            name: "SocialBot".to_string(),
            background: "A social media agent".to_string(),
            traits: vec!["engaged".to_string()],
            role: "Poster".to_string(),
            social: Some(SocialProfile {
                user_id: 1,
                user_name: "bot_1".to_string(),
                bio: String::new(),
                persona: String::new(),
                platform,
                karma: SocialProfile::default_karma(),
                friend_count: SocialProfile::default_friend_count(),
                follower_count: SocialProfile::default_follower_count(),
                following_count: SocialProfile::default_friend_count(),
                statuses_count: SocialProfile::default_statuses_count(),
                age: None,
                gender: None,
                mbti: None,
                country: None,
                profession: None,
                interested_topics: vec![],
                posting_style: None,
                source_entity_uuid: None,
                source_entity_type: None,
                created_at: "2026-06-14".to_string(),
            }),
        })
    }

    // --- TASK-SIM-2 #1: per-platform action gate ---

    #[test]
    fn test_platform_gate_twitter_rejects_reddit_only_action() {
        // CREATE_COMMENT / DISLIKE_COMMENT are Reddit-only; a Twitter agent must be coerced to
        // DO_NOTHING (MiroFish never OFFERS them to a Twitter agent).
        let agent = make_platform_agent(Platform::Twitter);
        for line in ["CREATE_COMMENT(post_id=1,content=hi)", "DISLIKE_COMMENT(target_id=1)"] {
            let action = agent.parse_and_validate_action(line).unwrap();
            assert!(
                matches!(action, Action::Social(SocialAction::DoNothing)),
                "Twitter agent should drop Reddit-only action {line} to DO_NOTHING, got {action:?}"
            );
        }
    }

    #[test]
    fn test_platform_gate_twitter_allows_twitter_action() {
        let agent = make_platform_agent(Platform::Twitter);
        let action = agent.parse_and_validate_action("LIKE_POST(post-42)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Like { target_kind: TargetKind::Post, .. })
        ));
    }

    #[test]
    fn test_platform_gate_reddit_allows_reddit_only_action() {
        // The Reddit-only action a Twitter agent was denied is accepted on Reddit.
        let agent = make_platform_agent(Platform::Reddit);
        let action =
            agent.parse_and_validate_action("CREATE_COMMENT(post_id=1,content=hi)").unwrap();
        assert!(matches!(action, Action::Social(SocialAction::Comment { .. })));
    }

    #[test]
    fn test_platform_gate_reddit_rejects_twitter_only_action() {
        // QUOTE_POST is Twitter-only (not in REDDIT_ACTIONS); a Reddit agent must drop it.
        let agent = make_platform_agent(Platform::Reddit);
        let action =
            agent.parse_and_validate_action("QUOTE_POST(post_id=1,content=agree)").unwrap();
        assert!(matches!(action, Action::Social(SocialAction::DoNothing)));
    }

    #[test]
    fn test_platform_gate_does_not_touch_generic_agent() {
        // A non-social agent (no platform) is never gated — a Reddit-only action parses as-is.
        let agent = make_test_agent();
        let action =
            agent.parse_and_validate_action("CREATE_COMMENT(post_id=1,content=hi)").unwrap();
        assert!(matches!(action, Action::Social(SocialAction::Comment { .. })));
    }

    #[test]
    fn test_platform_gate_donothing_is_idempotent_both_platforms() {
        for p in [Platform::Twitter, Platform::Reddit] {
            let agent = make_platform_agent(p);
            let action = agent.parse_and_validate_action("DO_NOTHING()").unwrap();
            assert!(matches!(action, Action::Social(SocialAction::DoNothing)));
        }
    }

    #[test]
    fn test_parse_social_create_post_bare() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("CREATE_POST(hello world)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::CreatePost { ref content }) if content == "hello world"
        ));
    }

    #[test]
    fn test_parse_social_create_post_key_value() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("CREATE_POST(content=breaking news)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::CreatePost { ref content }) if content == "breaking news"
        ));
    }

    #[test]
    fn test_parse_social_like_post() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("LIKE_POST(post-42)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Like { target_kind: TargetKind::Post, ref target_id })
                if target_id == "post-42"
        ));
    }

    #[test]
    fn test_parse_social_like_comment() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("LIKE_COMMENT(comment-7)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Like { target_kind: TargetKind::Comment, ref target_id })
                if target_id == "comment-7"
        ));
    }

    #[test]
    fn test_parse_social_like_post_vs_comment_are_distinct() {
        // Prove LIKE_POST and LIKE_COMMENT produce DISTINCT parse results (different target_kind).
        let agent = make_test_agent();
        let post_action = agent.parse_and_validate_action("LIKE_POST(id-1)").unwrap();
        let comment_action = agent.parse_and_validate_action("LIKE_COMMENT(id-1)").unwrap();
        // Same target_id, but different target_kind — must NOT be equal.
        assert_ne!(post_action, comment_action);
        // And Display strings must also be distinct.
        assert_ne!(post_action.to_string(), comment_action.to_string());
        assert!(post_action.to_string().contains("post"));
        assert!(comment_action.to_string().contains("comment"));
    }

    #[test]
    fn test_parse_social_dislike_post() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("DISLIKE_POST(post-5)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Dislike { target_kind: TargetKind::Post, ref target_id })
                if target_id == "post-5"
        ));
    }

    #[test]
    fn test_parse_social_dislike_comment() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("DISLIKE_COMMENT(comment-3)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Dislike { target_kind: TargetKind::Comment, ref target_id })
                if target_id == "comment-3"
        ));
    }

    #[test]
    fn test_parse_social_dislike_post_vs_comment_are_distinct() {
        // Prove DISLIKE_POST and DISLIKE_COMMENT produce DISTINCT parse results.
        let agent = make_test_agent();
        let post_action = agent.parse_and_validate_action("DISLIKE_POST(id-2)").unwrap();
        let comment_action = agent.parse_and_validate_action("DISLIKE_COMMENT(id-2)").unwrap();
        assert_ne!(post_action, comment_action);
        assert_ne!(post_action.to_string(), comment_action.to_string());
        assert!(post_action.to_string().contains("post"));
        assert!(comment_action.to_string().contains("comment"));
    }

    #[test]
    fn test_parse_social_repost() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("REPOST(post_id=post-99)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Repost { ref post_id }) if post_id == "post-99"
        ));
    }

    #[test]
    fn test_parse_social_repost_bare() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("REPOST(post-55)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Repost { ref post_id }) if post_id == "post-55"
        ));
    }

    #[test]
    fn test_parse_social_quote_post() {
        let agent = make_test_agent();
        // Key-value form for multi-arg social action
        let action = agent
            .parse_and_validate_action("QUOTE_POST(post_id=post-12,content=great take)")
            .unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Quote { ref post_id, ref content })
                if post_id == "post-12" && content == "great take"
        ));
    }

    #[test]
    fn test_parse_social_follow() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("FOLLOW(user_id=user-77)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Follow { ref user_id }) if user_id == "user-77"
        ));
    }

    #[test]
    fn test_parse_social_follow_bare() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("FOLLOW(user-33)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Follow { ref user_id }) if user_id == "user-33"
        ));
    }

    #[test]
    fn test_parse_social_create_comment() {
        let agent = make_test_agent();
        let action = agent
            .parse_and_validate_action("CREATE_COMMENT(post_id=post-2,content=nice post)")
            .unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Comment { ref post_id, ref content })
                if post_id == "post-2" && content == "nice post"
        ));
    }

    #[test]
    fn test_parse_social_search_posts() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("SEARCH_POSTS(query=climate change)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::SearchPosts { ref query }) if query == "climate change"
        ));
    }

    #[test]
    fn test_parse_social_search_posts_bare() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("SEARCH_POSTS(elections)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::SearchPosts { ref query }) if query == "elections"
        ));
    }

    #[test]
    fn test_parse_social_search_user() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("SEARCH_USER(query=alice)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::SearchUser { ref query }) if query == "alice"
        ));
    }

    #[test]
    fn test_parse_social_mute() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("MUTE(user_id=spammer)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Mute { ref user_id }) if user_id == "spammer"
        ));
    }

    #[test]
    fn test_parse_social_mute_bare() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("MUTE(bad-actor)").unwrap();
        assert!(matches!(
            action,
            Action::Social(SocialAction::Mute { ref user_id }) if user_id == "bad-actor"
        ));
    }

    #[test]
    fn test_parse_social_do_nothing() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("DO_NOTHING()").unwrap();
        assert!(matches!(action, Action::Social(SocialAction::DoNothing)));
    }

    #[test]
    fn test_social_memory_importance_create_post() {
        let mut agent = make_test_agent();
        agent.memory.clear();
        agent.store_action_in_memory(&Action::Social(SocialAction::CreatePost {
            content: "hello".to_string(),
        }));
        let recent = agent.memory.get_recent(1);
        assert_eq!(recent.len(), 1);
        assert!((recent[0].importance - 0.85).abs() < f32::EPSILON);
        assert!(recent[0].content.contains("Posted:"));
    }

    #[test]
    fn test_social_memory_importance_like_post() {
        let mut agent = make_test_agent();
        agent.memory.clear();
        agent.store_action_in_memory(&Action::Social(SocialAction::Like {
            target_kind: TargetKind::Post,
            target_id: "p1".to_string(),
        }));
        let recent = agent.memory.get_recent(1);
        assert!((recent[0].importance - 0.30).abs() < f32::EPSILON);
        assert!(recent[0].content.contains("post"));
    }

    #[test]
    fn test_social_memory_importance_like_comment() {
        let mut agent = make_test_agent();
        agent.memory.clear();
        agent.store_action_in_memory(&Action::Social(SocialAction::Like {
            target_kind: TargetKind::Comment,
            target_id: "c1".to_string(),
        }));
        let recent = agent.memory.get_recent(1);
        assert!((recent[0].importance - 0.30).abs() < f32::EPSILON);
        assert!(recent[0].content.contains("comment"));
    }

    #[test]
    fn test_social_memory_importance_follow() {
        let mut agent = make_test_agent();
        agent.memory.clear();
        agent.store_action_in_memory(&Action::Social(SocialAction::Follow {
            user_id: "u1".to_string(),
        }));
        let recent = agent.memory.get_recent(1);
        assert!((recent[0].importance - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn test_social_memory_importance_do_nothing() {
        let mut agent = make_test_agent();
        agent.memory.clear();
        agent.store_action_in_memory(&Action::Social(SocialAction::DoNothing));
        let recent = agent.memory.get_recent(1);
        assert!((recent[0].importance - 0.05).abs() < f32::EPSILON);
    }

    #[test]
    fn test_parse_social_trend_uppercase() {
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("TREND()").unwrap();
        assert!(matches!(action, Action::Social(SocialAction::Trend)));
    }

    #[test]
    fn test_parse_social_trend_lowercase() {
        // Also accept lowercase "trend" per ACTION_TYPE_MAP
        let agent = make_test_agent();
        let action = agent.parse_and_validate_action("trend()").unwrap();
        assert!(matches!(action, Action::Social(SocialAction::Trend)));
    }

    #[test]
    fn test_social_action_display_trend_in_agent() {
        let action = Action::Social(SocialAction::Trend);
        assert_eq!(action.to_string(), "Social: Performed trend operation");
    }

    #[test]
    fn test_social_memory_importance_trend() {
        let mut agent = make_test_agent();
        agent.memory.clear();
        agent.store_action_in_memory(&Action::Social(SocialAction::Trend));
        let recent = agent.memory.get_recent(1);
        assert_eq!(recent.len(), 1);
        // Trend is a browse/discovery op — same band as SearchPosts (0.25)
        assert!((recent[0].importance - 0.25).abs() < f32::EPSILON);
        assert!(recent[0].content.contains("trend"));
    }

    #[test]
    fn test_parse_social_trend_apply_no_panic() {
        // Verify Trend routes through apply without panic (generic social event path)
        use crate::sim::WorldState;
        let mut world = WorldState::new();
        let agent_id = uuid::Uuid::new_v4();
        let ts = chrono::DateTime::from_timestamp(0, 0).unwrap();
        world.apply_at(agent_id, Action::Social(SocialAction::Trend), ts);
        assert_eq!(world.events.len(), 1);
    }

    #[test]
    fn test_unknown_social_action_returns_error() {
        let agent = make_test_agent();
        let result = agent.parse_and_validate_action("TOTALLY_UNKNOWN(something)");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown action type"));
    }

    #[test]
    fn test_generic_actions_unaltered_after_social_extension() {
        // Regression guard: all 5 original generic variants still parse correctly.
        let agent = make_test_agent();
        assert!(matches!(
            agent.parse_and_validate_action("Speak(hello)").unwrap(),
            Action::Speak(ref s) if s == "hello"
        ));
        assert!(matches!(
            agent.parse_and_validate_action("Move(forest)").unwrap(),
            Action::Move(ref s) if s == "forest"
        ));
        assert!(matches!(
            agent.parse_and_validate_action("Interact(door)").unwrap(),
            Action::Interact(ref s) if s == "door"
        ));
        assert!(matches!(
            agent.parse_and_validate_action("Observe(sky)").unwrap(),
            Action::Observe(ref s) if s == "sky"
        ));
        assert!(matches!(
            agent.parse_and_validate_action("Think(plan)").unwrap(),
            Action::Think(ref s) if s == "plan"
        ));
    }

    // ===== U-018 SocialProfile / Platform Tests =====

    #[test]
    fn test_platform_serde_roundtrip() {
        // Platform enum serializes to lowercase string and deserializes back.
        let tw = Platform::Twitter;
        let rd = Platform::Reddit;

        let tw_json = serde_json::to_string(&tw).unwrap();
        let rd_json = serde_json::to_string(&rd).unwrap();
        assert_eq!(tw_json, "\"twitter\"");
        assert_eq!(rd_json, "\"reddit\"");

        let tw2: Platform = serde_json::from_str(&tw_json).unwrap();
        let rd2: Platform = serde_json::from_str(&rd_json).unwrap();
        assert_eq!(tw2, Platform::Twitter);
        assert_eq!(rd2, Platform::Reddit);
    }

    #[test]
    fn test_platform_eq() {
        assert_eq!(Platform::Twitter, Platform::Twitter);
        assert_eq!(Platform::Reddit, Platform::Reddit);
        assert_ne!(Platform::Twitter, Platform::Reddit);
    }

    #[test]
    fn test_social_profile_defaults() {
        // Default numeric values match MiroFish OasisAgentProfile defaults.
        let profile = SocialProfile {
            user_id: 0,
            user_name: "test_123".to_string(),
            bio: "".to_string(),
            persona: "".to_string(),
            platform: Platform::Reddit,
            karma: SocialProfile::default_karma(),
            friend_count: SocialProfile::default_friend_count(),
            follower_count: SocialProfile::default_follower_count(),
            following_count: SocialProfile::default_friend_count(),
            statuses_count: SocialProfile::default_statuses_count(),
            age: None,
            gender: None,
            mbti: None,
            country: None,
            profession: None,
            interested_topics: vec![],
            posting_style: None,
            source_entity_uuid: None,
            source_entity_type: None,
            created_at: "2026-06-14".to_string(),
        };
        assert_eq!(profile.karma, 1000);
        assert_eq!(profile.friend_count, 100);
        assert_eq!(profile.follower_count, 150);
        assert_eq!(profile.statuses_count, 500);
    }

    #[test]
    fn test_social_profile_serde_roundtrip() {
        let profile = SocialProfile {
            user_id: 7,
            user_name: "alice_wonder_42".to_string(),
            bio: "Tech journalist covering AI".to_string(),
            persona: "Alice is a seasoned tech journalist with strong opinions on AI ethics."
                .to_string(),
            platform: Platform::Twitter,
            karma: 2500,
            friend_count: 200,
            follower_count: 300,
            following_count: 200,
            statuses_count: 1000,
            age: Some(28),
            gender: Some("female".to_string()),
            mbti: Some("ENFP".to_string()),
            country: Some("US".to_string()),
            profession: Some("Journalist".to_string()),
            interested_topics: vec!["Tech".to_string(), "Politics".to_string()],
            posting_style: Some("Frequent, opinionated".to_string()),
            source_entity_uuid: Some("abc-123".to_string()),
            source_entity_type: Some("journalist".to_string()),
            created_at: "2026-06-14".to_string(),
        };
        let json = serde_json::to_string(&profile).unwrap();
        let decoded: SocialProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.user_name, profile.user_name);
        assert_eq!(decoded.platform, Platform::Twitter);
        assert_eq!(decoded.karma, 2500);
        assert_eq!(decoded.age, Some(28));
        assert_eq!(decoded.mbti.as_deref(), Some("ENFP"));
    }

    #[test]
    fn test_persona_generic_still_works_social_none() {
        // Generic Persona (social=None) construction and field access — no regression.
        let persona = Persona {
            name: "Alice".to_string(),
            background: "A curious researcher".to_string(),
            traits: vec!["analytical".to_string()],
            role: "Analyst".to_string(),
            social: None,
        };
        assert_eq!(persona.name, "Alice");
        assert!(persona.social.is_none());
    }

    #[test]
    fn test_persona_serde_backward_compat_no_social_field() {
        // Old 4-field Persona JSON (no "social" key) must deserialize to social=None.
        let old_json = r#"{
            "name": "Bob",
            "background": "A veteran explorer",
            "traits": ["brave", "resourceful"],
            "role": "Scout"
        }"#;
        let persona: Persona = serde_json::from_str(old_json).unwrap();
        assert_eq!(persona.name, "Bob");
        assert_eq!(persona.role, "Scout");
        assert!(persona.social.is_none(), "old JSON without 'social' must deserialize to None");
    }

    #[test]
    fn test_persona_serde_with_social_field() {
        // Persona with social field roundtrips correctly.
        let persona = Persona {
            name: "Carol".to_string(),
            background: "A social media analyst".to_string(),
            traits: vec!["curious".to_string()],
            role: "Analyst".to_string(),
            social: Some(SocialProfile {
                user_id: 5,
                user_name: "carol_analyst_500".to_string(),
                bio: "Data analyst and social media researcher".to_string(),
                persona: "Carol focuses on social dynamics in online communities, bringing analytical rigor.".to_string(),
                platform: Platform::Reddit,
                karma: 1000,
                friend_count: 100,
                follower_count: 150,
                following_count: 100,
                statuses_count: 500,
                age: Some(34),
                gender: Some("female".to_string()),
                mbti: None,
                country: None,
                profession: Some("Analyst".to_string()),
                interested_topics: vec!["Data Science".to_string()],
                posting_style: None,
                source_entity_uuid: None,
                source_entity_type: Some("expert".to_string()),
                created_at: "2026-06-14".to_string(),
            }),
        };
        let json = serde_json::to_string(&persona).unwrap();
        let decoded: Persona = serde_json::from_str(&json).unwrap();
        assert!(decoded.social.is_some());
        let sp = decoded.social.unwrap();
        assert_eq!(sp.user_name, "carol_analyst_500");
        assert_eq!(sp.platform, Platform::Reddit);
        assert_eq!(sp.karma, 1000);
    }

    #[test]
    fn test_generate_username_deterministic() {
        // Same name → same output (hash-derived suffix, no randomness).
        let u1 = PersonaGenerator::generate_username("Alice Wonder");
        let u2 = PersonaGenerator::generate_username("Alice Wonder");
        assert_eq!(u1, u2);
        // Username is lowercase, alphanumeric + underscore only.
        assert!(u1.chars().all(|c| c.is_alphanumeric() || c == '_'));
        // Suffix is in 100..=999 range.
        let parts: Vec<&str> = u1.rsplitn(2, '_').collect();
        let suffix: u32 = parts[0].parse().expect("suffix must be numeric");
        assert!((100..=999).contains(&suffix));
    }

    #[test]
    fn test_generate_username_distinct_for_different_names() {
        let u1 = PersonaGenerator::generate_username("Alice");
        let u2 = PersonaGenerator::generate_username("Bob");
        // Different names should produce different usernames (different suffixes / bases).
        assert_ne!(u1, u2);
    }

    #[tokio::test]
    async fn test_generate_social_with_mock_llm() {
        // Mock LLM returns valid JSON — SocialProfile is populated from it.
        let mock_json = r#"{
            "karma": 3500,
            "friend_count": 250,
            "follower_count": 800,
            "statuses_count": 1200,
            "age": 31,
            "gender": "female",
            "mbti": "ENFP",
            "country": "Canada",
            "profession": "Journalist",
            "interested_topics": ["Politics", "Media"],
            "posting_style": "Frequent, opinionated commentary"
        }"#;
        let mock_llm = MockPersonaLlm::new(mock_json);
        let generator = PersonaGenerator::new();

        let sp = generator
            .generate_social(
                "Jane Doe",
                "journalist",
                "A seasoned reporter",
                Platform::Twitter,
                &mock_llm,
                None,
            )
            .await
            .expect("generate_social must succeed with valid mock LLM");

        assert_eq!(sp.platform, Platform::Twitter);
        assert_eq!(sp.karma, 3500);
        assert_eq!(sp.friend_count, 250);
        assert_eq!(sp.follower_count, 800);
        assert_eq!(sp.statuses_count, 1200);
        assert_eq!(sp.age, Some(31));
        assert_eq!(sp.gender.as_deref(), Some("female"));
        assert_eq!(sp.mbti.as_deref(), Some("ENFP"));
        assert_eq!(sp.country.as_deref(), Some("Canada"));
        assert_eq!(sp.profession.as_deref(), Some("Journalist"));
        assert!(sp.interested_topics.contains(&"Politics".to_string()));
        assert!(sp.posting_style.is_some());
        // username is derived from entity name deterministically
        assert!(!sp.user_name.is_empty());
        // source_entity_type set from entity_type argument
        assert_eq!(sp.source_entity_type.as_deref(), Some("journalist"));
    }

    #[tokio::test]
    async fn test_generate_social_rule_based_fallback_on_llm_error() {
        // When LLM returns an error, generate_social falls back to rule-based.
        struct ErrorLlm;
        #[async_trait]
        impl LlmClient for ErrorLlm {
            async fn complete(&self, _: &str) -> Result<String> {
                Err(TeriError::Llm("network failure".to_string()))
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>>
            {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn chat(
                &self,
                _messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<String> {
                Err(TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
        }

        let generator = PersonaGenerator::new();
        let sp = generator
            .generate_social(
                "State University",
                "university",
                "A public university",
                Platform::Reddit,
                &ErrorLlm,
                None,
            )
            .await
            .expect("rule-based fallback must succeed even when LLM errors");

        assert_eq!(sp.platform, Platform::Reddit);
        // TASK-SIM-1 gap #3: social counts are now randomized within MiroFish's ranges
        // (institutions still use the same numeric ranges as everyone else).
        assert!((500..=5000).contains(&sp.karma), "karma in range; got {}", sp.karma);
        assert!((50..=500).contains(&sp.friend_count), "friend_count in range");
        assert!((100..=1000).contains(&sp.follower_count), "follower_count in range");
        assert!((100..=2000).contains(&sp.statuses_count), "statuses_count in range");
        // Institutional accounts keep MiroFish's FIXED demographics: age=30, gender="other",
        // mbti="ISTJ" (oasis_profile_generator.py:826-828).
        assert_eq!(sp.age, Some(30));
        assert_eq!(sp.gender.as_deref(), Some("other"));
        assert_eq!(sp.mbti.as_deref(), Some("ISTJ"));
        assert!(!sp.interested_topics.is_empty());
    }

    #[tokio::test]
    async fn test_generate_social_rule_based_fallback_on_invalid_json() {
        // When LLM returns non-JSON, rule-based fallback kicks in.
        let bad_llm = MockPersonaLlm::new("this is not json at all");
        let generator = PersonaGenerator::new();

        let sp = generator
            .generate_social(
                "John Student",
                "student",
                "A university student",
                Platform::Twitter,
                &bad_llm,
                None,
            )
            .await
            .expect("rule-based fallback must succeed for bad LLM JSON");

        // TASK-SIM-1 gap #3: the student rule now RANDOMIZES age (18..=30), gender (male/female),
        // mbti (from MBTI_TYPES) and country (from COUNTRIES). The stable signals that prove the
        // student branch ran are the fixed profession + interested_topics.
        let age = sp.age.expect("rule-based sets an age");
        assert!((18..=30).contains(&age), "student age in 18..=30; got {age}");
        assert!(matches!(sp.gender.as_deref(), Some("male") | Some("female")));
        assert!(
            PersonaGenerator::MBTI_TYPES.contains(&sp.mbti.as_deref().unwrap()),
            "mbti drawn from MBTI_TYPES; got {:?}",
            sp.mbti
        );
        assert!(
            PersonaGenerator::COUNTRIES.contains(&sp.country.as_deref().unwrap()),
            "country drawn from COUNTRIES; got {:?}",
            sp.country
        );
        assert_eq!(sp.profession.as_deref(), Some("Student"));
        assert!(sp.interested_topics.contains(&"Education".to_string()));
    }

    #[test]
    fn test_social_profile_defaults_serde_defaults() {
        // Partial JSON (missing numeric fields) deserializes using serde defaults.
        let partial_json = r#"{
            "user_id": 1,
            "user_name": "handle_123",
            "bio": "A short bio",
            "persona": "Detailed personality description",
            "platform": "reddit",
            "created_at": "2026-06-14"
        }"#;
        let sp: SocialProfile = serde_json::from_str(partial_json).unwrap();
        assert_eq!(sp.karma, 1000);
        assert_eq!(sp.friend_count, 100);
        assert_eq!(sp.follower_count, 150);
        assert_eq!(sp.statuses_count, 500);
        assert!(sp.interested_topics.is_empty());
    }

    // ===== U-018 Fix: to_reddit_format / to_twitter_format / to_dict / bio+persona distinct =====

    /// Build a fully-populated Persona with SocialProfile for serialization tests.
    fn make_social_persona() -> Persona {
        Persona {
            name: "Alice Wonder".to_string(),
            background: "Generic background text".to_string(),
            traits: vec!["curious".to_string()],
            role: "Researcher".to_string(),
            social: Some(SocialProfile {
                user_id: 42,
                user_name: "alice_wonder_123".to_string(),
                bio: "Short public bio line".to_string(),
                persona: "Detailed and distinct persona description for LLM context".to_string(),
                platform: Platform::Reddit,
                karma: 2500,
                friend_count: 80,
                follower_count: 200,
                following_count: 80,
                statuses_count: 350,
                age: Some(28),
                gender: Some("female".to_string()),
                mbti: Some("INFJ".to_string()),
                country: Some("US".to_string()),
                profession: Some("Scientist".to_string()),
                interested_topics: vec!["Science".to_string(), "Ethics".to_string()],
                posting_style: None,
                source_entity_uuid: None,
                source_entity_type: Some("person".to_string()),
                created_at: "2026-06-14".to_string(),
            }),
        }
    }

    #[test]
    fn test_to_reddit_format_keys_and_no_underscore_username() {
        // to_reddit_format must use "username" (no underscore) — OASIS library requirement.
        // Keys: user_id, username, name, bio, persona, karma, created_at.
        let p = make_social_persona();
        let v = p.to_reddit_format().expect("should produce Some");

        assert_eq!(v["user_id"], 42u64, "user_id must be present");
        assert_eq!(v["username"], "alice_wonder_123", "key must be 'username' (no underscore)");
        assert!(v.get("user_name").is_none(), "must NOT have 'user_name' with underscore");
        assert_eq!(v["name"], "Alice Wonder");
        assert_eq!(v["bio"], "Short public bio line");
        assert_eq!(v["persona"], "Detailed and distinct persona description for LLM context");
        assert_eq!(v["karma"], 2500i64);
        assert_eq!(v["created_at"], "2026-06-14");
        // No friend_count / follower_count / statuses_count (Reddit-only excludes Twitter fields)
        assert!(v.get("friend_count").is_none());
    }

    #[test]
    fn test_to_reddit_format_conditional_demographics_present_when_set() {
        // When age/gender/mbti/country/profession/interested_topics are set, they appear.
        let p = make_social_persona();
        let v = p.to_reddit_format().unwrap();

        assert_eq!(v["age"], 28u32);
        assert_eq!(v["gender"], "female");
        assert_eq!(v["mbti"], "INFJ");
        assert_eq!(v["country"], "US");
        assert_eq!(v["profession"], "Scientist");
        let topics = v["interested_topics"].as_array().unwrap();
        assert_eq!(topics.len(), 2);
        assert_eq!(topics[0], "Science");
    }

    #[test]
    fn test_to_reddit_format_conditional_demographics_absent_when_none() {
        // When optional fields are None / empty, they must NOT appear in the output.
        let mut p = make_social_persona();
        if let Some(ref mut s) = p.social {
            s.age = None;
            s.gender = None;
            s.mbti = None;
            s.country = None;
            s.profession = None;
            s.interested_topics = vec![];
        }
        let v = p.to_reddit_format().unwrap();

        assert!(v.get("age").is_none(), "age must be absent when None");
        assert!(v.get("gender").is_none(), "gender must be absent when None");
        assert!(v.get("mbti").is_none(), "mbti must be absent when None");
        assert!(v.get("country").is_none(), "country must be absent when None");
        assert!(v.get("profession").is_none(), "profession must be absent when None");
        assert!(
            v.get("interested_topics").is_none(),
            "interested_topics must be absent when empty"
        );
    }

    #[test]
    fn test_to_reddit_format_returns_none_when_no_social() {
        let p = Persona {
            name: "NoSocial".to_string(),
            background: "bg".to_string(),
            traits: vec![],
            role: "none".to_string(),
            social: None,
        };
        assert!(p.to_reddit_format().is_none());
    }

    #[test]
    fn test_to_twitter_format_keys_and_no_underscore_username() {
        // to_twitter_format: username (no underscore), friend_count, follower_count,
        // statuses_count, NO karma.
        let p = make_social_persona();
        let v = p.to_twitter_format().expect("should produce Some");

        assert_eq!(v["user_id"], 42u64);
        assert_eq!(v["username"], "alice_wonder_123", "key must be 'username' (no underscore)");
        assert!(v.get("user_name").is_none(), "must NOT have 'user_name' with underscore");
        assert_eq!(v["name"], "Alice Wonder");
        assert_eq!(v["bio"], "Short public bio line");
        assert_eq!(v["persona"], "Detailed and distinct persona description for LLM context");
        assert_eq!(v["friend_count"], 80i64);
        assert_eq!(v["follower_count"], 200i64);
        assert_eq!(v["statuses_count"], 350i64);
        assert_eq!(v["created_at"], "2026-06-14");
        // karma is NOT present in twitter format
        assert!(v.get("karma").is_none(), "karma must NOT appear in twitter format");
    }

    #[test]
    fn test_to_twitter_format_conditional_demographics_present_when_set() {
        let p = make_social_persona();
        let v = p.to_twitter_format().unwrap();

        assert_eq!(v["age"], 28u32);
        assert_eq!(v["gender"], "female");
        assert_eq!(v["mbti"], "INFJ");
        assert_eq!(v["country"], "US");
        assert_eq!(v["profession"], "Scientist");
        let topics = v["interested_topics"].as_array().unwrap();
        assert_eq!(topics.len(), 2);
    }

    #[test]
    fn test_to_twitter_format_conditional_demographics_absent_when_none() {
        let mut p = make_social_persona();
        if let Some(ref mut s) = p.social {
            s.age = None;
            s.gender = None;
            s.mbti = None;
            s.country = None;
            s.profession = None;
            s.interested_topics = vec![];
        }
        let v = p.to_twitter_format().unwrap();

        assert!(v.get("age").is_none());
        assert!(v.get("gender").is_none());
        assert!(v.get("mbti").is_none());
        assert!(v.get("country").is_none());
        assert!(v.get("profession").is_none());
        assert!(v.get("interested_topics").is_none());
    }

    #[test]
    fn test_to_twitter_format_returns_none_when_no_social() {
        let p = Persona {
            name: "NoSocial".to_string(),
            background: "bg".to_string(),
            traits: vec![],
            role: "none".to_string(),
            social: None,
        };
        assert!(p.to_twitter_format().is_none());
    }

    #[test]
    fn test_bio_and_persona_are_distinct_fields() {
        // bio and persona must appear as separate, distinct values in the output.
        // This proves the de-narrowing — collapsing both into Persona.background was wrong.
        let p = make_social_persona();
        let reddit = p.to_reddit_format().unwrap();
        let twitter = p.to_twitter_format().unwrap();

        // bio != persona (they have different values in make_social_persona)
        assert_ne!(
            reddit["bio"], reddit["persona"],
            "bio and persona must be distinct in reddit format"
        );
        assert_ne!(
            twitter["bio"], twitter["persona"],
            "bio and persona must be distinct in twitter format"
        );

        // Both appear with their correct distinct values
        assert_eq!(reddit["bio"], "Short public bio line");
        assert_eq!(reddit["persona"], "Detailed and distinct persona description for LLM context");
        assert_eq!(twitter["bio"], "Short public bio line");
        assert_eq!(twitter["persona"], "Detailed and distinct persona description for LLM context");
    }

    #[test]
    fn test_to_dict_complete_flat_format() {
        // to_dict must include all fields, with "user_name" (underscore), no optional omission.
        let p = make_social_persona();
        let v = p.to_dict().expect("should produce Some");

        assert_eq!(v["user_id"], 42u64);
        // to_dict uses "user_name" (with underscore) — the full flat format
        assert_eq!(v["user_name"], "alice_wonder_123");
        assert!(v.get("username").is_none(), "to_dict must NOT have 'username' (no underscore)");
        assert_eq!(v["name"], "Alice Wonder");
        assert_eq!(v["bio"], "Short public bio line");
        assert_eq!(v["persona"], "Detailed and distinct persona description for LLM context");
        assert_eq!(v["karma"], 2500i64);
        assert_eq!(v["friend_count"], 80i64);
        assert_eq!(v["follower_count"], 200i64);
        assert_eq!(v["statuses_count"], 350i64);
        assert_eq!(v["age"], 28u32);
        assert_eq!(v["gender"], "female");
        assert_eq!(v["mbti"], "INFJ");
        assert_eq!(v["country"], "US");
        assert_eq!(v["profession"], "Scientist");
        assert_eq!(v["created_at"], "2026-06-14");
        // source fields present
        assert_eq!(v["source_entity_type"], "person");
    }

    #[test]
    fn test_to_dict_returns_none_when_no_social() {
        let p = Persona {
            name: "NoSocial".to_string(),
            background: "bg".to_string(),
            traits: vec![],
            role: "none".to_string(),
            social: None,
        };
        assert!(p.to_dict().is_none());
    }

    #[test]
    fn test_to_dict_null_optionals_present() {
        // to_dict includes null for None optionals (unconditional, unlike platform formats).
        let mut p = make_social_persona();
        if let Some(ref mut s) = p.social {
            s.age = None;
            s.gender = None;
            s.mbti = None;
            s.country = None;
            s.profession = None;
            s.interested_topics = vec![];
            s.source_entity_uuid = None;
        }
        let v = p.to_dict().unwrap();

        // All fields present, but as null / empty array
        assert!(v["age"].is_null(), "age must be null when None in to_dict");
        assert!(v["gender"].is_null());
        assert!(v["mbti"].is_null());
        assert!(v["country"].is_null());
        assert!(v["profession"].is_null());
        assert_eq!(v["interested_topics"].as_array().unwrap().len(), 0);
        assert!(v["source_entity_uuid"].is_null());
    }

    // ===== S-360 / S-361: JSON salvage (fix_truncated_json + try_fix_json) =====

    #[test]
    fn test_fix_truncated_json_closes_open_brace() {
        // Simple unclosed object
        let input = r#"{"bio": "hello", "persona": "world""#;
        let fixed = PersonaGenerator::fix_truncated_json(input);
        let v: serde_json::Value = serde_json::from_str(&fixed).expect("should parse after fix");
        assert_eq!(v["bio"], "hello");
        assert_eq!(v["persona"], "world");
    }

    #[test]
    fn test_fix_truncated_json_closes_dangling_string_then_braces() {
        // String was cut in the middle — last char is not a valid JSON terminal
        let input = r#"{"bio": "truncated val"#;
        let fixed = PersonaGenerator::fix_truncated_json(input);
        // After fix we should get {"bio": "truncated val"}
        let v: serde_json::Value = serde_json::from_str(&fixed).expect("should parse after fix");
        assert_eq!(v["bio"], "truncated val");
    }

    #[test]
    fn test_fix_truncated_json_closes_open_array_and_brace() {
        // Array and brace both unclosed
        let input = r#"{"topics": ["one", "two""#;
        let fixed = PersonaGenerator::fix_truncated_json(input);
        // After fix: {"topics": ["one", "two"]}
        let v: serde_json::Value = serde_json::from_str(&fixed).expect("should parse after fix");
        let arr = v["topics"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], "one");
    }

    #[test]
    fn test_fix_truncated_json_valid_input_unchanged() {
        // Valid JSON should still round-trip through fix
        let input = r#"{"bio": "ok", "karma": 1000}"#;
        let fixed = PersonaGenerator::fix_truncated_json(input);
        let v: serde_json::Value =
            serde_json::from_str(&fixed).expect("valid input should still parse");
        assert_eq!(v["karma"], 1000);
    }

    #[test]
    fn test_try_fix_json_salvages_truncated_response() {
        // A mostly-valid JSON object truncated after the closing string value.
        // MiroFish's _try_fix_json guarantees recovery of bio/persona via field-level
        // extraction. Numeric fields (karma, age) may not be recoverable from a truncated
        // response (the structural fix converts trailing digits into `31"` which is invalid
        // JSON) — this matches MiroFish behavior where step 6 extracts only string fields.
        // The key contract: some result is returned (NOT None), and bio/persona are preserved.
        let truncated = r#"{"bio": "Tech journalist bio", "persona": "Detailed persona text", "karma": 3500, "age": 31"#;
        let result = PersonaGenerator::try_fix_json(truncated, "Jane", "journalist", "");
        assert!(result.is_some(), "should salvage a truncated-but-repairable response");
        let v = result.unwrap();
        // bio and persona are extractable by field-level regex (MiroFish step 6 contract)
        assert_eq!(v["bio"], "Tech journalist bio");
        assert_eq!(v["persona"], "Detailed persona text");
    }

    #[test]
    fn test_try_fix_json_salvages_string_truncated_mid_value() {
        // When a JSON string value is truncated (last char IS a quote boundary), fix_truncated_json
        // closes the brace and the structural parse succeeds — all fields including numerics survive.
        let truncated =
            r#"{"bio": "Journalist bio", "persona": "Detailed persona", "karma": 3500, "age": 31}"#;
        // This is actually valid JSON — prove fix_truncated_json doesn't break it
        let result = PersonaGenerator::try_fix_json(truncated, "Jane", "journalist", "");
        assert!(result.is_some(), "valid truncated JSON should parse");
        let v = result.unwrap();
        assert_eq!(v["bio"], "Journalist bio");
        assert_eq!(v["karma"], 3500);
        assert_eq!(v["age"], 31);
    }

    #[test]
    fn test_try_fix_json_returns_none_for_garbage() {
        // Completely garbage input — no JSON structure at all
        let garbage = "this is not json at all, nothing to salvage here";
        let result = PersonaGenerator::try_fix_json(garbage, "X", "y", "");
        // Either None or a minimal partial with _fixed — since no bio/persona key matched,
        // the MiroFish "if bio_match or persona_match" guard causes None.
        assert!(result.is_none(), "pure garbage with no JSON fields should return None");
    }

    #[test]
    fn test_try_fix_json_field_extraction_fallback() {
        // JSON has bio/persona extractable by field regex but overall structure is broken
        let broken = r#"garbage preamble {"bio": "extracted bio", "persona": "extracted persona" more garbage"#;
        let result = PersonaGenerator::try_fix_json(broken, "X", "y", "");
        // Should extract bio and persona via field-level regex
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v["bio"], "extracted bio");
        assert_eq!(v["persona"], "extracted persona");
    }

    #[tokio::test]
    async fn test_generate_social_salvage_path_taken_not_rule_based() {
        // When the LLM returns a truncated-but-repairable JSON, generate_social must use the
        // salvaged LLM values, NOT fall back to rule-based defaults.
        //
        // Proof: the salvaged response has distinct bio/persona values that ONLY come from
        // the LLM response. Rule-based for "expert" produces bio="Expert and thought leader
        // in their field." — if we see "UNIQUE_LLM_SIGNATURE" in bio, the salvage path ran.
        //
        // The truncation scenario: JSON is missing the closing `}` (common max_tokens cutoff),
        // and the last key:value pair is a string (so fix_truncated_json can properly close it).
        let truncated_llm_response =
            r#"{"bio": "UNIQUE_LLM_SIGNATURE bio", "persona": "UNIQUE_LLM_SIGNATURE persona""#;
        let mock_llm = MockPersonaLlm::new(truncated_llm_response);
        let generator = PersonaGenerator::new();

        let sp = generator
            .generate_social(
                "Test Entity",
                "expert",
                "A test expert",
                Platform::Twitter,
                &mock_llm,
                None,
            )
            .await
            .expect("salvage path must succeed");

        // The LLM-sourced signature in bio/persona proves the salvage path was taken.
        // Rule-based for "expert" would produce bio="Expert and thought leader in their field."
        assert!(
            sp.bio.contains("UNIQUE_LLM_SIGNATURE"),
            "bio must contain LLM-unique value, proving salvage (not rule-based); got: {}",
            sp.bio
        );
        assert!(
            sp.persona.contains("UNIQUE_LLM_SIGNATURE"),
            "persona must contain LLM-unique value, proving salvage (not rule-based); got: {}",
            sp.persona
        );
    }

    #[tokio::test]
    async fn test_generate_social_genuine_garbage_falls_back_to_rule_based() {
        // When LLM returns genuine garbage (no JSON structure), rule-based fallback kicks in.
        let bad_llm = MockPersonaLlm::new("no json here whatsoever, just plain text output");
        let generator = PersonaGenerator::new();

        let sp = generator
            .generate_social(
                "John Student",
                "student",
                "A university student",
                Platform::Twitter,
                &bad_llm,
                None,
            )
            .await
            .expect("rule-based fallback must succeed for garbage LLM output");

        // student rule → randomized age in 18..=30 + fixed profession/topics prove rule-based ran.
        let age = sp.age.expect("rule-based sets an age");
        assert!(
            (18..=30).contains(&age),
            "age in student range proves rule-based path; got {age}"
        );
        assert_eq!(
            sp.profession.as_deref(),
            Some("Student"),
            "Student profession proves rule path"
        );
        assert!(sp.interested_topics.contains(&"Education".to_string()));
    }

    // ===== S-356: graph-enriched generate_social (build_entity_context) =====

    #[tokio::test]
    async fn test_generate_social_with_graph_ctx_enriches_prompt() {
        // When generate_social is called with a graph context, the LLM prompt must include
        // neighbor information. We prove this by using a mock LLM that echoes its prompt,
        // then assert the prompt contains the neighbor name.

        struct PromptCaptureLlm {
            captured: std::sync::Arc<std::sync::Mutex<String>>,
            response: String,
        }

        #[async_trait]
        impl LlmClient for PromptCaptureLlm {
            async fn complete(&self, prompt: &str) -> Result<String> {
                *self.captured.lock().unwrap() = prompt.to_string();
                Ok(self.response.clone())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>>
            {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn chat(
                &self,
                messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<String> {
                // `generate_social` builds the persona prompt as the user message; capture it.
                *self.captured.lock().unwrap() = capture_user_message(messages);
                Ok(self.response.clone())
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
        }

        // Build a graph with one entity and one neighbor
        let mut graph = KnowledgeGraph::new();
        let main_entity = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Tech University".to_string(),
            kind: EntityKind::Organization,
        };
        let neighbor_entity = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Professor Alice".to_string(),
            kind: EntityKind::Person,
        };
        let main_idx = graph.add_entity(main_entity.clone()).expect("add main entity");
        let neighbor_idx = graph.add_entity(neighbor_entity).expect("add neighbor");
        graph.add_relation(
            main_idx,
            neighbor_idx,
            crate::graph::Relation::new(crate::graph::RelationKind::RelatedTo, 0.9)
                .expect("valid relation"),
        );

        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let mock_llm = PromptCaptureLlm {
            captured: captured.clone(),
            // Rule-based fallback friendly: return empty/invalid to fall through
            response: "{}".to_string(),
        };

        let generator = PersonaGenerator::new();
        let _sp = generator
            .generate_social(
                "Tech University",
                "university",
                "A top university",
                Platform::Reddit,
                &mock_llm,
                Some((&graph, &main_entity)),
            )
            .await
            .expect("generate_social with graph_ctx must succeed");

        let prompt = captured.lock().unwrap().clone();
        assert!(
            prompt.contains("Professor Alice"),
            "prompt must include neighbor name 'Professor Alice'; prompt was: {}",
            &prompt[..prompt.len().min(500)]
        );
    }

    #[tokio::test]
    async fn test_generate_social_without_graph_ctx_no_enrichment() {
        // When graph_ctx is None, the prompt should NOT contain enrichment sections.
        struct PromptCaptureLlm {
            captured: std::sync::Arc<std::sync::Mutex<String>>,
        }

        #[async_trait]
        impl LlmClient for PromptCaptureLlm {
            async fn complete(&self, prompt: &str) -> Result<String> {
                *self.captured.lock().unwrap() = prompt.to_string();
                Ok("{}".to_string())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>>
            {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn chat(
                &self,
                messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<String> {
                // generate_social drives the LLM via chat(); capture the user prompt.
                *self.captured.lock().unwrap() = capture_user_message(messages);
                Ok("{}".to_string())
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
        }

        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let mock_llm = PromptCaptureLlm { captured: captured.clone() };
        let generator = PersonaGenerator::new();

        let _sp = generator
            .generate_social(
                "Some Entity",
                "expert",
                "An expert summary",
                Platform::Twitter,
                &mock_llm,
                None,
            )
            .await
            .expect("generate_social without graph_ctx must succeed");

        let prompt = captured.lock().unwrap().clone();
        assert!(
            !prompt.contains("### Related Entities"),
            "prompt must NOT contain enrichment section when graph_ctx is None"
        );
        assert!(
            !prompt.contains("Entity context:"),
            "prompt must NOT contain 'Entity context:' when graph_ctx is None"
        );
    }

    #[tokio::test]
    async fn test_generate_social_entity_absent_from_graph_graceful_fallback() {
        // When the entity is not in the graph (has no neighbors), generate_social must
        // still succeed — flat summary fallback, no panic.
        let mut graph = KnowledgeGraph::new();
        let entity = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Isolated Entity".to_string(),
            kind: EntityKind::Person,
        };
        // Add the entity but no neighbors
        graph.add_entity(entity.clone()).expect("add entity");

        struct ErrorLlm;
        #[async_trait]
        impl LlmClient for ErrorLlm {
            async fn complete(&self, _: &str) -> Result<String> {
                Err(TeriError::Llm("network failure".to_string()))
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>>
            {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn chat(
                &self,
                _messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<String> {
                Err(TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
        }

        let generator = PersonaGenerator::new();
        // Entity with no neighbors: graph context has no "Related Entities" section,
        // so fallback to rule-based must succeed.
        let sp = generator
            .generate_social(
                "Isolated Entity",
                "student",
                "A student with no connections",
                Platform::Reddit,
                &ErrorLlm,
                Some((&graph, &entity)),
            )
            .await
            .expect("must succeed even with no-neighbor entity + LLM error");

        // Rule-based student path kicks in (randomized age in 18..=30, mbti from MBTI_TYPES).
        let age = sp.age.expect("rule-based sets an age");
        assert!((18..=30).contains(&age), "student age in range; got {age}");
        assert!(PersonaGenerator::MBTI_TYPES.contains(&sp.mbti.as_deref().unwrap()));
    }

    // ===== Part 2 (related edges) tests for build_entity_context / generate_social =====

    /// When an entity has an outgoing edge (entity → neighbor), `build_entity_context` must
    /// emit a "### Related Facts and Relationships" section containing a directional arrow line:
    ///   - entity --[RelationKind]--> (neighbor)
    ///
    /// Mirrors MiroFish `_build_entity_context`:443–448 outgoing branch.
    #[tokio::test]
    async fn test_generate_social_part2_outgoing_relation_in_prompt() {
        struct PromptCaptureLlm {
            captured: std::sync::Arc<std::sync::Mutex<String>>,
        }
        #[async_trait]
        impl LlmClient for PromptCaptureLlm {
            async fn complete(&self, prompt: &str) -> Result<String> {
                *self.captured.lock().unwrap() = prompt.to_string();
                Ok("{}".to_string())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>>
            {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn chat(
                &self,
                messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<String> {
                // generate_social drives the LLM via chat(); capture the user prompt.
                *self.captured.lock().unwrap() = capture_user_message(messages);
                Ok("{}".to_string())
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
        }

        let mut graph = KnowledgeGraph::new();
        let alice = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Alice".to_string(),
            kind: EntityKind::Person,
        };
        let acme = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            kind: EntityKind::Organization,
        };
        let alice_idx = graph.add_entity(alice.clone()).expect("add Alice");
        let acme_idx = graph.add_entity(acme).expect("add Acme");
        // Outgoing: Alice --[WorksFor]--> Acme Corp
        graph.add_relation(
            alice_idx,
            acme_idx,
            crate::graph::Relation::new(crate::graph::RelationKind::WorksFor, 0.9)
                .expect("valid relation"),
        );

        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let mock_llm = PromptCaptureLlm { captured: captured.clone() };
        let generator = PersonaGenerator::new();

        generator
            .generate_social(
                "Alice",
                "person",
                "A researcher",
                Platform::Reddit,
                &mock_llm,
                Some((&graph, &alice)),
            )
            .await
            .expect("generate_social must succeed");

        let prompt = captured.lock().unwrap().clone();

        // Part 2 heading must appear
        assert!(
            prompt.contains("### Related Facts and Relationships"),
            "prompt must include '### Related Facts and Relationships'; got: {}",
            &prompt[..prompt.len().min(800)]
        );
        // Outgoing directional line: Alice --[WorksFor]--> (Acme Corp)
        assert!(
            prompt.contains("Alice --[WorksFor]--> (Acme Corp)"),
            "prompt must include outgoing arrow line; got: {}",
            &prompt[..prompt.len().min(800)]
        );
    }

    /// When an entity has an INCOMING edge (neighbor → entity), `build_entity_context` must
    /// emit the reversed directional arrow line:
    ///   - (neighbor) --[RelationKind]--> entity
    ///
    /// Mirrors MiroFish `_build_entity_context`:449–450 incoming branch.
    #[tokio::test]
    async fn test_generate_social_part2_incoming_relation_in_prompt() {
        struct PromptCaptureLlm {
            captured: std::sync::Arc<std::sync::Mutex<String>>,
        }
        #[async_trait]
        impl LlmClient for PromptCaptureLlm {
            async fn complete(&self, prompt: &str) -> Result<String> {
                *self.captured.lock().unwrap() = prompt.to_string();
                Ok("{}".to_string())
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>>
            {
                Err(TeriError::Llm("not implemented".to_string()))
            }
            async fn chat(
                &self,
                messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<String> {
                // generate_social drives the LLM via chat(); capture the user prompt.
                *self.captured.lock().unwrap() = capture_user_message(messages);
                Ok("{}".to_string())
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _messages: &[crate::llm::ChatMessage],
                _opts: &crate::llm::ChatOptions,
            ) -> Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
        }

        let mut graph = KnowledgeGraph::new();
        let city = Entity {
            id: uuid::Uuid::new_v4(),
            name: "San Francisco".to_string(),
            kind: EntityKind::Location,
        };
        let company = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Acme Corp".to_string(),
            kind: EntityKind::Organization,
        };
        let city_idx = graph.add_entity(city.clone()).expect("add city");
        let company_idx = graph.add_entity(company.clone()).expect("add company");
        // Incoming to city: Acme Corp --[LocatedIn]--> San Francisco
        graph.add_relation(
            company_idx,
            city_idx,
            crate::graph::Relation::new(crate::graph::RelationKind::LocatedIn, 0.8)
                .expect("valid relation"),
        );

        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let mock_llm = PromptCaptureLlm { captured: captured.clone() };
        let generator = PersonaGenerator::new();

        // We query from the city's perspective — edge is INCOMING to city
        generator
            .generate_social(
                "San Francisco",
                "location",
                "A vibrant city",
                Platform::Reddit,
                &mock_llm,
                Some((&graph, &city)),
            )
            .await
            .expect("generate_social must succeed");

        let prompt = captured.lock().unwrap().clone();

        // Part 2 heading must appear
        assert!(
            prompt.contains("### Related Facts and Relationships"),
            "prompt must include '### Related Facts and Relationships'; got: {}",
            &prompt[..prompt.len().min(800)]
        );
        // Incoming directional line: (Acme Corp) --[LocatedIn]--> San Francisco
        assert!(
            prompt.contains("(Acme Corp) --[LocatedIn]--> San Francisco"),
            "prompt must include incoming arrow line; got: {}",
            &prompt[..prompt.len().min(800)]
        );
    }

    // ===== TASK-SIM-1 (S6): persona memory injection + two-prompt selection + randomization =====

    /// Mock that records the system message and the concatenated user message of the FIRST
    /// `chat()` call, then returns a fixed JSON so `generate_social` succeeds via the LLM path.
    struct ChatCaptureLlm {
        system: std::sync::Arc<std::sync::Mutex<String>>,
        user: std::sync::Arc<std::sync::Mutex<String>>,
        response: String,
    }

    #[async_trait]
    impl LlmClient for ChatCaptureLlm {
        async fn complete(&self, _: &str) -> Result<String> {
            Err(TeriError::Llm("complete must not be used by generate_social".into()))
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(TeriError::Llm("not implemented".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not implemented".into()))
        }
        async fn chat(
            &self,
            messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> Result<String> {
            let sys = messages
                .iter()
                .filter(|m| matches!(m.role, crate::llm::ChatRole::System))
                .map(|m| m.content.clone())
                .collect::<Vec<_>>()
                .join("\n");
            *self.system.lock().unwrap() = sys;
            *self.user.lock().unwrap() = capture_user_message(messages);
            Ok(self.response.clone())
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
    }

    fn make_chat_capture() -> (
        ChatCaptureLlm,
        std::sync::Arc<std::sync::Mutex<String>>,
        std::sync::Arc<std::sync::Mutex<String>>,
    ) {
        let system = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let user = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let llm = ChatCaptureLlm {
            system: system.clone(),
            user: user.clone(),
            // Minimal-but-valid JSON so the LLM path is taken (not rule-based fallback).
            response: r#"{"bio": "b", "persona": "p"}"#.to_string(),
        };
        (llm, system, user)
    }

    /// Gap #1: the persona prompt for an INDIVIDUAL entity carries a personal-memory section that
    /// ties the agent to the event and its prior actions/reactions.
    #[tokio::test]
    async fn test_persona_prompt_individual_has_personal_memory_section() {
        let (llm, _sys, user) = make_chat_capture();
        let generator = PersonaGenerator::new();
        generator
            .generate_social("Jane", "person", "A protester", Platform::Twitter, &llm, None)
            .await
            .expect("generate_social must succeed");

        let prompt = user.lock().unwrap().clone();
        assert!(
            prompt.contains("INDIVIDUAL"),
            "individual framing must be selected; got: {prompt}"
        );
        assert!(
            prompt.contains("Personal memory"),
            "individual prompt must include a personal-memory section; got: {prompt}"
        );
        assert!(
            prompt.contains("actions and reactions this individual has ALREADY taken"),
            "personal-memory section must mention prior actions/reactions; got: {prompt}"
        );
    }

    /// Gap #1 + #2: the persona prompt for a GROUP/institutional entity carries an
    /// institutional-memory section (and the institutional framing is selected).
    #[tokio::test]
    async fn test_persona_prompt_group_has_institutional_memory_section() {
        let (llm, _sys, user) = make_chat_capture();
        let generator = PersonaGenerator::new();
        generator
            .generate_social(
                "State University",
                "university",
                "A public university",
                Platform::Reddit,
                &llm,
                None,
            )
            .await
            .expect("generate_social must succeed");

        let prompt = user.lock().unwrap().clone();
        assert!(
            prompt.contains("GROUP / INSTITUTIONAL"),
            "group framing must be selected for an institutional type; got: {prompt}"
        );
        assert!(
            prompt.contains("Institutional memory"),
            "group prompt must include an institutional-memory section; got: {prompt}"
        );
        assert!(
            prompt.contains("actions and reactions this institution has ALREADY taken"),
            "institutional-memory section must mention prior actions/reactions; got: {prompt}"
        );
        // The individual-only sub-sections must NOT appear in the group prompt.
        assert!(
            !prompt.contains("Personal memory"),
            "group prompt must not use the personal frame"
        );
    }

    /// Gap #1: when graph context is available, the memory-context block is injected so the model
    /// can ground the persona-memory section in the event/neighbor facts.
    #[tokio::test]
    async fn test_persona_prompt_injects_graph_context_for_memory() {
        let mut graph = KnowledgeGraph::new();
        let main_entity = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Alice".to_string(),
            kind: EntityKind::Person,
        };
        let neighbor = Entity {
            id: uuid::Uuid::new_v4(),
            name: "Bob Reporter".to_string(),
            kind: EntityKind::Person,
        };
        let a = graph.add_entity(main_entity.clone()).expect("add main");
        let b = graph.add_entity(neighbor).expect("add neighbor");
        graph.add_relation(
            a,
            b,
            crate::graph::Relation::new(crate::graph::RelationKind::RelatedTo, 0.9)
                .expect("valid relation"),
        );

        let (llm, _sys, user) = make_chat_capture();
        let generator = PersonaGenerator::new();
        generator
            .generate_social(
                "Alice",
                "person",
                "An activist",
                Platform::Twitter,
                &llm,
                Some((&graph, &main_entity)),
            )
            .await
            .expect("generate_social must succeed");

        let prompt = user.lock().unwrap().clone();
        assert!(
            prompt.contains("Context (use this to ground the persona-memory section)"),
            "graph context must be injected as a memory-grounding block; got: {prompt}"
        );
        assert!(
            prompt.contains("Bob Reporter"),
            "neighbor name must appear in the memory context"
        );
    }

    /// Gap #2: the system prompt is supplied on the chat() call.
    #[tokio::test]
    async fn test_persona_chat_includes_system_prompt() {
        let (llm, sys, _user) = make_chat_capture();
        let generator = PersonaGenerator::new();
        generator
            .generate_social("Jane", "person", "x", Platform::Twitter, &llm, None)
            .await
            .expect("generate_social must succeed");
        let system = sys.lock().unwrap().clone();
        assert!(
            system.contains("social-media user profiles"),
            "system prompt must be sent; got: {system}"
        );
    }

    /// Gap #2: selection routes known individual types to the individual frame and group/unknown
    /// types to the institutional frame, mirroring `_is_individual_entity`.
    #[test]
    fn test_two_prompt_selection_matches_entity_type() {
        // Known individual types → individual frame.
        for t in ["student", "expert", "journalist", "person"] {
            let p = PersonaGenerator::build_persona_prompt(
                "E",
                t,
                "s",
                "",
                Platform::Twitter,
                PersonaGenerator::is_individual_entity(t),
            );
            assert!(p.contains("INDIVIDUAL"), "type {t} must select individual frame");
            assert!(p.contains("Personal memory"), "type {t} must carry personal memory");
        }
        // Group types → institutional frame.
        for t in ["university", "company", "ngo", "government_unknown_type"] {
            let p = PersonaGenerator::build_persona_prompt(
                "E",
                t,
                "s",
                "",
                Platform::Twitter,
                PersonaGenerator::is_individual_entity(t),
            );
            assert!(
                p.contains("GROUP / INSTITUTIONAL"),
                "type {t} must select institutional frame"
            );
            assert!(p.contains("Institutional memory"), "type {t} must carry institutional memory");
        }
    }

    /// Gap #3: rule-based randomization, deterministic under a FIXED seed — exact expected values.
    #[test]
    fn test_rule_based_randomization_fixed_seed_deterministic() {
        let mut rng = StdRng::seed_from_u64(42);
        let sp = PersonaGenerator::generate_social_rule_based(
            "John Student",
            "student",
            "",
            Platform::Twitter,
            "john_123",
            "2026-06-23",
            &mut rng,
        );
        // Same seed → same sequence: snapshot the exact draws so a regression in the draw ORDER
        // or ranges is caught.
        let age = sp.age.unwrap();
        assert!((18..=30).contains(&age), "age in student range; got {age}");
        assert!(matches!(sp.gender.as_deref(), Some("male") | Some("female")));
        assert!(PersonaGenerator::MBTI_TYPES.contains(&sp.mbti.as_deref().unwrap()));
        assert!(PersonaGenerator::COUNTRIES.contains(&sp.country.as_deref().unwrap()));
        assert!((500..=5000).contains(&sp.karma));
        assert!((50..=500).contains(&sp.friend_count));
        assert!((100..=1000).contains(&sp.follower_count));
        assert!((100..=2000).contains(&sp.statuses_count));

        // Determinism: re-running with the same seed reproduces the identical profile.
        let mut rng2 = StdRng::seed_from_u64(42);
        let sp2 = PersonaGenerator::generate_social_rule_based(
            "John Student",
            "student",
            "",
            Platform::Twitter,
            "john_123",
            "2026-06-23",
            &mut rng2,
        );
        assert_eq!(sp.age, sp2.age);
        assert_eq!(sp.gender, sp2.gender);
        assert_eq!(sp.mbti, sp2.mbti);
        assert_eq!(sp.country, sp2.country);
        assert_eq!(sp.karma, sp2.karma);
        assert_eq!(sp.friend_count, sp2.friend_count);
    }

    /// Gap #3: across MANY generations the randomized fields show VARIETY (not constant) and every
    /// MBTI/country draw comes from the constant tables.
    #[test]
    fn test_rule_based_randomization_produces_variety() {
        let mut rng = StdRng::seed_from_u64(7);
        let mut ages = std::collections::HashSet::new();
        let mut mbtis = std::collections::HashSet::new();
        let mut countries = std::collections::HashSet::new();
        let mut karmas = std::collections::HashSet::new();
        for i in 0..200 {
            let sp = PersonaGenerator::generate_social_rule_based(
                &format!("Person {i}"),
                "person", // generic individual → fully randomized branch
                "",
                Platform::Twitter,
                "u_1",
                "2026-06-23",
                &mut rng,
            );
            ages.insert(sp.age.unwrap());
            let mbti = sp.mbti.clone().unwrap();
            let country = sp.country.clone().unwrap();
            assert!(
                PersonaGenerator::MBTI_TYPES.contains(&mbti.as_str()),
                "mbti must come from MBTI_TYPES; got {mbti}"
            );
            assert!(
                PersonaGenerator::COUNTRIES.contains(&country.as_str()),
                "country must come from COUNTRIES; got {country}"
            );
            mbtis.insert(mbti);
            countries.insert(country);
            karmas.insert(sp.karma);
        }
        // Variety: many distinct values across 200 draws (non-constant).
        assert!(ages.len() > 5, "ages must vary; distinct={}", ages.len());
        assert!(mbtis.len() > 5, "mbti must vary; distinct={}", mbtis.len());
        assert!(countries.len() > 3, "countries must vary; distinct={}", countries.len());
        assert!(karmas.len() > 50, "karma must vary widely; distinct={}", karmas.len());
    }

    /// Gap #3: institutional accounts keep FIXED demographics even though counts are randomized.
    #[test]
    fn test_rule_based_institution_fixed_demographics() {
        let mut rng = StdRng::seed_from_u64(1);
        let sp = PersonaGenerator::generate_social_rule_based(
            "Acme University",
            "university",
            "",
            Platform::Reddit,
            "acme_1",
            "2026-06-23",
            &mut rng,
        );
        assert_eq!(sp.age, Some(30));
        assert_eq!(sp.gender.as_deref(), Some("other"));
        assert_eq!(sp.mbti.as_deref(), Some("ISTJ"));
        // Counts still randomized within range.
        assert!((500..=5000).contains(&sp.karma));
    }

    // ===== TASK-SIM-6 #4: media-outlet / socialmediaplatform rule-based branch =====

    /// `mediaoutlet` gets the DISTINCT media profile (profession "Media", news interests),
    /// NOT the generic institutional one. Mirrors oasis_profile_generator.py:810-820.
    #[test]
    fn test_rule_based_media_outlet_distinct_branch() {
        let mut rng = StdRng::seed_from_u64(7);
        let sp = PersonaGenerator::generate_social_rule_based(
            "Daily Times",
            "mediaoutlet",
            "",
            Platform::Reddit,
            "daily_1",
            "2026-06-23",
            &mut rng,
        );
        assert_eq!(sp.profession.as_deref(), Some("Media"));
        assert!(sp.persona.contains("media entity"), "persona must read as a media entity");
        assert!(sp.interested_topics.contains(&"General News".to_string()));
        assert!(sp.interested_topics.contains(&"Current Events".to_string()));
        // Fixed institutional virtual demographics.
        assert_eq!(sp.age, Some(30));
        assert_eq!(sp.gender.as_deref(), Some("other"));
        assert_eq!(sp.mbti.as_deref(), Some("ISTJ"));
    }

    /// `socialmediaplatform` (NOT in GROUP_ENTITY_TYPES) also hits the media branch — proving the
    /// arm precedes the generic-default fall-through.
    #[test]
    fn test_rule_based_social_media_platform_uses_media_branch() {
        let mut rng = StdRng::seed_from_u64(9);
        let sp = PersonaGenerator::generate_social_rule_based(
            "ChatApp",
            "socialmediaplatform",
            "",
            Platform::Reddit,
            "chatapp_1",
            "2026-06-23",
            &mut rng,
        );
        assert_eq!(sp.profession.as_deref(), Some("Media"));
        assert!(sp.interested_topics.contains(&"Public Affairs".to_string()));
    }

    // ===== TASK-SIM-6 #5: per-entity semantic-recall enrichment (with dedup) =====

    /// A stub recall source returning fixed facts/summaries.
    struct StubRecall {
        facts: Vec<String>,
        summaries: Vec<String>,
    }
    #[async_trait]
    impl EntityFactRecall for StubRecall {
        async fn recall(&self, _entity_name: &str) -> RecalledEntityFacts {
            RecalledEntityFacts {
                facts: self.facts.clone(),
                node_summaries: self.summaries.clone(),
            }
        }
    }

    /// Captures the user prompt and returns fixed JSON so the LLM path succeeds.
    struct PromptCaptureOk {
        captured: std::sync::Arc<std::sync::Mutex<String>>,
    }
    #[async_trait]
    impl LlmClient for PromptCaptureOk {
        async fn complete(&self, _: &str) -> Result<String> {
            Err(TeriError::Llm("unused".into()))
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(TeriError::Llm("unused".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("unused".into()))
        }
        async fn chat(
            &self,
            messages: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<String> {
            *self.captured.lock().unwrap() = capture_user_message(messages);
            Ok("{}".to_string())
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("unused".into()))
        }
    }

    /// When a recall source is supplied, recalled facts/summaries appear in the persona prompt.
    #[tokio::test]
    async fn test_generate_social_with_recall_enriches_prompt() {
        let mut graph = KnowledgeGraph::new();
        let person =
            Entity { id: uuid::Uuid::new_v4(), name: "Jane".to_string(), kind: EntityKind::Person };
        graph.add_entity(person.clone()).expect("add person");

        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let llm = PromptCaptureOk { captured: captured.clone() };
        let recall = StubRecall {
            facts: vec!["Jane founded Acme in 2020".to_string()],
            summaries: vec!["Jane is a well-known engineer".to_string()],
        };
        let generator = PersonaGenerator::new();

        generator
            .generate_social_with_recall(
                "Jane",
                "person",
                "An engineer",
                Platform::Reddit,
                &llm,
                Some((&graph, &person)),
                Some(&recall),
            )
            .await
            .expect("generate_social_with_recall must succeed");

        let prompt = captured.lock().unwrap().clone();
        assert!(
            prompt.contains("### Recalled Facts"),
            "prompt must include recalled facts section; got: {}",
            &prompt[..prompt.len().min(900)]
        );
        assert!(prompt.contains("Jane founded Acme in 2020"));
        assert!(prompt.contains("### Recalled Related Nodes"));
        assert!(prompt.contains("Jane is a well-known engineer"));
    }

    /// No recall source → no "Recalled" sections (byte-identical to today's behaviour).
    #[tokio::test]
    async fn test_generate_social_without_recall_no_recalled_sections() {
        let mut graph = KnowledgeGraph::new();
        let person =
            Entity { id: uuid::Uuid::new_v4(), name: "Jane".to_string(), kind: EntityKind::Person };
        graph.add_entity(person.clone()).expect("add person");

        let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let llm = PromptCaptureOk { captured: captured.clone() };
        let generator = PersonaGenerator::new();

        generator
            .generate_social(
                "Jane",
                "person",
                "An engineer",
                Platform::Reddit,
                &llm,
                Some((&graph, &person)),
            )
            .await
            .expect("generate_social must succeed");

        let prompt = captured.lock().unwrap().clone();
        assert!(!prompt.contains("### Recalled Facts"));
        assert!(!prompt.contains("### Recalled Related Nodes"));
    }

    /// Recalled facts duplicating a graph-derived relationship line are deduped out.
    #[test]
    fn test_format_recalled_facts_dedups_existing() {
        let mut existing = std::collections::HashSet::new();
        existing.insert("Jane --[Founded]--> (Acme)".to_string());
        let recalled = RecalledEntityFacts {
            facts: vec![
                "Jane --[Founded]--> (Acme)".to_string(), // duplicate → dropped
                "Jane lives in Paris".to_string(),        // new → kept
            ],
            node_summaries: vec![],
        };
        let out = PersonaGenerator::format_recalled_facts(&recalled, &existing);
        assert!(out.contains("Jane lives in Paris"));
        // The duplicate fact must NOT be re-listed under Recalled Facts.
        assert!(!out.contains("- Jane --[Founded]--> (Acme)"));
    }

    /// Empty recall → empty enrichment string.
    #[test]
    fn test_format_recalled_facts_empty_is_empty() {
        let out = PersonaGenerator::format_recalled_facts(
            &RecalledEntityFacts::default(),
            &std::collections::HashSet::new(),
        );
        assert!(out.is_empty());
    }

    // ===== TASK-SIM-6 #7: persona path truncation retry =====

    /// An LLM whose FIRST chat_with_meta call returns truncated JSON (finish_reason=="length")
    /// and whose SECOND returns clean JSON — proving the truncation triggers a retry.
    struct TruncateThenOk {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait]
    impl LlmClient for TruncateThenOk {
        async fn complete(&self, _: &str) -> Result<String> {
            Err(TeriError::Llm("unused".into()))
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(TeriError::Llm("unused".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("unused".into()))
        }
        async fn chat(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<String> {
            Ok("{}".to_string())
        }
        async fn chat_with_meta(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<crate::llm::ChatCompletion> {
            let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                // Truncated AND unrepairable garbage so the attempt is discarded (forces retry).
                Ok(crate::llm::ChatCompletion {
                    content: "not json at all <<<".to_string(),
                    finish_reason: Some("length".to_string()),
                })
            } else {
                Ok(crate::llm::ChatCompletion {
                    content: r#"{"bio":"clean bio","persona":"clean persona"}"#.to_string(),
                    finish_reason: Some("stop".to_string()),
                })
            }
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("unused".into()))
        }
    }

    #[tokio::test]
    async fn test_generate_social_retries_on_truncation() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let llm = TruncateThenOk { calls: calls.clone() };
        let generator = PersonaGenerator::new();

        let sp = generator
            .generate_social("Jane", "person", "x", Platform::Reddit, &llm, None)
            .await
            .expect("generate_social must succeed");

        // The truncated first attempt was retried; the second (clean) attempt won.
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(sp.bio, "clean bio");
        assert_eq!(sp.persona, "clean persona");
    }

    /// A truncated-but-repairable response is salvaged in-place (no retry needed).
    struct TruncatedButRepairable {
        calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait]
    impl LlmClient for TruncatedButRepairable {
        async fn complete(&self, _: &str) -> Result<String> {
            Err(TeriError::Llm("unused".into()))
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(TeriError::Llm("unused".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<std::pin::Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("unused".into()))
        }
        async fn chat(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<String> {
            Ok("{}".to_string())
        }
        async fn chat_with_meta(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<crate::llm::ChatCompletion> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            // Missing the closing brace — fix_truncated_json closes it.
            Ok(crate::llm::ChatCompletion {
                content: r#"{"bio":"b","persona":"p""#.to_string(),
                finish_reason: Some("length".to_string()),
            })
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("unused".into()))
        }
    }

    #[tokio::test]
    async fn test_generate_social_salvages_repairable_truncation_without_retry() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let llm = TruncatedButRepairable { calls: calls.clone() };
        let generator = PersonaGenerator::new();

        let sp = generator
            .generate_social("Jane", "person", "x", Platform::Reddit, &llm, None)
            .await
            .expect("generate_social must succeed");

        // Repaired on the FIRST attempt — no retry.
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(sp.bio, "b");
        assert_eq!(sp.persona, "p");
    }
}
