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

use std::collections::HashMap;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, TeriError};
use crate::i18n::get_language_instruction;
use crate::llm::{ChatMessage, ChatOptions, LlmClient};
use crate::models::project::python_isoformat_local;
use crate::services::entity_reader::EntityNode;

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

        map.insert("simulation_id".to_string(), Value::String(self.simulation_id.clone()));
        map.insert("project_id".to_string(), Value::String(self.project_id.clone()));
        map.insert("graph_id".to_string(), Value::String(self.graph_id.clone()));
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
            serde_json::to_value(&self.event_config).expect("EventConfig is always serializable"),
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
        map.insert("llm_model".to_string(), Value::String(self.llm_model.clone()));
        map.insert("llm_base_url".to_string(), Value::String(self.llm_base_url.clone()));
        map.insert("generated_at".to_string(), Value::String(self.generated_at.clone()));
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
        let mults = obj["activity_multipliers"].as_object().expect("should be an object");
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
            assert!(w[0] < w[1], "key order violated in CHINA_TIMEZONE_CONFIG JSON");
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
        assert!(json.contains("  \"simulation_id\""), "to_json() must use 2-space indentation");
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

// ===========================================================================
// SimulationConfigGenerator (S-430..S-449)
//
// Port of `backend/app/services/simulation_config_generator.py` L200-727
// (MiroFish).  Covers: struct + class constants + constructor + context/LLM
// foundation + time/event stages.
//
// NOT ported here (later sub-cycles):
//   _assign_initial_post_agents (S-450)
//   _generate_agent_configs_batch (S-451)
//   _generate_agent_config_by_rule (S-452)
//   generate_config (S-439) — orchestrates all stages including the above
//
// # finish_reason decision
//
// Python's `_call_llm_with_retry` detects truncation via `finish_reason ==
// "length"` and immediately applies `_fix_truncated_json` before any parse
// attempt.  teri's `LlmClient::chat` returns a content `String`; it does NOT
// surface `finish_reason` (the OpenAI adapter extracts `.choices[0].message.
// content` and discards the finish_reason field — DECISION-7).
//
// Decision: we adopt strategy (a) — always attempt `_fix_truncated_json`
// salvage when the initial parse fails, which subsumes the truncation-detection
// case:
//
// 1. Call `chat` → raw `String`.
// 2. `serde_json::from_str` on raw → success → return.
// 3. Failure → run `_fix_truncated_json(raw)` → try parse → success → return.
// 4. Failure → run `_try_fix_config_json(raw)` → success → return.
// 5. Failure → record error, next attempt (with lower temperature).
//
// This loses NO salvage behaviour:
//   - Python's "finish_reason==length → fix → parse" path becomes step 3.
//   - Python's "parse fail → try_fix_config_json" path becomes step 4.
//   - All other repair paths are included.
//
// The only Python behaviour this can't reproduce is the case where truncated
// output parses as valid JSON but is semantically incomplete — but Python's own
// code wouldn't catch that either (it just returns the first successful parse).
// ===========================================================================

/// Simulation configuration generator.
///
/// Port of `SimulationConfigGenerator` (`simulation_config_generator.py:200`).
///
/// Uses an injected `LlmClient` (no direct OpenAI import); model_name and
/// base_url are stored for embedding into `SimulationParameters.llm_*` fields.
///
/// # Type parameter
/// `L` follows the `OntologyGenerator<L: LlmClient>` pattern from
/// `src/services/ontology.rs`.
///
/// S-430
pub struct SimulationConfigGenerator<L: LlmClient> {
    /// Injected LLM client.
    client: L,
    /// S-438 `self.model_name`
    pub model_name: String,
    /// S-438 `self.base_url`
    pub base_url: String,
}

impl<L: LlmClient> SimulationConfigGenerator<L> {
    // -----------------------------------------------------------------------
    // Class constants (S-431..S-437)
    // -----------------------------------------------------------------------

    /// S-431 — `MAX_CONTEXT_LENGTH = 50000`
    pub const MAX_CONTEXT_LENGTH: usize = 50_000;

    /// S-432 — `AGENTS_PER_BATCH = 15`
    pub const AGENTS_PER_BATCH: usize = 15;

    /// S-433 — `TIME_CONFIG_CONTEXT_LENGTH = 10000`
    pub const TIME_CONFIG_CONTEXT_LENGTH: usize = 10_000;

    /// S-434 — `EVENT_CONFIG_CONTEXT_LENGTH = 8000`
    pub const EVENT_CONFIG_CONTEXT_LENGTH: usize = 8_000;

    /// S-435 — `ENTITY_SUMMARY_LENGTH = 300`
    pub const ENTITY_SUMMARY_LENGTH: usize = 300;

    /// S-436 — `AGENT_SUMMARY_LENGTH = 300`
    pub const AGENT_SUMMARY_LENGTH: usize = 300;

    /// S-437 — `ENTITIES_PER_TYPE_DISPLAY = 20`
    pub const ENTITIES_PER_TYPE_DISPLAY: usize = 20;

    // -----------------------------------------------------------------------
    // S-438 — __init__
    // -----------------------------------------------------------------------

    /// Construct a `SimulationConfigGenerator`.
    ///
    /// Port of `SimulationConfigGenerator.__init__` (`simulation_config_generator.py:225-241`).
    ///
    /// In MiroFish the constructor reads `Config.LLM_*` env vars and builds an
    /// OpenAI client.  In teri the LLM client is injected; `model_name` and
    /// `base_url` are passed explicitly and stored for embedding in the output
    /// `SimulationParameters`.
    pub fn new(client: L, model_name: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self { client, model_name: model_name.into(), base_url: base_url.into() }
    }

    // -----------------------------------------------------------------------
    // S-440 — _build_context
    // -----------------------------------------------------------------------

    /// Build the LLM context string, capped to `MAX_CONTEXT_LENGTH` chars.
    ///
    /// Port of `SimulationConfigGenerator._build_context`
    /// (`simulation_config_generator.py:381-407`).
    ///
    /// Algorithm (all lengths in CHARS, not bytes — matching Python `len()` on str):
    /// 1. Generate entity summary via `_summarize_entities`.
    /// 2. Build context_parts: `## 模拟需求\n{req}` and `\n## 实体信息 ({n}个)\n{summary}`.
    /// 3. `remaining_length = MAX_CONTEXT_LENGTH - sum(char_len(parts)) - 500`.
    /// 4. If `remaining_length > 0` and `document_text` is non-empty:
    ///    a. Take first `remaining_length` chars of `document_text`.
    ///    b. If the original was longer, append `\n...(文档已截断)`.
    ///    c. Append `\n## 原始文档内容\n{doc_text}` to context_parts.
    /// 5. Join parts with `"\n"`.
    pub fn build_context(
        &self,
        simulation_requirement: &str,
        document_text: &str,
        entities: &[EntityNode],
    ) -> String {
        let entity_summary = self.summarize_entities(entities);

        let part1 = format!("## 模拟需求\n{simulation_requirement}");
        let part2 = format!("\n## 实体信息 ({}个)\n{entity_summary}", entities.len());

        let current_length: usize = part1.chars().count() + part2.chars().count();
        let remaining_length =
            (Self::MAX_CONTEXT_LENGTH as isize - current_length as isize - 500).max(0) as usize;

        let mut context_parts = vec![part1, part2];

        if remaining_length > 0 && !document_text.is_empty() {
            let doc_chars: Vec<char> = document_text.chars().collect();
            let truncated = doc_chars.len() > remaining_length;
            let doc_text: String = doc_chars.into_iter().take(remaining_length).collect();
            let doc_text =
                if truncated { format!("{doc_text}\n...(文档已截断)") } else { doc_text };
            context_parts.push(format!("\n## 原始文档内容\n{doc_text}"));
        }

        context_parts.join("\n")
    }

    // -----------------------------------------------------------------------
    // S-441 — _summarize_entities
    // -----------------------------------------------------------------------

    /// Generate a compact entity summary grouped by type.
    ///
    /// Port of `SimulationConfigGenerator._summarize_entities`
    /// (`simulation_config_generator.py:409-432`).
    ///
    /// Algorithm:
    /// 1. Group entities by `get_entity_type()` (default `"Unknown"`).
    /// 2. For each group:
    ///    a. Header line: `\n### {type} ({n}个)`.
    ///    b. Up to `ENTITIES_PER_TYPE_DISPLAY` entries (truncated to `ENTITY_SUMMARY_LENGTH` chars).
    ///    c. If more remain: `  ... 还有 {k} 个`.
    /// 3. Join all lines with `"\n"`.
    ///
    /// Summary truncation is CHAR-based (`.chars()`) to match Python `len()` on str,
    /// which counts Unicode scalar values (not bytes).  This is important for
    /// Chinese summaries where each character is 3 bytes in UTF-8.
    pub fn summarize_entities(&self, entities: &[EntityNode]) -> String {
        // Group by entity type, preserving insertion order (Python dict preserves insertion order
        // since 3.7; we match that by iterating entities once).
        let mut by_type: HashMap<String, Vec<&EntityNode>> = HashMap::new();
        let mut type_order: Vec<String> = Vec::new();

        for e in entities {
            let t = e.get_entity_type().unwrap_or_else(|| "Unknown".to_string());
            if !by_type.contains_key(&t) {
                type_order.push(t.clone());
                by_type.insert(t.clone(), Vec::new());
            }
            by_type.get_mut(&t).unwrap().push(e);
        }

        let mut lines: Vec<String> = Vec::new();

        for entity_type in &type_order {
            let type_entities = &by_type[entity_type];
            lines.push(format!("\n### {} ({}个)", entity_type, type_entities.len()));

            let display_count = Self::ENTITIES_PER_TYPE_DISPLAY;
            let summary_len = Self::ENTITY_SUMMARY_LENGTH;

            for e in type_entities.iter().take(display_count) {
                // CHAR-based truncation — Python: len(e.summary) > summary_len
                let char_count = e.summary.chars().count();
                let summary_preview = if char_count > summary_len {
                    // take first summary_len chars + "..."
                    let truncated: String = e.summary.chars().take(summary_len).collect();
                    format!("{truncated}...")
                } else {
                    e.summary.clone()
                };
                lines.push(format!("- {}: {summary_preview}", e.name));
            }

            if type_entities.len() > display_count {
                lines.push(format!("  ... 还有 {} 个", type_entities.len() - display_count));
            }
        }

        lines.join("\n")
    }

    // -----------------------------------------------------------------------
    // S-442 — _call_llm_with_retry
    // -----------------------------------------------------------------------

    /// Call the LLM with up to 3 attempts, descending temperature, and JSON salvage.
    ///
    /// Port of `SimulationConfigGenerator._call_llm_with_retry`
    /// (`simulation_config_generator.py:434-481`).
    ///
    /// # Algorithm
    /// - `max_attempts = 3`
    /// - Per attempt temperature: `0.7 - (attempt * 0.1)` (0.7, 0.6, 0.5)
    /// - Call `chat(messages, opts)` → raw `String`.
    /// - Step 1: try `serde_json::from_str(raw)` → success → return.
    /// - Step 2: run `fix_truncated_json(raw)` → try parse → success → return.
    ///   (Subsumes Python's finish_reason=="length" branch — see module docstring.)
    /// - Step 3: run `try_fix_config_json(raw)` → `Some(v)` → return.
    /// - On exception / all parse fails: `sleep(2 * (attempt + 1))` and retry.
    /// - After 3 exhausted attempts: return last error.
    ///
    /// Return type: `Result<Value>` (a `serde_json::Value::Object`).
    pub async fn call_llm_with_retry(&self, prompt: &str, system_prompt: &str) -> Result<Value> {
        let max_attempts = 3usize;
        let mut last_error: Option<TeriError> = None;

        for attempt in 0..max_attempts {
            let temperature = 0.7 - (attempt as f32 * 0.1);
            let messages = [ChatMessage::system(system_prompt), ChatMessage::user(prompt)];
            let opts = ChatOptions {
                temperature: Some(temperature),
                max_tokens: None,
                response_format: None,
            };

            match self.client.chat(&messages, &opts).await {
                Ok(raw) => {
                    // Step 1: direct parse
                    if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                        return Ok(v);
                    }

                    // Step 2: fix-truncated then parse
                    let fixed = Self::fix_truncated_json(&raw);
                    if let Ok(v) = serde_json::from_str::<Value>(&fixed) {
                        return Ok(v);
                    }

                    // Step 3: try_fix_config_json salvage
                    if let Some(v) = Self::try_fix_config_json(&raw) {
                        return Ok(v);
                    }

                    // All parse paths failed — treat as a soft error and retry
                    last_error = Some(TeriError::Config(format!(
                        "JSON parse failed after all repair attempts (attempt {})",
                        attempt + 1
                    )));
                    tokio::time::sleep(std::time::Duration::from_secs(2 * (attempt as u64 + 1)))
                        .await;
                }
                Err(e) => {
                    tracing::warn!(
                        "LLM调用失败 (attempt {}): {}",
                        attempt + 1,
                        &e.to_string()[..e.to_string().len().min(80)]
                    );
                    last_error = Some(e);
                    tokio::time::sleep(std::time::Duration::from_secs(2 * (attempt as u64 + 1)))
                        .await;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| TeriError::Config("LLM调用失败".into())))
    }

    // -----------------------------------------------------------------------
    // S-443 — _fix_truncated_json
    // -----------------------------------------------------------------------

    /// Repair a truncated JSON string by closing unbalanced braces/brackets.
    ///
    /// Port of `SimulationConfigGenerator._fix_truncated_json`
    /// (`simulation_config_generator.py:483-499`).
    ///
    /// Algorithm (operates on chars, not bytes):
    /// 1. Strip whitespace.
    /// 2. Count `open_braces = count('{') - count('}')`.
    /// 3. Count `open_brackets = count('[') - count(']')`.
    /// 4. If last char not in `{'"', '}', ']'}`: append `'"'`.
    /// 5. Append `']'` × `open_brackets`.
    /// 6. Append `'}'` × `open_braces`.
    pub fn fix_truncated_json(content: &str) -> String {
        let content = content.trim();

        let open_braces = content.chars().filter(|&c| c == '{').count() as isize
            - content.chars().filter(|&c| c == '}').count() as isize;
        let open_brackets = content.chars().filter(|&c| c == '[').count() as isize
            - content.chars().filter(|&c| c == ']').count() as isize;

        let mut result = content.to_string();

        // Python: if content and content[-1] not in '",}]'
        if let Some(last_ch) = result.chars().last()
            && last_ch != '"'
            && last_ch != ','
            && last_ch != '}'
            && last_ch != ']'
        {
            result.push('"');
        }

        for _ in 0..open_brackets.max(0) {
            result.push(']');
        }
        for _ in 0..open_braces.max(0) {
            result.push('}');
        }

        result
    }

    // -----------------------------------------------------------------------
    // S-444 — _try_fix_config_json
    // -----------------------------------------------------------------------

