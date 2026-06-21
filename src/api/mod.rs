pub mod graph;
pub mod report;
pub mod simulation;
pub mod streaming;

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
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

// ---------------------------------------------------------------------------
// ApiError — shared graph-API error type (U-025 shared seam; inherited by U-026/U-027)
//
// Maps to MiroFish's two `jsonify({...}), <status>` error envelopes:
//   • client/404: {"success": false, "error": "<msg>"}
//   • server/500: {"success": false, "error": "<msg>", "traceback": "<string>"}
//
// All success+error response bodies for /api/graph, /api/simulation, /api/report
// MUST use these constructors so the wire shape is consistent and parity-verified.
// ---------------------------------------------------------------------------

/// Graph-API error type implementing `IntoResponse`.
///
/// Carries the exact `{success:false, error[, traceback]}` body MiroFish returns,
/// plus the HTTP status code.  Returned as `Err(ApiError)` from handlers typed
/// `Result<Json<Value>, ApiError>`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: Value,
}

impl ApiError {
    /// Build a client-error response (4xx).
    ///
    /// Body: `{"success": false, "error": "<msg>"}` (2-key shape).
    /// Mirrors MiroFish's `jsonify({"success": False, "error": t(...)})` + 40x status.
    pub fn client(status: StatusCode, error_msg: impl Into<String>) -> Self {
        Self {
            status,
            body: serde_json::json!({
                "success": false,
                "error": error_msg.into()
            }),
        }
    }

    /// Build a client-error response with extra keys appended after `error`.
    ///
    /// Body: `{"success": false, "error": "<msg>", ...extra}`.
    /// Used by `/build` which also returns `"task_id"` in the 400 `graphBuilding` case
    /// (graph.py:329: `jsonify({..., "task_id": project.graph_build_task_id})`).
    pub fn client_with(
        status: StatusCode,
        error_msg: impl Into<String>,
        extra: Map<String, Value>,
    ) -> Self {
        let mut map = Map::new();
        map.insert("success".to_string(), Value::Bool(false));
        map.insert("error".to_string(), Value::String(error_msg.into()));
        for (k, v) in extra {
            map.insert(k, v);
        }
        Self { status, body: Value::Object(map) }
    }

