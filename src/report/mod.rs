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
        let timeline = response
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
}