    /// Attempt multi-step JSON repair and return the parsed value if successful.
    ///
    /// Port of `SimulationConfigGenerator._try_fix_config_json`
    /// (`simulation_config_generator.py:501-533`).
    ///
    /// Algorithm:
    /// 1. `fix_truncated_json(content)`.
    /// 2. Regex-extract `\{[\s\S]*\}` (the outermost JSON object).
    /// 3. Replace newlines/collapse whitespace INSIDE string literals
    ///    (regex `"[^"\\]*(?:\\.[^"\\]*)*"` — matches JSON strings).
    /// 4. Try `serde_json::from_str`.
    /// 5. On failure: strip control chars `[\x00-\x1f\x7f-\x9f]`, collapse
    ///    whitespace, try again.
    /// 6. Return `Some(Value)` on any success, `None` on all failures.
    pub fn try_fix_config_json(content: &str) -> Option<Value> {
        // Step 1: fix truncation
        let content = Self::fix_truncated_json(content);

        // Step 2: extract the outermost JSON object
        let obj_re = Regex::new(r"\{[\s\S]*\}").expect("static regex");
        let json_str = obj_re.find(&content)?.as_str().to_string();

        // Step 3: fix newlines inside JSON string literals
        // Regex matches a JSON string (handles escaped chars)
        let str_re = Regex::new(r#""[^"\\]*(?:\\.[^"\\]*)*""#).expect("static regex");
        let ws_re = Regex::new(r"\s+").expect("static regex");

        let json_str = str_re
            .replace_all(&json_str, |caps: &regex::Captures| {
                let s = caps.get(0).unwrap().as_str();
                // Replace \n and \r with space inside the string literal
                let s = s.replace(['\n', '\r'], " ");
                // Collapse runs of whitespace inside the literal
                ws_re.replace_all(&s, " ").into_owned()
            })
            .into_owned();

        // Step 4: try parse
        if let Ok(v) = serde_json::from_str::<Value>(&json_str) {
            return Some(v);
        }

        // Step 5: strip control chars + collapse whitespace + retry
        let ctrl_re = Regex::new(r"[\x00-\x1f\x7f-\x9f]").expect("static regex");
        let json_str = ctrl_re.replace_all(&json_str, " ").into_owned();
        let json_str = ws_re.replace_all(&json_str, " ").into_owned();

        serde_json::from_str::<Value>(&json_str).ok()
    }

    // -----------------------------------------------------------------------
    // S-445 — _generate_time_config
    // -----------------------------------------------------------------------

    /// Call LLM to generate a time simulation configuration.
    ///
    /// Port of `SimulationConfigGenerator._generate_time_config`
    /// (`simulation_config_generator.py:535-595`).
    ///
    /// - Truncates `context` to `TIME_CONFIG_CONTEXT_LENGTH` chars.
    /// - `max_agents_allowed = max(1, int(num_entities * 0.9))`.
    /// - System prompt appends `get_language_instruction()`.
    /// - On LLM failure: falls back to `_get_default_time_config(num_entities)`.
    pub async fn generate_time_config(&self, context: &str, num_entities: usize) -> Value {
        // CHAR-based truncation — Python: context[:self.TIME_CONFIG_CONTEXT_LENGTH]
        let context_truncated: String =
            context.chars().take(Self::TIME_CONFIG_CONTEXT_LENGTH).collect();

        let max_agents_allowed = (num_entities as f64 * 0.9).max(1.0) as usize;

        let prompt = format!(
            r#"基于以下模拟需求，生成时间模拟配置。

{context_truncated}

## 任务
请生成时间配置JSON。

### 基本原则（仅供参考，需根据具体事件和参与群体灵活调整）：
- 请根据模拟场景推断目标用户群体所在时区和作息习惯，以下为东八区(UTC+8)的参考示例
- 凌晨0-5点几乎无人活动（活跃度系数0.05）
- 早上6-8点逐渐活跃（活跃度系数0.4）
- 工作时间9-18点中等活跃（活跃度系数0.7）
- 晚间19-22点是高峰期（活跃度系数1.5）
- 23点后活跃度下降（活跃度系数0.5）
- 一般规律：凌晨低活跃、早间渐增、工作时段中等、晚间高峰
- **重要**：以下示例值仅供参考，你需要根据事件性质、参与群体特点来调整具体时段
  - 例如：学生群体高峰可能是21-23点；媒体全天活跃；官方机构只在工作时间
  - 例如：突发热点可能导致深夜也有讨论，off_peak_hours 可适当缩短

### 返回JSON格式（不要markdown）

示例：
{{
    "total_simulation_hours": 72,
    "minutes_per_round": 60,
    "agents_per_hour_min": 5,
    "agents_per_hour_max": 50,
    "peak_hours": [19, 20, 21, 22],
    "off_peak_hours": [0, 1, 2, 3, 4, 5],
    "morning_hours": [6, 7, 8],
    "work_hours": [9, 10, 11, 12, 13, 14, 15, 16, 17, 18],
    "reasoning": "针对该事件的时间配置说明"
}}

字段说明：
- total_simulation_hours (int): 模拟总时长，24-168小时，突发事件短、持续话题长
- minutes_per_round (int): 每轮时长，30-120分钟，建议60分钟
- agents_per_hour_min (int): 每小时最少激活Agent数（取值范围: 1-{max_agents_allowed}）
- agents_per_hour_max (int): 每小时最多激活Agent数（取值范围: 1-{max_agents_allowed}）
- peak_hours (int数组): 高峰时段，根据事件参与群体调整
- off_peak_hours (int数组): 低谷时段，通常深夜凌晨
- morning_hours (int数组): 早间时段
- work_hours (int数组): 工作时段
- reasoning (string): 简要说明为什么这样配置"#
        );

        let lang_instruction = get_language_instruction();
        let system_prompt = format!(
            "你是社交媒体模拟专家。返回纯JSON格式，时间配置需符合模拟场景中目标用户群体的作息习惯。\n\n{lang_instruction}"
        );

        match self.call_llm_with_retry(&prompt, &system_prompt).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("时间配置LLM生成失败: {e}, 使用默认配置");
                self.get_default_time_config(num_entities)
            }
        }
    }

    // -----------------------------------------------------------------------
    // S-446 — _get_default_time_config
    // -----------------------------------------------------------------------

    /// Return the default time configuration (Chinese work/life schedule).
    ///
    /// Port of `SimulationConfigGenerator._get_default_time_config`
    /// (`simulation_config_generator.py:597-609`).
    pub fn get_default_time_config(&self, num_entities: usize) -> Value {
        serde_json::json!({
            "total_simulation_hours": 72,
            "minutes_per_round": 60,
            "agents_per_hour_min": (num_entities / 15).max(1),
            "agents_per_hour_max": (num_entities / 5).max(5),
            "peak_hours": [19, 20, 21, 22],
            "off_peak_hours": [0, 1, 2, 3, 4, 5],
            "morning_hours": [6, 7, 8],
            "work_hours": [9, 10, 11, 12, 13, 14, 15, 16, 17, 18],
            "reasoning": "使用默认中国人作息配置（每轮1小时）"
        })
    }

    // -----------------------------------------------------------------------
    // S-447 — _parse_time_config
    // -----------------------------------------------------------------------

    /// Parse an LLM time-config result dict into a `TimeSimulationConfig`.
    ///
    /// Port of `SimulationConfigGenerator._parse_time_config`
    /// (`simulation_config_generator.py:611-644`).
    ///
    /// Validation and clamping (preserving ALL branches):
    /// - `agents_per_hour_min` defaults to `max(1, num_entities // 15)`.
    /// - `agents_per_hour_max` defaults to `max(5, num_entities // 5)`.
    /// - If `min > num_entities`: correct to `max(1, num_entities // 10)`.
    /// - If `max > num_entities`: correct to `max(min + 1, num_entities // 2)`.
    /// - If `min >= max`: correct min to `max(1, max // 2)`.
    ///
    /// All other fields use direct extraction with Python defaults.
    pub fn parse_time_config(&self, result: &Value, num_entities: usize) -> TimeSimulationConfig {
        // Helper to extract an integer field
        let get_int = |key: &str, default: i64| -> i64 {
            result.get(key).and_then(Value::as_i64).unwrap_or(default)
        };
        let get_arr_i64 = |key: &str, default: Vec<i64>| -> Vec<i64> {
            result
                .get(key)
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(Value::as_i64).collect())
                .unwrap_or(default)
        };

        let default_min = (num_entities / 15).max(1) as i64;
        let default_max = (num_entities / 5).max(5) as i64;

        let mut agents_per_hour_min = get_int("agents_per_hour_min", default_min);
        let mut agents_per_hour_max = get_int("agents_per_hour_max", default_max);

        let num_entities_i64 = num_entities as i64;

        // Validate: ensure not exceeding total agent count
        if agents_per_hour_min > num_entities_i64 {
            tracing::warn!(
                "agents_per_hour_min ({agents_per_hour_min}) 超过总Agent数 ({num_entities_i64})，已修正"
            );
            agents_per_hour_min = (num_entities_i64 / 10).max(1);
        }

        if agents_per_hour_max > num_entities_i64 {
            tracing::warn!(
                "agents_per_hour_max ({agents_per_hour_max}) 超过总Agent数 ({num_entities_i64})，已修正"
            );
            agents_per_hour_max = (agents_per_hour_min + 1).max(num_entities_i64 / 2);
        }

        // Ensure min < max
        if agents_per_hour_min >= agents_per_hour_max {
            agents_per_hour_min = (agents_per_hour_max / 2).max(1);
            tracing::warn!("agents_per_hour_min >= max，已修正为 {agents_per_hour_min}");
        }

        TimeSimulationConfig {
            total_simulation_hours: get_int("total_simulation_hours", 72),
            minutes_per_round: get_int("minutes_per_round", 60),
            agents_per_hour_min,
            agents_per_hour_max,
            peak_hours: get_arr_i64("peak_hours", vec![19, 20, 21, 22]),
            peak_activity_multiplier: 1.5,
            off_peak_hours: get_arr_i64("off_peak_hours", vec![0, 1, 2, 3, 4, 5]),
            off_peak_activity_multiplier: 0.05,
            morning_hours: get_arr_i64("morning_hours", vec![6, 7, 8]),
            morning_activity_multiplier: 0.4,
            work_hours: get_arr_i64("work_hours", (9..19).collect()),
            work_activity_multiplier: 0.7,
        }
    }

    // -----------------------------------------------------------------------
    // S-448 — _generate_event_config
    // -----------------------------------------------------------------------

    /// Call LLM to generate an event configuration.
    ///
    /// Port of `SimulationConfigGenerator._generate_event_config`
    /// (`simulation_config_generator.py:646-717`).
    ///
    /// - Builds entity type info (unique types + up to 3 examples per type).
    /// - Truncates `context` to `EVENT_CONFIG_CONTEXT_LENGTH` chars.
    /// - System prompt appends `get_language_instruction()` + PascalCase note.
    /// - On LLM failure: returns empty-default dict.
    pub async fn generate_event_config(
        &self,
        context: &str,
        simulation_requirement: &str,
        entities: &[EntityNode],
    ) -> Value {
        // Build entity type info: unique types + up to 3 representative names
        let mut type_examples: HashMap<String, Vec<String>> = HashMap::new();
        let mut type_order: Vec<String> = Vec::new();

        for e in entities {
            let etype = e.get_entity_type().unwrap_or_else(|| "Unknown".to_string());
            if !type_examples.contains_key(&etype) {
                type_order.push(etype.clone());
                type_examples.insert(etype.clone(), Vec::new());
            }
            let examples = type_examples.get_mut(&etype).unwrap();
            if examples.len() < 3 {
                examples.push(e.name.clone());
            }
        }

        let type_info = type_order
            .iter()
            .map(|t| {
                let examples = type_examples[t].join(", ");
                format!("- {t}: {examples}")
            })
            .collect::<Vec<_>>()
            .join("\n");

        // CHAR-based truncation
        let context_truncated: String =
            context.chars().take(Self::EVENT_CONFIG_CONTEXT_LENGTH).collect();

        let prompt = format!(
            r#"基于以下模拟需求，生成事件配置。

模拟需求: {simulation_requirement}

{context_truncated}

## 可用实体类型及示例
{type_info}

## 任务
请生成事件配置JSON：
- 提取热点话题关键词
- 描述舆论发展方向
- 设计初始帖子内容，**每个帖子必须指定 poster_type（发布者类型）**

**重要**: poster_type 必须从上面的"可用实体类型"中选择，这样初始帖子才能分配给合适的 Agent 发布。
例如：官方声明应由 Official/University 类型发布，新闻由 MediaOutlet 发布，学生观点由 Student 发布。

返回JSON格式（不要markdown）：
{{
    "hot_topics": ["关键词1", "关键词2", ...],
    "narrative_direction": "<舆论发展方向描述>",
    "initial_posts": [
        {{"content": "帖子内容", "poster_type": "实体类型（必须从可用类型中选择）"}},
        ...
    ],
    "reasoning": "<简要说明>"
}}"#
        );

        let lang_instruction = get_language_instruction();
        let system_prompt = format!(
            "你是舆论分析专家。返回纯JSON格式。注意 poster_type 必须精确匹配可用实体类型。\n\n{lang_instruction}\nIMPORTANT: The 'poster_type' field value MUST be in English PascalCase exactly matching the available entity types. Only 'content', 'narrative_direction', 'hot_topics' and 'reasoning' fields should use the specified language."
        );

        match self.call_llm_with_retry(&prompt, &system_prompt).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("事件配置LLM生成失败: {e}, 使用默认配置");
                serde_json::json!({
                    "hot_topics": [],
                    "narrative_direction": "",
                    "initial_posts": [],
                    "reasoning": "使用默认配置"
                })
            }
        }
    }

    // -----------------------------------------------------------------------
    // S-449 — _parse_event_config
    // -----------------------------------------------------------------------

    /// Parse an LLM event-config result dict into an `EventConfig`.
    ///
    /// Port of `SimulationConfigGenerator._parse_event_config`
    /// (`simulation_config_generator.py:719-726`).
    ///
    /// Field extraction with empty-collection defaults.
    /// `scheduled_events` is always `[]` (Python hardcodes this too).
    pub fn parse_event_config(&self, result: &Value) -> EventConfig {
        let initial_posts = result
            .get("initial_posts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let hot_topics = result
            .get("hot_topics")
            .and_then(Value::as_array)
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();

        let narrative_direction = result
            .get("narrative_direction")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        EventConfig { initial_posts, scheduled_events: vec![], hot_topics, narrative_direction }
    }

    // -----------------------------------------------------------------------
    // S-450 — _assign_initial_post_agents
    // -----------------------------------------------------------------------

    /// Assign agent IDs to initial posts based on poster_type matching.
    ///
    /// Port of `SimulationConfigGenerator._assign_initial_post_agents`
    /// (`simulation_config_generator.py:728-811`).
    ///
    /// # Algorithm
    /// 1. If `event_config.initial_posts` is empty, return unchanged.
    /// 2. Build `agents_by_type: HashMap<String, Vec<&AgentActivityConfig>>` keyed by
    ///    `agent.entity_type.to_lowercase()`, preserving insertion order of agents.
    /// 3. Type alias table (insertion order matters for match priority — stored as Vec of pairs):
    ///    `official → [official, university, governmentagency, government]`
    ///    `university → [university, official]`
    ///    `mediaoutlet → [mediaoutlet, media]`
    ///    `student → [student, person]`
    ///    `professor → [professor, expert, teacher]`
    ///    `alumni → [alumni, person]`
    ///    `organization → [organization, ngo, company, group]`
    ///    `person → [person, student, alumni]`
    /// 4. `used_indices` round-robin counter per type key.
    /// 5. For each post: (a) Direct match → round-robin pick from `agents_by_type`.
    ///    (b) Alias match → iterate alias table IN INSERTION ORDER, first hit wins.
    ///    (c) Fallback → highest `influence_weight` (stable tie-break: first-in-original-order).
    ///    If `agent_configs` is empty: agent_id = 0.
    /// 6. Append `{content, poster_type: ORIGINAL cased value (default "Unknown"), poster_agent_id}`.
    pub fn assign_initial_post_agents(
        &self,
        mut event_config: EventConfig,
        agent_configs: &[AgentActivityConfig],
    ) -> EventConfig {
        if event_config.initial_posts.is_empty() {
            return event_config;
        }

        // Build agents_by_type index (lowercase key, preserve insertion order within each type)
        let mut agents_by_type: HashMap<String, Vec<&AgentActivityConfig>> = HashMap::new();
        for agent in agent_configs {
            let etype = agent.entity_type.to_lowercase();
            agents_by_type.entry(etype).or_default().push(agent);
        }

        // Type alias table — MUST be a Vec of pairs to preserve Python dict insertion order.
        // Python L750-759: official first, then university, mediaoutlet, student, professor,
        // alumni, organization, person.  Order governs which alias group wins when multiple
        // groups match a poster_type.
        let type_aliases: Vec<(&str, Vec<&str>)> = vec![
            ("official", vec!["official", "university", "governmentagency", "government"]),
            ("university", vec!["university", "official"]),
            ("mediaoutlet", vec!["mediaoutlet", "media"]),
            ("student", vec!["student", "person"]),
            ("professor", vec!["professor", "expert", "teacher"]),
            ("alumni", vec!["alumni", "person"]),
            ("organization", vec!["organization", "ngo", "company", "group"]),
            ("person", vec!["person", "student", "alumni"]),
        ];

        // Round-robin counter: maps type key → next index to use
        let mut used_indices: HashMap<String, usize> = HashMap::new();

        let mut updated_posts: Vec<Value> = Vec::with_capacity(event_config.initial_posts.len());

        for post in &event_config.initial_posts {
            let poster_type_lower =
                post.get("poster_type").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let content = post.get("content").and_then(Value::as_str).unwrap_or("").to_string();
            // Preserve the ORIGINAL cased poster_type value (Python L804: post.get("poster_type", "Unknown"))
            let original_poster_type =
                post.get("poster_type").and_then(Value::as_str).unwrap_or("Unknown").to_string();

            let mut matched_agent_id: Option<i64> = None;

            // (1) Direct match
            if let Some(agents) = agents_by_type.get(&poster_type_lower) {
                let idx = used_indices.get(&poster_type_lower).copied().unwrap_or(0) % agents.len();
                matched_agent_id = Some(agents[idx].agent_id);
                used_indices.insert(poster_type_lower.clone(), idx + 1);
            } else {
                // (2) Alias match — iterate in insertion order
                'outer: for (alias_key, aliases) in &type_aliases {
                    if aliases.contains(&poster_type_lower.as_str())
                        || *alias_key == poster_type_lower
                    {
                        for alias in aliases {
                            if let Some(agents) = agents_by_type.get(*alias) {
                                let idx =
                                    used_indices.get(*alias).copied().unwrap_or(0) % agents.len();
                                matched_agent_id = Some(agents[idx].agent_id);
                                used_indices.insert(alias.to_string(), idx + 1);
                                break 'outer;
                            }
                        }
                        // This alias group matched the poster_type but none of its member
                        // types are present among the agents. Python only breaks the outer
                        // loop when a match was actually found (`if matched_agent_id is not
                        // None: break`), so we must NOT break here — continue scanning the
                        // remaining alias groups (a later group may resolve a member). Only
                        // when no group resolves does the influence-max fallback (3) apply.
                    }
                }
            }

            // (3) Fallback: highest influence_weight agent (stable sort → first-in-original wins ties)
            if matched_agent_id.is_none() {
                if !agent_configs.is_empty() {
                    // Python: sorted(agent_configs, key=lambda a: a.influence_weight, reverse=True)
                    // Python sort is stable, so on ties the first in the original order is kept.
                    // We replicate by iterating once and tracking the current best.
                    let best = agent_configs
                        .iter()
                        .reduce(|best, a| {
                            // Strict greater-than: only replace on strictly higher influence
                            // so that on ties we keep the first (original order).
                            if a.influence_weight > best.influence_weight { a } else { best }
                        })
                        .unwrap(); // agent_configs is non-empty
                    matched_agent_id = Some(best.agent_id);
                } else {
                    matched_agent_id = Some(0);
                }
            }

            updated_posts.push(serde_json::json!({
                "content": content,
                "poster_type": original_poster_type,
                "poster_agent_id": matched_agent_id.unwrap(),
            }));
        }

        event_config.initial_posts = updated_posts;
        event_config
    }

    // -----------------------------------------------------------------------
    // S-451 — _generate_agent_configs_batch
    // -----------------------------------------------------------------------

    /// Generate a batch of `AgentActivityConfig`s for the given entities.
    ///
    /// Port of `SimulationConfigGenerator._generate_agent_configs_batch`
    /// (`simulation_config_generator.py:813-906`).
    ///
    /// # Algorithm
    /// 1. Build `entity_list` (JSON array) with `agent_id = start_idx + i`,
    ///    `entity_name`, `entity_type`, `summary` truncated to `AGENT_SUMMARY_LENGTH` chars.
    /// 2. Build prompt and system_prompt (byte-verbatim Chinese strings).
    /// 3. Call `call_llm_with_retry(prompt, system_prompt)`.
    ///    - On success: build `llm_configs: HashMap<i64, Value>` keyed by agent_id.
    ///    - On ANY error: set `llm_configs = {}` and proceed with rule fallback (no fault).
    /// 4. For each entity: if `llm_configs.get(agent_id)` is empty/missing → use
    ///    `generate_agent_config_by_rule(entity)`.
    /// 5. Build `AgentActivityConfig` using `.get(key).or_default()` fallbacks that match
    ///    Python's `cfg.get("key", default)` — NOTE these defaults differ from the
    ///    dataclass defaults: posts_per_hour=0.5, comments_per_hour=1.0, active_hours=9..=22.
    pub async fn generate_agent_configs_batch(
        &self,
        _context: &str,
        entities: &[EntityNode],
        start_idx: i64,
        simulation_requirement: &str,
    ) -> Vec<AgentActivityConfig> {
        // Step 1 — build entity_list
        let summary_len = Self::AGENT_SUMMARY_LENGTH;
        let entity_list: Vec<Value> = entities
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let summary: String = if e.summary.is_empty() {
                    String::new()
                } else {
                    e.summary.chars().take(summary_len).collect()
                };
                serde_json::json!({
                    "agent_id": start_idx + i as i64,
                    "entity_name": e.name,
                    "entity_type": e.get_entity_type().unwrap_or_else(|| "Unknown".to_string()),
                    "summary": summary,
                })
            })
            .collect();

        // Step 2 — build prompt (byte-verbatim, with json.dumps(ensure_ascii=False, indent=2))
        let entity_list_json =
            serde_json::to_string_pretty(&entity_list).expect("entity_list is always serializable");

        let prompt = format!(
            "基于以下信息，为每个实体生成社交媒体活动配置。\n\n模拟需求: {simulation_requirement}\n\n## 实体列表\n```json\n{entity_list_json}\n```\n\n## 任务\n为每个实体生成活动配置，注意：\n- **时间符合目标用户群体作息**：以下为参考（东八区），请根据模拟场景调整\n- **官方机构**（University/GovernmentAgency）：活跃度低(0.1-0.3)，工作时间(9-17)活动，响应慢(60-240分钟)，影响力高(2.5-3.0)\n- **媒体**（MediaOutlet）：活跃度中(0.4-0.6)，全天活动(8-23)，响应快(5-30分钟)，影响力高(2.0-2.5)\n- **个人**（Student/Person/Alumni）：活跃度高(0.6-0.9)，主要晚间活动(18-23)，响应快(1-15分钟)，影响力低(0.8-1.2)\n- **公众人物/专家**：活跃度中(0.4-0.6)，影响力中高(1.5-2.0)\n\n返回JSON格式（不要markdown）：\n{{\n    \"agent_configs\": [\n        {{\n            \"agent_id\": <必须与输入一致>,\n            \"activity_level\": <0.0-1.0>,\n            \"posts_per_hour\": <发帖频率>,\n            \"comments_per_hour\": <评论频率>,\n            \"active_hours\": [<活跃小时列表，考虑中国人作息>],\n            \"response_delay_min\": <最小响应延迟分钟>,\n            \"response_delay_max\": <最大响应延迟分钟>,\n            \"sentiment_bias\": <-1.0到1.0>,\n            \"stance\": \"<supportive/opposing/neutral/observer>\",\n            \"influence_weight\": <影响力权重>\n        }},\n        ...\n    ]\n}}"
        );

        let base_system =
            "你是社交媒体行为分析专家。返回纯JSON，配置需符合模拟场景中目标用户群体的作息习惯。";
        let lang_instruction = get_language_instruction();
        let system_prompt = format!(
            "{base_system}\n\n{lang_instruction}\nIMPORTANT: The 'stance' field value MUST be one of the English strings: 'supportive', 'opposing', 'neutral', 'observer'. All JSON field names and numeric values must remain unchanged. Only natural language text fields should use the specified language."
        );

        // Step 3 — call LLM; on ANY error fall back to empty map (rule generation below)
        let llm_configs: HashMap<i64, Value> =
            match self.call_llm_with_retry(&prompt, &system_prompt).await {
                Ok(result) => result
                    .get("agent_configs")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|cfg| {
                                let id = cfg.get("agent_id")?.as_i64()?;
                                Some((id, cfg.clone()))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                Err(e) => {
                    tracing::warn!("Agent配置批次LLM生成失败: {e}, 使用规则生成");
                    HashMap::new()
                }
            };

        // Step 4+5 — build AgentActivityConfig for each entity
        // NOTE: the .get() defaults here DIFFER from the struct's dataclass defaults.
        // Python L895-902: activity_level=0.5, posts_per_hour=0.5, comments_per_hour=1.0,
        // active_hours=list(range(9,23))=[9..=22], delay_min=5, delay_max=60,
        // sentiment_bias=0.0, stance="neutral", influence_weight=1.0.
        let default_active_hours_batch: Vec<i64> = (9i64..23).collect(); // [9..=22], 14 elements

        entities
            .iter()
            .enumerate()
            .map(|(i, entity)| {
                let agent_id = start_idx + i as i64;
                let cfg = match llm_configs.get(&agent_id) {
                    Some(v)
                        if !v.is_null()
                            && v.as_object().map(|o| !o.is_empty()).unwrap_or(false) =>
                    {
                        v.clone()
                    }
                    _ => self.generate_agent_config_by_rule(entity),
                };

                let get_f64 = |key: &str, default: f64| -> f64 {
                    cfg.get(key).and_then(Value::as_f64).unwrap_or(default)
                };
                let get_i64 = |key: &str, default: i64| -> i64 {
                    cfg.get(key).and_then(Value::as_i64).unwrap_or(default)
                };
                let get_str = |key: &str, default: &str| -> String {
                    cfg.get(key).and_then(Value::as_str).unwrap_or(default).to_string()
                };
                let get_hours = |key: &str, default: Vec<i64>| -> Vec<i64> {
                    cfg.get(key)
                        .and_then(Value::as_array)
                        .map(|arr| arr.iter().filter_map(Value::as_i64).collect())
                        .unwrap_or(default)
                };

                AgentActivityConfig {
                    agent_id,
                    entity_uuid: entity.uuid.clone(),
                    entity_name: entity.name.clone(),
                    entity_type: entity.get_entity_type().unwrap_or_else(|| "Unknown".to_string()),
                    activity_level: get_f64("activity_level", 0.5),
                    posts_per_hour: get_f64("posts_per_hour", 0.5), // NOTE: 0.5, not 1.0
                    comments_per_hour: get_f64("comments_per_hour", 1.0), // NOTE: 1.0, not 2.0
                    active_hours: get_hours("active_hours", default_active_hours_batch.clone()), // [9..=22]
                    response_delay_min: get_i64("response_delay_min", 5),
                    response_delay_max: get_i64("response_delay_max", 60),
                    sentiment_bias: get_f64("sentiment_bias", 0.0),
                    stance: get_str("stance", "neutral"),
                    influence_weight: get_f64("influence_weight", 1.0),
                }
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // S-439 — generate_config
    // -----------------------------------------------------------------------

    /// Orchestrate all stages to produce a complete `SimulationParameters`.
    ///
    /// Port of `SimulationConfigGenerator.generate_config`
    /// (`simulation_config_generator.py:243-379`).
    ///
    /// # Parameters
    /// - `simulation_id`, `project_id`, `graph_id`, `simulation_requirement`, `document_text`:
    ///   passed straight through to `SimulationParameters`.
    /// - `entities`: the filtered entity list used for all generation stages.
    /// - `enable_twitter`: whether to include a Twitter `PlatformConfig` (Python default `True`).
    /// - `enable_reddit`: whether to include a Reddit `PlatformConfig` (Python default `True`).
    /// - `progress_callback`: optional `(current_step, total_steps, message)` callback invoked
    ///   after each stage completes.  Python type: `Optional[Callable[[int, int, str], None]]`.
    ///   Rust idiom: `Option<&mut dyn FnMut(i64, i64, &str)>` — mutable borrow avoids a generic
    ///   type parameter on the method while keeping it ergonomic at call sites.
    ///
    /// # Step numbering (contractual)
    /// - `num_batches = ceil(entities.len() / AGENTS_PER_BATCH)` — integer ceiling division.
    /// - `total_steps = 3 + num_batches` (time config + event config + N agent batches +
    ///   platform config, where platform config is reported at `total_steps`).
    /// - Steps 1, 2 → time config, event config.
    /// - Steps 3 .. 3+num_batches-1 → agent-config batches.
    /// - Step `total_steps` → platform config.
    ///
    /// # Platform config literal values (NOTE: reddit differs from struct defaults)
    /// Twitter: recency=0.4, popularity=0.3, relevance=0.3, viral=10, echo=0.5 (= defaults).
    /// Reddit:  recency=0.3, popularity=0.4, relevance=0.3, viral=15, echo=0.6 (NON-default).
    ///
    /// S-439
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    pub async fn generate_config(
        &self,
        simulation_id: impl Into<String>,
        project_id: impl Into<String>,
        graph_id: impl Into<String>,
        simulation_requirement: impl Into<String>,
        document_text: &str,
        entities: &[EntityNode],
        enable_twitter: bool,
        enable_reddit: bool,
        mut progress_callback: Option<&mut dyn FnMut(i64, i64, &str)>,
    ) -> SimulationParameters {
        use crate::i18n::{t, t_args};

        let simulation_id = simulation_id.into();
        let project_id = project_id.into();
        let graph_id = graph_id.into();
        let simulation_requirement = simulation_requirement.into();

        tracing::info!(
            "开始智能生成模拟配置: simulation_id={}, 实体数={}",
            simulation_id,
            entities.len()
        );

        // Calculate total steps: num_batches = ceil(len / AGENTS_PER_BATCH).
        // Python: math.ceil(len(entities) / self.AGENTS_PER_BATCH).
        // Use div_ceil for integer ceiling division.
        let num_batches = entities.len().div_ceil(Self::AGENTS_PER_BATCH);
        let total_steps: i64 = (3 + num_batches) as i64;
        // Python tracks current_step as a nonlocal; we track it here so the macro can log it.
        // `allow(unused_assignments)` silences the "initial value 0 immediately overwritten"
        // lint — the variable IS read inside the macro after each assignment.
        #[allow(unused_assignments)]
        let mut current_step: i64 = 0;

        // Inner report_progress closure — captures callback mutably.
        // Python: sets current_step, calls callback, logs.
        macro_rules! report_progress {
            ($step:expr, $msg:expr) => {{
                current_step = $step as i64;
                let msg: String = $msg;
                if let Some(ref mut cb) = progress_callback {
                    cb(current_step, total_steps, &msg);
                }
                tracing::info!("[{}/{}] {}", current_step, total_steps, msg);
            }};
        }

        // 1. Build base context
        let context = self.build_context(&simulation_requirement, document_text, entities);

        let mut reasoning_parts: Vec<String> = Vec::new();

        // ===== Step 1: time config =====
        report_progress!(1, t("progress.generatingTimeConfig"));
        let num_entities = entities.len();
        let time_config_result = self.generate_time_config(&context, num_entities).await;
        let time_config = self.parse_time_config(&time_config_result, num_entities);
        let time_reasoning = time_config_result
            .get("reasoning")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| t("common.success"));
        reasoning_parts.push(format!("{}: {}", t("progress.timeConfigLabel"), time_reasoning));

        // ===== Step 2: event config =====
        report_progress!(2, t("progress.generatingEventConfig"));
        let event_config_result =
            self.generate_event_config(&context, &simulation_requirement, entities).await;
        let mut event_config = self.parse_event_config(&event_config_result);
        let event_reasoning = event_config_result
            .get("reasoning")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| t("common.success"));
        reasoning_parts.push(format!("{}: {}", t("progress.eventConfigLabel"), event_reasoning));

        // ===== Steps 3..N: agent configs in batches =====
        let mut all_agent_configs: Vec<AgentActivityConfig> = Vec::new();
        for batch_idx in 0..num_batches {
            let start_idx = batch_idx * Self::AGENTS_PER_BATCH;
            let end_idx = (start_idx + Self::AGENTS_PER_BATCH).min(entities.len());
            let batch_entities = &entities[start_idx..end_idx];

            report_progress!(
                3 + batch_idx,
                t_args(
                    "progress.generatingAgentConfig",
                    &[("start", &(start_idx + 1)), ("end", &end_idx), ("total", &entities.len()),],
                )
            );

            let batch_configs = self
                .generate_agent_configs_batch(
                    &context,
                    batch_entities,
                    start_idx as i64,
                    &simulation_requirement,
                )
                .await;
            all_agent_configs.extend(batch_configs);
        }
        reasoning_parts
            .push(t_args("progress.agentConfigResult", &[("count", &all_agent_configs.len())]));

        // ===== Assign initial-post agents =====
        tracing::info!("为初始帖子分配合适的发布者 Agent...");
        event_config = self.assign_initial_post_agents(event_config, &all_agent_configs);
        let assigned_count = event_config
            .initial_posts
            .iter()
            .filter(|p| !p.get("poster_agent_id").map(Value::is_null).unwrap_or(true))
            .count();
        reasoning_parts.push(t_args("progress.postAssignResult", &[("count", &assigned_count)]));

        // ===== Last step: platform configs =====
        report_progress!(total_steps, t("progress.generatingPlatformConfig"));

        let twitter_config = if enable_twitter {
            Some(PlatformConfig {
                platform: "twitter".to_string(),
                recency_weight: 0.4,
                popularity_weight: 0.3,
                relevance_weight: 0.3,
                viral_threshold: 10,
                echo_chamber_strength: 0.5,
            })
        } else {
            None
        };

        let reddit_config = if enable_reddit {
            // NOTE: reddit values differ from PlatformConfig struct defaults.
            // recency=0.3 (not 0.4), popularity=0.4 (not 0.3), viral=15 (not 10), echo=0.6 (not 0.5).
            Some(PlatformConfig {
                platform: "reddit".to_string(),
                recency_weight: 0.3,
                popularity_weight: 0.4,
                relevance_weight: 0.3,
                viral_threshold: 15,
                echo_chamber_strength: 0.6,
            })
        } else {
            None
        };

        // Build final params — generated_at uses the struct default (python_isoformat_local).
        let params = SimulationParameters {
            simulation_id,
            project_id,
            graph_id,
            simulation_requirement,
            time_config,
            agent_configs: all_agent_configs,
            event_config,
            twitter_config,
            reddit_config,
            llm_model: self.model_name.clone(),
            llm_base_url: self.base_url.clone(),
            generated_at: python_isoformat_local(),
            generation_reasoning: reasoning_parts.join(" | "),
        };

        tracing::info!("模拟配置生成完成: {} 个Agent配置", params.agent_configs.len());

        params
    }

    // -----------------------------------------------------------------------
    // S-452 — _generate_agent_config_by_rule
    // -----------------------------------------------------------------------

    /// Generate a rule-based agent config dict for a single entity (Chinese lifestyle schedule).
    ///
    /// Port of `SimulationConfigGenerator._generate_agent_config_by_rule`
    /// (`simulation_config_generator.py:908-989`).
    ///
    /// Returns a `serde_json::Value` (JSON object) with the same field names that
    /// `generate_agent_configs_batch` reads via `.get(key)` — this allows the same
    /// `.get(key, default)` pattern to work for both LLM-generated and rule-generated configs.
    ///
    /// # Branches (6 total, all ported, exact numeric values and active_hours lists)
    /// 1. `["university", "governmentagency", "ngo"]` — official institutions, work hours
    /// 2. `["mediaoutlet"]` — media, full-day coverage
    /// 3. `["professor", "expert", "official"]` — experts, work+evening
    /// 4. `["student"]` — students, morning+evening (explicit list)
    /// 5. `["alumni"]` — alumni, lunch+evening (explicit list)
    /// 6. else (普通人) — general public, daytime+evening (explicit list)
    pub fn generate_agent_config_by_rule(&self, entity: &EntityNode) -> Value {
        let entity_type =
            entity.get_entity_type().unwrap_or_else(|| "Unknown".to_string()).to_lowercase();

        match entity_type.as_str() {
            "university" | "governmentagency" | "ngo" => {
                // 官方机构：工作时间活动，低频率，高影响力
                // active_hours: list(range(9, 18)) → [9, 10, 11, 12, 13, 14, 15, 16, 17]
                serde_json::json!({
                    "activity_level": 0.2_f64,
                    "posts_per_hour": 0.1_f64,
                    "comments_per_hour": 0.05_f64,
                    "active_hours": [9, 10, 11, 12, 13, 14, 15, 16, 17],
                    "response_delay_min": 60_i64,
                    "response_delay_max": 240_i64,
                    "sentiment_bias": 0.0_f64,
                    "stance": "neutral",
                    "influence_weight": 3.0_f64,
                })
            }
            "mediaoutlet" => {
                // 媒体：全天活动，中等频率，高影响力
                // active_hours: list(range(7, 24)) → [7, 8, 9, ..., 23]
                serde_json::json!({
                    "activity_level": 0.5_f64,
                    "posts_per_hour": 0.8_f64,
                    "comments_per_hour": 0.3_f64,
                    "active_hours": [7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23],
                    "response_delay_min": 5_i64,
                    "response_delay_max": 30_i64,
                    "sentiment_bias": 0.0_f64,
                    "stance": "observer",
                    "influence_weight": 2.5_f64,
                })
            }
            "professor" | "expert" | "official" => {
                // 专家/教授：工作+晚间活动，中等频率
                // active_hours: list(range(8, 22)) → [8, 9, 10, ..., 21]
                serde_json::json!({
                    "activity_level": 0.4_f64,
                    "posts_per_hour": 0.3_f64,
                    "comments_per_hour": 0.5_f64,
                    "active_hours": [8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21],
                    "response_delay_min": 15_i64,
                    "response_delay_max": 90_i64,
                    "sentiment_bias": 0.0_f64,
                    "stance": "neutral",
                    "influence_weight": 2.0_f64,
                })
            }
            "student" => {
                // 学生：晚间为主，高频率 (上午+晚间)
                // active_hours: explicit [8,9,10,11,12,13,18,19,20,21,22,23]
                serde_json::json!({
                    "activity_level": 0.8_f64,
                    "posts_per_hour": 0.6_f64,
                    "comments_per_hour": 1.5_f64,
                    "active_hours": [8, 9, 10, 11, 12, 13, 18, 19, 20, 21, 22, 23],
                    "response_delay_min": 1_i64,
                    "response_delay_max": 15_i64,
                    "sentiment_bias": 0.0_f64,
                    "stance": "neutral",
                    "influence_weight": 0.8_f64,
                })
            }
            "alumni" => {
                // 校友：晚间为主 (午休+晚间)
                // active_hours: explicit [12,13,19,20,21,22,23]
                serde_json::json!({
                    "activity_level": 0.6_f64,
                    "posts_per_hour": 0.4_f64,
                    "comments_per_hour": 0.8_f64,
                    "active_hours": [12, 13, 19, 20, 21, 22, 23],
                    "response_delay_min": 5_i64,
                    "response_delay_max": 30_i64,
                    "sentiment_bias": 0.0_f64,
                    "stance": "neutral",
                    "influence_weight": 1.0_f64,
                })
            }
            _ => {
                // 普通人：白天+晚间
                // active_hours: explicit [9,10,11,12,13,18,19,20,21,22,23]
                serde_json::json!({
                    "activity_level": 0.7_f64,
                    "posts_per_hour": 0.5_f64,
                    "comments_per_hour": 1.2_f64,
                    "active_hours": [9, 10, 11, 12, 13, 18, 19, 20, 21, 22, 23],
                    "response_delay_min": 2_i64,
                    "response_delay_max": 20_i64,
                    "sentiment_bias": 0.0_f64,
                    "stance": "neutral",
                    "influence_weight": 1.0_f64,
                })
            }
        }
    }
}

