//! FIX-1/FIX-2 verification (INVARIANT 4): drive the in-process `run_cmd` composition
//! (`teri::pipeline::run_pipeline`) end-to-end with an INJECTED MOCK LLM — no live GGUF
//! backend (the backend honesty guard blocks real runs). Asserts the pipeline produces a
//! report and that a `verdict.json` is written with the expected shape.
//!
//! This exercises the exact same service-layer composition the production `teri run` drives
//! (seed → ontology → graph extraction → persona prepare → simulation → report), but with a
//! content-aware mock adapter so it runs deterministically offline.

use async_trait::async_trait;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};

use teri::error::{Result, TeriError};
use teri::llm::{ChatMessage, ChatOptions, LlmClient};
use teri::services::simulation_runner::RunnerStatus;

/// A deterministic, content-aware mock LLM. Returns valid JSON shaped for each pipeline
/// stage, keyed on the prompt/message content. Counts calls so we can assert the pipeline
/// actually drove the LLM (not a canned-output short-circuit).
#[derive(Clone, Default)]
struct MockLlm {
    complete_calls: std::sync::Arc<AtomicUsize>,
    chat_json_calls: std::sync::Arc<AtomicUsize>,
}

impl MockLlm {
    fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl LlmClient for MockLlm {
    async fn complete(&self, prompt: &str) -> Result<String> {
        self.complete_calls.fetch_add(1, Ordering::SeqCst);
        // Persona generation (agent/mod.rs:1404) — a JSON object profile.
        if prompt.contains("social media profile") {
            return Ok(r#"{
                "bio": "Climate policy analyst and frequent commenter.",
                "persona": "A thoughtful, data-driven participant who weighs tradeoffs.",
                "karma": 1200,
                "friend_count": 80,
                "follower_count": 210,
                "statuses_count": 640,
                "age": 41,
                "gender": "female",
                "mbti": "INTJ",
                "country": "Canada",
                "profession": "Analyst",
                "interested_topics": ["policy", "energy"],
                "posting_style": "measured, evidence-first, posts a few times a day"
            }"#
            .to_string());
        }
        // Relation extraction (graph/mod.rs relation prompt) — array of edges.
        if prompt.contains("provided entities") || prompt.contains("\"from\"") {
            return Ok(r#"[
                {"from": "Acme Corp", "to": "Jane Doe", "kind": "RelatedTo", "weight": 0.8}
            ]"#
            .to_string());
        }
        // Entity extraction (graph/mod.rs entity prompt) — array of entities.
        Ok(r#"[
            {"name": "Jane Doe", "kind": "Person"},
            {"name": "Acme Corp", "kind": "Organization"}
        ]"#
        .to_string())
    }

    async fn complete_json<T: serde::de::DeserializeOwned>(&self, _prompt: &str) -> Result<T> {
        // The report agent may call complete_json on one path; return a permissive object.
        serde_json::from_value(serde_json::json!({})).map_err(|e| TeriError::Llm(e.to_string()))
    }

    async fn stream(
        &self,
        _prompt: &str,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
        Err(TeriError::Llm("stream not used by the pipeline".into()))
    }

    async fn chat(&self, _messages: &[ChatMessage], _opts: &ChatOptions) -> Result<String> {
        // Used by the sim-config generator and the report section ReACT loop. Return a
        // plausible non-empty answer (sections fall back gracefully if this is unusable).
        Ok(
            "Final Answer: Based on the simulated discussion, sentiment is cautiously positive."
                .to_string(),
        )
    }

    async fn chat_json<T: serde::de::DeserializeOwned>(
        &self,
        _messages: &[ChatMessage],
        _opts: &ChatOptions,
    ) -> Result<T> {
        self.chat_json_calls.fetch_add(1, Ordering::SeqCst);
        // A superset object satisfying BOTH chat_json consumers:
        //   * ontology generation  → {entity_types, edge_types, analysis_summary}
        //   * report outline plan   → {title, summary, sections:[{title}]}
        // and the sim-config generator if it ever routes here.
        let v = serde_json::json!({
            "entity_types": [
                {"name": "Person", "description": "An individual.", "attributes": [], "examples": []},
                {"name": "Organization", "description": "A company.", "attributes": [], "examples": []}
            ],
            "edge_types": [
                {"name": "RELATED_TO", "description": "Generic association.",
                 "source_types": ["Person"], "target_types": ["Organization"]}
            ],
            "analysis_summary": "A small social graph for testing.",
            "title": "Simulation Analysis Report",
            "summary": "Cautiously positive sentiment emerged across the simulated agents.",
            "sections": [
                {"title": "Overview"},
                {"title": "Key Dynamics"},
                {"title": "Predicted Outcome"}
            ]
        });
        serde_json::from_value(v).map_err(|e| TeriError::Llm(e.to_string()))
    }
}

