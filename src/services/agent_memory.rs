//! Agent long-term memory write-back from the simulation loop.
//!
//! Closes the "agent LTM / vector write-back from the sim loop unwired" gap (RUNBOOK §13):
//! `MemoryStore::write_vec_text` / `write_ltm` previously had **no** sim-path callers, so the
//! swarm's posts/comments were never persisted as recallable agent memory — the agents had no
//! long-term memory of what they said. This writer hooks the monitor's per-action stream (the
//! same point [`GraphMemoryUpdater`](crate::services::graph_memory) consumes) and, for each
//! content-bearing action, persists the text as BOTH a chronological [`MemoryEntry`] via
//! `write_ltm` (no embedding — survives offline) AND a semantic [`VectorEntry`] via
//! `write_vec_text` (embedding-backed — powers [`MemoryStore::semantic_recall`]), under a
//! deterministic per-`(simulation, agent)` namespace so an agent's memories cluster and recall
//! can target one agent.
//!
//! **Distinct from [`GraphVectorIndex`](crate::services::graph_builder::GraphVectorIndex)**, which
//! embeds the *synthesized knowledge-graph* entities/edges under the GRAPH namespace for the
//! report's graph-search lens. This embeds the *raw agent activity* under per-AGENT namespaces —
//! the agent LTM the gap names. The two do not overlap (different write methods, different
//! namespaces).
//!
//! **Best-effort by contract:** an embedding-endpoint failure (keyless/offline backend) is logged
//! and counted, never propagated — a missing embeddings endpoint must not fail a simulation. The
//! plain `write_ltm` entry is written regardless, so the textual memory survives even with no
//! embedding backend (keyless-safe, no-downgrade).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use uuid::Uuid;

use crate::embedding::EmbeddingClient;
use crate::memory::{MemoryEntry, MemoryStore};

/// Fixed teri namespace UUID for deriving per-agent LTM namespaces (UUID v5). Stable across
/// processes and runs so the same `(simulation, agent)` always maps to the same namespace key,
/// letting an agent's memories accumulate under one prefix.
const TERI_AGENT_LTM_NS: Uuid = Uuid::from_u128(0x7e71_a6e0_4c74_6d5e_8ea6_e000_0000_0001);

/// Default importance for a remembered agent action. Uniform for now (every utterance is equally
/// memorable); a future slice may weight by action type or engagement.
const DEFAULT_IMPORTANCE: f32 = 1.0;

/// Writes simulated agent activity into the long-term memory store as both chronological and
/// semantic memory. Constructed once per run and shared (`Arc`) by the monitor.
pub struct AgentMemoryWriter {
    store: Arc<MemoryStore>,
    embedder: Arc<EmbeddingClient>,
    /// Actions persisted as a plain `MemoryEntry` (`write_ltm` succeeded).
    persisted: AtomicU64,
    /// Actions also embedded into the vector store (`write_vec_text` succeeded). `<= persisted`.
    embedded: AtomicU64,
    /// Actions skipped because they carried no rememberable content (do-nothing, like/follow…).
    skipped: AtomicU64,
}

impl AgentMemoryWriter {
    /// Build a writer over a shared memory store + embedding client.
    pub fn new(store: Arc<MemoryStore>, embedder: Arc<EmbeddingClient>) -> Self {
        Self {
            store,
            embedder,
            persisted: AtomicU64::new(0),
            embedded: AtomicU64::new(0),
            skipped: AtomicU64::new(0),
        }
    }

    /// Deterministic per-`(simulation, agent)` namespace UUID (v5 over `"{sim_id}:{agent_id}"`).
    /// The same agent in the same simulation always resolves to the same namespace, so its
    /// memories share the `agent:{uuid}:vec:` / `:ltm:` key prefix and can be recalled together.
    pub fn agent_namespace(simulation_id: &str, agent_id: i64) -> Uuid {
        Uuid::new_v5(&TERI_AGENT_LTM_NS, format!("{simulation_id}:{agent_id}").as_bytes())
    }