    /// Build a server-error response (500).
    ///
    /// Body: `{"success": false, "error": "<msg>", "traceback": "<string>"}` (3-key shape).
    /// Mirrors MiroFish's `jsonify({"success": False, "error": str(e),
    /// "traceback": traceback.format_exc()})` + 500.
    ///
    /// `[≠] U025-TRACEBACK`: the `traceback` VALUE is a Rust backtrace string, not a Python
    /// stack.  The 3-key CONTRACT is preserved; the value being Rust text is non-contractual
    /// (the frontend treats `traceback` as opaque debug text).
    pub fn server(err: impl std::fmt::Display) -> Self {
        let bt = std::backtrace::Backtrace::capture().to_string();
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: serde_json::json!({
                "success": false,
                "error": err.to_string(),
                "traceback": bt
            }),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

/// Construct an `OpenAiAdapter` from teri config for use within a single request.
#[allow(dead_code)] // Used by sub-cycles (c)–(f); not yet called by (b)-only routes
///
/// Per-request construction is cheap (`reqwest::Client::new()` — no network, no handshake).
/// Mirrors MiroFish's pattern of constructing `OntologyGenerator()` / `GraphBuilderService()`
/// fresh inside each handler (graph.py:217, 390, 581, 609) — **not a downgrade**.
///
/// DECISION-U025-1: `ApiState` carries no LLM client because `LlmClient` has generic methods
/// that are NOT dyn-compatible, and axum state cannot be generic.  Construct per-request.
///
/// `[!] U025-CLONE`: derive `Clone` on `OpenAiAdapter` is deferred to sub-cycle (c/d) when
/// `build_graph_async`/spawn need it by value.  The project routes don't spawn — no Clone needed.
pub(crate) fn build_llm(config: &crate::Config) -> crate::llm::OpenAiAdapter {
    crate::llm::OpenAiAdapter::new(&config.llm)
}

/// Build the optional "boost" LLM client — port of `create_model(config, use_boost=True)`
/// (`run_parallel_simulation.py:984-1037`). The dual-LLM optimization lets a parallel run drive
/// each platform's agents against a DIFFERENT API provider (twitter → the general LLM, reddit →
/// the boost LLM) for higher concurrency.
///
/// Boost is gated on `LLM_BOOST_API_KEY` exactly like Python's `has_boost_config = bool(boost_api_key)`:
/// absent/empty → `None` (the run falls back to the single general LLM for every agent, byte-identical
/// to a non-boost run). When present, the boost client uses `LLM_BOOST_API_KEY`, then
/// `LLM_BOOST_BASE_URL` (falling back to the general `base_url` when unset — mirroring Python's
/// `if llm_base_url:` conditional override, which leaves the previously-set general base in place when
/// the boost base is empty), and `LLM_BOOST_MODEL_NAME` (falling back to the general `model`, which
/// already encodes `LLM_MODEL_NAME`-or-default). Secrets are read from the process env (envctl-injected),
/// never persisted.
pub(crate) fn build_boost_llm(config: &crate::Config) -> Option<crate::llm::OpenAiAdapter> {
    let boost_api_key = std::env::var("LLM_BOOST_API_KEY").unwrap_or_default();
    if boost_api_key.is_empty() {
        return None;
    }
    let boost_base_url = std::env::var("LLM_BOOST_BASE_URL").unwrap_or_default();
    let boost_model = std::env::var("LLM_BOOST_MODEL_NAME").unwrap_or_default();
    let cfg = crate::config::LlmConfig {
        api_key: boost_api_key,
        base_url: if boost_base_url.is_empty() {
            config.llm.base_url.clone()
        } else {
            boost_base_url
        },
        model: if boost_model.is_empty() { config.llm.model.clone() } else { boost_model },
        ..config.llm.clone()
    };
    Some(crate::llm::OpenAiAdapter::new(&cfg))
}

pub struct ApiState {
    pub config: crate::Config,
    /// U-026: shared simulation registry — owns the `state.json` cache (cross-request
    /// coherent) and is the SAME instance the runner holds (`sim_runner.manager`), so
    /// `mark_state_json_stopped` writes stay consistent. DECISION-U026-1.
    pub sim_manager: std::sync::Arc<crate::services::simulation_manager::SimulationManager>,
    /// U-026: shared simulation runner — owns the live `runs` handle map, so a sim started
    /// by `POST /simulation/start` is visible to later `GET /:id/run-status` / `POST /stop`
    /// on subsequent requests. Concrete monomorphization over `OpenAiAdapter`
    /// (DECISION-U026-1): `LlmClient` is not dyn-compatible and axum state cannot be generic,
    /// but `build_llm()` always yields `OpenAiAdapter`, so `SimulationRunner<OpenAiAdapter>`
    /// is a single concrete type that lives in non-generic state. DECISION-U025-1 preserved
    /// (no `dyn`, no generic `ApiState`).
    pub sim_runner: std::sync::Arc<
        crate::services::simulation_runner::SimulationRunner<crate::llm::OpenAiAdapter>,
    >,
    /// Workstream B: shared redb vector store for graph entity/edge embeddings, namespaced
    /// per `graph_id`. Built once from `config.persistence.memory_db_path` so build-time
    /// embedding (graph_builder) and search-time cosine (report tools) hit the SAME store.
    /// `None` if the store could not be opened (the path is unwritable) — callers then fall
    /// back to keyword search, never failing the request (keyless-safe, no-downgrade).
    pub graph_vectors: Option<std::sync::Arc<crate::memory::MemoryStore>>,
    /// Workstream B: shared embedding client (OpenAI-compatible `/v1/embeddings`, keyless-aware)
    /// used to embed entity/edge text at build time and the query at search time.
    pub embedder: std::sync::Arc<crate::embedding::EmbeddingClient>,
}

impl ApiState {
    /// Build app state from config.
    ///
    /// The simulation runtime registry (`sim_manager` + `sim_runner`) is constructed
    /// **internally** from `config` so this stays a one-argument constructor — every
    /// `create_app`/test call-site (39 of them) is unaffected by the U-026 state extension
    /// (the architect's preferred mitigation; blast radius = this constructor only).
    pub fn new(config: crate::Config) -> Self {
        // Shared simulation manager (U-023) — owns the state.json cache.
        let sim_manager = std::sync::Arc::new(
            crate::services::simulation_manager::SimulationManager::from_config(&config),
        );
        // Workstream B: open the shared graph-vector store (redb) once. A failure to open
        // (e.g. an unwritable MEMORY_DB_PATH in a constrained test) is non-fatal: graph_vectors
        // stays None and search falls back to keyword (no-downgrade, keyless-safe).
        let graph_vectors = crate::memory::MemoryStore::new(&config.persistence.memory_db_path)
            .map(std::sync::Arc::new)
            .ok();
        // Shared embedding client (keyless-aware; same LlmConfig as the LLM adapters).
        let embedder = std::sync::Arc::new(crate::embedding::EmbeddingClient::new(&config.llm));
        // Workstream B (U6): vector index passed to graph-memory updaters so sim-accrued facts
        // are re-embedded into the store. `None` when the store is unavailable.
        let vector_index =
            graph_vectors
                .as_ref()
                .map(|store| crate::services::graph_builder::GraphVectorIndex {
                    embedder: embedder.clone(),
                    store: store.clone(),
                });
        // Graph-memory manager (U-021) registry — builds per-platform updaters lazily; the
        // concrete `OpenAiAdapter` monomorphization is fixed here at the state boundary.
        let graph_mgr = std::sync::Arc::new(crate::services::graph_memory::GraphMemoryManager::<
            crate::llm::OpenAiAdapter,
        >::with_vector_index(vector_index));
        // Shared runner (U-022) — shares the SAME manager Arc (so state.json writes are
        // consistent) and uses the same sim-data dir as the manager.
        let sim_runner =
            std::sync::Arc::new(crate::services::simulation_runner::SimulationRunner::new(
                std::path::PathBuf::from(&config.oasis_simulation_data_dir),
                graph_mgr,
                sim_manager.clone(),
            ));

        Self { config, sim_manager, sim_runner, graph_vectors, embedder }
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
