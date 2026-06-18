//! Graph-build service — port of `backend/app/services/graph_builder.py` (MiroFish).
//!
//! # Symbol mapping (S-189)
//!
//! | Source symbol                           | Rust mapping                                        |
//! |-----------------------------------------|-----------------------------------------------------|
//! | S-189 `build_graph_async`               | `build_graph_async` (this module)                   |
//! | S-192 `set_ontology`                    | `KnowledgeGraph::set_ontology` (graph/mod.rs)       |
//!
//! The Zep-SaaS methods (`create_graph`, `add_text_batches`, `_wait_for_episodes`,
//! `get_graph_data`, `delete_graph`) are `[≠]` — inexpressible substrate (no Zep client in teri).
//! Their behaviors are mapped onto native petgraph (Decision-1).
//!
//! # DECISION-8 milestone map
//!
//! | MiroFish milestone         | %    | teri mapping                              | port / `[≠]` |
//! |----------------------------|------|-------------------------------------------|--------------|
//! | startBuildingGraph         | 5    | emitted at worker start                   | **PORT**     |
//! | create_graph (Zep)         | 10   | — (no Zep graph object)                   | **`[≠]`**    |
//! | set_ontology               | 15   | `g.set_ontology(&ontology)` + emit        | **PORT**     |
//! | text split                 | 20   | `split_text(text, …)` + emit              | **PORT**     |
//! | add_text_batches (Zep)     | 20–60| 2-pass extraction progress (via callback) | **PORT** re-mapped |
//! | wait_for_episodes (Zep)    | 60–90| — (no Zep episode queue)                  | **`[≠]`**    |
//! | fetch_graph_info           | 90   | native rollup node/edge/types             | **PORT**     |
//! | complete                   | 100  | `complete_task`                           | **PORT**     |
//!
//! # Locale propagation (idiom map: thread-local → task-local)
//! MiroFish captures `current_locale = get_locale()` before spawning the daemon thread, then
//! calls `set_locale(locale)` inside the thread.  Teri's Tokio equivalent:
//!
//! - Capture `locale = i18n::get_locale()` before `tokio::spawn`.
//! - Wrap the spawned future in `i18n::with_locale(locale, async move { … })`.
//!
//! This ensures all `i18n::t()` calls inside the worker see the caller's locale.
//!
//! # `batch_size` parameter (`[≠]`)
//! MiroFish's `batch_size` controls how many chunks are sent per `graph.add_batch()` call to
//! the Zep API (with a 1-second sleep between batches for rate-limiting).  Teri has no Zep
//! batch endpoint; LLM calls are per-chunk with the adapter's own retry/backoff.  The parameter
//! is accepted for call-shape parity and ignored.

use serde_json::{Value, json};
use std::collections::HashMap;

use crate::error::TeriError;
use crate::graph::{KnowledgeGraph, collect_entity_type_names};
use crate::llm::LlmClient;
use crate::seed::{SeedDocument, text_processor};
use crate::task::{TaskManager, TaskStatus};

