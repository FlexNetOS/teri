//! Simulation configuration data model.
//!
//! Port of `backend/app/services/simulation_config_generator.py` lines 1-197 (MiroFish).
//! Covers: `CHINA_TIMEZONE_CONFIG` const, and the five dataclasses
//! `AgentActivityConfig`, `TimeSimulationConfig`, `EventConfig`, `PlatformConfig`,
//! `SimulationParameters` — plus `to_dict()` / `to_json()` on `SimulationParameters`.
//!
//! Sub-cycle (a) of U-019; the `SimulationConfigGenerator` class is NOT ported here.
//!
//! # Symbols ported: S-374..S-429
//!
//! ## Serialization order faithfulness
//! Python's `dataclasses.asdict()` preserves declaration order. All structs in this
//! module declare their fields in the SAME order as the Python dataclass. `serde`
//! serializes struct fields in declaration order (the order the fields appear in the
//! Rust source), which therefore matches asdict() output exactly.
//!
//! ## `generated_at` default
//! Python uses `datetime.now().isoformat()` — local naive, µs omitted when zero.
//! We reuse `crate::models::project::python_isoformat_local()` which was made
//! `pub(crate)` specifically for this purpose.
//!
//! ## `ensure_ascii=False`
//! `serde_json` serializes to UTF-8 by default; Chinese characters are NOT escaped.
//! This matches `json.dumps(..., ensure_ascii=False)`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::project::python_isoformat_local;

// ---------------------------------------------------------------------------
// CHINA_TIMEZONE_CONFIG (S-374)
// ---------------------------------------------------------------------------

/// China timezone / work-schedule configuration (Beijing time).
///
/// Port of `CHINA_TIMEZONE_CONFIG` dict (L29-48 in the Python source).
/// Returned as a `serde_json::Value` so that the exact JSON shape — including
/// key order and numeric types — matches the Python dict.  Later sub-cycles
/// that consume this const receive a `Value` they can embed directly.
///
/// The observable JSON shape:
/// ```json
/// {
///   "dead_hours":   [0,1,2,3,4,5],
///   "morning_hours":[6,7,8],
///   "work_hours":   [9,10,11,12,13,14,15,16,17,18],
///   "peak_hours":   [19,20,21,22],
///   "night_hours":  [23],
///   "activity_multipliers": {
///     "dead":0.05, "morning":0.4, "work":0.7, "peak":1.5, "night":0.5
///   }
/// }
/// ```
pub fn china_timezone_config() -> Value {
    serde_json::json!({
        "dead_hours":    [0, 1, 2, 3, 4, 5],
        "morning_hours": [6, 7, 8],
        "work_hours":    [9, 10, 11, 12, 13, 14, 15, 16, 17, 18],
        "peak_hours":    [19, 20, 21, 22],
        "night_hours":   [23],
        "activity_multipliers": {
            "dead":    0.05,
            "morning": 0.4,
            "work":    0.7,
            "peak":    1.5,
            "night":   0.5
        }
    })
}

// ---------------------------------------------------------------------------
// AgentActivityConfig (S-375..S-388)
// ---------------------------------------------------------------------------

fn default_active_hours() -> Vec<i64> {
    // Python: list(range(8, 23)) — 8 inclusive, 23 EXCLUSIVE → [8..=22], 15 elements.
    (8..23).collect()
}

fn default_activity_level() -> f64 {
    0.5
}

fn default_posts_per_hour() -> f64 {
    1.0
}

fn default_comments_per_hour() -> f64 {
    2.0
}

fn default_response_delay_min() -> i64 {
    5
}

fn default_response_delay_max() -> i64 {
    60
}

fn default_sentiment_bias() -> f64 {
    0.0
}

fn default_stance() -> String {
    "neutral".to_string()
}

fn default_influence_weight() -> f64 {
    1.0
}

