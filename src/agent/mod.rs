use crate::error::{Result, TeriError};
use crate::graph::{Entity, KnowledgeGraph};
use crate::llm::LlmClient;
use crate::sim::{Action, SocialAction, TargetKind, WorldState};
use chrono::Utc;
use minijinja::{Environment, context};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persona {
    pub name: String,
    pub background: String,
    pub traits: Vec<String>,
    pub role: String,
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

    /// Pure read phase of a step: retrieve context, call LLM, return validated action.
    /// Does NOT mutate agent state or memory — safe to call concurrently across agents.
    /// Pair with `commit_action` to complete the step.
    pub async fn prepare_action<L: LlmClient>(
        &self,
        world: &WorldState,
        llm: &L,
    ) -> Result<Action> {
        let relevant_memories = self.retrieve_relevant_memories(world);
        let context = self.construct_context(world, &relevant_memories);
        let action_str = self.generate_action_with_fallback(&context, llm).await?;
        self.parse_and_validate_action(&action_str)
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

        // Construct context from world state + memories
        let context = self.construct_context(world, &relevant_memories);

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

    /// Construct context string from world state and memories
    fn construct_context(&self, world: &WorldState, memories: &[&MemoryEntry]) -> String {
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
                return Ok(Action::Social(sa));
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
            "CREATE_POST" => Some(SocialAction::CreatePost {
                content: get_arg(content, "content"),
            }),
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
            "REPOST" => Some(SocialAction::Repost {
                post_id: get_arg(content, "post_id"),
            }),
            "QUOTE_POST" => Some(SocialAction::Quote {
                post_id: get_arg(content, "post_id"),
                content: get_arg(content, "content"),
            }),
            "FOLLOW" => Some(SocialAction::Follow {
                user_id: get_arg(content, "user_id"),
            }),
            "CREATE_COMMENT" => Some(SocialAction::Comment {
                post_id: get_arg(content, "post_id"),
                content: get_arg(content, "content"),
            }),
            "SEARCH_POSTS" => Some(SocialAction::SearchPosts {
                query: get_arg(content, "query"),
            }),
            "SEARCH_USER" => Some(SocialAction::SearchUser {
                query: get_arg(content, "query"),
            }),
            "MUTE" => Some(SocialAction::Mute {
                user_id: get_arg(content, "user_id"),
            }),
            "TREND" | "trend" => Some(SocialAction::Trend),
            "DO_NOTHING" => Some(SocialAction::DoNothing),
            _ => None,
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
                    SocialAction::CreatePost { content } => {
                        (format!("Posted: {}", content), 0.85)
                    }
                    // Medium-high: social graph modifications
                    SocialAction::Follow { user_id } => {
                        (format!("Followed user: {}", user_id), 0.75)
                    }
                    SocialAction::Mute { user_id } => {
                        (format!("Muted user: {}", user_id), 0.75)
                    }
                    // Medium: content amplification
                    SocialAction::Repost { post_id } => {
                        (format!("Reposted: {}", post_id), 0.65)
                    }
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

        // Convert HashMap to Vec of tuples for MiniJinja iteration
        let world_variables_seq: Vec<(String, f32)> = world_variables.into_iter().collect();

        let template_context = context! {
            agent_name => &agent.persona.name,
            agent_role => &agent.persona.role,
            agent_state => format!("{:?}", agent.state),
            agent_background => &agent.persona.background,
            agent_traits => &agent.persona.traits,
            world_tick => world_tick,
            recent_events => recent_events,
            relevant_memories => relevant_memories,
            world_variables => world_variables_seq,
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
    }

    #[test]
    fn test_agent_creation() {
        let persona = Persona {
            name: "Alice".to_string(),
            background: "A curious researcher".to_string(),
            traits: vec!["analytical".to_string(), "creative".to_string()],
            role: "Analyst".to_string(),
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
        };
        assert!(generator.validate_persona(&invalid_persona).is_err());

        // Test empty background
        let invalid_persona = Persona {
            name: "Valid Name".to_string(),
            background: "".to_string(),
            traits: vec!["valid".to_string()],
            role: "Valid role".to_string(),
        };
        assert!(generator.validate_persona(&invalid_persona).is_err());

        // Test too many traits
        let invalid_persona = Persona {
            name: "Valid Name".to_string(),
            background: "Valid background".to_string(),
            traits: (0..11).map(|i| format!("trait_{}", i)).collect(), // 11 traits
            role: "Valid role".to_string(),
        };
        assert!(generator.validate_persona(&invalid_persona).is_err());

        // Test valid persona
        let valid_persona = Persona {
            name: "Valid Name".to_string(),
            background: "Valid background".to_string(),
            traits: vec!["trait1".to_string(), "trait2".to_string()],
            role: "Valid role".to_string(),
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
        }

        let mock_llm = ErrorMockLlm;
        let persona = Persona {
            name: "TestAgent".to_string(),
            background: "A test agent".to_string(),
            traits: vec!["test".to_string()],
            role: "Tester".to_string(),
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
        })
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
}