// ===========================================================================
// Tests for SimulationConfigGenerator
// ===========================================================================

#[cfg(test)]
mod generator_tests {
    use super::*;
    use crate::llm::{ChatMessage, ChatOptions};
    use async_trait::async_trait;
    use futures::Stream;
    use serde::de::DeserializeOwned;
    use serde_json::Map;
    use std::pin::Pin;

    // -----------------------------------------------------------------------
    // Mock LlmClient for deterministic unit tests
    // -----------------------------------------------------------------------

    /// A mock LLM client that returns a fixed JSON string on `chat`.
    struct MockLlm {
        /// The raw string to return from `chat`.
        response: String,
        /// If `Some`, fail `chat` with this error message on attempts ≤ fail_until.
        /// After that, return `response`.
        fail_until: Option<usize>,
        call_count: std::sync::Mutex<usize>,
    }

    impl MockLlm {
        fn always(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
                fail_until: None,
                call_count: std::sync::Mutex::new(0),
            }
        }

        fn fail_then(fail_count: usize, response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
                fail_until: Some(fail_count),
                call_count: std::sync::Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmClient for MockLlm {
        async fn complete(&self, _prompt: &str) -> crate::error::Result<String> {
            Ok(self.response.clone())
        }

        async fn complete_json<T: DeserializeOwned>(
            &self,
            _prompt: &str,
        ) -> crate::error::Result<T> {
            serde_json::from_str(&self.response).map_err(|e| TeriError::Config(e.to_string()))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> crate::error::Result<Pin<Box<dyn Stream<Item = crate::error::Result<String>> + Send>>>
        {
            unimplemented!("not needed for tests")
        }

        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _opts: &ChatOptions,
        ) -> crate::error::Result<String> {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
            let current = *count;
            drop(count);

            if let Some(fail_until) = self.fail_until
                && current <= fail_until
            {
                return Err(TeriError::Config("mock failure".into()));
            }
            Ok(self.response.clone())
        }

        async fn chat_json<T: DeserializeOwned>(
            &self,
            messages: &[ChatMessage],
            opts: &ChatOptions,
        ) -> crate::error::Result<T> {
            let raw = self.chat(messages, opts).await?;
            serde_json::from_str(&raw).map_err(|e| TeriError::Config(e.to_string()))
        }
    }

    fn make_node(name: &str, labels: Vec<&str>, summary: &str) -> EntityNode {
        EntityNode::new(
            "uuid",
            name,
            labels.into_iter().map(str::to_string).collect(),
            summary,
            Map::new(),
        )
    }

    fn make_gen(response: impl Into<String>) -> SimulationConfigGenerator<MockLlm> {
        SimulationConfigGenerator::new(MockLlm::always(response), "model-x", "http://localhost")
    }

    // -----------------------------------------------------------------------
    // _summarize_entities (S-441)
    // -----------------------------------------------------------------------

    #[test]
    fn summarize_entities_groups_by_type() {
        let entities = vec![
            make_node("Alice", vec!["Entity", "Person"], "A person"),
            make_node("Bob", vec!["Entity", "Person"], "Another person"),
            make_node("CCTV", vec!["Entity", "MediaOutlet"], "A media outlet"),
        ];
        let g = make_gen("{}");
        let summary = g.summarize_entities(&entities);

        // Must contain both type headers
        assert!(summary.contains("### Person (2个)"), "missing Person header");
        assert!(summary.contains("### MediaOutlet (1个)"), "missing MediaOutlet header");
        assert!(summary.contains("- Alice:"), "missing Alice entry");
        assert!(summary.contains("- Bob:"), "missing Bob entry");
        assert!(summary.contains("- CCTV:"), "missing CCTV entry");
    }

    #[test]
    fn summarize_entities_unknown_type_fallback() {
        let entities = vec![make_node("X", vec!["Entity", "Node"], "only base labels")];
        let g = make_gen("{}");
        let summary = g.summarize_entities(&entities);
        assert!(summary.contains("### Unknown (1个)"), "should use Unknown for Entity/Node only");
    }

    #[test]
    fn summarize_entities_char_truncates_long_summary() {
        // Build a summary of exactly 301 chars (> ENTITY_SUMMARY_LENGTH=300)
        let long_summary: String = "中".repeat(301); // 301 Chinese chars
        let entities = vec![make_node("测试", vec!["Person"], &long_summary)];
        let g = make_gen("{}");
        let summary = g.summarize_entities(&entities);

        // The preview should be 300 chars + "..."
        let expected_preview: String = "中".repeat(300) + "...";
        assert!(
            summary.contains(&expected_preview),
            "summary should contain 300-char truncated preview"
        );
    }

    #[test]
    fn summarize_entities_no_truncation_for_short_summary() {
        let short_summary = "Short summary.";
        let entities = vec![make_node("A", vec!["Person"], short_summary)];
        let g = make_gen("{}");
        let summary = g.summarize_entities(&entities);
        assert!(summary.contains("Short summary."), "short summary must not be truncated");
        assert!(!summary.contains("Short summary...."), "should not add ... to short summaries");
    }

    #[test]
    fn summarize_entities_shows_tail_line_when_overflow() {
        // 21 entities of same type; ENTITIES_PER_TYPE_DISPLAY=20 → "  ... 还有 1 个"
        let entities: Vec<EntityNode> = (0..21)
            .map(|i| make_node(&format!("Person{i}"), vec!["Person"], "desc"))
            .collect();
        let g = make_gen("{}");
        let summary = g.summarize_entities(&entities);
        assert!(
            summary.contains("  ... 还有 1 个"),
            "should show tail line for overflow: {summary}"
        );
    }

    #[test]
    fn summarize_entities_no_tail_at_exact_display_count() {
        let entities: Vec<EntityNode> =
            (0..20).map(|i| make_node(&format!("P{i}"), vec!["Person"], "d")).collect();
        let g = make_gen("{}");
        let summary = g.summarize_entities(&entities);
        assert!(!summary.contains("还有"), "no tail line when exactly at display count");
    }

    // -----------------------------------------------------------------------
    // _build_context (S-440)
    // -----------------------------------------------------------------------

    #[test]
    fn build_context_includes_requirement_and_entity_summary() {
        let entities = vec![make_node("Alice", vec!["Person"], "test")];
        let g = make_gen("{}");
        let ctx = g.build_context("Test requirement", "Some document text", &entities);
        assert!(ctx.contains("## 模拟需求"), "missing requirement header");
        assert!(ctx.contains("Test requirement"));
        assert!(ctx.contains("## 实体信息 (1个)"), "missing entity info header");
        assert!(ctx.contains("## 原始文档内容"), "should include document text");
    }

    #[test]
    fn build_context_truncates_document_when_over_budget() {
        // Fill context budget by making a huge entity summary
        // Easiest: make a very long document that should be truncated
        let entities = vec![];
        let g = make_gen("{}");
        // Use a document of 60000 chars (> MAX_CONTEXT_LENGTH=50000)
        let doc: String = "X".repeat(60_000);
        let ctx = g.build_context("req", &doc, &entities);
        // Should be truncated and contain the truncation marker
        assert!(ctx.contains("...(文档已截断)"), "should have truncation marker in context");
        // Total char length should be under MAX_CONTEXT_LENGTH + some margin for markers
        // The marker itself adds chars, just verify truncation happened
        let doc_section_start = ctx.find("## 原始文档内容").unwrap();
        let doc_section = &ctx[doc_section_start..];
        assert!(doc_section.chars().count() < 51_500, "document section should be capped");
    }

    #[test]
    fn build_context_no_truncation_for_short_document() {
        let entities = vec![];
        let g = make_gen("{}");
        let ctx = g.build_context("req", "Short doc.", &entities);
        assert!(ctx.contains("Short doc."), "short doc should appear verbatim");
        assert!(!ctx.contains("...(文档已截断)"), "should not have truncation marker");
    }

    #[test]
    fn build_context_skips_document_when_empty() {
        let entities = vec![];
        let g = make_gen("{}");
        let ctx = g.build_context("req", "", &entities);
        assert!(!ctx.contains("## 原始文档内容"), "should not include doc section when empty");
    }

    // -----------------------------------------------------------------------
    // _fix_truncated_json (S-443)
    // -----------------------------------------------------------------------

    #[test]
    fn fix_truncated_json_closes_open_brace() {
        let truncated = r#"{"key": "val""#;
        let fixed = SimulationConfigGenerator::<MockLlm>::fix_truncated_json(truncated);
        let parsed: Value = serde_json::from_str(&fixed).expect("should be valid JSON after fix");
        assert_eq!(parsed["key"].as_str().unwrap(), "val");
    }

    #[test]
    fn fix_truncated_json_closes_open_bracket_and_brace() {
        let truncated = r#"{"items": ["a", "b""#;
        let fixed = SimulationConfigGenerator::<MockLlm>::fix_truncated_json(truncated);
        // After fix: {"items": ["a", "b"]}  (no extra quote because last char is '"')
        let parsed: Value = serde_json::from_str(&fixed).expect("should be valid: {fixed}");
        let items = parsed["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn fix_truncated_json_appends_quote_when_last_char_not_in_set() {
        // Last char is 'x' — not in {'"', ',', '}', ']'} → append '"'
        let truncated = r#"{"key": "val"#; // missing closing "
        let fixed = SimulationConfigGenerator::<MockLlm>::fix_truncated_json(truncated);
        // After fix: {"key": "val"} (quote + brace appended)
        let parsed: Value = serde_json::from_str(&fixed).expect("should be valid: {fixed}");
        assert_eq!(parsed["key"].as_str().unwrap(), "val");
    }

    #[test]
    fn fix_truncated_json_already_valid_unchanged() {
        let valid = r#"{"key": "value"}"#;
        let fixed = SimulationConfigGenerator::<MockLlm>::fix_truncated_json(valid);
        let parsed: Value = serde_json::from_str(&fixed).unwrap();
        assert_eq!(parsed["key"].as_str().unwrap(), "value");
    }

    // -----------------------------------------------------------------------
    // _try_fix_config_json (S-444)
    // -----------------------------------------------------------------------

    #[test]
    fn try_fix_config_json_salvages_newline_in_string() {
        // A JSON string with a literal newline inside — invalid JSON, should be fixed
        let broken = "{\"key\": \"val\nue\"}";
        let result = SimulationConfigGenerator::<MockLlm>::try_fix_config_json(broken);
        assert!(result.is_some(), "should salvage newline-in-string JSON");
        assert_eq!(result.unwrap()["key"].as_str().unwrap(), "val ue");
    }

    #[test]
    fn try_fix_config_json_salvages_control_chars() {
        // Control char embedded in JSON — invalid
        let broken = "{\"key\": \"val\x01ue\"}";
        let result = SimulationConfigGenerator::<MockLlm>::try_fix_config_json(broken);
        assert!(result.is_some(), "should salvage control-char JSON");
    }

    #[test]
    fn try_fix_config_json_returns_none_for_completely_garbage() {
        let garbage = "this is not json at all !!!";
        let result = SimulationConfigGenerator::<MockLlm>::try_fix_config_json(garbage);
        assert!(result.is_none(), "completely non-JSON should return None");
    }

    #[test]
    fn try_fix_config_json_extracts_embedded_json_object() {
        // JSON embedded in surrounding text
        let wrapped = r#"Here is your result: {"answer": 42} hope that helps"#;
        let result = SimulationConfigGenerator::<MockLlm>::try_fix_config_json(wrapped);
        assert!(result.is_some(), "should extract embedded JSON object");
        assert_eq!(result.unwrap()["answer"].as_i64().unwrap(), 42);
    }

    // -----------------------------------------------------------------------
    // _call_llm_with_retry (S-442) — temperature stepping + retry
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn call_llm_with_retry_succeeds_on_first_attempt() {
        let g = make_gen(r#"{"result": "ok"}"#);
        let v = g.call_llm_with_retry("prompt", "system").await.unwrap();
        assert_eq!(v["result"].as_str().unwrap(), "ok");
    }

    #[tokio::test]
    async fn call_llm_with_retry_retries_on_error_and_succeeds() {
        // Fail on first 1 attempt, succeed on attempt 2
        let llm = MockLlm::fail_then(1, r#"{"ok": true}"#);
        let g = SimulationConfigGenerator::new(llm, "m", "b");
        // This will fail attempt 1 (error), succeed attempt 2
        // Note: real sleep is 2*(0+1)=2s on failure — we don't want to sleep in tests.
        // The mock advances call_count per chat() call.
        // Attempt 1 fails (call_count=1 ≤ fail_until=1) → sleep 2s... but in test we can't
        // avoid the sleep. Accept that this test takes ~2s.
        let v = g.call_llm_with_retry("p", "s").await.unwrap();
        assert!(v["ok"].as_bool().unwrap());
    }

    // -----------------------------------------------------------------------
    // _parse_time_config (S-447) defaults + clamping
    // -----------------------------------------------------------------------

    #[test]
    fn parse_time_config_uses_defaults_when_fields_missing() {
        let g = make_gen("{}");
        let result = serde_json::json!({});
        let tc = g.parse_time_config(&result, 30);

        assert_eq!(tc.total_simulation_hours, 72);
        assert_eq!(tc.minutes_per_round, 60);
        assert_eq!(tc.peak_hours, vec![19, 20, 21, 22]);
        assert_eq!(tc.off_peak_hours, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(tc.morning_hours, vec![6, 7, 8]);
        assert_eq!(tc.work_hours, (9..19).collect::<Vec<i64>>());
        // Fixed multipliers
        assert_eq!(tc.peak_activity_multiplier, 1.5);
        assert_eq!(tc.off_peak_activity_multiplier, 0.05);
        assert_eq!(tc.morning_activity_multiplier, 0.4);
        assert_eq!(tc.work_activity_multiplier, 0.7);
    }

    #[test]
    fn parse_time_config_clamps_min_when_exceeds_entity_count() {
        let g = make_gen("{}");
        // num_entities=10, but LLM returns min=50 (> 10) → corrected
        let result = serde_json::json!({
            "agents_per_hour_min": 50,
            "agents_per_hour_max": 100
        });
        let tc = g.parse_time_config(&result, 10);
        // min corrected to max(1, 10//10) = 1
        assert_eq!(tc.agents_per_hour_min, 1);
        // max corrected to max(min+1, 10//2) = max(2, 5) = 5
        assert_eq!(tc.agents_per_hour_max, 5);
    }

    #[test]
    fn parse_time_config_enforces_min_less_than_max() {
        let g = make_gen("{}");
        // min == max — must correct
        let result = serde_json::json!({
            "agents_per_hour_min": 10,
            "agents_per_hour_max": 10
        });
        let tc = g.parse_time_config(&result, 100);
        // Neither exceeds 100, but min >= max: min = max(1, 10//2) = 5
        assert!(tc.agents_per_hour_min < tc.agents_per_hour_max);
        assert_eq!(tc.agents_per_hour_min, 5);
        assert_eq!(tc.agents_per_hour_max, 10);
    }

    #[test]
    fn parse_time_config_extracts_all_fields_from_llm() {
        let g = make_gen("{}");
        let result = serde_json::json!({
            "total_simulation_hours": 48,
            "minutes_per_round": 30,
            "agents_per_hour_min": 2,
            "agents_per_hour_max": 8,
            "peak_hours": [20, 21],
            "off_peak_hours": [1, 2, 3],
            "morning_hours": [7, 8],
            "work_hours": [9, 10, 11, 12, 13, 14, 15, 16, 17, 18]
        });
        let tc = g.parse_time_config(&result, 50);
        assert_eq!(tc.total_simulation_hours, 48);
        assert_eq!(tc.minutes_per_round, 30);
        assert_eq!(tc.agents_per_hour_min, 2);
        assert_eq!(tc.agents_per_hour_max, 8);
        assert_eq!(tc.peak_hours, vec![20, 21]);
        assert_eq!(tc.off_peak_hours, vec![1, 2, 3]);
        assert_eq!(tc.morning_hours, vec![7, 8]);
    }

    // -----------------------------------------------------------------------
    // _parse_event_config (S-449) defaults + mapping
    // -----------------------------------------------------------------------

    #[test]
    fn parse_event_config_uses_empty_defaults_when_missing() {
        let g = make_gen("{}");
        let result = serde_json::json!({});
        let ec = g.parse_event_config(&result);
        assert!(ec.initial_posts.is_empty());
        assert!(ec.scheduled_events.is_empty());
        assert!(ec.hot_topics.is_empty());
        assert_eq!(ec.narrative_direction, "");
    }

    #[test]
    fn parse_event_config_maps_all_fields() {
        let g = make_gen("{}");
        let result = serde_json::json!({
            "hot_topics": ["AI", "教育"],
            "narrative_direction": "向积极方向发展",
            "initial_posts": [
                {"content": "帖子1", "poster_type": "Student"}
            ]
        });
        let ec = g.parse_event_config(&result);
        assert_eq!(ec.hot_topics, vec!["AI", "教育"]);
        assert_eq!(ec.narrative_direction, "向积极方向发展");
        assert_eq!(ec.initial_posts.len(), 1);
        assert_eq!(ec.initial_posts[0]["content"].as_str().unwrap(), "帖子1");
        // scheduled_events always empty (hardcoded)
        assert!(ec.scheduled_events.is_empty());
    }

    #[test]
    fn parse_event_config_scheduled_events_always_empty() {
        let g = make_gen("{}");
        // Even if LLM returns scheduled_events, the parse ignores it and uses []
        let result = serde_json::json!({
            "scheduled_events": [{"type": "something"}]
        });
        let ec = g.parse_event_config(&result);
        // Python hardcodes scheduled_events=[] in the dataclass constructor
        assert!(ec.scheduled_events.is_empty());
    }

    // -----------------------------------------------------------------------
    // _generate_time_config (S-445) — mock integration
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_time_config_returns_parsed_llm_value() {
        let llm_response = serde_json::json!({
            "total_simulation_hours": 48,
            "minutes_per_round": 60,
            "agents_per_hour_min": 3,
            "agents_per_hour_max": 15,
            "peak_hours": [19, 20, 21, 22],
            "off_peak_hours": [0, 1, 2, 3, 4, 5],
            "morning_hours": [6, 7, 8],
            "work_hours": [9, 10, 11, 12, 13, 14, 15, 16, 17, 18],
            "reasoning": "test"
        });
        let g = make_gen(llm_response.to_string());
        let context = "## 模拟需求\ntest requirement";
        let result = g.generate_time_config(context, 30).await;
        assert_eq!(result["total_simulation_hours"].as_i64().unwrap(), 48);
        assert_eq!(result["reasoning"].as_str().unwrap(), "test");
    }

    #[tokio::test]
    async fn generate_time_config_falls_back_to_default_on_llm_failure() {
        // Return non-JSON to trigger all repair attempts → final fallback
        let g = make_gen("not valid json at all !!!");
        let result = g.generate_time_config("context", 15).await;
        // Default config must have total_simulation_hours = 72
        assert_eq!(result["total_simulation_hours"].as_i64().unwrap(), 72);
        assert_eq!(result["reasoning"].as_str().unwrap(), "使用默认中国人作息配置（每轮1小时）");
    }

    // -----------------------------------------------------------------------
    // _generate_event_config (S-448) — mock integration
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_event_config_returns_parsed_llm_value() {
        let llm_response = serde_json::json!({
            "hot_topics": ["topic1", "topic2"],
            "narrative_direction": "方向",
            "initial_posts": [{"content": "post", "poster_type": "Person"}],
            "reasoning": "test"
        });
        let entities = vec![make_node("Alice", vec!["Entity", "Person"], "desc")];
        let g = make_gen(llm_response.to_string());
        let result = g.generate_event_config("context", "requirement", &entities).await;
        let arr = result["hot_topics"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(result["reasoning"].as_str().unwrap(), "test");
    }

    #[tokio::test]
    async fn generate_event_config_falls_back_to_empty_on_llm_failure() {
        let g = make_gen("not json !!!");
        let entities = vec![];
        let result = g.generate_event_config("context", "req", &entities).await;
        assert!(result["hot_topics"].as_array().unwrap().is_empty());
        assert_eq!(result["narrative_direction"].as_str().unwrap(), "");
        assert!(result["initial_posts"].as_array().unwrap().is_empty());
    }

    // -----------------------------------------------------------------------
    // _get_default_time_config (S-446)
    // -----------------------------------------------------------------------

    #[test]
    fn get_default_time_config_shape() {
        let g = make_gen("{}");
        let cfg = g.get_default_time_config(30);
        assert_eq!(cfg["total_simulation_hours"].as_i64().unwrap(), 72);
        assert_eq!(cfg["minutes_per_round"].as_i64().unwrap(), 60);
        // agents_per_hour_min = max(1, 30 // 15) = 2
        assert_eq!(cfg["agents_per_hour_min"].as_i64().unwrap(), 2);
        // agents_per_hour_max = max(5, 30 // 5) = 6
        assert_eq!(cfg["agents_per_hour_max"].as_i64().unwrap(), 6);
        assert_eq!(cfg["peak_hours"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn get_default_time_config_min_one_for_zero_entities() {
        let g = make_gen("{}");
        let cfg = g.get_default_time_config(0);
        // max(1, 0 // 15) = 1; max(5, 0 // 5) = 5
        assert_eq!(cfg["agents_per_hour_min"].as_i64().unwrap(), 1);
        assert_eq!(cfg["agents_per_hour_max"].as_i64().unwrap(), 5);
    }

    // -----------------------------------------------------------------------
    // Class constants (S-431..S-437)
    // -----------------------------------------------------------------------

    #[test]
    fn class_constants_match_python() {
        assert_eq!(SimulationConfigGenerator::<MockLlm>::MAX_CONTEXT_LENGTH, 50_000);
        assert_eq!(SimulationConfigGenerator::<MockLlm>::AGENTS_PER_BATCH, 15);
        assert_eq!(SimulationConfigGenerator::<MockLlm>::TIME_CONFIG_CONTEXT_LENGTH, 10_000);
        assert_eq!(SimulationConfigGenerator::<MockLlm>::EVENT_CONFIG_CONTEXT_LENGTH, 8_000);
        assert_eq!(SimulationConfigGenerator::<MockLlm>::ENTITY_SUMMARY_LENGTH, 300);
        assert_eq!(SimulationConfigGenerator::<MockLlm>::AGENT_SUMMARY_LENGTH, 300);
        assert_eq!(SimulationConfigGenerator::<MockLlm>::ENTITIES_PER_TYPE_DISPLAY, 20);
    }

    // -----------------------------------------------------------------------
    // _assign_initial_post_agents (S-450) tests
    // -----------------------------------------------------------------------

    fn make_agent(agent_id: i64, entity_type: &str, influence: f64) -> AgentActivityConfig {
        AgentActivityConfig {
            agent_id,
            entity_uuid: format!("uuid-{agent_id}"),
            entity_name: format!("Agent{agent_id}"),
            entity_type: entity_type.to_string(),
            activity_level: 0.5,
            posts_per_hour: 1.0,
            comments_per_hour: 2.0,
            active_hours: (8..23).collect(),
            response_delay_min: 5,
            response_delay_max: 60,
            sentiment_bias: 0.0,
            stance: "neutral".to_string(),
            influence_weight: influence,
        }
    }

    fn make_event_with_posts(posts: Vec<Value>) -> EventConfig {
        EventConfig {
            initial_posts: posts,
            scheduled_events: vec![],
            hot_topics: vec![],
            narrative_direction: String::new(),
        }
    }

    #[test]
    fn assign_initial_post_agents_empty_posts_returns_unchanged() {
        let g = make_gen("{}");
        let event = EventConfig::default();
        let agents = vec![make_agent(1, "student", 1.0)];
        let result = g.assign_initial_post_agents(event, &agents);
        assert!(result.initial_posts.is_empty());
    }

    #[test]
    fn assign_initial_post_agents_direct_match() {
        let g = make_gen("{}");
        let agents = vec![make_agent(42, "Student", 1.0)];
        let posts = vec![serde_json::json!({"content": "hello", "poster_type": "Student"})];
        let event = make_event_with_posts(posts);
        let result = g.assign_initial_post_agents(event, &agents);
        let p = &result.initial_posts[0];
        assert_eq!(p["poster_agent_id"].as_i64().unwrap(), 42);
        // Preserves ORIGINAL cased poster_type
        assert_eq!(p["poster_type"].as_str().unwrap(), "Student");
        assert_eq!(p["content"].as_str().unwrap(), "hello");
    }

    #[test]
    fn assign_initial_post_agents_direct_match_round_robin() {
        let g = make_gen("{}");
        // Two student agents
        let agents = vec![make_agent(10, "student", 1.0), make_agent(11, "student", 1.0)];
        let posts = vec![
            serde_json::json!({"content": "a", "poster_type": "student"}),
            serde_json::json!({"content": "b", "poster_type": "student"}),
            serde_json::json!({"content": "c", "poster_type": "student"}),
        ];
        let event = make_event_with_posts(posts);
        let result = g.assign_initial_post_agents(event, &agents);
        // Round-robin: post0→agent10 (idx=0), post1→agent11 (idx=1), post2→agent10 (idx=0)
        assert_eq!(result.initial_posts[0]["poster_agent_id"].as_i64().unwrap(), 10);
        assert_eq!(result.initial_posts[1]["poster_agent_id"].as_i64().unwrap(), 11);
        assert_eq!(result.initial_posts[2]["poster_agent_id"].as_i64().unwrap(), 10);
    }

    #[test]
    fn assign_initial_post_agents_alias_match_media() {
        let g = make_gen("{}");
        // No "media" agents directly, but "mediaoutlet" is present
        let agents = vec![make_agent(5, "mediaoutlet", 2.5)];
        // poster_type "media" should alias-match "mediaoutlet" (the mediaoutlet alias group contains "media")
        let posts = vec![serde_json::json!({"content": "news", "poster_type": "media"})];
        let event = make_event_with_posts(posts);
        let result = g.assign_initial_post_agents(event, &agents);
        assert_eq!(result.initial_posts[0]["poster_agent_id"].as_i64().unwrap(), 5);
    }

    #[test]
    fn assign_initial_post_agents_alias_match_by_alias_key() {
        let g = make_gen("{}");
        // poster_type == "official" (an alias_key); the aliases list is
        // ["official","university","governmentagency","government"].
        // agents_by_type has "official" → should match via direct OR alias
        let agents = vec![make_agent(7, "official", 2.0)];
        let posts = vec![serde_json::json!({"content": "statement", "poster_type": "official"})];
        let event = make_event_with_posts(posts);
        let result = g.assign_initial_post_agents(event, &agents);
        // Direct match (official in agents_by_type)
        assert_eq!(result.initial_posts[0]["poster_agent_id"].as_i64().unwrap(), 7);
    }

    #[test]
    fn assign_initial_post_agents_alias_poster_type_university_in_official_group() {
        // poster_type "university" appears in the "official" alias group
        // AND "university" is also its own alias_key with aliases ["university","official"]
        // First matching group in insertion order wins → "official" group is index 0
        let g = make_gen("{}");
        let agents = vec![make_agent(3, "official", 2.0)];
        let posts = vec![serde_json::json!({"content": "x", "poster_type": "university"})];
        let event = make_event_with_posts(posts);
        let result = g.assign_initial_post_agents(event, &agents);
        // The "official" alias group (first in table) lists "official" first; "official" is in agents_by_type
        assert_eq!(result.initial_posts[0]["poster_agent_id"].as_i64().unwrap(), 3);
    }

    #[test]
    fn assign_initial_post_agents_continues_past_empty_alias_group() {
        // Regression (opus parity gate, sub-cycle c FAIL→fix): a poster_type can match an
        // alias GROUP whose member types are all absent, while a LATER group resolves.
        // Python only breaks the outer loop when a match was actually found, so it keeps
        // scanning; an earlier unconditional break wrongly fell through to influence-max.
        //
        // poster_type "person" first matches the "student" group ["student","person"]
        // (insertion index 3) — both absent here — then the "alumni" group
        // ["alumni","person"] (index 5), where "alumni" IS present → agent 100.
        // The influence-max fallback would instead pick the higher-influence "official"
        // agent 200, so this test distinguishes the correct alias-scan from the bug.
        let g = make_gen("{}");
        let agents = vec![make_agent(100, "alumni", 1.0), make_agent(200, "official", 9.0)];
        let posts = vec![serde_json::json!({"content": "hi", "poster_type": "person"})];
        let event = make_event_with_posts(posts);
        let result = g.assign_initial_post_agents(event, &agents);
        assert_eq!(result.initial_posts[0]["poster_agent_id"].as_i64().unwrap(), 100);
    }

    #[test]
    fn assign_initial_post_agents_influence_fallback_no_match() {
        let g = make_gen("{}");
        // poster_type "alien" — no direct or alias match
        // agents: three with different influences; highest wins, ties → first in original order
        let agents = vec![
            make_agent(1, "person", 1.0),
            make_agent(2, "student", 2.0), // highest
            make_agent(3, "alumni", 1.5),
        ];
        let posts = vec![serde_json::json!({"content": "hi", "poster_type": "alien"})];
        let event = make_event_with_posts(posts);
        let result = g.assign_initial_post_agents(event, &agents);
        // agent 2 has highest influence_weight 2.0
        assert_eq!(result.initial_posts[0]["poster_agent_id"].as_i64().unwrap(), 2);
    }

    #[test]
    fn assign_initial_post_agents_influence_fallback_tie_first_wins() {
        let g = make_gen("{}");
        // Two agents with identical influence; first in original order should win (stable sort)
        let agents = vec![
            make_agent(10, "person", 2.0),  // tied highest, FIRST
            make_agent(11, "student", 2.0), // tied highest, second
        ];
        let posts = vec![serde_json::json!({"content": "hi", "poster_type": "alien"})];
        let event = make_event_with_posts(posts);
        let result = g.assign_initial_post_agents(event, &agents);
        // Stable: first in original order wins the tie
        assert_eq!(result.initial_posts[0]["poster_agent_id"].as_i64().unwrap(), 10);
    }

    #[test]
    fn assign_initial_post_agents_influence_fallback_no_agents() {
        let g = make_gen("{}");
        let posts = vec![serde_json::json!({"content": "hi", "poster_type": "alien"})];
        let event = make_event_with_posts(posts);
        let result = g.assign_initial_post_agents(event, &[]);
        // No agents → poster_agent_id = 0
        assert_eq!(result.initial_posts[0]["poster_agent_id"].as_i64().unwrap(), 0);
    }

    #[test]
    fn assign_initial_post_agents_default_poster_type_unknown() {
        let g = make_gen("{}");
        // Post with no "poster_type" key → original_poster_type defaults to "Unknown"
        let posts = vec![serde_json::json!({"content": "missing type"})];
        let event = make_event_with_posts(posts);
        let agents = vec![make_agent(1, "student", 1.0)];
        let result = g.assign_initial_post_agents(event, &agents);
        assert_eq!(result.initial_posts[0]["poster_type"].as_str().unwrap(), "Unknown");
    }

    #[test]
    fn assign_initial_post_agents_preserves_original_casing() {
        let g = make_gen("{}");
        let agents = vec![make_agent(5, "mediaoutlet", 2.5)];
        // Original poster_type uses PascalCase "MediaOutlet"
        let posts = vec![serde_json::json!({"content": "news", "poster_type": "MediaOutlet"})];
        let event = make_event_with_posts(posts);
        let result = g.assign_initial_post_agents(event, &agents);
        // poster_type in output must be the ORIGINAL cased value, not lowercased
        assert_eq!(result.initial_posts[0]["poster_type"].as_str().unwrap(), "MediaOutlet");
    }

    // -----------------------------------------------------------------------
    // _generate_agent_config_by_rule (S-452) tests
    // -----------------------------------------------------------------------

    fn make_node_with_label(name: &str, label: &str) -> EntityNode {
        EntityNode::new("uuid", name, vec![label.to_string()], "summary", Map::new())
    }

    #[test]
    fn generate_agent_config_by_rule_university() {
        let g = make_gen("{}");
        let entity = make_node_with_label("NTU", "University");
        let cfg = g.generate_agent_config_by_rule(&entity);
        assert_eq!(cfg["activity_level"].as_f64().unwrap(), 0.2);
        assert_eq!(cfg["posts_per_hour"].as_f64().unwrap(), 0.1);
        assert_eq!(cfg["comments_per_hour"].as_f64().unwrap(), 0.05);
        let hours: Vec<i64> = serde_json::from_value(cfg["active_hours"].clone()).unwrap();
        assert_eq!(hours, (9i64..18).collect::<Vec<_>>(), "university active_hours = range(9,18)");
        assert_eq!(cfg["response_delay_min"].as_i64().unwrap(), 60);
        assert_eq!(cfg["response_delay_max"].as_i64().unwrap(), 240);
        assert_eq!(cfg["sentiment_bias"].as_f64().unwrap(), 0.0);
        assert_eq!(cfg["stance"].as_str().unwrap(), "neutral");
        assert_eq!(cfg["influence_weight"].as_f64().unwrap(), 3.0);
    }

    #[test]
    fn generate_agent_config_by_rule_governmentagency() {
        let g = make_gen("{}");
        let entity = make_node_with_label("Gov", "GovernmentAgency");
        let cfg = g.generate_agent_config_by_rule(&entity);
        assert_eq!(cfg["influence_weight"].as_f64().unwrap(), 3.0);
        let hours: Vec<i64> = serde_json::from_value(cfg["active_hours"].clone()).unwrap();
        assert_eq!(hours, (9i64..18).collect::<Vec<_>>());
    }

    #[test]
    fn generate_agent_config_by_rule_ngo() {
        let g = make_gen("{}");
        let entity = make_node_with_label("NGO", "ngo");
        let cfg = g.generate_agent_config_by_rule(&entity);
        assert_eq!(cfg["influence_weight"].as_f64().unwrap(), 3.0);
    }

    #[test]
    fn generate_agent_config_by_rule_mediaoutlet() {
        let g = make_gen("{}");
        let entity = make_node_with_label("CNN", "MediaOutlet");
        let cfg = g.generate_agent_config_by_rule(&entity);
        assert_eq!(cfg["activity_level"].as_f64().unwrap(), 0.5);
        assert_eq!(cfg["posts_per_hour"].as_f64().unwrap(), 0.8);
        assert_eq!(cfg["comments_per_hour"].as_f64().unwrap(), 0.3);
        let hours: Vec<i64> = serde_json::from_value(cfg["active_hours"].clone()).unwrap();
        assert_eq!(hours, (7i64..24).collect::<Vec<_>>(), "mediaoutlet active_hours = range(7,24)");
        assert_eq!(cfg["response_delay_min"].as_i64().unwrap(), 5);
        assert_eq!(cfg["response_delay_max"].as_i64().unwrap(), 30);
        assert_eq!(cfg["stance"].as_str().unwrap(), "observer");
        assert_eq!(cfg["influence_weight"].as_f64().unwrap(), 2.5);
    }

    #[test]
    fn generate_agent_config_by_rule_professor() {
        let g = make_gen("{}");
        let entity = make_node_with_label("Prof Chen", "Professor");
        let cfg = g.generate_agent_config_by_rule(&entity);
        assert_eq!(cfg["activity_level"].as_f64().unwrap(), 0.4);
        assert_eq!(cfg["posts_per_hour"].as_f64().unwrap(), 0.3);
        assert_eq!(cfg["comments_per_hour"].as_f64().unwrap(), 0.5);
        let hours: Vec<i64> = serde_json::from_value(cfg["active_hours"].clone()).unwrap();
        assert_eq!(hours, (8i64..22).collect::<Vec<_>>(), "professor active_hours = range(8,22)");
        assert_eq!(cfg["response_delay_min"].as_i64().unwrap(), 15);
        assert_eq!(cfg["response_delay_max"].as_i64().unwrap(), 90);
        assert_eq!(cfg["stance"].as_str().unwrap(), "neutral");
        assert_eq!(cfg["influence_weight"].as_f64().unwrap(), 2.0);
    }

    #[test]
    fn generate_agent_config_by_rule_expert() {
        let g = make_gen("{}");
        let entity = make_node_with_label("Dr X", "Expert");
        let cfg = g.generate_agent_config_by_rule(&entity);
        assert_eq!(cfg["influence_weight"].as_f64().unwrap(), 2.0);
        let hours: Vec<i64> = serde_json::from_value(cfg["active_hours"].clone()).unwrap();
        assert_eq!(hours, (8i64..22).collect::<Vec<_>>());
    }

    #[test]
    fn generate_agent_config_by_rule_student() {
        let g = make_gen("{}");
        let entity = make_node_with_label("Alice", "Student");
        let cfg = g.generate_agent_config_by_rule(&entity);
        assert_eq!(cfg["activity_level"].as_f64().unwrap(), 0.8);
        assert_eq!(cfg["posts_per_hour"].as_f64().unwrap(), 0.6);
        assert_eq!(cfg["comments_per_hour"].as_f64().unwrap(), 1.5);
        let hours: Vec<i64> = serde_json::from_value(cfg["active_hours"].clone()).unwrap();
        assert_eq!(
            hours,
            vec![8, 9, 10, 11, 12, 13, 18, 19, 20, 21, 22, 23],
            "student active_hours explicit list"
        );
        assert_eq!(cfg["response_delay_min"].as_i64().unwrap(), 1);
        assert_eq!(cfg["response_delay_max"].as_i64().unwrap(), 15);
        assert_eq!(cfg["influence_weight"].as_f64().unwrap(), 0.8);
    }

    #[test]
    fn generate_agent_config_by_rule_alumni() {
        let g = make_gen("{}");
        let entity = make_node_with_label("Bob", "Alumni");
        let cfg = g.generate_agent_config_by_rule(&entity);
        assert_eq!(cfg["activity_level"].as_f64().unwrap(), 0.6);
        assert_eq!(cfg["posts_per_hour"].as_f64().unwrap(), 0.4);
        assert_eq!(cfg["comments_per_hour"].as_f64().unwrap(), 0.8);
        let hours: Vec<i64> = serde_json::from_value(cfg["active_hours"].clone()).unwrap();
        assert_eq!(hours, vec![12, 13, 19, 20, 21, 22, 23], "alumni active_hours explicit list");
        assert_eq!(cfg["response_delay_min"].as_i64().unwrap(), 5);
        assert_eq!(cfg["response_delay_max"].as_i64().unwrap(), 30);
        assert_eq!(cfg["influence_weight"].as_f64().unwrap(), 1.0);
    }

    #[test]
    fn generate_agent_config_by_rule_else_person() {
        let g = make_gen("{}");
        let entity = make_node_with_label("普通人", "Person");
        let cfg = g.generate_agent_config_by_rule(&entity);
        assert_eq!(cfg["activity_level"].as_f64().unwrap(), 0.7);
        assert_eq!(cfg["posts_per_hour"].as_f64().unwrap(), 0.5);
        assert_eq!(cfg["comments_per_hour"].as_f64().unwrap(), 1.2);
        let hours: Vec<i64> = serde_json::from_value(cfg["active_hours"].clone()).unwrap();
        assert_eq!(
            hours,
            vec![9, 10, 11, 12, 13, 18, 19, 20, 21, 22, 23],
            "else active_hours explicit list"
        );
        assert_eq!(cfg["response_delay_min"].as_i64().unwrap(), 2);
        assert_eq!(cfg["response_delay_max"].as_i64().unwrap(), 20);
        assert_eq!(cfg["influence_weight"].as_f64().unwrap(), 1.0);
    }

    #[test]
    fn generate_agent_config_by_rule_unknown_type_falls_through() {
        let g = make_gen("{}");
        // Unknown type → else branch (普通人)
        let entity = make_node_with_label("X", "SomeRandomType");
        let cfg = g.generate_agent_config_by_rule(&entity);
        assert_eq!(cfg["activity_level"].as_f64().unwrap(), 0.7);
        assert_eq!(cfg["influence_weight"].as_f64().unwrap(), 1.0);
    }

    // -----------------------------------------------------------------------
    // _generate_agent_configs_batch (S-451) tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn generate_agent_configs_batch_happy_path_uses_llm_values() {
        // LLM returns valid agent_configs for two entities
        let llm_json = serde_json::json!({
            "agent_configs": [
                {
                    "agent_id": 0,
                    "activity_level": 0.9,
                    "posts_per_hour": 1.2,
                    "comments_per_hour": 2.1,
                    "active_hours": [8, 9, 18, 19, 20],
                    "response_delay_min": 3,
                    "response_delay_max": 30,
                    "sentiment_bias": 0.1,
                    "stance": "supportive",
                    "influence_weight": 1.5,
                },
                {
                    "agent_id": 1,
                    "activity_level": 0.3,
                    "posts_per_hour": 0.2,
                    "comments_per_hour": 0.1,
                    "active_hours": [9, 10, 11, 12, 13, 14, 15, 16, 17],
                    "response_delay_min": 60,
                    "response_delay_max": 180,
                    "sentiment_bias": 0.0,
                    "stance": "neutral",
                    "influence_weight": 3.0,
                }
            ]
        });
        let g = make_gen(llm_json.to_string());
        let entities = vec![
            make_node("Alice", vec!["Entity", "Student"], "student"),
            make_node("Uni", vec!["Entity", "University"], "uni"),
        ];
        let configs = g
            .generate_agent_configs_batch("context", &entities, 0, "test requirement")
            .await;

        assert_eq!(configs.len(), 2);
        // entity 0: Alice, LLM values
        assert_eq!(configs[0].agent_id, 0);
        assert_eq!(configs[0].activity_level, 0.9);
        assert_eq!(configs[0].posts_per_hour, 1.2);
        assert_eq!(configs[0].stance, "supportive");
        assert_eq!(configs[0].active_hours, vec![8, 9, 18, 19, 20]);
        // entity 1: Uni, LLM values
        assert_eq!(configs[1].agent_id, 1);
        assert_eq!(configs[1].influence_weight, 3.0);
        assert_eq!(configs[1].stance, "neutral");
    }

    #[tokio::test]
    async fn generate_agent_configs_batch_llm_failure_falls_back_to_rules() {
        // LLM returns invalid JSON → all entries use rule-based fallback
        let g = make_gen("not json at all!!!");
        let entities = vec![
            make_node("Alice", vec!["Entity", "Student"], "student desc"),
            make_node("CNN", vec!["Entity", "MediaOutlet"], "media desc"),
        ];
        let configs = g.generate_agent_configs_batch("context", &entities, 0, "test").await;
        assert_eq!(configs.len(), 2);
        // Alice (Student) → rule: activity_level=0.8
        assert_eq!(configs[0].agent_id, 0);
        assert_eq!(configs[0].activity_level, 0.8);
        assert_eq!(configs[0].posts_per_hour, 0.6);
        assert_eq!(configs[0].comments_per_hour, 1.5);
        assert_eq!(configs[0].active_hours, vec![8, 9, 10, 11, 12, 13, 18, 19, 20, 21, 22, 23]);
        // CNN (MediaOutlet) → rule: activity_level=0.5
        assert_eq!(configs[1].agent_id, 1);
        assert_eq!(configs[1].activity_level, 0.5);
        assert_eq!(configs[1].stance, "observer");
    }

    #[tokio::test]
    async fn generate_agent_configs_batch_missing_agent_id_in_llm_response_falls_back() {
        // LLM returns configs for agent_id=0 but not agent_id=1
        let llm_json = serde_json::json!({
            "agent_configs": [
                {
                    "agent_id": 0,
                    "activity_level": 0.99,
                    "posts_per_hour": 0.5,
                    "comments_per_hour": 1.0,
                    "active_hours": [10, 11, 12],
                    "response_delay_min": 5,
                    "response_delay_max": 60,
                    "sentiment_bias": 0.0,
                    "stance": "opposing",
                    "influence_weight": 1.0,
                }
                // agent_id=1 is absent
            ]
        });
        let g = make_gen(llm_json.to_string());
        let entities = vec![
            make_node("Alice", vec!["Entity", "Student"], "s"),
            make_node("Uni", vec!["Entity", "University"], "u"),
        ];
        let configs = g.generate_agent_configs_batch("context", &entities, 0, "test").await;
        assert_eq!(configs.len(), 2);
        // agent 0 from LLM
        assert_eq!(configs[0].activity_level, 0.99);
        // agent 1 missing → rule for University: activity_level=0.2, influence=3.0
        assert_eq!(configs[1].activity_level, 0.2);
        assert_eq!(configs[1].influence_weight, 3.0);
    }

    #[tokio::test]
    async fn generate_agent_configs_batch_defaults_differ_from_dataclass() {
        // When the LLM response for an agent omits fields, we use the BATCH defaults (not struct defaults).
        // Batch defaults: posts_per_hour=0.5 (struct default=1.0), comments_per_hour=1.0 (struct=2.0),
        // active_hours=[9..=22] 14 elements (struct default=[8..=22] 15 elements).
        let llm_json = serde_json::json!({
            "agent_configs": [
                {
                    "agent_id": 5,
                    // No posts_per_hour, comments_per_hour, or active_hours → use batch defaults
                    "activity_level": 0.5,
                    "response_delay_min": 5,
                    "response_delay_max": 60,
                    "sentiment_bias": 0.0,
                    "stance": "neutral",
                    "influence_weight": 1.0,
                }
            ]
        });
        let g = make_gen(llm_json.to_string());
        let entities = vec![make_node("Entity5", vec!["Entity", "Person"], "desc")];
        let configs = g.generate_agent_configs_batch("ctx", &entities, 5, "test").await;
        assert_eq!(configs.len(), 1);
        let c = &configs[0];
        assert_eq!(c.agent_id, 5);
        // Batch defaults (NOT struct defaults)
        assert_eq!(c.posts_per_hour, 0.5, "batch default posts_per_hour must be 0.5, not 1.0");
        assert_eq!(
            c.comments_per_hour, 1.0,
            "batch default comments_per_hour must be 1.0, not 2.0"
        );
        assert_eq!(
            c.active_hours,
            (9i64..23).collect::<Vec<_>>(),
            "batch default active_hours=[9..=22]"
        );
        assert_eq!(c.active_hours.len(), 14, "batch default active_hours has 14 elements");
    }

    #[tokio::test]
    async fn generate_agent_configs_batch_start_idx_applied() {
        // start_idx=10: agent_ids should be 10, 11
        let llm_json = serde_json::json!({
            "agent_configs": [
                {"agent_id": 10, "activity_level": 0.5, "posts_per_hour": 0.5,
                 "comments_per_hour": 1.0, "active_hours": [9,10],
                 "response_delay_min": 5, "response_delay_max": 60,
                 "sentiment_bias": 0.0, "stance": "neutral", "influence_weight": 1.0},
                {"agent_id": 11, "activity_level": 0.3, "posts_per_hour": 0.1,
                 "comments_per_hour": 0.05, "active_hours": [9,10,11,12,13,14,15,16,17],
                 "response_delay_min": 60, "response_delay_max": 240,
                 "sentiment_bias": 0.0, "stance": "neutral", "influence_weight": 3.0}
            ]
        });
        let g = make_gen(llm_json.to_string());
        let entities = vec![
            make_node("Alice", vec!["Entity", "Student"], "s"),
            make_node("Uni", vec!["Entity", "University"], "u"),
        ];
        let configs = g.generate_agent_configs_batch("ctx", &entities, 10, "req").await;
        assert_eq!(configs[0].agent_id, 10);
        assert_eq!(configs[1].agent_id, 11);
    }

    #[tokio::test]
    async fn generate_agent_configs_batch_entity_uuid_and_name_preserved() {
        let g = make_gen("{\"agent_configs\": []}"); // empty → all use rules
        let mut entity = make_node("TestEntity", vec!["Entity", "Alumni"], "desc");
        entity.uuid = "my-uuid-123".to_string();
        let configs = g.generate_agent_configs_batch("ctx", &[entity], 0, "req").await;
        assert_eq!(configs[0].entity_uuid, "my-uuid-123");
        assert_eq!(configs[0].entity_name, "TestEntity");
    }

    #[tokio::test]
    async fn generate_agent_configs_batch_summary_truncated_to_agent_summary_length() {
        // Verify that entity summaries longer than AGENT_SUMMARY_LENGTH (300 chars) are
        // truncated char-by-char (not byte-by-byte) when building the prompt.
        // We can't inspect the prompt directly, but we can verify the call succeeds
        // and the returned config is valid (smoke test).
        let long_summary = "A".repeat(500); // 500 chars > 300 AGENT_SUMMARY_LENGTH
        let g = make_gen("{\"agent_configs\": []}");
        let entity = make_node("LongSummaryEntity", vec!["Entity", "Student"], &long_summary);
        let configs = g.generate_agent_configs_batch("ctx", &[entity], 0, "req").await;
        assert_eq!(configs.len(), 1);
        // rule-based for Student
        assert_eq!(configs[0].activity_level, 0.8);
    }

    // -----------------------------------------------------------------------
    // generate_config (S-439) tests
    // -----------------------------------------------------------------------

    /// Build a mock LLM that returns fixed time/event/agent JSON in sequence.
    ///
    /// The mock always returns the same JSON regardless of which method calls it —
    /// each stage calls `chat` once per invocation and reads the fields it needs,
    /// ignoring unknown ones. We compose a response that satisfies all three stage
    /// parsers simultaneously.
    fn make_multi_stage_gen(n_entities: usize) -> (SimulationConfigGenerator<MockLlm>, String) {
        // Build agent_configs entries that cover all n_entities agent_ids
        let agent_cfgs: Vec<serde_json::Value> = (0..n_entities)
            .map(|i| {
                serde_json::json!({
                    "agent_id": i as i64,
                    "activity_level": 0.5_f64,
                    "posts_per_hour": 0.5_f64,
                    "comments_per_hour": 1.0_f64,
                    "active_hours": [9, 10, 11, 12, 18, 19, 20, 21, 22],
                    "response_delay_min": 5_i64,
                    "response_delay_max": 60_i64,
                    "sentiment_bias": 0.0_f64,
                    "stance": "neutral",
                    "influence_weight": 1.0_f64,
                })
            })
            .collect();

        // One JSON blob that works for time config, event config, AND agent_configs batch —
        // parsers only read their own fields and ignore the rest.
        let combined = serde_json::json!({
            // time config fields
            "total_simulation_hours": 48_i64,
            "minutes_per_round": 60_i64,
            "agents_per_hour_min": 2_i64,
            "agents_per_hour_max": 8_i64,
            "peak_hours": [19_i64, 20, 21, 22],
            "off_peak_hours": [0_i64, 1, 2, 3, 4, 5],
            "morning_hours": [6_i64, 7, 8],
            "work_hours": [9_i64, 10, 11, 12, 13, 14, 15, 16, 17, 18],
            "reasoning": "test-time-reasoning",
            // event config fields
            "hot_topics": ["AI", "教育"],
            "narrative_direction": "积极",
            "initial_posts": [
                {"content": "post1", "poster_type": "Student"}
            ],
            // agent_configs batch fields
            "agent_configs": agent_cfgs,
        });
        let json_str = combined.to_string();
        let g = make_gen(json_str.clone());
        (g, json_str)
    }

    #[tokio::test]
    async fn generate_config_total_steps_formula() {
        // total_steps = 3 + ceil(n / 15)
        // For N=17 entities: ceil(17/15)=2 → total_steps=5
        let n = 17usize;
        let (g, _) = make_multi_stage_gen(n);
        let entities: Vec<EntityNode> = (0..n)
            .map(|i| make_node(&format!("E{i}"), vec!["Entity", "Student"], "desc"))
            .collect();

        let mut steps_received: Vec<(i64, i64, String)> = Vec::new();
        let params = g
            .generate_config(
                "sim-1",
                "proj-1",
                "graph-1",
                "requirement",
                "doc text",
                &entities,
                true,
                true,
                Some(&mut |step, total, msg| {
                    steps_received.push((step, total, msg.to_string()));
                }),
            )
            .await;

        // Expected total_steps = 3 + ceil(17/15) = 3 + 2 = 5
        let expected_total: i64 = 5;
        assert_eq!(params.agent_configs.len(), n, "agent_configs length must equal entity count");

        // Every callback invocation must report the same total_steps
        for (_, total, _) in &steps_received {
            assert_eq!(
                *total, expected_total,
                "all callbacks must see total_steps={expected_total}"
            );
        }

        // Step sequence: 1, 2, 3 (batch 0), 4 (batch 1), 5 (platform)
        let steps: Vec<i64> = steps_received.iter().map(|(s, _, _)| *s).collect();
        assert_eq!(steps, vec![1, 2, 3, 4, 5], "step sequence must be [1,2,3,4,5]");
    }

    #[tokio::test]
    async fn generate_config_zero_entities_total_steps_3() {
        // 0 entities → ceil(0/15)=0 → total_steps=3; no agent-batch steps.
        let (g, _) = make_multi_stage_gen(0);
        let entities: Vec<EntityNode> = vec![];

        let mut steps_received: Vec<(i64, i64)> = Vec::new();
        let params = g
            .generate_config(
                "sim-z",
                "proj-z",
                "graph-z",
                "req",
                "",
                &entities,
                true,
                true,
                Some(&mut |step, total, _| {
                    steps_received.push((step, total));
                }),
            )
            .await;

        assert_eq!(params.agent_configs.len(), 0, "zero entities → zero agent configs");
        // Callbacks: step 1 (time), step 2 (event), step 3 (platform) — no batch steps.
        let steps: Vec<i64> = steps_received.iter().map(|(s, _)| *s).collect();
        assert_eq!(steps, vec![1, 2, 3], "zero entities: step sequence must be [1,2,3]");
        let totals: Vec<i64> = steps_received.iter().map(|(_, t)| *t).collect();
        assert!(totals.iter().all(|&t| t == 3), "all totals must be 3");
    }

    #[tokio::test]
    async fn generate_config_twitter_config_present_with_correct_values() {
        let (g, _) = make_multi_stage_gen(1);
        let entities = vec![make_node("E0", vec!["Entity", "Student"], "d")];
        let params = g.generate_config("s", "p", "g", "r", "", &entities, true, true, None).await;

        let tw = params
            .twitter_config
            .expect("twitter_config must be present when enable_twitter=true");
        assert_eq!(tw.platform, "twitter");
        assert_eq!(tw.recency_weight, 0.4);
        assert_eq!(tw.popularity_weight, 0.3);
        assert_eq!(tw.relevance_weight, 0.3);
        assert_eq!(tw.viral_threshold, 10);
        assert_eq!(tw.echo_chamber_strength, 0.5);
    }

    #[tokio::test]
    async fn generate_config_reddit_config_non_default_literals() {
        // Reddit config has VALUES that differ from PlatformConfig struct defaults:
        //   recency=0.3 (default 0.4), popularity=0.4 (default 0.3), viral=15 (default 10), echo=0.6 (default 0.5).
        let (g, _) = make_multi_stage_gen(1);
        let entities = vec![make_node("E0", vec!["Entity", "Student"], "d")];
        let params = g.generate_config("s", "p", "g", "r", "", &entities, true, true, None).await;

        let rd = params
            .reddit_config
            .expect("reddit_config must be present when enable_reddit=true");
        assert_eq!(rd.platform, "reddit");
        assert_eq!(rd.recency_weight, 0.3, "reddit recency_weight must be 0.3 (not default 0.4)");
        assert_eq!(
            rd.popularity_weight, 0.4,
            "reddit popularity_weight must be 0.4 (not default 0.3)"
        );
        assert_eq!(rd.relevance_weight, 0.3);
        assert_eq!(rd.viral_threshold, 15, "reddit viral_threshold must be 15 (not default 10)");
        assert_eq!(
            rd.echo_chamber_strength, 0.6,
            "reddit echo_chamber_strength must be 0.6 (not default 0.5)"
        );
    }

    #[tokio::test]
    async fn generate_config_enable_twitter_false_no_twitter_config() {
        let (g, _) = make_multi_stage_gen(1);
        let entities = vec![make_node("E0", vec!["Entity", "Student"], "d")];
        let params = g.generate_config("s", "p", "g", "r", "", &entities, false, true, None).await;
        assert!(
            params.twitter_config.is_none(),
            "twitter_config must be None when enable_twitter=false"
        );
        assert!(
            params.reddit_config.is_some(),
            "reddit_config must be Some when enable_reddit=true"
        );
    }

    #[tokio::test]
    async fn generate_config_enable_reddit_false_no_reddit_config() {
        let (g, _) = make_multi_stage_gen(1);
        let entities = vec![make_node("E0", vec!["Entity", "Student"], "d")];
        let params = g.generate_config("s", "p", "g", "r", "", &entities, true, false, None).await;
        assert!(
            params.reddit_config.is_none(),
            "reddit_config must be None when enable_reddit=false"
        );
        assert!(
            params.twitter_config.is_some(),
            "twitter_config must be Some when enable_twitter=true"
        );
    }

    #[tokio::test]
    async fn generate_config_both_disabled_no_platform_configs() {
        let (g, _) = make_multi_stage_gen(1);
        let entities = vec![make_node("E0", vec!["Entity", "Student"], "d")];
        let params = g.generate_config("s", "p", "g", "r", "", &entities, false, false, None).await;
        assert!(params.twitter_config.is_none());
        assert!(params.reddit_config.is_none());
    }

    #[tokio::test]
    async fn generate_config_generation_reasoning_joins_with_pipe() {
        // reasoning_parts joined by " | "; must contain all 4 sections.
        let n = 2usize;
        let (g, _) = make_multi_stage_gen(n);
        let entities: Vec<EntityNode> = (0..n)
            .map(|i| make_node(&format!("E{i}"), vec!["Entity", "Student"], "d"))
            .collect();
        let params = g.generate_config("s", "p", "g", "r", "", &entities, true, true, None).await;

        let reasoning = &params.generation_reasoning;
        // Must contain the " | " separator
        assert!(reasoning.contains(" | "), "reasoning must join parts with \" | \"");
        // Must contain time config label
        assert!(
            reasoning.contains("Time Config") || reasoning.contains("时间配置"),
            "reasoning must contain time config label: {reasoning}"
        );
        // Must contain event config label
        assert!(
            reasoning.contains("Event Config") || reasoning.contains("事件配置"),
            "reasoning must contain event config label: {reasoning}"
        );
        // Must contain agent config result
        assert!(
            reasoning.contains("Agent Config") || reasoning.contains("Agent配置"),
            "reasoning must contain agent config result: {reasoning}"
        );
        // Must contain post assignment result
        assert!(
            reasoning.contains("Post Assignment") || reasoning.contains("帖子分配"),
            "reasoning must contain post assignment result: {reasoning}"
        );
        // Must have exactly 3 " | " separators (4 parts, 3 joins)
        assert_eq!(
            reasoning.matches(" | ").count(),
            3,
            "reasoning must have exactly 3 ' | ' separators (4 parts): {reasoning}"
        );
    }

    #[tokio::test]
    async fn generate_config_llm_model_and_base_url_in_params() {
        // llm_model and llm_base_url must be passed from self.model_name / self.base_url.
        let llm = MockLlm::always(
            serde_json::json!({
                "total_simulation_hours": 72_i64, "minutes_per_round": 60_i64,
                "agents_per_hour_min": 1_i64, "agents_per_hour_max": 5_i64,
                "peak_hours": [19_i64], "off_peak_hours": [0_i64],
                "morning_hours": [6_i64], "work_hours": [9_i64],
                "hot_topics": [], "narrative_direction": "", "initial_posts": [],
                "agent_configs": [],
            })
            .to_string(),
        );
        let g = SimulationConfigGenerator::new(llm, "my-model-x", "http://example.com");
        let entities = vec![make_node("E0", vec!["Entity", "Student"], "d")];
        let params = g.generate_config("s", "p", "g", "r", "", &entities, false, false, None).await;
        assert_eq!(params.llm_model, "my-model-x");
        assert_eq!(params.llm_base_url, "http://example.com");
    }

    #[tokio::test]
    async fn generate_config_simulation_id_project_graph_in_params() {
        let (g, _) = make_multi_stage_gen(0);
        let params = g
            .generate_config(
                "sim-123",
                "proj-456",
                "graph-789",
                "My requirement",
                "",
                &[],
                false,
                false,
                None,
            )
            .await;
        assert_eq!(params.simulation_id, "sim-123");
        assert_eq!(params.project_id, "proj-456");
        assert_eq!(params.graph_id, "graph-789");
        assert_eq!(params.simulation_requirement, "My requirement");
    }

    #[tokio::test]
    async fn generate_config_progress_callback_invoked_correct_sequence() {
        // N=16 → ceil(16/15)=2 → total_steps=5; steps = [1,2,3,4,5]
        let n = 16usize;
        let (g, _) = make_multi_stage_gen(n);
        let entities: Vec<EntityNode> = (0..n)
            .map(|i| make_node(&format!("E{i}"), vec!["Entity", "Student"], "d"))
            .collect();

        let mut calls: Vec<(i64, i64, String)> = Vec::new();
        g.generate_config(
            "s",
            "p",
            "g",
            "r",
            "",
            &entities,
            true,
            true,
            Some(&mut |step, total, msg| calls.push((step, total, msg.to_string()))),
        )
        .await;

        let steps: Vec<i64> = calls.iter().map(|(s, _, _)| *s).collect();
        let totals: Vec<i64> = calls.iter().map(|(_, t, _)| *t).collect();
        assert_eq!(steps, vec![1, 2, 3, 4, 5]);
        assert!(totals.iter().all(|&t| t == 5), "all totals must be 5");

        // Step 1 message must relate to time config
        let msg1 = &calls[0].2;
        assert!(
            msg1.contains("time") || msg1.contains("Time") || msg1.contains("时间"),
            "step 1 message must be about time config: {msg1}"
        );
        // Step 2 message must relate to event config
        let msg2 = &calls[1].2;
        assert!(
            msg2.contains("event") || msg2.contains("Event") || msg2.contains("事件"),
            "step 2 message must be about event config: {msg2}"
        );
        // Last step message must relate to platform config
        let msg_last = &calls[4].2;
        assert!(
            msg_last.contains("platform")
                || msg_last.contains("Platform")
                || msg_last.contains("平台"),
            "last step message must be about platform config: {msg_last}"
        );
    }

    #[tokio::test]
    async fn generate_config_agent_configs_length_equals_entities() {
        // All N=15 entity configs must be generated (exactly one batch).
        let n = 15usize;
        let (g, _) = make_multi_stage_gen(n);
        let entities: Vec<EntityNode> = (0..n)
            .map(|i| make_node(&format!("E{i}"), vec!["Entity", "Student"], "d"))
            .collect();
        let params = g.generate_config("s", "p", "g", "r", "", &entities, true, true, None).await;
        assert_eq!(params.agent_configs.len(), n);
        // ceil(15/15) = 1 → total_steps = 4
        // (not tested here — covered by generate_config_total_steps_formula)
    }

    #[tokio::test]
    async fn generate_config_generated_at_is_isoformat() {
        let (g, _) = make_multi_stage_gen(0);
        let params = g.generate_config("s", "p", "g", "r", "", &[], false, false, None).await;
        let ts = &params.generated_at;
        assert!(ts.len() >= 19, "generated_at must be at least 19 chars: {ts}");
        assert_eq!(&ts[10..11], "T", "generated_at must have T separator: {ts}");
        assert!(!ts.ends_with('Z'), "generated_at must be local naive: {ts}");
    }

    #[tokio::test]
    async fn generate_config_reasoning_uses_success_fallback_when_no_reasoning_key() {
        // When the LLM result lacks a "reasoning" key, t("common.success") is used.
        // The combined JSON used here has "reasoning": "test-time-reasoning" for time config
        // and also for event config — to test the fallback we use a response with no "reasoning".
        let no_reasoning = serde_json::json!({
            "total_simulation_hours": 72_i64, "minutes_per_round": 60_i64,
            "agents_per_hour_min": 1_i64, "agents_per_hour_max": 5_i64,
            "peak_hours": [19_i64], "off_peak_hours": [0_i64],
            "morning_hours": [6_i64], "work_hours": [9_i64],
            "hot_topics": [], "narrative_direction": "", "initial_posts": [],
            "agent_configs": [],
            // NOTE: no "reasoning" field
        })
        .to_string();
        let g = make_gen(no_reasoning);
        let entities = vec![make_node("E0", vec!["Entity", "Student"], "d")];
        let params = g.generate_config("s", "p", "g", "r", "", &entities, false, false, None).await;

        let reasoning = &params.generation_reasoning;
        // The t("common.success") fallback should appear in the reasoning string.
        // "Success" (en locale) or "成功" (zh locale) depending on locale.
        assert!(
            reasoning.contains("Success")
                || reasoning.contains("成功")
                || reasoning.contains("success"),
            "reasoning must contain common.success fallback when no reasoning key: {reasoning}"
        );
    }
}