/// Per-agent activity configuration.
///
/// Port of `AgentActivityConfig` dataclass (L51-80).  Fields are declared in
/// the same order as the Python dataclass so that `serde`'s serialization order
/// matches Python's `asdict()` output exactly.
///
/// `agent_id`, `entity_uuid`, `entity_name`, `entity_type` are required (no
/// default); all other fields have the exact Python defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentActivityConfig {
    /// S-376 — required, no default.
    pub agent_id: i64,
    /// S-377 — required, no default.
    pub entity_uuid: String,
    /// S-378 — required, no default.
    pub entity_name: String,
    /// S-379 — required, no default.
    pub entity_type: String,

    /// S-380 — `activity_level: float = 0.5`
    #[serde(default = "default_activity_level")]
    pub activity_level: f64,

    /// S-381 — `posts_per_hour: float = 1.0`
    #[serde(default = "default_posts_per_hour")]
    pub posts_per_hour: f64,

    /// S-382 — `comments_per_hour: float = 2.0`
    #[serde(default = "default_comments_per_hour")]
    pub comments_per_hour: f64,

    /// S-383 — `active_hours: List[int] = field(default_factory=lambda: list(range(8, 23)))`
    /// = [8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22]  (15 elements; 23 exclusive)
    #[serde(default = "default_active_hours")]
    pub active_hours: Vec<i64>,

    /// S-384 — `response_delay_min: int = 5`
    #[serde(default = "default_response_delay_min")]
    pub response_delay_min: i64,

    /// S-385 — `response_delay_max: int = 60`
    #[serde(default = "default_response_delay_max")]
    pub response_delay_max: i64,

    /// S-386 — `sentiment_bias: float = 0.0`
    #[serde(default = "default_sentiment_bias")]
    pub sentiment_bias: f64,

    /// S-387 — `stance: str = "neutral"`
    #[serde(default = "default_stance")]
    pub stance: String,

    /// S-388 — `influence_weight: float = 1.0`
    #[serde(default = "default_influence_weight")]
    pub influence_weight: f64,
}

impl AgentActivityConfig {
    /// Construct with the four required fields; all optional fields take their Python defaults.
    pub fn new(
        agent_id: i64,
        entity_uuid: impl Into<String>,
        entity_name: impl Into<String>,
        entity_type: impl Into<String>,
    ) -> Self {
        Self {
            agent_id,
            entity_uuid: entity_uuid.into(),
            entity_name: entity_name.into(),
            entity_type: entity_type.into(),
            activity_level: default_activity_level(),
            posts_per_hour: default_posts_per_hour(),
            comments_per_hour: default_comments_per_hour(),
            active_hours: default_active_hours(),
            response_delay_min: default_response_delay_min(),
            response_delay_max: default_response_delay_max(),
            sentiment_bias: default_sentiment_bias(),
            stance: default_stance(),
            influence_weight: default_influence_weight(),
        }
    }
}

// ---------------------------------------------------------------------------
// TimeSimulationConfig (S-389..S-401)
// ---------------------------------------------------------------------------

fn default_total_simulation_hours() -> i64 {
    72
}

fn default_minutes_per_round() -> i64 {
    60
}

fn default_agents_per_hour_min() -> i64 {
    5
}

fn default_agents_per_hour_max() -> i64 {
    20
}

fn default_peak_hours() -> Vec<i64> {
    vec![19, 20, 21, 22]
}

fn default_peak_activity_multiplier() -> f64 {
    1.5
}

fn default_off_peak_hours() -> Vec<i64> {
    vec![0, 1, 2, 3, 4, 5]
}

fn default_off_peak_activity_multiplier() -> f64 {
    0.05
}

fn default_morning_hours() -> Vec<i64> {
    vec![6, 7, 8]
}

fn default_morning_activity_multiplier() -> f64 {
    0.4
}

fn default_work_hours() -> Vec<i64> {
    vec![9, 10, 11, 12, 13, 14, 15, 16, 17, 18]
}

fn default_work_activity_multiplier() -> f64 {
    0.7
}

