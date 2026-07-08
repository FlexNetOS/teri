use crate::error::{Result, TeriError};
use crate::llm::LlmClient;
use crate::sim::SimulationResult;
use async_stream::try_stream;
use futures::{Stream, StreamExt};
use minijinja::{Environment, context};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use uuid::Uuid;

const REPORT_TEMPLATE: &str = r#"You are a prediction analysis system that synthesizes simulation results into insightful reports.

## User Query
{{ query }}

## Simulation Summary
- Total Ticks: {{ total_ticks }}
- Unique Agents: {{ agent_count }}
- Total Events: {{ total_events }}

## Key Events from Simulation
{% for event in key_events %}
- Tick {{ event.tick }}: {{ event.description }} ({{ event.actor }})
{% endfor %}

## Agent Activity
{% for agent in agents %}
- {{ agent.name }}: {{ agent.action_count }} actions, State: {{ agent.final_state }}
{% endfor %}

## Task
Analyze the simulation to answer the user's query. Provide a structured prediction report.

Generate a JSON object with the following structure:
```json
{
    "summary": "string - 2-3 sentence synthesis of what happened and what it predicts for the query",
    "timeline": [
        {
            "tick": number,
            "description": "string - what happened at this tick that was significant",
            "significance": 0.0-1.0
        }
    ],
    "agent_highlights": [
        {
            "agent_id": "uuid string",
            "agent_name": "string",
            "summary": "string - 1-2 sentences about this agent's role and impact"
        }
    ],
    "confidence": 0.0-1.0
}
```

Respond with only the JSON object:
"#;

const AGENT_CHAT_TEMPLATE: &str = include_str!("../../templates/agent_chat.jinja");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub tick: u32,
    pub description: String,
    pub significance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHighlight {
    pub agent_id: Uuid,
    pub agent_name: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionReport {
    pub id: Uuid,
    pub summary: String,
    pub timeline: Vec<TimelineEvent>,
    pub agent_highlights: Vec<AgentHighlight>,
    pub confidence: f32,
    pub raw_query: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub sender: String,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub agent_id: Option<Uuid>,
}

/// Intent categories used by `ReportAgent::chat` to decide which simulation
/// context is relevant to a given message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChatIntent {
    Timeline,
    AgentSummary,
    Confidence,
    General,
}

pub struct ReportAgent;

impl ReportAgent {
    pub fn new() -> Self {
        Self
    }

    pub fn create_empty_report(query: String) -> PredictionReport {
        PredictionReport {
            id: Uuid::new_v4(),
            summary: String::new(),
            timeline: Vec::new(),
            agent_highlights: Vec::new(),
            confidence: 0.0,
            raw_query: query,
            created_at: chrono::Utc::now(),
        }
    }

    pub async fn generate_stream<L: LlmClient + ?Sized>(
        result: &SimulationResult,
        query: &str,
        llm: &L,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<PredictionReport>> + Send>>> {
        let env = Environment::new();
        let template = env
            .template_from_str(REPORT_TEMPLATE)
            .map_err(|e| TeriError::Report(format!("Template parsing error: {}", e)))?;

        let key_events = Self::extract_key_events(result);
        let agents = Self::summarize_agents(result);
        let total_ticks = result.final_snapshot().map(|s| s.tick).unwrap_or(0);
        let total_events: usize = result.history.iter().map(|s| s.events.len()).sum();

        let ctx = context! {
            query => query,
            total_ticks => total_ticks,
            agent_count => agents.len(),
            total_events => total_events,
            key_events => key_events,
            agents => agents,
        };

        let prompt = template
            .render(ctx)
            .map_err(|e| TeriError::Report(format!("Failed to render report template: {}", e)))?;

        let mut stream = llm.stream(&prompt).await?;
        let query_owned = query.to_string();

        let result_stream = try_stream! {
            let mut buffer = String::new();

            // Yield initial partial report to ensure ≥2 chunks
            yield PredictionReport {
                id: Uuid::new_v4(),
                summary: String::from("[Generating...]"),
                timeline: Vec::new(),
                agent_highlights: Vec::new(),
                confidence: 0.0,
                raw_query: query_owned.clone(),
                created_at: chrono::Utc::now(),
            };

            // Stream text chunks and accumulate
            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
                buffer.push_str(&chunk);

                // Try to parse complete JSON when buffer is large enough
                if buffer.len() > 100 && buffer.contains("}")
                    && let Ok(response) = serde_json::from_str::<serde_json::Value>(&buffer)
                    && let Some(report) = Self::parse_report_from_json(&response, &query_owned) {
                    yield report;
                    return;
                }
            }

            // Final parsing with complete buffer
            if let Ok(response) = serde_json::from_str::<serde_json::Value>(&buffer)
                && let Some(report) = Self::parse_report_from_json(&response, &query_owned) {
                yield report;
                return;
            }

            // If we get here, return error
            Err(TeriError::Report("Failed to parse streaming response".to_string()))?;
        };

        Ok(Box::pin(result_stream))
    }

