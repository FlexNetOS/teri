pub mod streaming;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSimRequest {
    pub seed_path: String,
    pub query: String,
    pub agent_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSimResponse {
    pub sim_id: Uuid,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimStatusResponse {
    pub sim_id: Uuid,
    pub tick: u32,
    pub status: String,
    pub agent_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectRequest {
    pub variable: String,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub agent_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SimStatus {
    Running,
    Completed,
    Failed,
}

/// Streaming tick event for Server-Sent Events (SSE).
///
/// Represents a single simulation tick snapshot ready to be streamed to API clients.
/// Used by the `/sim/:id/stream` endpoint to push live updates to connected browsers.
#[derive(Debug, Clone, Serialize)]
pub struct TickStreamEvent {
    /// Tick number (0-indexed)
    pub tick: u32,
    /// Serialized world snapshot data
    pub data: serde_json::Value,
    /// Optional event ID for SSE client deduplication
    pub event_id: String,
}

impl TickStreamEvent {
    /// Create a new tick stream event from a world snapshot.
    ///
    /// # Errors
    /// Returns error if serialization fails.
    pub fn from_snapshot(snapshot: &crate::sim::WorldSnapshot) -> crate::error::Result<Self> {
        Ok(Self {
            tick: snapshot.tick,
            data: serde_json::to_value(snapshot)?,
            event_id: format!("tick-{}", snapshot.tick),
        })
    }

    /// Create a gap-notification event for ticks missed due to broadcast lag (14A).
    ///
    /// When a broadcast receiver gets `RecvError::Lagged(n)`, emit this event
    /// so SSE clients know they missed `n` ticks and can request a history replay.
    pub fn lag_gap(missed_ticks: u64) -> Self {
        Self {
            tick: 0,
            data: serde_json::json!({ "gap": true, "missed_ticks": missed_ticks }),
            event_id: format!("gap-{}", missed_ticks),
        }
    }

    /// Create the terminal end-of-simulation event.
    ///
    /// This is the in-band signal that a stream consumer receives as the final event on the
    /// SSE stream, mirroring MiroFish `action_logger.log_simulation_end` (~line 105) which
    /// emits a `simulation_end` marker that `simulation_runner.py` (~line 623) monitors to
    /// mark the sim completed.
    ///
    /// Design: encoded like `lag_gap` — a sentinel value in `data` rather than a new struct
    /// field, so the SSE wire format stays uniform (`{ tick, data, event_id }` always).
    /// The `event_id` is the fixed string `"sim-end"` (no count suffix: there is exactly one
    /// per simulation). `tick` is set to `total_ticks` so consumers know the final tick index.
    ///
    /// # Usage
    /// The SSE handler (U-026) awaits `SimEngine::subscribe_completion()`, then emits this
    /// event as the final SSE frame before closing the connection.
    pub fn sim_end(total_ticks: u32) -> Self {
        Self {
            tick: total_ticks,
            data: serde_json::json!({ "sim_end": true, "total_ticks": total_ticks }),
            event_id: "sim-end".to_string(),
        }
    }
}

/// Configuration for streaming backpressure handling.
///
/// Controls how the API layer buffers ticks when consumers are slow,
/// preventing unbounded memory growth.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Maximum number of buffered ticks before dropping old events.
    /// Once this limit is hit, oldest ticks are dropped to make room for new ones.
    /// Range: 1..1000, typical: 50-100
    pub max_buffer_size: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self { max_buffer_size: 100 }
    }
}

impl StreamConfig {
    /// Create a streaming config optimized for low-latency applications (small buffers).
    pub fn low_latency() -> Self {
        Self { max_buffer_size: 10 }
    }

    /// Create a streaming config optimized for reliable delivery (large buffers).
    pub fn reliable_delivery() -> Self {
        Self { max_buffer_size: 500 }
    }
}

pub struct ApiState {
    pub config: crate::Config,
}

impl ApiState {
    pub fn new(config: crate::Config) -> Self {
        Self { config }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_sim_request() {
        let req = CreateSimRequest {
            seed_path: "/path/to/seed.txt".to_string(),
            query: "What will happen?".to_string(),
            agent_count: Some(100),
        };

        assert_eq!(req.seed_path, "/path/to/seed.txt");
    }

    #[test]
    fn test_chat_request() {
        let req = ChatRequest { message: "Hello".to_string(), agent_id: Some(Uuid::new_v4()) };

        assert_eq!(req.message, "Hello");
    }

    #[test]
    fn test_tick_stream_event_creation() {
        let snapshot = crate::sim::WorldSnapshot {
            tick: 42,
            agents: Default::default(),
            events: Vec::new(),
            variables: Default::default(),
        };

        let event = TickStreamEvent::from_snapshot(&snapshot).unwrap();
        assert_eq!(event.tick, 42);
        assert_eq!(event.event_id, "tick-42");
    }

    #[test]
    fn test_stream_config_defaults() {
        let config = StreamConfig::default();
        assert_eq!(config.max_buffer_size, 100);
    }

    #[test]
    fn test_stream_config_low_latency() {
        let config = StreamConfig::low_latency();
        assert_eq!(config.max_buffer_size, 10);
    }

    #[test]
    fn test_stream_config_reliable_delivery() {
        let config = StreamConfig::reliable_delivery();
        assert_eq!(config.max_buffer_size, 500);
    }

    #[test]
    fn test_tick_stream_event_lag_gap() {
        let event = TickStreamEvent::lag_gap(42);
        assert_eq!(event.event_id, "gap-42");
        assert_eq!(event.data["missed_ticks"], 42);
        assert_eq!(event.data["gap"], true);
    }

    #[test]
    fn test_tick_stream_event_sim_end_tick() {
        let event = TickStreamEvent::sim_end(10);
        assert_eq!(event.tick, 10, "tick must equal total_ticks");
    }

    #[test]
    fn test_tick_stream_event_sim_end_event_id() {
        let event = TickStreamEvent::sim_end(10);
        assert_eq!(event.event_id, "sim-end", "event_id must be the fixed sentinel string");
    }

    #[test]
    fn test_tick_stream_event_sim_end_data_fields() {
        let event = TickStreamEvent::sim_end(7);
        assert_eq!(event.data["sim_end"], true, "data.sim_end must be true");
        assert_eq!(event.data["total_ticks"], 7, "data.total_ticks must match arg");
    }

    #[test]
    fn test_tick_stream_event_sim_end_zero_ticks() {
        // Edge: sim that ran 0 ticks (config.max_ticks == 0)
        let event = TickStreamEvent::sim_end(0);
        assert_eq!(event.tick, 0);
        assert_eq!(event.data["total_ticks"], 0);
        assert_eq!(event.event_id, "sim-end");
    }
}
