//! Integration proof for the agent LTM write-back (vector + chronological).
//!
//! Drives `AgentMemoryWriter::write_action` against a real `MemoryStore` and a mock
//! OpenAI-compatible embeddings endpoint, then asserts the persisted utterances are
//! retrievable via BOTH `semantic_recall` (vector) and `read_ltm` (chronological), and that a
//! structural (do-nothing) action was NOT remembered. This is the round-trip the sim-loop monitor
//! exercises per action — closing the "agent LTM write-back unwired" gap.

use httpmock::prelude::*;
use serde_json::json;
use std::sync::Arc;

use teri::config::{LlmConfig, LlmProvider};
use teri::embedding::EmbeddingClient;
use teri::memory::MemoryStore;
use teri::services::agent_memory::AgentMemoryWriter;

fn embed_config(server: &MockServer) -> LlmConfig {
    LlmConfig {
        base_url: server.base_url(),
        api_key: String::new(),
        model: "unused".to_string(),
        embed_model: "all-MiniLM-L6-v2".to_string(),
        timeout_secs: 5,
        max_retries: 0,
        max_tokens: 2048,
        provider: LlmProvider::Openai,
    }
}

#[tokio::test]
async fn agent_utterances_are_persisted_and_recallable() {
    // Mock embeddings: a fixed vector for every call (the cosine-ranking quality is the store's
    // own tested concern; here we prove the write→store→recall wiring round-trips).
    let server = MockServer::start();
    let embed_mock = server.mock(|when, then| {
        when.method(POST).path("/embeddings");
        then.status(200).header("Content-Type", "application/json").body(
            r#"{"object":"list","data":[{"object":"embedding","embedding":[0.11,0.22,0.33],"index":0}],"model":"all-MiniLM-L6-v2","usage":{"prompt_tokens":3,"total_tokens":3}}"#,
        );
    });

    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(MemoryStore::new(tmp.path()).unwrap());
    let embedder = Arc::new(EmbeddingClient::new(&embed_config(&server)));
    let writer = AgentMemoryWriter::new(store.clone(), embedder.clone());

    let sim_id = "sim_recall_test";
    let agent_id = 1_i64;

    // Two content-bearing actions by the same agent → both remembered under one namespace.
    writer
        .write_action(
            sim_id,
            &json!({
                "agent_id": agent_id, "agent_name": "Jane", "action_type": "create_post",
                "action_args": {"content": "Climate policy will reshape the energy market."}
            }),
            "reddit",
        )
        .await;
    writer
        .write_action(
            sim_id,
            &json!({
                "agent_id": agent_id, "agent_name": "Jane", "action_type": "create_comment",
                "result": "The transition costs are understated."
            }),
            "reddit",
        )
        .await;
    // A structural action → skipped (no content to remember).
    writer
        .write_action(
            sim_id,
            &json!({"agent_id": agent_id, "agent_name": "Jane", "action_type": "do_nothing"}),
            "reddit",
        )
        .await;

    // Stats: 2 persisted (chronological), 2 embedded (vector), 1 skipped.
    let (persisted, embedded, skipped) = writer.stats();
    assert_eq!(persisted, 2, "two content-bearing actions persisted");
    assert_eq!(embedded, 2, "two actions embedded into the vector store");
    assert_eq!(skipped, 1, "the do_nothing action was skipped");

    let ns = AgentMemoryWriter::agent_namespace(sim_id, agent_id);

    // (1) Vector recall: semantic_recall returns the stored utterances.
    let recalled = store
        .semantic_recall(ns, &embedder, "What about climate policy?", 10)
        .await
        .unwrap();
    assert_eq!(recalled.len(), 2, "both vector memories recalled");
    let contents: Vec<&str> = recalled.iter().map(|e| e.content.as_str()).collect();
    assert!(
        contents.iter().any(|c| c.contains("reshape the energy market")),
        "post recalled"
    );
    assert!(
        contents.iter().any(|c| c.contains("transition costs are understated")),
        "comment recalled"
    );

    // (2) Chronological recall: read_ltm returns the same utterances.
    let ltm = store.read_ltm(ns, 10).await.unwrap();
    assert_eq!(ltm.len(), 2, "both chronological memories stored");

    // (3) Namespace isolation: a different agent's namespace is empty.
    let other_ns = AgentMemoryWriter::agent_namespace(sim_id, 999);
    assert!(
        store.read_ltm(other_ns, 10).await.unwrap().is_empty(),
        "other agent has no memories"
    );

    // 3 embed calls: 2 writes + 1 query (do_nothing never embeds).
    embed_mock.assert_hits(3);
}

#[tokio::test]
async fn embeddings_offline_still_persists_chronological_memory() {
    // No embeddings endpoint (server returns 500) — the vector write fails best-effort, but the
    // chronological LTM must still be written (keyless-safe, no-downgrade).
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/embeddings");
        then.status(500).body(r#"{"error":"down"}"#);
    });

    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(MemoryStore::new(tmp.path()).unwrap());
    let embedder = Arc::new(EmbeddingClient::new(&embed_config(&server)));
    let writer = AgentMemoryWriter::new(store.clone(), embedder.clone());

    let sim_id = "sim_offline";
    writer
        .write_action(
            sim_id,
            &json!({
                "agent_id": 2, "agent_name": "Bob", "action_type": "create_post",
                "action_args": {"content": "Offline but still remembered."}
            }),
            "twitter",
        )
        .await;

    let (persisted, embedded, _skipped) = writer.stats();
    assert_eq!(persisted, 1, "chronological memory written despite embed failure");
    assert_eq!(embedded, 0, "vector write failed (embeddings down)");

    let ns = AgentMemoryWriter::agent_namespace(sim_id, 2);
    let ltm = store.read_ltm(ns, 10).await.unwrap();
    assert_eq!(ltm.len(), 1, "the utterance survived offline via write_ltm");
    assert!(ltm[0].content.contains("Offline but still remembered"));
}