    fn parse_report_from_json(
        response: &serde_json::Value,
        query: &str,
    ) -> Option<PredictionReport> {
        let summary = response.get("summary")?.as_str()?.to_string();
        let mut timeline: Vec<TimelineEvent> = response
            .get("timeline")
            .and_then(|v| v.as_array())?
            .iter()
            .filter_map(|v| {
                let tick = v.get("tick")?.as_u64()? as u32;
                let description = v.get("description")?.as_str()?.to_string();
                let significance = v.get("significance")?.as_f64()? as f32;
                Some(TimelineEvent { tick, description, significance })
            })
            .collect();
        timeline.sort_by(|a, b| {
            b.significance.partial_cmp(&a.significance).unwrap_or(std::cmp::Ordering::Equal)
        });

        let agent_highlights = response
            .get("agent_highlights")
            .and_then(|v| v.as_array())?
            .iter()
            .filter_map(|v| {
                let agent_id = v.get("agent_id")?.as_str()?.parse::<Uuid>().ok()?;
                let agent_name = v.get("agent_name")?.as_str()?.to_string();
                let summary = v.get("summary")?.as_str()?.to_string();
                Some(AgentHighlight { agent_id, agent_name, summary })
            })
            .collect();

        let confidence = response.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

        Some(PredictionReport {
            id: Uuid::new_v4(),
            summary,
            timeline,
            agent_highlights,
            confidence,
            raw_query: query.to_string(),
            created_at: chrono::Utc::now(),
        })
    }

    pub async fn generate<L: LlmClient + ?Sized>(
        result: &SimulationResult,
        query: &str,
        llm: &L,
    ) -> Result<PredictionReport> {
        let env = Environment::new();
        let template = env
            .template_from_str(REPORT_TEMPLATE)
            .map_err(|e| TeriError::Report(format!("Template parsing error: {}", e)))?;

        let key_events = Self::extract_key_events(result);
        let agents = Self::summarize_agents(result);
        let total_ticks = result.final_snapshot().map(|s| s.tick).unwrap_or(0);
        let total_events: usize = result.history.iter().map(|s| s.events.len()).sum();

        let ctx = context! {
            query => query,
            total_ticks => total_ticks,
            agent_count => agents.len(),
            total_events => total_events,
            key_events => key_events,
            agents => agents,
        };

        let prompt = template
            .render(ctx)
            .map_err(|e| TeriError::Report(format!("Failed to render report template: {}", e)))?;

        let response = llm.complete_json::<serde_json::Value>(&prompt).await?;

        Self::parse_report_from_json(&response, query).ok_or_else(|| {
            TeriError::Report("Failed to parse LLM response into report".to_string())
        })
    }

    fn extract_key_events(result: &SimulationResult) -> Vec<serde_json::Value> {
        let mut events = Vec::new();
        for snapshot in &result.history {
            for event in &snapshot.events {
                let actor = event.agent_id.to_string();
                let description = format!("{}", event.action);
                events.push(serde_json::json!({
                    "tick": snapshot.tick,
                    "description": description,
                    "actor": actor,
                }));
            }
        }
        events.sort_by_key(|e| e.get("tick").and_then(|v| v.as_u64()).unwrap_or(0));
        events.into_iter().take(10).collect()
    }

