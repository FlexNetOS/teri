//! FIX-1 (keystone): the in-process `teri run` pipeline.
//!
//! Composes the five MiroFish stages end-to-end **in-process**, reusing the exact
//! service-layer functions the HTTP handlers (`api/graph.rs`, `api/simulation.rs`,
//! `api/report.rs`) already call — no logic is reimplemented here, only sequenced:
//!
//! 1. **seed** — ingest the `--seed` material (`SeedIngestor` → `text_processor`).
//! 2. **graph build** — generate the ontology (`OntologyGenerator`) then run the REAL
//!    two-pass LLM entity/relation extraction (`KnowledgeGraph::build_with_progress_and_ontology`,
//!    the same keystone the `/build` handler drives via `graph_builder`).
//! 3. **persona generation** — `SimulationManager::prepare_simulation` (the same call the
//!    `/prepare` handler's worker makes), which fans out `PersonaGenerator::generate_social`
//!    per entity and generates the sim config.
//! 4. **simulation** — `SimulationRunner::start_simulation` (the same call `/start` makes),
//!    with the memory write-back wired (`enable_graph_memory_update` + a shared
//!    `Arc<Mutex<KnowledgeGraph>>` updater handle).
//! 5. **report** — `ReportAgent::generate_report` over the 4 graph tools (the same call the
//!    `/generate` handler's worker makes), saved via `ReportManager`.
//!
//! The pipeline is generic over `L: LlmClient + Clone + Send + Sync + 'static`, so the same
//! composition is driven by the live provider-selected adapter (`build_provider_llm`,
//! FIX-3) in production and by an injected mock LLM in tests (no live GGUF backend — the
//! backend honesty guard blocks real runs).
//!
//! FIX-2: [`run_pipeline`] returns a [`PipelineOutcome`] which `main.rs` renders to
//! `verdict.json` via [`PipelineOutcome::to_verdict_json`].

use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::error::{Result, TeriError};
use crate::graph::KnowledgeGraph;
use crate::llm::LlmClient;
use crate::report::{Report, ReportStatus};
use crate::services::simulation_runner::RunnerStatus;

/// The `RunInputs` plus the optional shared graph-memory write-back handle returned by
/// [`build_run_inputs`]. Factored into an alias to keep the function signature readable
/// (clippy::type_complexity).
type RunInputsBundle<L> = (
    crate::services::simulation_runner::RunInputs<L>,
    Option<Arc<Mutex<KnowledgeGraph>>>,
);

/// The platform a `teri run` simulates. Reddit is the default single-platform run — it
/// matches `prepare_simulation`'s realtime-output preference (Reddit > Twitter) and needs
/// only one profile file, keeping the in-process spine minimal.
const RUN_PLATFORM: &str = "reddit";

/// How long to wait for the simulation to reach a terminal `RunnerStatus` before giving up.
/// A mock-LLM run completes in well under a second; a live run is bounded by `max_rounds`.
const SIM_POLL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(900);
/// Poll interval while waiting for the simulation to finish. The monitor reads
/// `actions.jsonl` every ~2s, so polling faster than that is pointless.
const SIM_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// The result of a full `teri run`, summarizing every stage for `verdict.json` (FIX-2).
#[derive(Debug, Clone)]
pub struct PipelineOutcome {
    /// The user's prediction query.
    pub query: String,
    /// The seed material path/URL that was ingested.
    pub seed: String,
    /// Requested agent count (`--agents`). The actual persona count is entity-derived
    /// (one persona per surviving entity); see `agents_generated`.
    pub agents_requested: usize,
    /// The number of personas actually generated (== prepared entity count).
    pub agents_generated: usize,
    /// The created project id (`proj_…`).
    pub project_id: String,
    /// The graph id (== the graph-build task id, as the HTTP path uses it).
    pub graph_id: String,
    /// The created simulation id (`sim_…`).
    pub simulation_id: String,
    /// Graph stats after the LLM extraction.
    pub graph_node_count: usize,
    pub graph_edge_count: usize,
    /// Terminal simulation runner status.
    pub sim_runner_status: RunnerStatus,
    pub sim_rounds_completed: i64,
    pub sim_total_rounds: i64,
    /// The generated report.
    pub report_id: String,
    pub report_status: ReportStatus,
    /// Report-save root key (the directory name under `{upload_folder}/reports/`).
    pub report_key: String,
    /// The report outline summary + section titles ("key findings").
    pub report_summary: Option<String>,
    pub report_section_titles: Vec<String>,
}