/// Time-simulation configuration based on Chinese work/life patterns.
///
/// Port of `TimeSimulationConfig` dataclass (L83-110).  All fields have defaults.
/// Field order matches Python declaration order for `asdict()` parity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeSimulationConfig {
    /// S-390 — `total_simulation_hours: int = 72`
    #[serde(default = "default_total_simulation_hours")]
    pub total_simulation_hours: i64,

    /// S-391 — `minutes_per_round: int = 60`
    #[serde(default = "default_minutes_per_round")]
    pub minutes_per_round: i64,

    /// S-392 — `agents_per_hour_min: int = 5`
    #[serde(default = "default_agents_per_hour_min")]
    pub agents_per_hour_min: i64,

    /// S-393 — `agents_per_hour_max: int = 20`
    #[serde(default = "default_agents_per_hour_max")]
    pub agents_per_hour_max: i64,

    /// S-394 — `peak_hours: List[int] = field(default_factory=lambda: [19,20,21,22])`
    #[serde(default = "default_peak_hours")]
    pub peak_hours: Vec<i64>,

    /// S-395 — `peak_activity_multiplier: float = 1.5`
    #[serde(default = "default_peak_activity_multiplier")]
    pub peak_activity_multiplier: f64,

    /// S-396 — `off_peak_hours: List[int] = field(default_factory=lambda: [0,1,2,3,4,5])`
    #[serde(default = "default_off_peak_hours")]
    pub off_peak_hours: Vec<i64>,

    /// S-397 — `off_peak_activity_multiplier: float = 0.05`
    #[serde(default = "default_off_peak_activity_multiplier")]
    pub off_peak_activity_multiplier: f64,

    /// S-398 — `morning_hours: List[int] = field(default_factory=lambda: [6,7,8])`
    #[serde(default = "default_morning_hours")]
    pub morning_hours: Vec<i64>,

    /// S-399 — `morning_activity_multiplier: float = 0.4`
    #[serde(default = "default_morning_activity_multiplier")]
    pub morning_activity_multiplier: f64,

    /// S-400 — `work_hours: List[int] = field(default_factory=lambda: [9,10,...,18])`
    #[serde(default = "default_work_hours")]
    pub work_hours: Vec<i64>,

    /// S-401 — `work_activity_multiplier: float = 0.7`
    #[serde(default = "default_work_activity_multiplier")]
    pub work_activity_multiplier: f64,
}

impl Default for TimeSimulationConfig {
    fn default() -> Self {
        Self {
            total_simulation_hours: default_total_simulation_hours(),
            minutes_per_round: default_minutes_per_round(),
            agents_per_hour_min: default_agents_per_hour_min(),
            agents_per_hour_max: default_agents_per_hour_max(),
            peak_hours: default_peak_hours(),
            peak_activity_multiplier: default_peak_activity_multiplier(),
            off_peak_hours: default_off_peak_hours(),
            off_peak_activity_multiplier: default_off_peak_activity_multiplier(),
            morning_hours: default_morning_hours(),
            morning_activity_multiplier: default_morning_activity_multiplier(),
            work_hours: default_work_hours(),
            work_activity_multiplier: default_work_activity_multiplier(),
        }
    }
}

// ---------------------------------------------------------------------------
// EventConfig (S-402..S-406)
// ---------------------------------------------------------------------------

/// Event configuration for a simulation.
///
/// Port of `EventConfig` dataclass (L113-126).  All fields default to empty.
/// Field order matches Python declaration order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EventConfig {
    /// S-403 — `initial_posts: List[Dict[str, Any]] = field(default_factory=list)`
    #[serde(default)]
    pub initial_posts: Vec<Value>,

    /// S-404 — `scheduled_events: List[Dict[str, Any]] = field(default_factory=list)`
    #[serde(default)]
    pub scheduled_events: Vec<Value>,

    /// S-405 — `hot_topics: List[str] = field(default_factory=list)`
    #[serde(default)]
    pub hot_topics: Vec<String>,

    /// S-406 — `narrative_direction: str = ""`
    #[serde(default)]
    pub narrative_direction: String,
}

// ---------------------------------------------------------------------------
// PlatformConfig (S-407..S-413)
// ---------------------------------------------------------------------------

fn default_recency_weight() -> f64 {
    0.4
}

fn default_popularity_weight() -> f64 {
    0.3
}

fn default_relevance_weight() -> f64 {
    0.3
}

fn default_viral_threshold() -> i64 {
    10
}

fn default_echo_chamber_strength() -> f64 {
    0.5
}

/// Platform-specific algorithm and propagation configuration.
///
/// Port of `PlatformConfig` dataclass (L129-143).  `platform` is required;
/// all other fields have Python defaults.  Field order matches Python declaration order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlatformConfig {
    /// S-408 — `platform: str` (required, no default — "twitter" or "reddit")
    pub platform: String,

    /// S-409 — `recency_weight: float = 0.4`
    #[serde(default = "default_recency_weight")]
    pub recency_weight: f64,

    /// S-410 — `popularity_weight: float = 0.3`
    #[serde(default = "default_popularity_weight")]
    pub popularity_weight: f64,

    /// S-411 — `relevance_weight: float = 0.3`
    #[serde(default = "default_relevance_weight")]
    pub relevance_weight: f64,

    /// S-412 — `viral_threshold: int = 10`
    #[serde(default = "default_viral_threshold")]
    pub viral_threshold: i64,

    /// S-413 — `echo_chamber_strength: float = 0.5`
    #[serde(default = "default_echo_chamber_strength")]
    pub echo_chamber_strength: f64,
}