/// Asynchronously builds a knowledge graph from `text` and returns a task_id immediately.
///
/// Port of `GraphBuilderService.build_graph_async` (S-189, `graph_builder.py:54`).
///
/// # Arguments
/// - `llm` — LLM adapter (must be `Clone + Send + Sync + 'static` for tokio::spawn).
/// - `text` — raw source text for graph extraction.
/// - `ontology` — validated ontology dict (output of `OntologyGenerator::validate_and_process`).
/// - `graph_name` — human-readable graph name; flows into task metadata and result.
/// - `chunk_size` — character window per chunk (default 500, matching MiroFish).
/// - `chunk_overlap` — character overlap between adjacent chunks (default 50, matching MiroFish).
/// - `_batch_size` — accepted for call-shape parity with MiroFish; ignored (`[≠]` — Zep
///   batching artifact, non-contractual in teri's native pipeline).
///
/// # Returns
/// A task_id string.  The caller can poll `TaskManager::global().get_task(&task_id)` for
/// progress and the final result.
///
/// # Result shape (teri-native, no Zep `graph_id`)
/// ```json
/// {
///   "graph_name": "<graph_name>",
///   "graph_info": {
///     "node_count": <usize>,
///     "edge_count": <usize>,
///     "entity_types": ["<distinct kind Display tokens, Other excluded>"]
///   },
///   "chunks_processed": <usize>,
///   "graph": <SerializableKnowledgeGraph JSON>
/// }
/// ```
/// `graph_id` is `[≠]` (Zep-server handle with no teri equivalent); the retrievable graph is
/// preserved by embedding it in the result.
pub fn build_graph_async<L>(
    llm: L,
    text: String,
    ontology: Value,
    graph_name: String,
    chunk_size: usize,
    chunk_overlap: usize,
    _batch_size: usize, // [≠] Zep-batching artifact; accepted, ignored
) -> String
where
    L: LlmClient + Clone + Send + Sync + 'static,
{
    // Create the task (S-189: mirrors Python `self.task_manager.create_task`).
    let mut metadata = HashMap::new();
    metadata.insert("graph_name".to_string(), json!(graph_name));
    metadata.insert("chunk_size".to_string(), json!(chunk_size));
    metadata.insert("text_length".to_string(), json!(text.len()));

    let task_id = TaskManager::global().create_task("graph_build", Some(metadata));

    // Capture locale before spawning (idiom-map: thread-local → task-local capture).
    let locale = crate::i18n::get_locale();

    // Spawn the worker (idiom-map: daemon thread → tokio::spawn).
    let task_id_worker = task_id.clone();
    tokio::spawn(crate::i18n::with_locale(locale, async move {
        build_graph_worker(
            task_id_worker,
            llm,
            text,
            ontology,
            graph_name,
            chunk_size,
            chunk_overlap,
        )
        .await;
    }));

    task_id
}

/// Inner worker — port of `_build_graph_worker` (`graph_builder.py:100`).
///
/// Drives the full pipeline: set_ontology → split → 2-pass extraction → rollup → complete/fail.
/// All exceptions (any `Err`) are caught and routed to `fail_task` (matching the Python
/// `try/except traceback` → `fail_task` pattern).
async fn build_graph_worker<L>(
    task_id: String,
    llm: L,
    text: String,
    ontology: Value,
    graph_name: String,
    chunk_size: usize,
    chunk_overlap: usize,
) where
    L: LlmClient + Clone + Send + Sync + 'static,
{
    let result = build_graph_worker_inner(
        &task_id,
        llm,
        text,
        ontology,
        graph_name,
        chunk_size,
        chunk_overlap,
    )
    .await;

    if let Err(e) = result {
        // Mirror Python `except Exception as e: traceback.format_exc()` → `fail_task`.
        TaskManager::global().fail_task(&task_id, e.to_string());
    }
}

