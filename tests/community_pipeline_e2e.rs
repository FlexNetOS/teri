//! S5 (TASK-SEAM-3) — the LLM-backed pipeline MIDDLE of the teri↔pebesen loop.
//!
//! `community_loop_e2e.rs` already covers the two ends (ingest: pebesen signal → SeedDocument;
//! feedback: teri prediction → live receiver → store → calibration). This test stitches the
//! **middle** so the whole loop is exercised in one pass:
//!
//!   pebesen signal → `signal_to_seed_document` → temp seed file
//!     → `pipeline::run_pipeline` (seed→graph→persona→sim→report, INJECTED MOCK LLM)
//!       → derive a `TopicSignal` from the report → push via `PebesenFeedback`
//!         → live in-process receiver → assert it lands scoped to the originating space.
//!
//! The inference backend is a deterministic, content-aware mock `LlmClient` injected directly
//! into `run_pipeline` (the backend-honesty guard only gates real `teri run`/`serve`, never an
//! injected adapter — see `pipeline_run.rs`), so the test runs offline and deterministically.

use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use chrono::Utc;
use pebesen_intelligence::{IntelligenceStore, http::router};
use teri::error::{Result, TeriError};
use teri::llm::{ChatMessage, ChatOptions, LlmClient};
use teri::seed::community::pebesen::PebesenFeedback;
use teri::seed::community::{
    CommunityContributor, CommunityDomain, CommunityFeedback, CommunitySignal, CommunityTopic,
    TopicSignal, signal_to_seed_document,
};

// ── Mock inference backend ──────────────────────────────────────────────────
// Deterministic, content-aware LLM keyed on each pipeline stage's prompt shape (NOT on the
// seed content), so it serves a community-derived seed exactly as it serves any other.
// Mirrors the proven mock in `pipeline_run.rs`.

#[derive(Clone, Default)]
struct MockLlm {
    complete_calls: std::sync::Arc<AtomicUsize>,
}

#[async_trait]
impl LlmClient for MockLlm {
    async fn complete(&self, prompt: &str) -> Result<String> {
        self.complete_calls.fetch_add(1, Ordering::SeqCst);
        if prompt.contains("social media profile") {
            return Ok(r#"{
                "bio": "Community moderator and frequent contributor.",
                "persona": "A steady, community-minded participant who mediates discussions.",
                "karma": 1500,
                "friend_count": 60,
                "follower_count": 180,
                "statuses_count": 540,
                "age": 38,
                "gender": "non-binary",
                "mbti": "ENFJ",
                "country": "Germany",
                "profession": "Maintainer",
                "interested_topics": ["rust", "governance"],
                "posting_style": "encouraging, asks clarifying questions"
            }"#
            .to_string());
        }
        if prompt.contains("provided entities") || prompt.contains("\"from\"") {
            return Ok(r#"[
                {"from": "Rust Lang", "to": "Ferris", "kind": "RelatedTo", "weight": 0.8}
            ]"#
            .to_string());
        }
        Ok(r#"[
            {"name": "Ferris", "kind": "Person"},
            {"name": "Rust Lang", "kind": "Organization"}
        ]"#
        .to_string())
    }

    async fn complete_json<T: serde::de::DeserializeOwned>(&self, _prompt: &str) -> Result<T> {
        serde_json::from_value(serde_json::json!({})).map_err(|e| TeriError::Llm(e.to_string()))
    }

    async fn stream(
        &self,
        _prompt: &str,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
        Err(TeriError::Llm("stream not used by the pipeline".into()))
    }

    async fn chat(&self, _messages: &[ChatMessage], _opts: &ChatOptions) -> Result<String> {
        Ok("Final Answer: Community momentum is rising; engagement trends upward.".to_string())
    }

    async fn chat_json<T: serde::de::DeserializeOwned>(
        &self,
        _messages: &[ChatMessage],
        _opts: &ChatOptions,
    ) -> Result<T> {
        let v = serde_json::json!({
            "entity_types": [
                {"name": "Person", "description": "An individual.", "attributes": [], "examples": []},
                {"name": "Organization", "description": "A group.", "attributes": [], "examples": []}
            ],
            "edge_types": [
                {"name": "RELATED_TO", "description": "Generic association.",
                 "source_types": ["Person"], "target_types": ["Organization"]}
            ],
            "analysis_summary": "A small community graph for testing.",
            "title": "Community Momentum Report",
            "summary": "Engagement is trending upward across the simulated community.",
            "sections": [
                {"title": "Overview"},
                {"title": "Engagement Dynamics"},
                {"title": "Predicted Momentum"}
            ]
        });
        serde_json::from_value(v).map_err(|e| TeriError::Llm(e.to_string()))
    }
}