impl PipelineOutcome {
    /// Render the `verdict.json` body (FIX-2). Mirrors the CLI-fork's verdict spirit:
    /// query, seed, agent count, report path/key-findings, graph stats.
    pub fn to_verdict_json(&self) -> Value {
        json!({
            "query": self.query,
            "seed": self.seed,
            "agents": {
                "requested": self.agents_requested,
                "generated": self.agents_generated,
            },
            "project_id": self.project_id,
            "graph": {
                "graph_id": self.graph_id,
                "node_count": self.graph_node_count,
                "edge_count": self.graph_edge_count,
            },
            "simulation": {
                "simulation_id": self.simulation_id,
                "runner_status": self.sim_runner_status.as_str(),
                "rounds_completed": self.sim_rounds_completed,
                "total_rounds": self.sim_total_rounds,
            },
            "report": {
                "report_id": self.report_id,
                "report_key": self.report_key,
                "status": format!("{:?}", self.report_status).to_lowercase(),
                "summary": self.report_summary,
                "key_findings": self.report_section_titles,
            },
        })
    }
}

/// Run the full `seed → graph → persona → simulation → report` pipeline in-process.
///
/// `config` is teri's loaded config (used for project/sim/report roots, graph-memory, etc.).
/// `llm` is the LLM adapter — provider-selected (`build_provider_llm`) in production, or a
/// mock in tests. `seed` is the `--seed` path or URL; `query` is the prediction question;
/// `agents` is the requested agent count (`--agents`).
///
/// Returns a [`PipelineOutcome`] for `verdict.json`. Errors propagate as `TeriError`.
pub async fn run_pipeline<L>(
    config: &crate::Config,
    llm: L,
    seed: &str,
    query: &str,
    agents: usize,
) -> Result<PipelineOutcome>
where
    L: LlmClient + Clone + Send + Sync + 'static,
{
    use crate::models::project::ProjectManager;
    use crate::seed::{SeedIngestor, text_processor};
    use crate::services::ontology::OntologyGenerator;

    // ── Stage 1: seed ──────────────────────────────────────────────────────
    tracing::info!(seed, "pipeline: ingesting seed material");
    let doc = if seed.starts_with("http://") || seed.starts_with("https://") {
        SeedIngestor::from_url(seed).await?
    } else {
        SeedIngestor::from_file(seed).await?
    };
    let text = text_processor::preprocess_text(&doc.raw_text);
    if text.trim().is_empty() {
        return Err(TeriError::Config(format!("seed produced no usable text: {seed}")));
    }

    let pm = ProjectManager::from_config(config);
    let mut project = pm.create_project(&format!("teri run: {query}"))?;
    let project_id = project.project_id.clone();
    pm.save_extracted_text(&project_id, &text)?;
    project.total_text_length = text.chars().count() as i64;
    project.simulation_requirement = Some(query.to_string());

    // ── Stage 2: graph build (real LLM ontology + extraction) ────────────────
    tracing::info!(project_id = %project_id, "pipeline: generating ontology");
    let ontology = OntologyGenerator::new(llm.clone())
        .generate(std::slice::from_ref(&text), query, None)
        .await?;
    project.ontology = Some(ontology.clone());

    let entity_types: Vec<String> = ontology
        .get("entity_types")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("name").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let edge_types: Vec<String> = ontology
        .get("edge_types")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("name").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    tracing::info!(
        entity_types = entity_types.len(),
        edge_types = edge_types.len(),
        "pipeline: building knowledge graph (LLM extraction)"
    );
    let mut progress = |pct: i64, msg: String| tracing::debug!(pct, %msg, "graph build");
    let graph = KnowledgeGraph::build_with_progress_and_ontology(
        &doc,
        &llm,
        &mut progress,
        &entity_types,
        &edge_types,
    )
    .await?;
    let graph_node_count = graph.entity_count();
    let graph_edge_count = graph.relation_count();

    // Register the graph in the global TaskManager exactly as the `/build` handler does
    // (graph_id == task_id; resolved via `result["graph"]`), so the same graph-resolution
    // path is honored and the report tools / graph-memory updater can find it by id.
    let graph_id = crate::task::TaskManager::global().create_task("graph_build", None);
    let graph_json: Value = serde_json::from_str(&graph.serialize_to_json()?)?;
    crate::task::TaskManager::global().complete_task(
        &graph_id,
        json!({
            "graph_name": "teri run graph",
            "graph_info": {
                "node_count": graph_node_count,
                "edge_count": graph_edge_count,
                "entity_types": entity_types,
            },
            "graph": graph_json,
        }),
    );
    project.graph_id = Some(graph_id.clone());
    project.graph_build_task_id = Some(graph_id.clone());
    pm.save_project(&mut project)?;
    tracing::info!(
        graph_id = %graph_id,
        nodes = graph_node_count,
        edges = graph_edge_count,
        "pipeline: graph built"
    );

    // ── Stage 3: persona generation (prepare) ────────────────────────────────
    let sim_manager =
        Arc::new(crate::services::simulation_manager::SimulationManager::from_config(config));
    // Single-platform reddit run (see RUN_PLATFORM).
    let sim_state = sim_manager.create_simulation(&project_id, &graph_id, false, true)?;
    let simulation_id = sim_state.simulation_id.clone();

    tracing::info!(simulation_id = %simulation_id, "pipeline: preparing simulation (personas + config)");
    let persona_generator = crate::agent::PersonaGenerator::new();
    let config_generator = crate::services::simulation_config::SimulationConfigGenerator::new(
        llm.clone(),
        config.llm.model.clone(),
        config.llm.base_url.clone(),
    );
    let prepared = sim_manager
        .prepare_simulation(
            &simulation_id,
            query,
            &text,
            None, // no entity-type filter — use all defined types
            true, // use_llm_for_profiles
            5,    // parallel_profile_count (matches the /prepare route default)
            &llm,
            &graph,
            &persona_generator,
            &config_generator,
            None,
        )
        .await?;
    let agents_generated = prepared.profiles_count.max(0) as usize;
    if prepared.status == crate::services::simulation_manager::SimulationStatus::Failed {
        return Err(TeriError::Sim(format!(
            "simulation prepare failed (status=Failed): {}",
            prepared
                .error
                .clone()
                .unwrap_or_else(|| "no entities extracted from seed".to_string())
        )));
    }

    // ── Stage 4: simulation (with memory write-back) ─────────────────────────
    let graph_mgr = Arc::new(crate::services::graph_memory::GraphMemoryManager::<L>::new());
    // Shared memory store + embedder, used for BOTH the agent-LTM write-back (the monitor
    // persists each utterance) AND the report's `recall_agent_discussion` lens (stage 5 reads the
    // SAME store the sim populated). Best-effort — if the store can't open (e.g. an unwritable
    // MEMORY_DB_PATH), both are disabled (`None`); the run is otherwise identical (keyless-safe).
    let memory_store = crate::memory::MemoryStore::new(&config.persistence.memory_db_path)
        .ok()
        .map(Arc::new);
    let embedder = Arc::new(crate::embedding::EmbeddingClient::new(&config.llm));
    let agent_memory = memory_store.as_ref().map(|store| {
        Arc::new(crate::services::agent_memory::AgentMemoryWriter::new(
            Arc::clone(store),
            Arc::clone(&embedder),
        ))
    });
    let sim_runner = Arc::new(
        crate::services::simulation_runner::SimulationRunner::new(
            std::path::PathBuf::from(&config.oasis_simulation_data_dir),
            Arc::clone(&graph_mgr),
            Arc::clone(&sim_manager),
        )
        .with_agent_memory(agent_memory),
    );

    tracing::info!(simulation_id = %simulation_id, "pipeline: starting simulation");
    let (inputs, graph_for_updater) =
        build_run_inputs(&sim_manager, llm.clone(), &simulation_id, RUN_PLATFORM, &graph)?;
    // Retain a handle to the graph the memory write-back mutates DURING the run. The updater
    // (`GraphMemoryUpdater`) writes the entities/relations the swarm's activity teaches into
    // this shared `Arc<Mutex<KnowledgeGraph>>`; the report (stage 5) must then run over THAT
    // post-simulation graph — what the simulation learned — not the pre-sim one. Without this
    // clone the only handle is moved into `start_simulation` and dropped here, so the
    // write-back wrote into a graph nobody reads (the "agent LTM write-back unwired" gap).
    // Cloning an `Option<Arc<_>>` is a refcount bump, not a graph copy.
    let updater_handle = graph_for_updater.clone();
    let run_state = sim_runner
        .start_simulation(
            &simulation_id,
            RUN_PLATFORM,
            None, // max_rounds — use the prepared config's total
            true, // enable_graph_memory_update (wires the write-back)
            Some(&graph_id),
            inputs,
            graph_for_updater,
        )
        .await?;
    let mut sim_runner_status = run_state.runner_status;
    let mut sim_rounds_completed = run_state.current_round;
    let mut sim_total_rounds = run_state.total_rounds;

    // Poll to a terminal status (Completed / Stopped / Failed), bounded by SIM_POLL_TIMEOUT.
    let deadline = std::time::Instant::now() + SIM_POLL_TIMEOUT;
    while !is_terminal(&sim_runner_status) {
        if std::time::Instant::now() >= deadline {
            tracing::warn!(simulation_id = %simulation_id, "pipeline: simulation poll timed out; stopping");
            let _ = sim_runner.stop_simulation(&simulation_id).await;
            break;
        }
        tokio::time::sleep(SIM_POLL_INTERVAL).await;
        if let Some(rs) = sim_runner.get_run_state(&simulation_id).await? {
            sim_runner_status = rs.runner_status;
            sim_rounds_completed = rs.current_round;
            sim_total_rounds = rs.total_rounds;
        }
    }
    tracing::info!(
        simulation_id = %simulation_id,
        status = sim_runner_status.as_str(),
        rounds = sim_rounds_completed,
        "pipeline: simulation finished"
    );

    // The graph the report reads. If the memory write-back produced an updated graph during
    // the run, the report runs over THAT (the entities/edges the swarm taught it); otherwise
    // it falls back to the pre-sim graph. This is what makes the LTM write-back observable in
    // the verdict instead of being silently discarded.
    let report_graph = resolve_report_graph(&graph, &updater_handle).await;
    // Recompute the verdict's graph stats from the post-sim graph so a write-back that grows
    // the graph is reflected in `verdict.json` (these shadow the pre-sim counts used above for
    // the TaskManager registration, which intentionally records the as-built graph).
    let graph_node_count = report_graph.entity_count();
    let graph_edge_count = report_graph.relation_count();

    // ── Stage 5: report (ReACT over the graph tools) ─────────────────────────
    tracing::info!(
        nodes = graph_node_count,
        edges = graph_edge_count,
        "pipeline: generating report over post-simulation graph"
    );
    let mut report_agent = crate::report::ReportAgent::new_react(&graph_id, &simulation_id, query)
        .with_cli_report_budget();
    // Attach the search lens (same store the sim's agent-LTM write-back populated) so the report's
    // `recall_agent_discussion` tool can surface what the swarm actually said. The lens also
    // upgrades quick_search/panorama to semantic cosine where graph vectors exist (keyword
    // fallback otherwise) — no regression when the store is empty.
    let search_lens =
        memory_store.as_ref().map(|store| crate::services::zep_tools::GraphSearchLens {
            embedder: Arc::clone(&embedder),
            store: Arc::clone(store),
        });
    let tools = crate::services::zep_tools::ReportTools::with_runner(
        &report_graph,
        &llm,
        Some(sim_runner.as_ref()),
    )
    .with_search_lens(search_lens);
    let report_manager = crate::report::manager::ReportManager::new(&config.upload_folder);
    let mut sink = crate::report::sink::NullSink;
    let report: Report = report_agent
        .generate_report(&tools, &llm, &report_manager, &mut sink, None)
        .await;

    let report_summary = report.outline.as_ref().map(|o| o.summary.clone());
    let report_section_titles = report
        .outline
        .as_ref()
        .map(|o| o.sections.iter().map(|s| s.title.clone()).collect())
        .unwrap_or_default();

    Ok(PipelineOutcome {
        query: query.to_string(),
        seed: seed.to_string(),
        agents_requested: agents,
        agents_generated,
        project_id,
        graph_id,
        simulation_id,
        graph_node_count,
        graph_edge_count,
        sim_runner_status,
        sim_rounds_completed,
        sim_total_rounds,
        report_id: report.report_id.clone(),
        report_status: report.status,
        report_key: report.report_id.clone(),
        report_summary,
        report_section_titles,
    })
}