    fn summarize_agents(result: &SimulationResult) -> Vec<serde_json::Value> {
        let mut agent_map: std::collections::HashMap<Uuid, (String, usize, String)> =
            std::collections::HashMap::new();

        for snapshot in &result.history {
            for (id, agent) in &snapshot.agents {
                let entry = agent_map
                    .entry(*id)
                    .or_insert_with(|| (agent.name.clone(), 0, agent.state.clone()));
                entry.1 += 1;
                entry.2 = agent.state.clone();
            }
        }

        let mut agents: Vec<_> = agent_map
            .into_iter()
            .map(|(id, (name, action_count, final_state))| {
                serde_json::json!({
                    "agent_id": id.to_string(),
                    "name": name,
                    "action_count": action_count,
                    "final_state": final_state,
                })
            })
            .collect();

        agents.sort_by(|a, b| {
            let a_count = a.get("action_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let b_count = b.get("action_count").and_then(|v| v.as_u64()).unwrap_or(0);
            b_count.cmp(&a_count)
        });

        agents
    }

    /// Classify a chat message so `chat()` retrieves only the context relevant to it.
    fn parse_intent(message: &str) -> ChatIntent {
        let lower = message.to_lowercase();
        if lower.contains("confiden") || lower.contains("certain") || lower.contains("how sure") {
            ChatIntent::Confidence
        } else if lower.contains("agent") || lower.contains("who") {
            ChatIntent::AgentSummary
        } else if lower.contains("timeline")
            || lower.contains("when")
            || lower.contains("event")
            || lower.contains("happened")
        {
            ChatIntent::Timeline
        } else {
            ChatIntent::General
        }
    }

    /// Answer a free-form question about a simulation's results.
    ///
    /// Routes `message` to an intent (timeline / agent activity / confidence / general),
    /// retrieves only the simulation context relevant to that intent, renders
    /// `agent_chat.jinja`, and returns the LLM's raw text response.
    pub async fn chat<L: LlmClient + ?Sized>(
        message: &str,
        result: &SimulationResult,
        llm: &L,
    ) -> Result<String> {
        let all_key_events = Self::extract_key_events(result);
        let all_agents = Self::summarize_agents(result);
        let total_ticks = result.final_snapshot().map(|s| s.tick).unwrap_or(0);
        let total_events: usize = result.history.iter().map(|s| s.events.len()).sum();
        let agent_count = all_agents.len();

        let (key_events, agents) = match Self::parse_intent(message) {
            ChatIntent::Timeline => (all_key_events, Vec::new()),
            ChatIntent::AgentSummary => (Vec::new(), all_agents),
            ChatIntent::Confidence => (Vec::new(), Vec::new()),
            ChatIntent::General => (all_key_events, all_agents),
        };

        let env = Environment::new();
        let template = env
            .template_from_str(AGENT_CHAT_TEMPLATE)
            .map_err(|e| TeriError::Report(format!("Template parsing error: {}", e)))?;

        let template_context = context! {
            has_simulation_context => true,
            total_ticks => total_ticks,
            agent_count => agent_count,
            total_events => total_events,
            key_events => key_events,
            agents => agents,
            message => message,
        };

        let prompt = template
            .render(template_context)
            .map_err(|e| TeriError::Report(format!("Failed to render chat template: {}", e)))?;

        llm.complete(&prompt).await
    }
}

impl Default for ReportAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Action, AgentSnapshot, Event, WorldSnapshot};

    #[test]
    fn test_timeline_event_creation() {
        let event = TimelineEvent {
            tick: 5,
            description: "Something happened".to_string(),
            significance: 0.8,
        };

        assert_eq!(event.tick, 5);
        assert_eq!(event.significance, 0.8);
    }

    #[test]
    fn test_agent_highlight_creation() {
        let highlight = AgentHighlight {
            agent_id: Uuid::new_v4(),
            agent_name: "Alice".to_string(),
            summary: "Alice was very active".to_string(),
        };

        assert_eq!(highlight.agent_name, "Alice");
    }

    #[test]
    fn test_prediction_report_creation() {
        let report = ReportAgent::create_empty_report("What will happen?".to_string());
        assert_eq!(report.raw_query, "What will happen?");
        assert!(report.summary.is_empty());
    }

    #[test]
    fn test_extract_key_events_from_simulation() {
        let agent_id = Uuid::new_v4();
        let mut agents = std::collections::HashMap::new();
        agents.insert(
            agent_id,
            AgentSnapshot { id: agent_id, name: "Alice".to_string(), state: "Active".to_string() },
        );

        let event = Event {
            agent_id,
            action: Action::Speak("Hello world".to_string()),
            timestamp: chrono::Utc::now(),
        };

        let snapshot = WorldSnapshot {
            tick: 1,
            agents,
            events: vec![event],
            variables: std::collections::HashMap::new(),
        };

        let result = SimulationResult { id: Uuid::new_v4(), history: vec![snapshot] };

        let events = ReportAgent::extract_key_events(&result);
        assert!(!events.is_empty());
        assert_eq!(events[0]["tick"], 1);
    }

    #[test]
    fn test_summarize_agents_from_simulation() {
        let agent_id = Uuid::new_v4();
        let mut agents = std::collections::HashMap::new();
        agents.insert(
            agent_id,
            AgentSnapshot { id: agent_id, name: "Bob".to_string(), state: "Idle".to_string() },
        );

        let snapshot = WorldSnapshot {
            tick: 1,
            agents,
            events: vec![],
            variables: std::collections::HashMap::new(),
        };

        let result = SimulationResult { id: Uuid::new_v4(), history: vec![snapshot] };

        let agent_summaries = ReportAgent::summarize_agents(&result);
        assert!(!agent_summaries.is_empty());
        assert_eq!(agent_summaries[0]["name"], "Bob");
    }

    fn snapshot_with_agents(tick: u32, agents: Vec<(Uuid, &str, &str)>) -> WorldSnapshot {
        let mut agent_map = std::collections::HashMap::new();
        for (id, name, state) in agents {
            agent_map
                .insert(id, AgentSnapshot { id, name: name.to_string(), state: state.to_string() });
        }
        WorldSnapshot {
            tick,
            agents: agent_map,
            events: vec![],
            variables: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn test_summarize_agents_selects_all_present_agents() {
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();
        let carol = Uuid::new_v4();

        let history = vec![
            snapshot_with_agents(1, vec![(alice, "Alice", "Idle"), (bob, "Bob", "Idle")]),
            snapshot_with_agents(2, vec![(alice, "Alice", "Idle"), (carol, "Carol", "Idle")]),
        ];
        let result = SimulationResult { id: Uuid::new_v4(), history };

        let agent_summaries = ReportAgent::summarize_agents(&result);
        let names: std::collections::HashSet<&str> =
            agent_summaries.iter().map(|a| a["name"].as_str().unwrap()).collect();

        assert_eq!(agent_summaries.len(), 3);
        assert_eq!(names, std::collections::HashSet::from(["Alice", "Bob", "Carol"]));
    }

    #[test]
    fn test_summarize_agents_orders_by_activity_descending() {
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();
        let carol = Uuid::new_v4();

        // Alice appears in 3 snapshots, Bob in 2, Carol in 1 — distinct activity levels.
        let history = vec![
            snapshot_with_agents(
                1,
                vec![(alice, "Alice", "Idle"), (bob, "Bob", "Idle"), (carol, "Carol", "Idle")],
            ),
            snapshot_with_agents(2, vec![(alice, "Alice", "Idle"), (bob, "Bob", "Idle")]),
            snapshot_with_agents(3, vec![(alice, "Alice", "Idle")]),
        ];
        let result = SimulationResult { id: Uuid::new_v4(), history };

        let agent_summaries = ReportAgent::summarize_agents(&result);

        assert_eq!(agent_summaries[0]["name"], "Alice");
        assert_eq!(agent_summaries[0]["action_count"], 3);
        assert_eq!(agent_summaries[1]["name"], "Bob");
        assert_eq!(agent_summaries[1]["action_count"], 2);
        assert_eq!(agent_summaries[2]["name"], "Carol");
        assert_eq!(agent_summaries[2]["action_count"], 1);
    }

    #[test]
    fn test_summarize_agents_reflects_most_recent_final_state() {
        let alice = Uuid::new_v4();

        let history = vec![
            snapshot_with_agents(1, vec![(alice, "Alice", "Idle")]),
            snapshot_with_agents(2, vec![(alice, "Alice", "Active")]),
            snapshot_with_agents(3, vec![(alice, "Alice", "Resting")]),
        ];
        let result = SimulationResult { id: Uuid::new_v4(), history };

        let agent_summaries = ReportAgent::summarize_agents(&result);

        assert_eq!(agent_summaries.len(), 1);
        assert_eq!(agent_summaries[0]["final_state"], "Resting");
    }

    // Mock LLM client for generate() tests; returns a fixed JSON response for both
    // `complete` and `complete_json` so tests can exercise the real parsing path.
    struct MockJsonLlm {
        response: String,
    }

    #[async_trait::async_trait]
    impl LlmClient for MockJsonLlm {
        async fn complete(&self, _prompt: &str) -> Result<String> {
            Ok(self.response.clone())
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _prompt: &str) -> Result<T> {
            serde_json::from_str(&self.response)
                .map_err(|e| TeriError::Llm(format!("JSON parsing error: {}", e)))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("Not implemented".to_string()))
        }
    }

    #[tokio::test]
    async fn test_generate_returns_populated_report_from_mock_llm() {
        let result = build_test_simulation_result();
        let agent_id = Uuid::new_v4();
        let mock_response = serde_json::json!({
            "summary": "The negotiation concluded successfully.",
            "timeline": [
                { "tick": 2, "description": "Alice proposed terms", "significance": 0.6 },
                { "tick": 5, "description": "Bob accepted the deal", "significance": 0.9 }
            ],
            "agent_highlights": [
                { "agent_id": agent_id.to_string(), "agent_name": "Alice", "summary": "Led the negotiation" }
            ],
            "confidence": 0.85
        })
        .to_string();
        let llm = MockJsonLlm { response: mock_response };

        let report = ReportAgent::generate(&result, "Will the negotiation succeed?", &llm)
            .await
            .expect("generate should succeed");

        assert_eq!(report.summary, "The negotiation concluded successfully.");
        assert_eq!(report.raw_query, "Will the negotiation succeed?");
        assert_eq!(report.timeline.len(), 2);
        assert_eq!(report.agent_highlights.len(), 1);
        assert_eq!(report.agent_highlights[0].agent_name, "Alice");
    }

    #[tokio::test]
    async fn test_generate_extracts_timeline_events_correctly() {
        let result = build_test_simulation_result();
        let mock_response = serde_json::json!({
            "summary": "Summary",
            "timeline": [
                { "tick": 3, "description": "A key event occurred", "significance": 0.75 }
            ],
            "agent_highlights": [],
            "confidence": 0.5
        })
        .to_string();
        let llm = MockJsonLlm { response: mock_response };

        let report = ReportAgent::generate(&result, "query", &llm)
            .await
            .expect("generate should succeed");

        assert_eq!(report.timeline.len(), 1);
        assert_eq!(report.timeline[0].tick, 3);
        assert_eq!(report.timeline[0].description, "A key event occurred");
        assert_eq!(report.timeline[0].significance, 0.75);
    }

    #[tokio::test]
    async fn test_generate_calculates_confidence_from_llm_response() {
        let result = build_test_simulation_result();
        let mock_response = serde_json::json!({
            "summary": "Summary",
            "timeline": [],
            "agent_highlights": [],
            "confidence": 0.42
        })
        .to_string();
        let llm = MockJsonLlm { response: mock_response };

        let report = ReportAgent::generate(&result, "query", &llm)
            .await
            .expect("generate should succeed");

        assert_eq!(report.confidence, 0.42);
    }

    #[tokio::test]
    async fn test_generate_defaults_confidence_to_zero_when_missing() {
        let result = build_test_simulation_result();
        let mock_response = serde_json::json!({
            "summary": "Summary",
            "timeline": [],
            "agent_highlights": []
        })
        .to_string();
        let llm = MockJsonLlm { response: mock_response };

        let report = ReportAgent::generate(&result, "query", &llm)
            .await
            .expect("generate should succeed");

        assert_eq!(report.confidence, 0.0);
    }

    #[tokio::test]
    async fn test_generate_returns_error_when_llm_response_missing_summary() {
        let result = build_test_simulation_result();
        let mock_response = serde_json::json!({
            "timeline": [],
            "agent_highlights": [],
            "confidence": 0.5
        })
        .to_string();
        let llm = MockJsonLlm { response: mock_response };

        let err = ReportAgent::generate(&result, "query", &llm)
            .await
            .expect_err("generate should fail");
        assert!(matches!(err, TeriError::Report(_)));
    }

    #[test]
    fn test_parse_report_extracts_all_timeline_event_fields() {
        let response = serde_json::json!({
            "summary": "Summary",
            "timeline": [
                { "tick": 1, "description": "First event", "significance": 0.2 },
                { "tick": 4, "description": "Second event", "significance": 0.7 },
                { "tick": 9, "description": "Third event", "significance": 0.5 }
            ],
            "agent_highlights": [],
            "confidence": 0.5
        });

        let report = ReportAgent::parse_report_from_json(&response, "query")
            .expect("parsing should succeed");

        assert_eq!(report.timeline.len(), 3);
        for (tick, description) in [(1, "First event"), (4, "Second event"), (9, "Third event")] {
            let event = report
                .timeline
                .iter()
                .find(|e| e.tick == tick)
                .unwrap_or_else(|| panic!("missing event for tick {tick}"));
            assert_eq!(event.description, description);
        }
    }

    #[test]
    fn test_parse_report_sorts_timeline_by_significance_descending() {
        let response = serde_json::json!({
            "summary": "Summary",
            "timeline": [
                { "tick": 1, "description": "Low", "significance": 0.2 },
                { "tick": 2, "description": "High", "significance": 0.9 },
                { "tick": 3, "description": "Medium", "significance": 0.5 }
            ],
            "agent_highlights": [],
            "confidence": 0.5
        });

        let report = ReportAgent::parse_report_from_json(&response, "query")
            .expect("parsing should succeed");

        let significances: Vec<f32> = report.timeline.iter().map(|e| e.significance).collect();
        assert_eq!(significances, vec![0.9, 0.5, 0.2]);
        assert_eq!(report.timeline[0].description, "High");
        assert_eq!(report.timeline[1].description, "Medium");
        assert_eq!(report.timeline[2].description, "Low");
    }

    #[test]
    fn test_parse_report_preserves_relative_order_for_equal_significance() {
        let response = serde_json::json!({
            "summary": "Summary",
            "timeline": [
                { "tick": 1, "description": "Alpha", "significance": 0.5 },
                { "tick": 2, "description": "Beta", "significance": 0.5 },
                { "tick": 3, "description": "Gamma", "significance": 0.5 }
            ],
            "agent_highlights": [],
            "confidence": 0.5
        });

        let report = ReportAgent::parse_report_from_json(&response, "query")
            .expect("parsing should succeed");

        let descriptions: Vec<&str> =
            report.timeline.iter().map(|e| e.description.as_str()).collect();
        assert_eq!(descriptions, vec!["Alpha", "Beta", "Gamma"]);
    }

    // Mock LLM client for streaming tests
    struct MockStreamingLlm {
        chunks: Vec<String>,
    }

    #[async_trait::async_trait]
    impl LlmClient for MockStreamingLlm {
        async fn complete(&self, _prompt: &str) -> Result<String> {
            Err(TeriError::Llm("Not implemented".to_string()))
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _prompt: &str) -> Result<T> {
            Err(TeriError::Llm("Not implemented".to_string()))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
            let chunks = self.chunks.clone();
            let stream = try_stream! {
                for chunk in chunks {
                    yield chunk;
                }
            };
            Ok(Box::pin(stream))
        }
    }

    #[tokio::test]
    async fn test_generate_stream_yields_multiple_chunks() {
        let agent_id = Uuid::new_v4();
        let mut agents = std::collections::HashMap::new();
        agents.insert(
            agent_id,
            AgentSnapshot { id: agent_id, name: "Alice".to_string(), state: "Active".to_string() },
        );

        let event = Event {
            agent_id,
            action: Action::Speak("Hello world".to_string()),
            timestamp: chrono::Utc::now(),
        };

        let snapshot = WorldSnapshot {
            tick: 1,
            agents,
            events: vec![event],
            variables: std::collections::HashMap::new(),
        };

        let result = SimulationResult { id: Uuid::new_v4(), history: vec![snapshot] };

        // Mock streaming response - split JSON across chunks
        let json_response = serde_json::json!({
            "summary": "Test prediction about simulation",
            "timeline": [
                {"tick": 1, "description": "Event occurred", "significance": 0.8}
            ],
            "agent_highlights": [
                {"agent_id": agent_id.to_string(), "agent_name": "Alice", "summary": "Alice was key"}
            ],
            "confidence": 0.75
        });

        let chunks = vec![json_response.to_string()];

        let mock_llm = MockStreamingLlm { chunks };
        let mut stream = ReportAgent::generate_stream(&result, "What happened?", &mock_llm)
            .await
            .expect("Failed to create stream");

        let mut chunk_count = 0;
        let mut last_report: Option<PredictionReport> = None;

        while let Some(report_result) = stream.next().await {
            let report = report_result.expect("Stream chunk failed");
            chunk_count += 1;
            last_report = Some(report);
        }

        assert!(
            chunk_count >= 2,
            "Expected at least 2 chunks from streaming, got {}",
            chunk_count
        );
        assert!(last_report.is_some(), "Expected final report");

        let final_report = last_report.unwrap();
        assert_eq!(final_report.raw_query, "What happened?");
        assert!(!final_report.summary.is_empty());
    }

    #[test]
    fn test_chat_message_creation() {
        let msg = ChatMessage {
            id: Uuid::new_v4(),
            sender: "user".to_string(),
            content: "Hello, agent!".to_string(),
            timestamp: chrono::Utc::now(),
            agent_id: None,
        };

        assert_eq!(msg.sender, "user");
        assert_eq!(msg.content, "Hello, agent!");
        assert!(msg.agent_id.is_none());
    }

    #[test]
    fn test_chat_message_with_agent_id() {
        let agent_id = Uuid::new_v4();
        let msg = ChatMessage {
            id: Uuid::new_v4(),
            sender: "Alice".to_string(),
            content: "I understand your question.".to_string(),
            timestamp: chrono::Utc::now(),
            agent_id: Some(agent_id),
        };

        assert_eq!(msg.sender, "Alice");
        assert_eq!(msg.agent_id, Some(agent_id));
    }

    #[test]
    fn test_chat_message_serialization() {
        let agent_id = Uuid::new_v4();
        let msg = ChatMessage {
            id: Uuid::new_v4(),
            sender: "Bob".to_string(),
            content: "Test message".to_string(),
            timestamp: chrono::Utc::now(),
            agent_id: Some(agent_id),
        };

        let json = serde_json::to_string(&msg).expect("Serialization failed");
        assert!(json.contains("\"sender\":\"Bob\""));
        assert!(json.contains("\"content\":\"Test message\""));
    }

    #[test]
    fn test_chat_message_deserialization() {
        let agent_id = Uuid::new_v4();
        let json = serde_json::json!({
            "id": Uuid::new_v4().to_string(),
            "sender": "Charlie",
            "content": "Deserialized message",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "agent_id": agent_id.to_string(),
        });

        let msg: ChatMessage = serde_json::from_value(json).expect("Deserialization failed");
        assert_eq!(msg.sender, "Charlie");
        assert_eq!(msg.content, "Deserialized message");
        assert_eq!(msg.agent_id, Some(agent_id));
    }

    #[test]
    fn test_chat_message_round_trip() {
        let original = ChatMessage {
            id: Uuid::new_v4(),
            sender: "user".to_string(),
            content: "Round trip test message".to_string(),
            timestamp: chrono::Utc::now(),
            agent_id: Some(Uuid::new_v4()),
        };

        let json = serde_json::to_string(&original).expect("Serialization failed");
        let deserialized: ChatMessage =
            serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(original.sender, deserialized.sender);
        assert_eq!(original.content, deserialized.content);
        assert_eq!(original.agent_id, deserialized.agent_id);
    }

    #[test]
    fn test_chat_message_without_agent_id_round_trip() {
        let original = ChatMessage {
            id: Uuid::new_v4(),
            sender: "system".to_string(),
            content: "System message".to_string(),
            timestamp: chrono::Utc::now(),
            agent_id: None,
        };

        let json = serde_json::to_string(&original).expect("Serialization failed");
        let deserialized: ChatMessage =
            serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(original.sender, deserialized.sender);
        assert_eq!(original.content, deserialized.content);
        assert!(deserialized.agent_id.is_none());
    }

    #[test]
    fn test_agent_chat_template_renders_with_persona_memory_and_context() {
        let env = Environment::new();
        let template = env.template_from_str(AGENT_CHAT_TEMPLATE).expect("Template parsing error");

        let ctx = context! {
            agent_name => "Alice",
            agent_role => "Diplomat",
            agent_background => "A seasoned negotiator.",
            agent_traits => vec!["curious", "calm"],
            relevant_memories => vec![serde_json::json!({
                "content": "Alice met Bob at the summit",
                "importance": 0.8,
            })],
            has_simulation_context => true,
            total_ticks => 10,
            agent_count => 2,
            total_events => 5,
            key_events => vec![serde_json::json!({
                "tick": 3,
                "description": "Alice initiates dialogue",
                "actor": "Alice",
            })],
            agents => vec![serde_json::json!({
                "name": "Alice",
                "action_count": 4,
                "final_state": "Active",
            })],
            conversation_history => vec![serde_json::json!({
                "sender": "user",
                "content": "How is the negotiation going?",
            })],
            message => "Will the negotiation succeed?",
        };

        let rendered = template.render(ctx).expect("Template rendering error");

        assert!(rendered.contains("Alice"));
        assert!(rendered.contains("Diplomat"));
        assert!(rendered.contains("Alice met Bob at the summit"));
        assert!(rendered.contains("## Simulation Context"));
        assert!(rendered.contains("Alice initiates dialogue"));
        assert!(rendered.contains("How is the negotiation going?"));
        assert!(rendered.contains("Will the negotiation succeed?"));
    }

    #[test]
    fn test_agent_chat_template_renders_with_message_only() {
        let env = Environment::new();
        let template = env.template_from_str(AGENT_CHAT_TEMPLATE).expect("Template parsing error");

        let ctx = context! {
            message => "Hello there",
        };

        let rendered =
            template.render(ctx).expect("Template should render with no optional context");

        assert!(rendered.contains("Hello there"));
        assert!(!rendered.contains("## Your Persona"));
        assert!(!rendered.contains("## Relevant Memories"));
        assert!(!rendered.contains("## Simulation Context"));
        assert!(!rendered.contains("## Conversation So Far"));
    }

    #[test]
    fn test_parse_intent_detects_timeline_questions() {
        assert_eq!(ReportAgent::parse_intent("What happened at tick 5?"), ChatIntent::Timeline);
        assert_eq!(ReportAgent::parse_intent("Can I see the timeline?"), ChatIntent::Timeline);
    }

    #[test]
    fn test_parse_intent_detects_agent_questions() {
        assert_eq!(
            ReportAgent::parse_intent("Which agent was most active?"),
            ChatIntent::AgentSummary
        );
        assert_eq!(
            ReportAgent::parse_intent("Who did the most talking?"),
            ChatIntent::AgentSummary
        );
    }

    #[test]
    fn test_parse_intent_detects_confidence_questions() {
        assert_eq!(
            ReportAgent::parse_intent("How confident are you in this outcome?"),
            ChatIntent::Confidence
        );
        assert_eq!(
            ReportAgent::parse_intent("Are you certain about that?"),
            ChatIntent::Confidence
        );
    }

    #[test]
    fn test_parse_intent_defaults_to_general() {
        assert_eq!(ReportAgent::parse_intent("Tell me more about this."), ChatIntent::General);
    }

    // Mock LLM client for chat tests; captures the rendered prompt for assertions.
    struct MockChatLlm {
        response: String,
        captured_prompt: std::sync::Mutex<Option<String>>,
    }

    #[async_trait::async_trait]
    impl LlmClient for MockChatLlm {
        async fn complete(&self, prompt: &str) -> Result<String> {
            *self.captured_prompt.lock().unwrap() = Some(prompt.to_string());
            Ok(self.response.clone())
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _prompt: &str) -> Result<T> {
            Err(TeriError::Llm("Not implemented".to_string()))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("Not implemented".to_string()))
        }
    }

    fn build_test_simulation_result() -> SimulationResult {
        let agent_id = Uuid::new_v4();
        let mut agents = std::collections::HashMap::new();
        agents.insert(
            agent_id,
            AgentSnapshot { id: agent_id, name: "Alice".to_string(), state: "Active".to_string() },
        );

        let event = Event {
            agent_id,
            action: Action::Speak("Hello world".to_string()),
            timestamp: chrono::Utc::now(),
        };

        let snapshot = WorldSnapshot {
            tick: 1,
            agents,
            events: vec![event],
            variables: std::collections::HashMap::new(),
        };

        SimulationResult { id: Uuid::new_v4(), history: vec![snapshot] }
    }

    #[tokio::test]
    async fn test_chat_with_mock_llm_and_simulation_context() {
        let result = build_test_simulation_result();
        let mock_llm = MockChatLlm {
            response: "The negotiation looks promising.".to_string(),
            captured_prompt: std::sync::Mutex::new(None),
        };

        let response =
            ReportAgent::chat("What happened during the negotiation?", &result, &mock_llm)
                .await
                .expect("chat should succeed");

        assert_eq!(response, "The negotiation looks promising.");

        let prompt = mock_llm.captured_prompt.lock().unwrap().clone().expect("prompt captured");
        assert!(prompt.contains("What happened during the negotiation?"));
        assert!(prompt.contains("## Simulation Context"));
        // Timeline intent: key events included, agent activity list omitted.
        assert!(prompt.contains("Spoke: Hello world"));
        assert!(!prompt.contains("Agent Activity:"));
    }

    #[tokio::test]
    async fn test_chat_routes_agent_summary_intent_context() {
        let result = build_test_simulation_result();
        let mock_llm = MockChatLlm {
            response: "Alice was the most active.".to_string(),
            captured_prompt: std::sync::Mutex::new(None),
        };

        ReportAgent::chat("Which agent was most active?", &result, &mock_llm)
            .await
            .expect("chat should succeed");

        let prompt = mock_llm.captured_prompt.lock().unwrap().clone().expect("prompt captured");
        assert!(prompt.contains("Agent Activity:"));
        assert!(!prompt.contains("Key Events:"));
    }

    #[tokio::test]
    async fn test_chat_routes_confidence_intent_context() {
        let result = build_test_simulation_result();
        let mock_llm = MockChatLlm {
            response: "Fairly confident.".to_string(),
            captured_prompt: std::sync::Mutex::new(None),
        };

        ReportAgent::chat("How confident are you?", &result, &mock_llm)
            .await
            .expect("chat should succeed");

        let prompt = mock_llm.captured_prompt.lock().unwrap().clone().expect("prompt captured");
        assert!(!prompt.contains("Key Events:"));
        assert!(!prompt.contains("Agent Activity:"));
        // Summary stats (ticks/agents/events) still present regardless of intent.
        assert!(prompt.contains("Total Ticks"));
    }

    #[tokio::test]
    async fn test_chat_routes_general_intent_includes_all_context() {
        let result = build_test_simulation_result();
        let mock_llm = MockChatLlm {
            response: "Here's an overview of the simulation.".to_string(),
            captured_prompt: std::sync::Mutex::new(None),
        };

        let response = ReportAgent::chat("Tell me about this.", &result, &mock_llm)
            .await
            .expect("chat should succeed");

        assert_eq!(response, "Here's an overview of the simulation.");

        let prompt = mock_llm.captured_prompt.lock().unwrap().clone().expect("prompt captured");
        // General intent (no keyword match) surfaces both timeline and agent context.
        assert!(prompt.contains("Key Events:"));
        assert!(prompt.contains("Agent Activity:"));
        assert!(prompt.contains("Spoke: Hello world"));
        assert!(prompt.contains("Alice"));
    }

    // Mock LLM client that always fails, for verifying chat() propagates LLM errors.
    struct MockFailingLlm;

    #[async_trait::async_trait]
    impl LlmClient for MockFailingLlm {
        async fn complete(&self, _prompt: &str) -> Result<String> {
            Err(TeriError::Llm("simulated LLM failure".to_string()))
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _prompt: &str) -> Result<T> {
            Err(TeriError::Llm("Not implemented".to_string()))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("Not implemented".to_string()))
        }
    }

    #[tokio::test]
    async fn test_chat_propagates_llm_error() {
        let result = build_test_simulation_result();

        let outcome = ReportAgent::chat("What happened?", &result, &MockFailingLlm).await;

        assert!(outcome.is_err());
    }
}