/// Build a teri Config rooted entirely under `root` so the test writes no global state.
fn test_config(root: &std::path::Path) -> teri::Config {
    // Config::build reads env; populate the path-bearing vars to point at the temp dir.
    let upload = root.join("uploads");
    let sim_data = root.join("uploads").join("simulations");
    let mem_db = root.join("data").join("memory");
    let graph_db = root.join("data").join("graph");
    std::fs::create_dir_all(&upload).unwrap();
    std::fs::create_dir_all(&sim_data).unwrap();
    std::fs::create_dir_all(&mem_db).unwrap();
    std::fs::create_dir_all(&graph_db).unwrap();

    // SAFETY: set within a single-threaded test before any concurrent env reads.
    unsafe {
        std::env::set_var("LLM_API_KEY", "test-key");
        std::env::set_var("UPLOAD_FOLDER", upload.to_str().unwrap());
        std::env::set_var("OASIS_SIMULATION_DATA_DIR", sim_data.to_str().unwrap());
        std::env::set_var("MEMORY_DB_PATH", mem_db.join("mem.redb").to_str().unwrap());
        std::env::set_var("GRAPH_DB_PATH", graph_db.to_str().unwrap());
    }
    teri::Config::load().expect("config load")
}

#[tokio::test]
async fn run_pipeline_with_mock_llm_produces_report_and_verdict() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = test_config(tmp.path());

    // A short seed document on disk (the `--seed` input).
    let seed_path = tmp.path().join("seed.txt");
    std::fs::write(
        &seed_path,
        "Jane Doe works at Acme Corp. Acme Corp announced a new climate policy initiative \
         that drew significant public discussion online about its likely effectiveness.",
    )
    .unwrap();

    let llm = MockLlm::new();
    let complete_counter = llm.complete_calls.clone();
    let chat_json_counter = llm.chat_json_calls.clone();

    let out_path = tmp.path().join("verdict.json");

    // Drive the FULL in-process pipeline composition (FIX-1).
    let outcome = teri::pipeline::run_pipeline(
        &config,
        llm,
        seed_path.to_str().unwrap(),
        "Will the climate policy initiative be well received?",
        25,
    )
    .await
    .expect("pipeline should complete with the mock LLM");

    // The LLM was actually driven (ontology + extraction + persona + report).
    assert!(
        complete_counter.load(Ordering::SeqCst) > 0,
        "complete() should have been called"
    );
    assert!(
        chat_json_counter.load(Ordering::SeqCst) > 0,
        "chat_json() should have been called"
    );

    // Graph build produced real entities/edges from the seed via LLM extraction.
    assert!(
        outcome.graph_node_count >= 2,
        "expected >=2 graph nodes, got {}",
        outcome.graph_node_count
    );

    // Persona generation produced agents (one per surviving entity).
    assert!(outcome.agents_generated >= 1, "expected >=1 generated agent");
    assert_eq!(outcome.agents_requested, 25);

    // Simulation reached a terminal status (not stuck running).
    assert!(
        matches!(
            outcome.sim_runner_status,
            RunnerStatus::Completed | RunnerStatus::Stopped | RunnerStatus::Failed
        ),
        "sim should be terminal, got {:?}",
        outcome.sim_runner_status
    );

    // A report was produced (non-empty id) with findings sections from the outline.
    assert!(!outcome.report_id.is_empty(), "report id should be set");
    assert!(
        !outcome.report_section_titles.is_empty(),
        "report should have key-findings sections"
    );

    // FIX-2: write verdict.json and assert its shape.
    let verdict = outcome.to_verdict_json();
    std::fs::write(&out_path, serde_json::to_string_pretty(&verdict).unwrap()).unwrap();
    assert!(out_path.exists(), "verdict.json must be written");

    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out_path).unwrap()).unwrap();
    assert_eq!(written["query"], "Will the climate policy initiative be well received?");
    assert_eq!(written["agents"]["requested"], 25);
    assert!(written["graph"]["node_count"].as_u64().unwrap() >= 2);
    assert!(written["report"]["report_id"].as_str().unwrap().starts_with("report_"));
    assert!(
        !written["report"]["key_findings"].as_array().unwrap().is_empty(),
        "verdict.json must summarize report key findings"
    );
    assert!(written["simulation"]["simulation_id"].as_str().unwrap().starts_with("sim_"));
}