    /// Extract the human-meaningful memory text from a raw action record, or `None` when the
    /// action carries nothing worth remembering. We remember utterances (posts/comments/quotes
    /// with text), not structural actions (do-nothing, like, follow) which have no content.
    ///
    /// Content is taken from `action_args.content` / `.text` / `.comment` (the fields OASIS
    /// writes for textual actions), falling back to the top-level `result` string. Empty or
    /// whitespace-only content yields `None`.
    pub fn action_memory_text(action_data: &Value, platform: &str) -> Option<String> {
        let action_type =
            action_data.get("action_type").and_then(Value::as_str).unwrap_or("").trim();
        if action_type.is_empty() || action_type.eq_ignore_ascii_case("do_nothing") {
            return None;
        }

        let content = action_data
            .get("action_args")
            .and_then(Value::as_object)
            .and_then(|args| {
                args.get("content")
                    .or_else(|| args.get("text"))
                    .or_else(|| args.get("comment"))
                    .and_then(Value::as_str)
            })
            .or_else(|| action_data.get("result").and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty())?;

        let agent_name =
            action_data.get("agent_name").and_then(Value::as_str).unwrap_or("agent").trim();
        Some(format!("[{platform}] {agent_name} {action_type}: {content}"))
    }

    /// Persist one action as agent long-term memory (best-effort, never errors out the sim).
    ///
    /// Always writes the chronological `MemoryEntry` (no network) so the textual memory survives
    /// offline; additionally embeds + writes the semantic `VectorEntry` when the embedding backend
    /// is reachable. Failures are logged at `debug` and counted, not propagated.
    pub async fn write_action(&self, simulation_id: &str, action_data: &Value, platform: &str) {
        let Some(text) = Self::action_memory_text(action_data, platform) else {
            self.skipped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let agent_id = action_data.get("agent_id").and_then(Value::as_i64).unwrap_or(0);
        let ns = Self::agent_namespace(simulation_id, agent_id);

        // (1) Chronological LTM — no embedding required, so this is the keyless-safe baseline.
        let entry = MemoryEntry {
            timestamp: chrono::Utc::now(),
            content: text.clone(),
            importance: DEFAULT_IMPORTANCE,
        };
        match self.store.write_ltm(ns, &entry).await {
            Ok(()) => {
                self.persisted.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                tracing::debug!(agent_id, error = %e, "agent LTM: chronological write failed");
            }
        }

        // (2) Semantic vector memory — best-effort; needs the embeddings endpoint.
        match self.store.write_vec_text(ns, &self.embedder, &text, DEFAULT_IMPORTANCE).await {
            Ok(()) => {
                self.embedded.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                tracing::debug!(
                    agent_id,
                    error = %e,
                    "agent LTM: vector embed skipped (embeddings backend unavailable)"
                );
            }
        }
    }

    /// `(persisted, embedded, skipped)` counters for end-of-run observability.
    pub fn stats(&self) -> (u64, u64, u64) {
        (
            self.persisted.load(Ordering::Relaxed),
            self.embedded.load(Ordering::Relaxed),
            self.skipped.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn namespace_is_deterministic_and_agent_specific() {
        let a1 = AgentMemoryWriter::agent_namespace("sim_1", 7);
        let a1_again = AgentMemoryWriter::agent_namespace("sim_1", 7);
        let a2 = AgentMemoryWriter::agent_namespace("sim_1", 8);
        let a3 = AgentMemoryWriter::agent_namespace("sim_2", 7);
        assert_eq!(a1, a1_again, "same (sim, agent) → same namespace");
        assert_ne!(a1, a2, "different agent → different namespace");
        assert_ne!(a1, a3, "different simulation → different namespace");
    }

    #[test]
    fn memory_text_extracts_content_and_skips_structural_actions() {
        // A post with content → remembered.
        let post = json!({
            "agent_name": "Jane",
            "action_type": "create_post",
            "action_args": {"content": "Climate policy will reshape the energy market."}
        });
        let text = AgentMemoryWriter::action_memory_text(&post, "reddit").unwrap();
        assert!(text.contains("Jane"));
        assert!(text.contains("create_post"));
        assert!(text.contains("reshape the energy market"));
        assert!(text.starts_with("[reddit]"));

        // A comment via `result` fallback → remembered.
        let comment = json!({
            "agent_name": "Bob",
            "action_type": "create_comment",
            "result": "I disagree, the costs are understated."
        });
        assert!(AgentMemoryWriter::action_memory_text(&comment, "twitter").is_some());

        // Structural / contentless actions → skipped.
        assert!(
            AgentMemoryWriter::action_memory_text(&json!({"action_type": "do_nothing"}), "reddit")
                .is_none()
        );
        assert!(
            AgentMemoryWriter::action_memory_text(
                &json!({"action_type": "like_post", "action_args": {}}),
                "reddit"
            )
            .is_none()
        );
        assert!(
            AgentMemoryWriter::action_memory_text(&json!({"action_type": ""}), "reddit").is_none()
        );
        // Empty content → skipped.
        assert!(
            AgentMemoryWriter::action_memory_text(
                &json!({"action_type": "create_post", "action_args": {"content": "   "}}),
                "reddit"
            )
            .is_none()
        );
    }
}