/// `true` if the runner has reached a terminal lifecycle state.
fn is_terminal(status: &RunnerStatus) -> bool {
    matches!(status, RunnerStatus::Completed | RunnerStatus::Stopped | RunnerStatus::Failed)
}

/// Resolve the graph the report should read.
///
/// The graph-memory write-back (`GraphMemoryUpdater`) mutates a shared
/// `Arc<Mutex<KnowledgeGraph>>` handle *during* the simulation — the entities and relations the
/// agents' activity teaches the graph. The report must run over THAT post-simulation graph so
/// the simulation's learning reaches the verdict. Returns a clone of the updated graph when the
/// write-back handle is present (graph-memory enabled), else a clone of the pre-sim `original`.
async fn resolve_report_graph(
    original: &KnowledgeGraph,
    updater_handle: &Option<Arc<Mutex<KnowledgeGraph>>>,
) -> KnowledgeGraph {
    match updater_handle {
        Some(handle) => handle.lock().await.clone(),
        None => original.clone(),
    }
}

/// Build the `RunInputs` for a single-platform run, in-process. This replicates the
/// in-crate body of `api::simulation::build_run_inputs` (which is private to the api module
/// and coupled to `ApiState`) using the same engine/producer/social/pool construction, so
/// the simulation runs identically to the HTTP `/start` path.
fn build_run_inputs<L>(
    sim_manager: &crate::services::simulation_manager::SimulationManager,
    llm: L,
    simulation_id: &str,
    platform: &str,
    graph: &KnowledgeGraph,
) -> Result<RunInputsBundle<L>>
where
    L: LlmClient + Clone + Send + Sync + 'static,
{
    use crate::agent::Platform;
    use crate::sim::action_logger::PlatformActionLogger;
    use crate::sim::activation::TimeActivationPolicy;
    use crate::sim::social_world::SocialWorldSet;
    use crate::sim::{PlatformLoggerSet, RunProducer, SimConfig, SimEngine};

    let sim_dir = sim_manager.get_simulation_dir(simulation_id)?;
    let sim_config = sim_manager
        .get_simulation_config(simulation_id)?
        .ok_or_else(|| TeriError::Sim(crate::i18n::t("api.configNotFound")))?;

    let mut engine = SimEngine::new(SimConfig::from_simulation_config(&sim_config, None, 8));
    engine.with_activation(Arc::new(TimeActivationPolicy::from_config(&sim_config, None)));

    let platform_enum = if platform == "reddit" { Platform::Reddit } else { Platform::Twitter };
    let logger = Arc::new(
        PlatformActionLogger::new(platform, &sim_dir)
            .map_err(|e| TeriError::Sim(format!("action logger init failed: {e}")))?,
    );
    let loggers = PlatformLoggerSet::single(platform_enum, logger);
    engine.with_producer(RunProducer { loggers, config: sim_config.clone() });
    engine.with_social(SocialWorldSet::new(vec![platform_enum], &sim_dir)?);

    let pool = crate::services::oasis_profile_export::load_agent_pool(&sim_dir, platform)?;
    let llm = Arc::new(llm);

    // Graph-memory write-back: a shared handle the updater mutates as actions arrive.
    let g = graph.clone();
    let updater_handle = Arc::new(Mutex::new(g.clone()));

    Ok((
        crate::services::simulation_runner::RunInputs {
            engine,
            pool,
            graph: g,
            llm,
            boost_llm: None,
        },
        Some(updater_handle),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Entity, EntityKind, KnowledgeGraph};
    use uuid::Uuid;

    fn entity(name: &str) -> Entity {
        Entity { id: Uuid::new_v4(), name: name.to_string(), kind: EntityKind::Person }
    }

    /// When the write-back handle is present, the report graph is the POST-simulation graph
    /// (the one the updater mutated), NOT the pre-sim original — proving the LTM write-back is
    /// observable by the report rather than discarded.
    #[tokio::test]
    async fn resolve_report_graph_uses_post_sim_graph_when_handle_present() {
        // Pre-sim graph: a single entity (what was extracted from the seed).
        let mut original = KnowledgeGraph::new();
        original.add_entity(entity("Jane Doe")).unwrap();
        assert_eq!(original.entity_count(), 1);

        // The shared handle the updater writes into — start it as a clone of `original`, then
        // simulate the write-back teaching the graph two NEW entities during the run.
        let handle: Arc<Mutex<KnowledgeGraph>> = Arc::new(Mutex::new(original.clone()));
        {
            let mut g = handle.lock().await;
            g.add_entity(entity("Acme Corp")).unwrap();
            g.add_entity(entity("Climate Policy")).unwrap();
        }

        let report_graph = resolve_report_graph(&original, &Some(Arc::clone(&handle))).await;
        // The report sees the 3-entity post-sim graph, not the 1-entity pre-sim one.
        assert_eq!(report_graph.entity_count(), 3, "report must read the post-sim graph");
        // The original is untouched (we cloned out of the handle, not aliased it).
        assert_eq!(original.entity_count(), 1, "pre-sim graph must be unchanged");
    }

    /// When graph-memory is disabled (no handle), the report falls back to the pre-sim graph.
    #[tokio::test]
    async fn resolve_report_graph_falls_back_to_original_when_no_handle() {
        let mut original = KnowledgeGraph::new();
        original.add_entity(entity("Jane Doe")).unwrap();

        let report_graph = resolve_report_graph(&original, &None).await;
        assert_eq!(report_graph.entity_count(), 1, "must fall back to the pre-sim graph");
    }
}