impl PlatformConfig {
    /// Construct with the required `platform` field; all optional fields take Python defaults.
    pub fn new(platform: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            recency_weight: default_recency_weight(),
            popularity_weight: default_popularity_weight(),
            relevance_weight: default_relevance_weight(),
            viral_threshold: default_viral_threshold(),
            echo_chamber_strength: default_echo_chamber_strength(),
        }
    }
}

// ---------------------------------------------------------------------------
// SimulationParameters (S-414..S-429)
// ---------------------------------------------------------------------------

fn default_time_config() -> TimeSimulationConfig {
    TimeSimulationConfig::default()
}

fn default_agent_configs() -> Vec<AgentActivityConfig> {
    Vec::new()
}

fn default_event_config() -> EventConfig {
    EventConfig::default()
}

fn default_llm_model() -> String {
    String::new()
}

fn default_llm_base_url() -> String {
    String::new()
}

fn default_generated_at() -> String {
    python_isoformat_local()
}

fn default_generation_reasoning() -> String {
    String::new()
}

/// Complete simulation parameter configuration.
///
/// Port of `SimulationParameters` dataclass (L146-197).
/// `simulation_id`, `project_id`, `graph_id`, `simulation_requirement` are required.
/// All other fields have Python defaults.  Field order matches Python declaration order
/// so that `to_dict()` / `asdict()` output is byte-identical.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationParameters {
    /// S-415 — required.
    pub simulation_id: String,
    /// S-416 — required.
    pub project_id: String,
    /// S-417 — required.
    pub graph_id: String,
    /// S-418 — required.
    pub simulation_requirement: String,

    /// S-419 — `time_config: TimeSimulationConfig = field(default_factory=TimeSimulationConfig)`
    #[serde(default = "default_time_config")]
    pub time_config: TimeSimulationConfig,

    /// S-420 — `agent_configs: List[AgentActivityConfig] = field(default_factory=list)`
    #[serde(default = "default_agent_configs")]
    pub agent_configs: Vec<AgentActivityConfig>,

    /// S-421 — `event_config: EventConfig = field(default_factory=EventConfig)`
    #[serde(default = "default_event_config")]
    pub event_config: EventConfig,

    /// S-422 — `twitter_config: Optional[PlatformConfig] = None`
    #[serde(default)]
    pub twitter_config: Option<PlatformConfig>,

    /// S-423 — `reddit_config: Optional[PlatformConfig] = None`
    #[serde(default)]
    pub reddit_config: Option<PlatformConfig>,

    /// S-424 — `llm_model: str = ""`
    #[serde(default = "default_llm_model")]
    pub llm_model: String,

    /// S-425 — `llm_base_url: str = ""`
    #[serde(default = "default_llm_base_url")]
    pub llm_base_url: String,

    /// S-426 — `generated_at: str = field(default_factory=lambda: datetime.now().isoformat())`
    /// Reuses `python_isoformat_local()` for local-naive, µs-omission-when-zero semantics.
    #[serde(default = "default_generated_at")]
    pub generated_at: String,

    /// S-427 — `generation_reasoning: str = ""`
    #[serde(default = "default_generation_reasoning")]
    pub generation_reasoning: String,
}

impl SimulationParameters {
    /// Construct with the four required fields; all optional fields take their Python defaults,
    /// including `generated_at` = `datetime.now().isoformat()` (local naive timestamp).
    pub fn new(
        simulation_id: impl Into<String>,
        project_id: impl Into<String>,
        graph_id: impl Into<String>,
        simulation_requirement: impl Into<String>,
    ) -> Self {
        Self {
            simulation_id: simulation_id.into(),
            project_id: project_id.into(),
            graph_id: graph_id.into(),
            simulation_requirement: simulation_requirement.into(),
            time_config: TimeSimulationConfig::default(),
            agent_configs: Vec::new(),
            event_config: EventConfig::default(),
            twitter_config: None,
            reddit_config: None,
            llm_model: String::new(),
            llm_base_url: String::new(),
            generated_at: python_isoformat_local(),
            generation_reasoning: String::new(),
        }
    }