/// Failable inner worker body.  Returns `Err` on any failure so the outer wrapper can route to
/// `fail_task`.  This is the direct port of `_build_graph_worker`'s `try` block.
///
/// Exposed as `pub(crate)` so tests can drive the pipeline directly (bypassing `tokio::spawn`
/// and TaskManager polling), which avoids CPU-starvation flakiness under heavy test parallelism.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn build_graph_worker_inner<L>(
    task_id: &str,
    llm: L,
    text: String,
    ontology: Value,
    graph_name: String,
    chunk_size: usize,
    chunk_overlap: usize,
) -> crate::error::Result<()>
where
    L: LlmClient + Clone + Send + Sync + 'static,
{
    // Milestone 5% — startBuildingGraph (PORT)
    TaskManager::global().update_task(
        task_id,
        Some(TaskStatus::Processing),
        Some(5),
        Some(crate::i18n::t("progress.startBuildingGraph")),
        None,
        None,
        None,
    );

    // [≠] Milestone 10% — create_graph (Zep): no Zep graph object in teri.
    //     The in-memory KnowledgeGraph is created below; no "graphCreated" emit.

    // Milestone 15% — set_ontology (PORT: S-192)
    // Extract the custom type names from the ontology dict for use by build_with_progress.
    let ontology_entity_types: Vec<String> = ontology
        .get("entity_types")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let ontology_edge_types: Vec<String> = ontology
        .get("edge_types")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    TaskManager::global().update_task(
        task_id,
        None,
        Some(15),
        Some(crate::i18n::t("progress.ontologySet")),
        None,
        None,
        None,
    );

    // Milestone 20% — textSplit (PORT)
    let chunks = text_processor::split_text(&text, chunk_size, chunk_overlap);
    let total_chunks = chunks.len();
    TaskManager::global().update_task(
        task_id,
        None,
        Some(20),
        Some(crate::i18n::t_args("progress.textSplit", &[("count", &total_chunks)])),
        None,
        None,
        None,
    );

    // Build a synthetic SeedDocument for the combined text so the pipeline can use it.
    let doc = SeedDocument {
        id: uuid::Uuid::new_v4(),
        raw_text: text,
        metadata: {
            let mut m = std::collections::HashMap::new();
            m.insert("graph_name".to_string(), graph_name.clone());
            m
        },
        created_at: chrono::Utc::now(),
    };

    // Milestone 20–60%: 2-pass LLM extraction (PORT: re-mapped from Zep add_text_batches).
    // `build_with_progress_and_ontology` requires `P: FnMut + Send` so it can run inside this
    // `tokio::spawn` future.  We pass a concrete closure (captures `task_id` by clone) that
    // calls `TaskManager::global().update_task(…)` directly — the TaskManager is a
    // process-global singleton (`Arc`-wrapped internally) and is always `Send`-accessible.
    //
    // Observable milestones from this inner call (20–60% range, per-chunk `sendingBatch`):
    // these fire into the TaskManager and are visible to task pollers, providing granular
    // progress within the 20–60% window that maps onto MiroFish's `add_text_batches` range.
    let task_id_cb = task_id.to_string();
    let mut extraction_progress = move |p: i64, m: String| {
        TaskManager::global().update_task(&task_id_cb, None, Some(p), Some(m), None, None, None);
    };
    let graph = KnowledgeGraph::build_with_progress_and_ontology(
        &doc,
        &llm,
        &mut extraction_progress,
        &ontology_entity_types,
        &ontology_edge_types,
    )
    .await?;

    // [≠] Milestone 60–90%: wait_for_episodes (Zep episode queue; no teri equivalent).
    //     No "waitingZepProcess" / "waitingEpisodes" / "zepProcessing" emitted.

    // Milestone 90% — fetchingGraphInfo (PORT: native rollup)
    let node_count = graph.entity_count();
    let edge_count = graph.relation_count();
    let entity_types = collect_entity_type_names(&graph);

    TaskManager::global().update_task(
        task_id,
        None,
        Some(90),
        Some(crate::i18n::t("progress.fetchingGraphInfo")),
        None,
        None,
        None,
    );

    // Serialize the graph into the result (replaces Zep's server-side graph_id handle).
    let graph_json_str = graph.serialize_to_json()?;
    let graph_json: Value = serde_json::from_str(&graph_json_str)
        .map_err(|e| TeriError::Graph(format!("Failed to re-parse graph JSON: {e}")))?;

    // Milestone 100% — complete (PORT)
    let result = json!({
        "graph_name": graph_name,
        "graph_info": {
            "node_count": node_count,
            "edge_count": edge_count,
            "entity_types": entity_types,
        },
        "chunks_processed": total_chunks,
        "graph": graph_json,
    });

    TaskManager::global().complete_task(task_id, result);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Entity, EntityKind, RelationKind};
    use crate::llm::{ChatMessage, ChatOptions, LlmClient};
    use async_trait::async_trait;
    use std::pin::Pin;

    // ---- Mock LLM ----

    #[derive(Clone)]
    struct MockGraphLlm {
        entity_resp: String,
        relation_resp: String,
    }

    impl MockGraphLlm {
        fn new(entity_resp: &str, relation_resp: &str) -> Self {
            Self { entity_resp: entity_resp.to_string(), relation_resp: relation_resp.to_string() }
        }
    }

    #[async_trait]
    impl LlmClient for MockGraphLlm {
        async fn complete(&self, prompt: &str) -> crate::error::Result<String> {
            if prompt.contains("Extract named entities") {
                Ok(self.entity_resp.clone())
            } else if prompt.contains("extract relations") {
                Ok(self.relation_resp.clone())
            } else {
                Ok("[]".to_string())
            }
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(
            &self,
            prompt: &str,
        ) -> crate::error::Result<T> {
            let r = self.complete(prompt).await?;
            serde_json::from_str(&r).map_err(|e| TeriError::Llm(format!("JSON parse: {e}")))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> crate::error::Result<
            Pin<Box<dyn futures::Stream<Item = crate::error::Result<String>> + Send>>,
        > {
            Err(TeriError::Llm("not used".into()))
        }

        async fn chat(
            &self,
            _messages: &[ChatMessage],
            _opts: &ChatOptions,
        ) -> crate::error::Result<String> {
            Err(TeriError::Llm("not used".into()))
        }

        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _messages: &[ChatMessage],
            _opts: &ChatOptions,
        ) -> crate::error::Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
    }

    // ---- Tests ----

    /// `build_graph_async` spawn contract: returns a non-empty task_id synchronously, and the
    /// task is immediately visible in the TaskManager as PENDING or later.
    ///
    /// This test verifies only the synchronous contract (spawn + return task_id).  Pipeline
    /// completion is tested separately via `build_graph_worker_inner` to avoid polling-under-
    /// CPU-contention flakiness when the full test suite runs in parallel.
    #[test]
    fn test_build_graph_async_returns_task_id_immediately() {
        // We need a Tokio runtime to call build_graph_async (it calls tokio::spawn internally).
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();

        let mock_llm = MockGraphLlm::new(r#"[{"name": "Alice", "kind": "Person"}]"#, r#"[]"#);

        let task_id = rt.block_on(async {
            build_graph_async(
                mock_llm,
                "Alice works here.".to_string(),
                json!({"entity_types": [], "edge_types": []}),
                "SpawnContractGraph".to_string(),
                500,
                50,
                3,
            )
        });

        assert!(!task_id.is_empty(), "task_id must be non-empty");
        // The task must be in the global registry immediately (before the worker can complete).
        let task = TaskManager::global()
            .get_task(&task_id)
            .expect("task must appear in TaskManager immediately");
        assert_eq!(task.task_type, "graph_build");
        // Status can be PENDING, PROCESSING, COMPLETED (worker may be fast) — just not absent.
        use crate::task::TaskStatus;
        assert!(
            matches!(
                task.status,
                TaskStatus::Pending
                    | TaskStatus::Processing
                    | TaskStatus::Completed
                    | TaskStatus::Failed
            ),
            "task must have a valid status"
        );
        // Don't join the runtime — let the background worker finish or be dropped.
        rt.shutdown_background();
    }

    /// The pipeline (worker inner) must produce a result with the correct shape when the LLM
    /// returns valid entity and relation JSON.
    ///
    /// Calls `build_graph_worker_inner` directly (bypasses `tokio::spawn`) so the test is not
    /// subject to CPU-starvation flakiness under parallel test suite execution.
    #[tokio::test]
    async fn test_build_graph_worker_inner_completes_with_result() {
        let mock_llm = MockGraphLlm::new(
            r#"[{"name": "Alice", "kind": "Person"}, {"name": "Acme", "kind": "Organization"}]"#,
            r#"[{"from": "Alice", "to": "Acme", "kind": "WorksFor", "weight": 0.9}]"#,
        );

        let task_id = TaskManager::global().create_task("graph_build", None);

        build_graph_worker_inner(
            &task_id,
            mock_llm,
            "Alice works at Acme Corp.".to_string(),
            json!({"entity_types": [], "edge_types": []}),
            "TestGraph".to_string(),
            500,
            50,
        )
        .await
        .expect("worker inner must not return Err");

        let task = TaskManager::global().get_task(&task_id).expect("task must exist");
        assert_eq!(
            task.status,
            crate::task::TaskStatus::Completed,
            "task must be COMPLETED; error: {:?}",
            task.error
        );

        // Result shape: graph_name, graph_info, chunks_processed, graph
        let result = task.result.expect("COMPLETED task must have a result");
        assert_eq!(result["graph_name"], "TestGraph");
        let graph_info = &result["graph_info"];
        assert_eq!(graph_info["node_count"], 2);
        assert_eq!(graph_info["edge_count"], 1);
        let types = graph_info["entity_types"].as_array().expect("entity_types must be an array");
        assert!(!types.is_empty(), "entity_types must not be empty");
        assert!(result.get("graph").is_some(), "result must contain serialized graph");
        assert!(
            result["chunks_processed"].as_u64().unwrap_or(0) > 0,
            "chunks_processed must be > 0"
        );
    }

    /// LLM failure inside `build_graph_worker_inner` must return `Err`, and the outer wrapper
    /// (`build_graph_worker`) must route that to `fail_task`.
    ///
    /// Calls `build_graph_worker_inner` directly to avoid spawn-based polling flakiness.
    #[tokio::test]
    async fn test_build_graph_worker_inner_llm_failure_returns_err() {
        #[derive(Clone)]
        struct FailLlm;
        #[async_trait]
        impl LlmClient for FailLlm {
            async fn complete(&self, _prompt: &str) -> crate::error::Result<String> {
                Err(TeriError::Llm("simulated failure".into()))
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(
                &self,
                _prompt: &str,
            ) -> crate::error::Result<T> {
                Err(TeriError::Llm("simulated failure".into()))
            }
            async fn stream(
                &self,
                _prompt: &str,
            ) -> crate::error::Result<
                Pin<Box<dyn futures::Stream<Item = crate::error::Result<String>> + Send>>,
            > {
                Err(TeriError::Llm("not used".into()))
            }
            async fn chat(
                &self,
                _messages: &[ChatMessage],
                _opts: &ChatOptions,
            ) -> crate::error::Result<String> {
                Err(TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _messages: &[ChatMessage],
                _opts: &ChatOptions,
            ) -> crate::error::Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
        }

        let task_id = TaskManager::global().create_task("graph_build", None);

        // worker_inner must return Err on LLM failure
        let result = build_graph_worker_inner(
            &task_id,
            FailLlm,
            "Some text.".to_string(),
            json!({"entity_types": [], "edge_types": []}),
            "FailGraph".to_string(),
            500,
            50,
        )
        .await;

        assert!(result.is_err(), "worker inner must return Err on LLM failure");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("simulated failure"), "error message must propagate: {err_msg}");

        // Simulate what build_graph_worker does: route Err to fail_task
        TaskManager::global().fail_task(&task_id, err_msg.clone());

        let task = TaskManager::global().get_task(&task_id).expect("task must exist");
        assert_eq!(
            task.status,
            crate::task::TaskStatus::Failed,
            "LLM failure must route to FAILED"
        );
        let recorded_err = task.error.expect("FAILED task must have an error message");
        assert!(
            recorded_err.contains("simulated failure"),
            "fail_task error must propagate: {recorded_err}"
        );
    }

    // ---- set_ontology tests ----

    /// `set_ontology` records both entity and edge type sets.
    #[test]
    fn test_set_ontology_records_both_type_sets() {
        let mut graph = KnowledgeGraph::new();
        let ontology = json!({
            "entity_types": [
                {"name": "MediaOutlet", "description": "A media outlet."},
                {"name": "Journalist", "description": "A journalist."}
            ],
            "edge_types": [
                {"name": "PUBLISHES_IN", "description": "Publishes in."},
                {"name": "COVERS_TOPIC", "description": "Covers a topic."}
            ]
        });

        graph.set_ontology(&ontology);

        assert_eq!(graph.ontology_entity_types, vec!["MediaOutlet", "Journalist"]);
        assert_eq!(graph.ontology_edge_types, vec!["PUBLISHES_IN", "COVERS_TOPIC"]);
    }

    /// `set_ontology` with empty arrays leaves both sets empty.
    #[test]
    fn test_set_ontology_empty_ontology() {
        let mut graph = KnowledgeGraph::new();
        graph.set_ontology(&json!({"entity_types": [], "edge_types": []}));
        assert!(graph.ontology_entity_types.is_empty());
        assert!(graph.ontology_edge_types.is_empty());
    }

    /// `set_ontology` is idempotent — second call replaces the first.
    #[test]
    fn test_set_ontology_is_idempotent_second_call_wins() {
        let mut graph = KnowledgeGraph::new();
        graph.set_ontology(&json!({
            "entity_types": [{"name": "OldType"}],
            "edge_types": [{"name": "OLD_EDGE"}]
        }));
        graph.set_ontology(&json!({
            "entity_types": [{"name": "NewType"}],
            "edge_types": [{"name": "NEW_EDGE"}]
        }));
        assert_eq!(graph.ontology_entity_types, vec!["NewType"]);
        assert_eq!(graph.ontology_edge_types, vec!["NEW_EDGE"]);
    }

    // ---- Custom variant parse tests ----

    /// A kind string matching a registered custom entity type must parse to `EntityKind::Custom`.
    #[test]
    fn test_parse_entities_custom_kind_maps_to_custom_variant() {
        let json = r#"[{"name": "BBC", "kind": "MediaOutlet"}]"#;
        let custom = vec!["MediaOutlet".to_string(), "Journalist".to_string()];
        let entities =
            KnowledgeGraph::parse_entities_json_with_custom(json, &custom).expect("parse ok");
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].kind, EntityKind::Custom("MediaOutlet".to_string()));
    }

    /// A built-in kind string still maps to the built-in variant even when custom kinds present.
    #[test]
    fn test_parse_entities_builtin_kind_still_maps_to_builtin() {
        let json = r#"[{"name": "Alice", "kind": "Person"}]"#;
        let custom = vec!["MediaOutlet".to_string()];
        let entities =
            KnowledgeGraph::parse_entities_json_with_custom(json, &custom).expect("parse ok");
        assert_eq!(entities[0].kind, EntityKind::Person);
    }

    /// An unknown-and-unregistered kind string falls through to `Other`.
    #[test]
    fn test_parse_entities_unknown_unregistered_maps_to_other() {
        let json = r#"[{"name": "X", "kind": "SomeRandomThing"}]"#;
        let custom = vec!["MediaOutlet".to_string()];
        let entities =
            KnowledgeGraph::parse_entities_json_with_custom(json, &custom).expect("parse ok");
        assert_eq!(entities[0].kind, EntityKind::Other);
    }

    /// A custom relation kind maps to `RelationKind::Custom` from the Pass-2 pipeline.
    #[tokio::test]
    async fn test_build_with_custom_relation_kind_emits_custom_variant() {
        #[derive(Clone)]
        struct OntologyMock;
        #[async_trait]
        impl LlmClient for OntologyMock {
            async fn complete(&self, prompt: &str) -> crate::error::Result<String> {
                if prompt.contains("Extract named entities") {
                    Ok(r#"[{"name": "BBC", "kind": "MediaOutlet"}]"#.to_string())
                } else {
                    // Relation with a custom edge kind
                    Ok(r#"[{"from": "BBC", "to": "BBC", "kind": "COVERS_TOPIC", "weight": 0.8}]"#
                        .to_string())
                }
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(
                &self,
                prompt: &str,
            ) -> crate::error::Result<T> {
                let r = self.complete(prompt).await?;
                serde_json::from_str(&r).map_err(|e| TeriError::Llm(e.to_string()))
            }
            async fn stream(
                &self,
                _prompt: &str,
            ) -> crate::error::Result<
                Pin<Box<dyn futures::Stream<Item = crate::error::Result<String>> + Send>>,
            > {
                Err(TeriError::Llm("not used".into()))
            }
            async fn chat(
                &self,
                _m: &[ChatMessage],
                _o: &ChatOptions,
            ) -> crate::error::Result<String> {
                Err(TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _m: &[ChatMessage],
                _o: &ChatOptions,
            ) -> crate::error::Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
        }

        let doc = crate::seed::SeedDocument {
            id: uuid::Uuid::new_v4(),
            raw_text: "BBC covers news.".to_string(),
            metadata: std::collections::HashMap::new(),
            created_at: chrono::Utc::now(),
        };

        let graph = KnowledgeGraph::build_with_progress_and_ontology(
            &doc,
            &OntologyMock,
            &mut |_p, _m| {},
            &["MediaOutlet".to_string()],
            &["COVERS_TOPIC".to_string()],
        )
        .await
        .expect("build ok");

        // Entity should be Custom("MediaOutlet")
        let bbc = graph.get_entity("BBC").expect("BBC should be present");
        assert_eq!(bbc.kind, EntityKind::Custom("MediaOutlet".to_string()));

        // Relation should be Custom("COVERS_TOPIC")
        let edges = graph.get_all_edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].2.kind, RelationKind::Custom("COVERS_TOPIC".to_string()));
    }

    /// `EntityKind::Custom` Display emits the PascalCase name verbatim.
    #[test]
    fn test_entity_kind_custom_display() {
        let k = EntityKind::Custom("MediaOutlet".to_string());
        assert_eq!(k.to_string(), "MediaOutlet");
    }

    /// `RelationKind::Custom` Display emits the UPPER_SNAKE_CASE name verbatim.
    #[test]
    fn test_relation_kind_custom_display() {
        let k = RelationKind::Custom("COVERS_TOPIC".to_string());
        assert_eq!(k.to_string(), "COVERS_TOPIC");
    }

    /// Built-in `EntityKind` variants are not affected by adding `Custom`.
    #[test]
    fn test_entity_kind_builtins_unchanged() {
        assert_eq!(EntityKind::Person.to_string(), "person");
        assert_eq!(EntityKind::Organization.to_string(), "organization");
        assert_eq!(EntityKind::Location.to_string(), "location");
        assert_eq!(EntityKind::Concept.to_string(), "concept");
        assert_eq!(EntityKind::Event.to_string(), "event");
        assert_eq!(EntityKind::Other.to_string(), "other");
    }

    /// Built-in `RelationKind` variants are not affected by adding `Custom`.
    #[test]
    fn test_relation_kind_builtins_unchanged() {
        assert_eq!(RelationKind::WorksFor.to_string(), "WorksFor");
        assert_eq!(RelationKind::LocatedIn.to_string(), "LocatedIn");
        assert_eq!(RelationKind::RelatedTo.to_string(), "RelatedTo");
        assert_eq!(RelationKind::Causes.to_string(), "Causes");
        assert_eq!(RelationKind::Affects.to_string(), "Affects");
        assert_eq!(RelationKind::Other.to_string(), "Other");
    }

    /// `collect_entity_type_names` excludes `Other` and includes custom names.
    #[test]
    fn test_collect_entity_type_names_excludes_other_includes_custom() {
        let mut graph = KnowledgeGraph::new();
        graph
            .add_entity(Entity {
                id: uuid::Uuid::new_v4(),
                name: "Alice".to_string(),
                kind: EntityKind::Person,
            })
            .expect("add ok");
        graph
            .add_entity(Entity {
                id: uuid::Uuid::new_v4(),
                name: "Unknown".to_string(),
                kind: EntityKind::Other,
            })
            .expect("add ok");
        graph
            .add_entity(Entity {
                id: uuid::Uuid::new_v4(),
                name: "BBC".to_string(),
                kind: EntityKind::Custom("MediaOutlet".to_string()),
            })
            .expect("add ok");

        let names = collect_entity_type_names(&graph);
        assert!(names.contains(&"person".to_string()), "person included");
        assert!(names.contains(&"MediaOutlet".to_string()), "Custom included");
        assert!(!names.contains(&"other".to_string()), "Other excluded");
    }
}