/// Build a teri Config rooted entirely under `root` (writes no global state).
fn test_config(root: &std::path::Path) -> teri::Config {
    let upload = root.join("uploads");
    let sim_data = upload.join("simulations");
    let mem_db = root.join("data").join("memory");
    let graph_db = root.join("data").join("graph");
    for d in [&upload, &sim_data, &mem_db, &graph_db] {
        std::fs::create_dir_all(d).unwrap();
    }
    // SAFETY: set within a single-threaded section before the pipeline reads config.
    unsafe {
        std::env::set_var("LLM_API_KEY", "test-key");
        std::env::set_var("UPLOAD_FOLDER", upload.to_str().unwrap());
        std::env::set_var("OASIS_SIMULATION_DATA_DIR", sim_data.to_str().unwrap());
        std::env::set_var("MEMORY_DB_PATH", mem_db.join("mem.redb").to_str().unwrap());
        std::env::set_var("GRAPH_DB_PATH", graph_db.to_str().unwrap());
    }
    teri::Config::load().expect("config load")
}

/// Start the real pebesen intelligence receiver in-process on an ephemeral port.
async fn spawn_receiver() -> (String, IntelligenceStore) {
    let store = IntelligenceStore::new();
    let app = router(store.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve receiver");
    });
    (format!("http://{addr}"), store)
}

fn community_fixture() -> (CommunityDomain, CommunitySignal, Vec<CommunityContributor>) {
    let domain = CommunityDomain {
        id: "22222222-2222-2222-2222-222222222222".to_string(),
        slug: "rust-lang".to_string(),
        name: "Rust Lang".to_string(),
        description: Some("All things Rust".to_string()),
        visibility: "public".to_string(),
        member_count: 42,
    };
    let signal = CommunitySignal {
        domain_id: domain.id.clone(),
        domain_slug: domain.slug.clone(),
        contributor_count: 1,
        topic_count: 4,
        active_topic_count: 2,
        recent_topics: vec![CommunityTopic {
            id: "t1".to_string(),
            stream_id: "s1".to_string(),
            name: "async traits stabilization".to_string(),
            status: "open".to_string(),
            created_at: None,
            last_active: Some(Utc::now()),
        }],
        captured_at: Utc::now(),
    };
    let contributors = vec![CommunityContributor {
        id: "u1".to_string(),
        username: "ferris".to_string(),
        display_name: "Ferris".to_string(),
        role: "owner".to_string(),
        joined_at: None,
    }];
    (domain, signal, contributors)
}

/// The full loop: ingest a pebesen signal, run the real pipeline middle on it with a mock LLM,
/// derive a prediction from the report, push it back, and assert it lands scoped correctly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn community_signal_through_pipeline_to_prediction() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = test_config(tmp.path());

    // ── Ingest: pebesen signal → SeedDocument → seed file on disk ──
    let (domain, signal, contributors) = community_fixture();
    let seed_doc = signal_to_seed_document(&domain, &signal, &contributors);
    assert_eq!(seed_doc.metadata.get("domain_id").unwrap(), &domain.id);
    let seed_path = tmp.path().join("community_seed.txt");
    std::fs::write(&seed_path, &seed_doc.raw_text).unwrap();

    // ── Middle: run the real pipeline on the community seed (mock LLM) ──
    let llm = MockLlm::default();
    let calls = llm.complete_calls.clone();
    let outcome = teri::pipeline::run_pipeline(
        &config,
        llm,
        seed_path.to_str().unwrap(),
        "How will community engagement trend over the next 30 days?",
        10,
    )
    .await
    .expect("pipeline runs on the community seed");

    // The middle really ran (not a canned short-circuit): the LLM was driven, a graph was
    // built, and a report was produced.
    assert!(calls.load(Ordering::SeqCst) > 0, "the pipeline must drive the LLM");
    assert!(outcome.graph_node_count > 0, "graph must have entities from the community seed");
    assert!(!outcome.report_id.is_empty(), "a report must be produced");

    // ── Feedback: derive a prediction from the report and push it back ──
    let (base, store) = spawn_receiver().await;
    let feedback = PebesenFeedback::new(&base);

    // Derive a topic signal from the report outcome. Confidence is synthesized report metadata
    // (NOT calibrated — that's TASK-AUTO-2); the rationale carries the report summary so the
    // prediction is traceable to the run that produced it.
    let rationale = outcome
        .report_summary
        .clone()
        .unwrap_or_else(|| "community engagement trending upward".to_string());
    let prediction = TopicSignal {
        topic_id: signal.recent_topics[0].id.clone(),
        domain_id: domain.id.clone(),
        momentum: 0.5,
        confidence: 0.7,
        rationale,
    };
    feedback
        .push_topic_signals(vec![prediction])
        .await
        .expect("push prediction to receiver");

    // ── Assert: the prediction landed in the receiver, scoped to the originating space ──
    let stored = store.list_by_space(&domain.id).expect("list by space");
    assert_eq!(stored.len(), 1, "exactly the one derived prediction must land in the space");
    assert_eq!(
        stored[0].payload.domain_id(),
        domain.id.as_str(),
        "prediction must be scoped to its space"
    );
    assert_eq!(stored[0].payload.confidence(), 0.7);
    assert_eq!(
        stored[0].payload.topic_id(),
        Some(signal.recent_topics[0].id.as_str()),
        "prediction must reference the originating topic"
    );

    // Loop closed: a pebesen signal drove a full teri prediction run whose output landed back in
    // pebesen, scoped to the space the signal came from.
}