    /// Convert to a `serde_json::Value` map with EXACTLY the 13 keys in declaration order.
    ///
    /// Port of `SimulationParameters.to_dict()` (L176-193).
    ///
    /// Key order: simulation_id, project_id, graph_id, simulation_requirement,
    /// time_config, agent_configs, event_config, twitter_config, reddit_config,
    /// llm_model, llm_base_url, generated_at, generation_reasoning.
    ///
    /// Each nested struct is serialized via `serde_json::to_value` which produces the
    /// same recursive dict that Python's `dataclasses.asdict()` would (preserving field
    /// declaration order within each struct).
    ///
    /// S-428
    pub fn to_dict(&self) -> Value {
        // Use serde_json::Map to guarantee insertion order (it is a LinkedHashMap
        // under the hood, preserving insertion order).
        let mut map = serde_json::Map::with_capacity(13);

        map.insert(
            "simulation_id".to_string(),
            Value::String(self.simulation_id.clone()),
        );
        map.insert(
            "project_id".to_string(),
            Value::String(self.project_id.clone()),
        );
        map.insert(
            "graph_id".to_string(),
            Value::String(self.graph_id.clone()),
        );
        map.insert(
            "simulation_requirement".to_string(),
            Value::String(self.simulation_requirement.clone()),
        );
        map.insert(
            "time_config".to_string(),
            serde_json::to_value(&self.time_config)
                .expect("TimeSimulationConfig is always serializable"),
        );
        map.insert(
            "agent_configs".to_string(),
            serde_json::to_value(&self.agent_configs)
                .expect("Vec<AgentActivityConfig> is always serializable"),
        );
        map.insert(
            "event_config".to_string(),
            serde_json::to_value(&self.event_config)
                .expect("EventConfig is always serializable"),
        );
        map.insert(
            "twitter_config".to_string(),
            serde_json::to_value(&self.twitter_config)
                .expect("Option<PlatformConfig> is always serializable"),
        );
        map.insert(
            "reddit_config".to_string(),
            serde_json::to_value(&self.reddit_config)
                .expect("Option<PlatformConfig> is always serializable"),
        );
        map.insert(
            "llm_model".to_string(),
            Value::String(self.llm_model.clone()),
        );
        map.insert(
            "llm_base_url".to_string(),
            Value::String(self.llm_base_url.clone()),
        );
        map.insert(
            "generated_at".to_string(),
            Value::String(self.generated_at.clone()),
        );
        map.insert(
            "generation_reasoning".to_string(),
            Value::String(self.generation_reasoning.clone()),
        );

        Value::Object(map)
    }

    /// Serialize to a 2-space-indented JSON string with raw UTF-8 (no \uXXXX escapes).
    ///
    /// Port of `SimulationParameters.to_json(indent=2)` (L195-197):
    ///   `json.dumps(self.to_dict(), ensure_ascii=False, indent=2)`
    ///
    /// `serde_json::to_string_pretty` uses 2-space indentation and does NOT escape
    /// non-ASCII characters (UTF-8 raw output), exactly matching `ensure_ascii=False`.
    ///
    /// S-429
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.to_dict())
            .expect("SimulationParameters::to_dict() always produces a valid Value")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- CHINA_TIMEZONE_CONFIG ---

    #[test]
    fn china_timezone_config_shape() {
        let cfg = china_timezone_config();
        let obj = cfg.as_object().expect("should be a JSON object");

        // Key existence
        assert!(obj.contains_key("dead_hours"));
        assert!(obj.contains_key("morning_hours"));
        assert!(obj.contains_key("work_hours"));
        assert!(obj.contains_key("peak_hours"));
        assert!(obj.contains_key("night_hours"));
        assert!(obj.contains_key("activity_multipliers"));

        // dead_hours = [0,1,2,3,4,5]
        let dead: Vec<i64> = serde_json::from_value(obj["dead_hours"].clone()).unwrap();
        assert_eq!(dead, vec![0, 1, 2, 3, 4, 5]);

        // morning_hours = [6,7,8]
        let morning: Vec<i64> = serde_json::from_value(obj["morning_hours"].clone()).unwrap();
        assert_eq!(morning, vec![6, 7, 8]);

        // work_hours = [9..=18] (10 elements)
        let work: Vec<i64> = serde_json::from_value(obj["work_hours"].clone()).unwrap();
        assert_eq!(work, vec![9, 10, 11, 12, 13, 14, 15, 16, 17, 18]);

        // peak_hours = [19,20,21,22]
        let peak: Vec<i64> = serde_json::from_value(obj["peak_hours"].clone()).unwrap();
        assert_eq!(peak, vec![19, 20, 21, 22]);

        // night_hours = [23]
        let night: Vec<i64> = serde_json::from_value(obj["night_hours"].clone()).unwrap();
        assert_eq!(night, vec![23]);

        // activity_multipliers
        let mults = obj["activity_multipliers"]
            .as_object()
            .expect("should be an object");
        assert_eq!(mults["dead"].as_f64().unwrap(), 0.05);
        assert_eq!(mults["morning"].as_f64().unwrap(), 0.4);
        assert_eq!(mults["work"].as_f64().unwrap(), 0.7);
        assert_eq!(mults["peak"].as_f64().unwrap(), 1.5);
        assert_eq!(mults["night"].as_f64().unwrap(), 0.5);
    }

    #[test]
    fn china_timezone_config_key_order() {
        // Verify that the 6 top-level keys appear in declaration order in the JSON string.
        let json = serde_json::to_string(&china_timezone_config()).unwrap();
        let positions = [
            "dead_hours",
            "morning_hours",
            "work_hours",
            "peak_hours",
            "night_hours",
            "activity_multipliers",
        ]
        .iter()
        .map(|k| json.find(k).expect("key must be present"))
        .collect::<Vec<_>>();

        for w in positions.windows(2) {
            assert!(
                w[0] < w[1],
                "key order violated in CHINA_TIMEZONE_CONFIG JSON"
            );
        }
    }

    // --- AgentActivityConfig ---

    #[test]
    fn agent_activity_config_defaults() {
        let cfg = AgentActivityConfig::new(1, "uuid-1", "Alice", "user");

        assert_eq!(cfg.agent_id, 1);
        assert_eq!(cfg.entity_uuid, "uuid-1");
        assert_eq!(cfg.entity_name, "Alice");
        assert_eq!(cfg.entity_type, "user");

        // Python defaults
        assert_eq!(cfg.activity_level, 0.5);
        assert_eq!(cfg.posts_per_hour, 1.0);
        assert_eq!(cfg.comments_per_hour, 2.0);

        // active_hours = list(range(8, 23)) — 23 EXCLUSIVE → 15 elements, last is 22
        assert_eq!(cfg.active_hours.len(), 15, "active_hours must have 15 elements");
        assert_eq!(cfg.active_hours[0], 8);
        assert_eq!(*cfg.active_hours.last().unwrap(), 22, "last active hour must be 22, not 23");
        assert!(
            !cfg.active_hours.contains(&23),
            "23 must NOT be in active_hours (range is exclusive)"
        );
        assert_eq!(cfg.active_hours, (8..23).collect::<Vec<i64>>());

        assert_eq!(cfg.response_delay_min, 5);
        assert_eq!(cfg.response_delay_max, 60);
        assert_eq!(cfg.sentiment_bias, 0.0);
        assert_eq!(cfg.stance, "neutral");
        assert_eq!(cfg.influence_weight, 1.0);
    }

    #[test]
    fn agent_activity_config_serde_round_trip() {
        let cfg = AgentActivityConfig::new(42, "u-42", "Bob", "kol");
        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: AgentActivityConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.agent_id, 42);
        assert_eq!(decoded.active_hours.len(), 15);
        assert_eq!(decoded.stance, "neutral");
    }

    // --- TimeSimulationConfig ---

    #[test]
    fn time_simulation_config_defaults() {
        let cfg = TimeSimulationConfig::default();

        assert_eq!(cfg.total_simulation_hours, 72);
        assert_eq!(cfg.minutes_per_round, 60);
        assert_eq!(cfg.agents_per_hour_min, 5);
        assert_eq!(cfg.agents_per_hour_max, 20);
        assert_eq!(cfg.peak_hours, vec![19, 20, 21, 22]);
        assert_eq!(cfg.peak_activity_multiplier, 1.5);
        assert_eq!(cfg.off_peak_hours, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(cfg.off_peak_activity_multiplier, 0.05);
        assert_eq!(cfg.morning_hours, vec![6, 7, 8]);
        assert_eq!(cfg.morning_activity_multiplier, 0.4);
        assert_eq!(cfg.work_hours, vec![9, 10, 11, 12, 13, 14, 15, 16, 17, 18]);
        assert_eq!(cfg.work_activity_multiplier, 0.7);
    }

    #[test]
    fn time_simulation_config_serde_round_trip() {
        let cfg = TimeSimulationConfig::default();
        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: TimeSimulationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, cfg);
    }

    // --- EventConfig ---

    #[test]
    fn event_config_defaults() {
        let cfg = EventConfig::default();
        assert!(cfg.initial_posts.is_empty());
        assert!(cfg.scheduled_events.is_empty());
        assert!(cfg.hot_topics.is_empty());
        assert_eq!(cfg.narrative_direction, "");
    }

    #[test]
    fn event_config_serde_round_trip() {
        let mut cfg = EventConfig::default();
        cfg.hot_topics.push("人工智能".to_string()); // Chinese UTF-8
        let json = serde_json::to_string(&cfg).unwrap();
        // ensure_ascii=False: Chinese must NOT be \uXXXX-escaped
        assert!(
            json.contains("人工智能"),
            "Chinese characters must be raw UTF-8 in JSON, got: {json}"
        );
        let decoded: EventConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.hot_topics[0], "人工智能");
    }

    // --- PlatformConfig ---

    #[test]
    fn platform_config_defaults() {
        let cfg = PlatformConfig::new("twitter");
        assert_eq!(cfg.platform, "twitter");
        assert_eq!(cfg.recency_weight, 0.4);
        assert_eq!(cfg.popularity_weight, 0.3);
        assert_eq!(cfg.relevance_weight, 0.3);
        assert_eq!(cfg.viral_threshold, 10);
        assert_eq!(cfg.echo_chamber_strength, 0.5);
    }

    #[test]
    fn platform_config_serde_round_trip() {
        let cfg = PlatformConfig::new("reddit");
        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: PlatformConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.platform, "reddit");
        assert_eq!(decoded.viral_threshold, 10);
    }

    // --- SimulationParameters ---

    #[test]
    fn simulation_parameters_defaults() {
        let p = SimulationParameters::new("sim-1", "proj-1", "graph-1", "Test requirement");

        assert_eq!(p.simulation_id, "sim-1");
        assert_eq!(p.project_id, "proj-1");
        assert_eq!(p.graph_id, "graph-1");
        assert_eq!(p.simulation_requirement, "Test requirement");

        // Nested defaults
        assert_eq!(p.time_config.total_simulation_hours, 72);
        assert!(p.agent_configs.is_empty());
        assert!(p.event_config.initial_posts.is_empty());
        assert!(p.twitter_config.is_none());
        assert!(p.reddit_config.is_none());
        assert_eq!(p.llm_model, "");
        assert_eq!(p.llm_base_url, "");
        assert!(!p.generated_at.is_empty(), "generated_at must be populated");
        assert_eq!(p.generation_reasoning, "");
    }

    #[test]
    fn simulation_parameters_to_dict_key_count_and_order() {
        let p = SimulationParameters::new("s", "p", "g", "req");
        let dict = p.to_dict();
        let obj = dict.as_object().expect("to_dict() must return a JSON object");

        // Exactly 13 keys
        assert_eq!(obj.len(), 13, "to_dict() must emit exactly 13 keys");

        // Key order must match Python's declaration order
        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        let expected = [
            "simulation_id",
            "project_id",
            "graph_id",
            "simulation_requirement",
            "time_config",
            "agent_configs",
            "event_config",
            "twitter_config",
            "reddit_config",
            "llm_model",
            "llm_base_url",
            "generated_at",
            "generation_reasoning",
        ];
        assert_eq!(keys, expected, "to_dict() key order must match Python's to_dict()");
    }

    #[test]
    fn simulation_parameters_to_dict_nested_shapes() {
        let p = SimulationParameters::new("s", "p", "g", "req");
        let dict = p.to_dict();
        let obj = dict.as_object().unwrap();

        // time_config is an object with the right fields
        let tc = obj["time_config"].as_object().expect("time_config must be an object");
        assert_eq!(tc["total_simulation_hours"].as_i64().unwrap(), 72);
        assert_eq!(tc["minutes_per_round"].as_i64().unwrap(), 60);

        // agent_configs is an empty array
        assert_eq!(obj["agent_configs"].as_array().unwrap().len(), 0);

        // event_config is an object
        let ec = obj["event_config"].as_object().expect("event_config must be an object");
        assert!(ec["initial_posts"].as_array().unwrap().is_empty());

        // twitter_config and reddit_config are null (Python None → null)
        assert!(obj["twitter_config"].is_null());
        assert!(obj["reddit_config"].is_null());
    }

    #[test]
    fn simulation_parameters_to_dict_with_platform_configs() {
        let mut p = SimulationParameters::new("s", "p", "g", "req");
        p.twitter_config = Some(PlatformConfig::new("twitter"));
        p.reddit_config = Some(PlatformConfig::new("reddit"));

        let dict = p.to_dict();
        let obj = dict.as_object().unwrap();

        // twitter_config: not null, platform field present
        let tw = obj["twitter_config"].as_object().expect("twitter_config must be an object");
        assert_eq!(tw["platform"].as_str().unwrap(), "twitter");
        assert_eq!(tw["viral_threshold"].as_i64().unwrap(), 10);

        let rd = obj["reddit_config"].as_object().expect("reddit_config must be an object");
        assert_eq!(rd["platform"].as_str().unwrap(), "reddit");
    }

    #[test]
    fn simulation_parameters_to_json_two_space_indent() {
        let p = SimulationParameters::new("s", "p", "g", "req");
        let json = p.to_json();

        // Must be valid JSON
        let _: Value = serde_json::from_str(&json).expect("to_json() must produce valid JSON");

        // Must use 2-space indentation (serde_json::to_string_pretty)
        assert!(
            json.contains("  \"simulation_id\""),
            "to_json() must use 2-space indentation"
        );
    }

    #[test]
    fn simulation_parameters_to_json_raw_utf8() {
        let mut p = SimulationParameters::new("s", "p", "g", "需求描述"); // Chinese
        p.event_config.narrative_direction = "舆论引导".to_string();

        let json = p.to_json();

        // Chinese must be raw UTF-8, not \uXXXX (ensure_ascii=False)
        assert!(
            json.contains("需求描述"),
            "Chinese in simulation_requirement must be raw UTF-8, got: {json}"
        );
        assert!(
            json.contains("舆论引导"),
            "Chinese in narrative_direction must be raw UTF-8, got: {json}"
        );
        // Must not contain escaped Chinese
        assert!(
            !json.contains("\\u9700"),
            "JSON must not contain \\u-escaped Chinese characters"
        );
    }

    #[test]
    fn simulation_parameters_to_json_round_trips() {
        let mut p = SimulationParameters::new("sim-42", "proj-7", "graph-3", "test");
        p.llm_model = "gpt-4o".to_string();
        p.twitter_config = Some(PlatformConfig::new("twitter"));

        let json = p.to_json();
        // Round-trip via serde
        let decoded: SimulationParameters = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.simulation_id, "sim-42");
        assert_eq!(decoded.llm_model, "gpt-4o");
        assert!(decoded.twitter_config.is_some());
    }

    #[test]
    fn simulation_parameters_to_dict_agent_configs_nested() {
        let mut p = SimulationParameters::new("s", "p", "g", "r");
        p.agent_configs.push(AgentActivityConfig::new(1, "u1", "Alice", "user"));

        let dict = p.to_dict();
        let obj = dict.as_object().unwrap();
        let agents = obj["agent_configs"].as_array().unwrap();
        assert_eq!(agents.len(), 1);

        let agent = agents[0].as_object().unwrap();
        assert_eq!(agent["agent_id"].as_i64().unwrap(), 1);
        assert_eq!(agent["entity_name"].as_str().unwrap(), "Alice");
        assert_eq!(agent["stance"].as_str().unwrap(), "neutral");

        let ah = agent["active_hours"].as_array().unwrap();
        assert_eq!(ah.len(), 15);
        assert_eq!(ah.last().unwrap().as_i64().unwrap(), 22);
    }

    #[test]
    fn simulation_parameters_generated_at_is_isoformat() {
        let p = SimulationParameters::new("s", "p", "g", "r");
        // Must match Python isoformat shape: YYYY-MM-DDTHH:MM:SS or YYYY-MM-DDTHH:MM:SS.ffffff
        // No timezone suffix (local naive).
        let ts = &p.generated_at;
        assert!(ts.len() >= 19, "generated_at must be at least 19 chars: {ts}");
        assert_eq!(&ts[4..5], "-", "generated_at must be ISO format: {ts}");
        assert_eq!(&ts[7..8], "-", "generated_at must be ISO format: {ts}");
        assert_eq!(&ts[10..11], "T", "generated_at must have T separator: {ts}");
        // No timezone suffix
        assert!(
            !ts.ends_with('Z') && !ts.contains('+') && !ts.contains(" UTC"),
            "generated_at must be local naive (no tz suffix): {ts}"
        );
    }
}
