use crate::error::{Result, TeriError};
use crate::sim::WorldSnapshot;
use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

// Key schema constants
// agent:{uuid}:ltm:{timestamp} → MemoryEntry
pub const AGENT_LTM_KEY_PREFIX: &str = "agent";
// world:{sim_id}:tick:{n} → WorldSnapshot
pub const WORLD_SNAPSHOT_KEY_PREFIX: &str = "world";
// agent:{uuid}:vec:{timestamp} → VectorEntry
pub const AGENT_VEC_KEY_PREFIX: &str = "agent_vec";

const KV_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("kv");

/// Process-monotonic counter that disambiguates `write_vec` keys sharing a millisecond timestamp.
/// Without it, two same-millisecond writes collide on `agent:{id}:vec:{ts}` and the second
/// silently overwrites the first (a latent bug that intermittently lost vectors under load).
static VEC_WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Same disambiguation for `write_ltm`. The vec path was fixed in Workstream B, but the LTM
/// path kept the bare `agent:{id}:ltm:{ts}` (millisecond) key, so two long-term memories an
/// agent writes in the same millisecond would collide and silently lose one. Mirror the vec
/// fix so the LTM store is collision-safe before it gets wired into the agent loop.
static LTM_WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub content: String,
    pub importance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub content: String,
    pub embedding: Vec<f32>,
    pub importance: f32,
}

#[derive(Clone)]
pub struct MemoryStore {
    // Redb instance for all memory operations
    db: Arc<Database>,
}

impl MemoryStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        // Ensure the directory exists
        let rocks_path = path.as_ref().join("rocksdb");
        std::fs::create_dir_all(&rocks_path)
            .map_err(|e| TeriError::Memory(format!("Failed to create db dir: {e}")))?;
        let db_file = rocks_path.join("teri.redb");
        let db = Database::create(&db_file)
            .map_err(|e| TeriError::Memory(format!("Failed to open redb: {e}")))?;

        // Initialize tables
        let write_txn =
            db.begin_write().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
        write_txn
            .open_table(KV_TABLE)
            .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
        write_txn
            .commit()
            .map_err(|e| TeriError::Memory(format!("Commit error: {e}")))?;

        Ok(Self { db: Arc::new(db) })
    }

    pub async fn write_ltm(&self, agent_id: Uuid, entry: &MemoryEntry) -> Result<()> {
        let ts = entry.timestamp.timestamp_millis();
        // Append a process-monotonic sequence so same-millisecond LTM writes don't collide
        // (mirrors `write_vec`). The `:ltm:` prefix is preserved, so `read_ltm` / `query_ltm`
        // prefix scans are unchanged.
        let seq = LTM_WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let key = format!("agent:{agent_id}:ltm:{ts}:{seq:020}");
        let value = serde_json::to_vec(entry)
            .map_err(|e| TeriError::Memory(format!("Serialization error: {e}")))?;
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let write_txn =
                db.begin_write().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
            {
                let mut table = write_txn
                    .open_table(KV_TABLE)
                    .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
                table
                    .insert(key.as_str(), value.as_slice())
                    .map_err(|e| TeriError::Memory(format!("Write error: {e}")))?;
            }
            write_txn.commit().map_err(|e| TeriError::Memory(format!("Commit error: {e}")))
        })
        .await
        .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))?
    }

    pub async fn read_ltm(&self, agent_id: Uuid, limit: usize) -> Result<Vec<MemoryEntry>> {
        let prefix = format!("agent:{agent_id}:ltm:");
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let read_txn =
                db.begin_read().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
            let table = read_txn
                .open_table(KV_TABLE)
                .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
            let mut entries = Vec::new();

            let iter = table
                .range(prefix.as_str()..)
                .map_err(|e| TeriError::Memory(format!("Iterator error: {e}")))?;
            for item in iter {
                let (k, v) =
                    item.map_err(|e| TeriError::Memory(format!("Iterator item error: {e}")))?;
                if !k.value().starts_with(&prefix) {
                    break;
                }
                let entry: MemoryEntry = serde_json::from_slice(v.value())
                    .map_err(|e| TeriError::Memory(format!("Deserialization error: {e}")))?;
                entries.push(entry);
                if entries.len() >= limit {
                    break;
                }
            }
            Ok(entries)
        })
        .await
        .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))?
    }

    pub async fn query_ltm(&self, agent_id: Uuid, query: &str) -> Result<Vec<MemoryEntry>> {
        let prefix = format!("agent:{agent_id}:ltm:");
        let db = self.db.clone();
        let query_lower = query.to_lowercase();
        tokio::task::spawn_blocking(move || {
            let read_txn =
                db.begin_read().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
            let table = read_txn
                .open_table(KV_TABLE)
                .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
            let mut entries = Vec::new();
            let iter = table
                .range(prefix.as_str()..)
                .map_err(|e| TeriError::Memory(format!("Iterator error: {e}")))?;
            for item in iter {
                let (k, v) =
                    item.map_err(|e| TeriError::Memory(format!("Iterator item error: {e}")))?;
                if !k.value().starts_with(&prefix) {
                    break;
                }
                let entry: MemoryEntry = serde_json::from_slice(v.value())
                    .map_err(|e| TeriError::Memory(format!("Deserialization error: {e}")))?;
                if entry.content.to_lowercase().contains(&query_lower) {
                    entries.push(entry);
                }
            }
            Ok(entries)
        })
        .await
        .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))?
    }

    pub async fn write_snapshot(
        &self,
        sim_id: Uuid,
        tick: u32,
        snapshot: &WorldSnapshot,
    ) -> Result<()> {
        let key = format!("world:{sim_id}:tick:{tick:010}");
        let value = bincode::serialize(snapshot)
            .map_err(|e| TeriError::Memory(format!("Serialization error: {e}")))?;
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let write_txn =
                db.begin_write().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
            {
                let mut table = write_txn
                    .open_table(KV_TABLE)
                    .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
                table
                    .insert(key.as_str(), value.as_slice())
                    .map_err(|e| TeriError::Memory(format!("Write error: {e}")))?;
            }
            write_txn.commit().map_err(|e| TeriError::Memory(format!("Commit error: {e}")))
        })
        .await
        .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))?
    }

    pub async fn read_snapshot(&self, sim_id: Uuid, tick: u32) -> Result<WorldSnapshot> {
        let key = format!("world:{sim_id}:tick:{tick:010}");
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let read_txn =
                db.begin_read().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
            let table = read_txn
                .open_table(KV_TABLE)
                .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
            let v_guard = table
                .get(key.as_str())
                .map_err(|e| TeriError::Memory(format!("Read error: {e}")))?;
            let v =
                v_guard.ok_or_else(|| TeriError::Memory(format!("Snapshot not found: {key}")))?;
            bincode::deserialize(v.value())
                .map_err(|e| TeriError::Memory(format!("Deserialization error: {e}")))
        })
        .await
        .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))?
    }

    pub async fn read_history(&self, sim_id: Uuid) -> Result<Vec<WorldSnapshot>> {
        self.read_history_limit(sim_id, usize::MAX).await
    }

    pub async fn read_history_limit(
        &self,
        sim_id: Uuid,
        limit: usize,
    ) -> Result<Vec<WorldSnapshot>> {
        let prefix = format!("world:{sim_id}:tick:");
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let read_txn =
                db.begin_read().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
            let table = read_txn
                .open_table(KV_TABLE)
                .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
            let mut snapshots = Vec::new();
            let iter = table
                .range(prefix.as_str()..)
                .map_err(|e| TeriError::Memory(format!("Iterator error: {e}")))?;
            for item in iter {
                let (k, v) =
                    item.map_err(|e| TeriError::Memory(format!("Iterator item error: {e}")))?;
                if !k.value().starts_with(&prefix) {
                    break;
                }
                let snapshot: WorldSnapshot = bincode::deserialize(v.value())
                    .map_err(|e| TeriError::Memory(format!("Deserialization error: {e}")))?;
                snapshots.push(snapshot);
                if snapshots.len() >= limit {
                    break;
                }
            }
            Ok(snapshots)
        })
        .await
        .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))?
    }

    pub async fn write_vec(&self, agent_id: Uuid, entry: &VectorEntry) -> Result<()> {
        let ts = entry.timestamp.timestamp_millis();
        // Workstream B: append a process-monotonic sequence so two writes sharing a millisecond
        // timestamp do not collide on the key (the prior `vec:{ts}` key silently overwrote
        // same-ms siblings). The `:vec:` prefix is preserved, so `read_vec` /
        // `query_vec_similarity` prefix scans are unchanged.
        let seq = VEC_WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let key = format!("agent:{agent_id}:vec:{ts}:{seq:020}");
        let value = serde_json::to_vec(entry)
            .map_err(|e| TeriError::Memory(format!("Serialization error: {e}")))?;
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let write_txn =
                db.begin_write().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
            {
                let mut table = write_txn
                    .open_table(KV_TABLE)
                    .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
                table
                    .insert(key.as_str(), value.as_slice())
                    .map_err(|e| TeriError::Memory(format!("Write error: {e}")))?;
            }
            write_txn.commit().map_err(|e| TeriError::Memory(format!("Commit error: {e}")))
        })
        .await
        .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))?
    }

    /// Write many `VectorEntry`s under one namespace in a single transaction, using a
    /// monotonic `(timestamp, index)` key so entries written in a tight loop never collide
    /// on the millisecond timestamp.
    ///
    /// Workstream B: graph build embeds dozens/hundreds of entities at once; `write_vec`'s
    /// `vec:{ts}` key (millisecond resolution) would overwrite siblings sharing a millisecond.
    /// The key here is `agent:{namespace}:vec:{ts}:{index:010}` — still under the `:vec:` prefix
    /// so `query_vec_similarity`/`read_vec` scan it unchanged. Returns the number written.
    pub async fn write_vec_batch(&self, namespace: Uuid, entries: &[VectorEntry]) -> Result<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        // Serialize up-front so the blocking closure owns plain bytes.
        let mut records: Vec<(String, Vec<u8>)> = Vec::with_capacity(entries.len());
        for (i, entry) in entries.iter().enumerate() {
            let ts = entry.timestamp.timestamp_millis();
            let key = format!("agent:{namespace}:vec:{ts}:{i:010}");
            let value = serde_json::to_vec(entry)
                .map_err(|e| TeriError::Memory(format!("Serialization error: {e}")))?;
            records.push((key, value));
        }
        let db = self.db.clone();
        let count = records.len();
        tokio::task::spawn_blocking(move || {
            let write_txn =
                db.begin_write().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
            {
                let mut table = write_txn
                    .open_table(KV_TABLE)
                    .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
                for (key, value) in &records {
                    table
                        .insert(key.as_str(), value.as_slice())
                        .map_err(|e| TeriError::Memory(format!("Write error: {e}")))?;
                }
            }
            write_txn.commit().map_err(|e| TeriError::Memory(format!("Commit error: {e}")))
        })
        .await
        .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))??;
        Ok(count)
    }

    pub async fn read_vec(&self, agent_id: Uuid, limit: usize) -> Result<Vec<VectorEntry>> {
        let prefix = format!("agent:{agent_id}:vec:");
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let read_txn =
                db.begin_read().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
            let table = read_txn
                .open_table(KV_TABLE)
                .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
            let mut entries = Vec::new();
            let iter = table
                .range(prefix.as_str()..)
                .map_err(|e| TeriError::Memory(format!("Iterator error: {e}")))?;
            for item in iter {
                let (k, v) =
                    item.map_err(|e| TeriError::Memory(format!("Iterator item error: {e}")))?;
                if !k.value().starts_with(&prefix) {
                    break;
                }
                let entry: VectorEntry = serde_json::from_slice(v.value())
                    .map_err(|e| TeriError::Memory(format!("Deserialization error: {e}")))?;
                entries.push(entry);
                if entries.len() >= limit {
                    break;
                }
            }
            Ok(entries)
        })
        .await
        .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))?
    }

    /// Searches stored vector entries for the agent by cosine similarity against `query_embedding`,
    /// returning up to `top_k` results sorted by descending similarity score.
    ///
    /// # Dimension mismatch policy
    /// If a stored entry's embedding length differs from the query length, that entry is **skipped
    /// gracefully** (not an error). This mirrors Zep/MiroFish's tolerance for mixed-dimension
    /// stores. If ALL stored entries are dimension-mismatched (or if the store is empty), returns
    /// `Ok(vec![])`.
    ///
    /// # Zero-norm policy
    /// Stored entries or query vectors with L2-norm == 0.0 are skipped (avoiding div-by-zero).
    ///
    /// # Embedding generation
    /// This method takes a **precomputed** `query_embedding`. To search by raw text, use
    /// [`semantic_recall`](Self::semantic_recall), which embeds the text via the
    /// [`EmbeddingClient`](crate::embedding::EmbeddingClient) (OpenAI-compatible `/v1/embeddings`)
    /// and feeds the result here — closing GAP-OQ3-EMBED. This precomputed-vector entry point
    /// stays for callers that already hold a query vector.
    pub async fn query_vec_similarity(
        &self,
        agent_id: Uuid,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<VectorEntry>> {
        if top_k == 0 {
            return Ok(vec![]);
        }

        let query_len = query_embedding.len();

        // Compute query L2-norm; if zero, no comparison is meaningful.
        let query_norm: f32 = query_embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if query_norm == 0.0 {
            return Ok(vec![]);
        }

        // Clone the embedding for the blocking task (Vec<f32> is Send + 'static).
        let query_embedding: Vec<f32> = query_embedding.to_vec();

        let prefix = format!("agent:{agent_id}:vec:");
        let db = self.db.clone();

        let scored_result: Result<Vec<(f32, VectorEntry)>> =
            tokio::task::spawn_blocking(move || {
                let read_txn =
                    db.begin_read().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
                let table = read_txn
                    .open_table(KV_TABLE)
                    .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;

                let mut results: Vec<(f32, VectorEntry)> = Vec::new();

                let iter = table
                    .range(prefix.as_str()..)
                    .map_err(|e| TeriError::Memory(format!("Iterator error: {e}")))?;

                for item in iter {
                    let (k, v) =
                        item.map_err(|e| TeriError::Memory(format!("Iterator item error: {e}")))?;
                    if !k.value().starts_with(&prefix) {
                        break;
                    }
                    let entry: VectorEntry = serde_json::from_slice(v.value())
                        .map_err(|e| TeriError::Memory(format!("Deserialization error: {e}")))?;

                    // Skip dimension-mismatched entries.
                    if entry.embedding.len() != query_len {
                        continue;
                    }

                    // Compute entry L2-norm; skip zero-norm.
                    let entry_norm: f32 = entry.embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
                    if entry_norm == 0.0 {
                        continue;
                    }

                    // Cosine similarity = dot(q, e) / (|q| * |e|).
                    let dot: f32 = query_embedding
                        .iter()
                        .zip(entry.embedding.iter())
                        .map(|(a, b)| a * b)
                        .sum();
                    let similarity = dot / (query_norm * entry_norm);

                    results.push((similarity, entry));
                }

                Ok(results)
            })
            .await
            .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))?;

        let mut scored = scored_result?;

        // Sort descending by similarity score.
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Return at most top_k results, discarding scores.
        let results = scored.into_iter().take(top_k).map(|(_, entry)| entry).collect();
        Ok(results)
    }

    /// Delete every `VectorEntry` stored under a namespace (`agent:{namespace}:vec:*`).
    ///
    /// Workstream B (R4): rebuilding a graph for the same `graph_id` must clear that graph's
    /// vector namespace first, or stale vectors from a prior build pollute search. The namespace
    /// `Uuid` is the per-graph key (see `graph_backend::graph_namespace`). Returns the number of
    /// entries removed. A namespace with no vectors is a no-op (`Ok(0)`).
    pub async fn clear_vec_namespace(&self, namespace: Uuid) -> Result<usize> {
        let prefix = format!("agent:{namespace}:vec:");
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let write_txn =
                db.begin_write().map_err(|e| TeriError::Memory(format!("Tx error: {e}")))?;
            let removed = {
                let mut table = write_txn
                    .open_table(KV_TABLE)
                    .map_err(|e| TeriError::Memory(format!("Table error: {e}")))?;
                // `retain` keeps every entry for which the predicate returns true and drops the
                // rest. We keep entries that do NOT start with this namespace's prefix, counting
                // the dropped ones. This avoids a separate collect-keys-then-remove pass.
                let mut count = 0usize;
                table
                    .retain(|k, _v| {
                        let drop_it = k.starts_with(prefix.as_str());
                        if drop_it {
                            count += 1;
                        }
                        !drop_it
                    })
                    .map_err(|e| TeriError::Memory(format!("Retain error: {e}")))?;
                count
            };
            write_txn
                .commit()
                .map_err(|e| TeriError::Memory(format!("Commit error: {e}")))?;
            Ok(removed)
        })
        .await
        .map_err(|e| TeriError::Memory(format!("Task join error: {e}")))?
    }

    /// Embed `content` via the [`EmbeddingClient`] and persist it as a [`VectorEntry`].
    ///
    /// This is the **generation→storage** half of GAP-OQ3-EMBED: it joins the embedding
    /// backend (`src/embedding.rs`, text→vector over an OpenAI-compatible `/v1/embeddings`
    /// endpoint) to the vector store. Previously `write_vec` required a *precomputed*
    /// embedding and there was no in-repo path that produced one; this method closes that.
    ///
    /// The vector dimensionality is whatever the configured embedding model returns; the
    /// cosine search ([`query_vec_similarity`](Self::query_vec_similarity)) tolerates mixed
    /// dimensions by skipping mismatches, so callers do not have to pin a dimension here.
    pub async fn write_vec_text(
        &self,
        agent_id: Uuid,
        embedder: &crate::embedding::EmbeddingClient,
        content: &str,
        importance: f32,
    ) -> Result<()> {
        let embedding = embedder.embed(content).await?;
        let entry = VectorEntry {
            timestamp: chrono::Utc::now(),
            content: content.to_string(),
            embedding,
            importance,
        };
        self.write_vec(agent_id, &entry).await
    }

    /// Semantic recall: embed `query_text` via the [`EmbeddingClient`], then return the
    /// `top_k` stored entries most cosine-similar to it.
    ///
    /// This is the **generation→search** half of GAP-OQ3-EMBED — the live feed that
    /// `query_vec_similarity` was written to consume. It is the text-in/entries-out surface
    /// a feature (e.g. semantic agent-memory recall) calls; `query_vec_similarity` remains
    /// available for callers that already hold a precomputed query vector.
    pub async fn semantic_recall(
        &self,
        agent_id: Uuid,
        embedder: &crate::embedding::EmbeddingClient,
        query_text: &str,
        top_k: usize,
    ) -> Result<Vec<VectorEntry>> {
        let query_embedding = embedder.embed(query_text).await?;
        self.query_vec_similarity(agent_id, &query_embedding, top_k).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_memory_store_creation() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let _store = MemoryStore::new(&db_path).expect("Failed to create memory store");
    }

    #[tokio::test]
    async fn test_snapshot_persistence() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");
        let sim_id = Uuid::new_v4();
        let snapshot = WorldSnapshot {
            tick: 1,
            agents: std::collections::HashMap::new(),
            events: Vec::new(),
            variables: std::collections::HashMap::new(),
        };
        store.write_snapshot(sim_id, 1, &snapshot).await.expect("Write snapshot failed");
        let read = store.read_snapshot(sim_id, 1).await.expect("Read snapshot failed");
        assert_eq!(read.tick, snapshot.tick);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        use futures::future::join_all;
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");
        let agent_id = Uuid::new_v4();
        let base_time = chrono::Utc::now();
        let mut futures = Vec::new();
        for i in 0..10 {
            let store_clone = store.clone();
            let entry = MemoryEntry {
                timestamp: base_time + chrono::Duration::milliseconds(i * 10),
                content: format!("Entry {}", i),
                importance: i as f32 * 0.1,
            };
            futures.push(tokio::spawn(async move {
                store_clone.write_ltm(agent_id, &entry).await.unwrap();
            }));
        }
        join_all(futures).await;
        let entries = store.read_ltm(agent_id, 20).await.expect("Read failed");
        assert_eq!(entries.len(), 10);
    }

    #[tokio::test]
    async fn test_error_handling_invalid_path() {
        let invalid_path =
            if cfg!(windows) { "C:\\invalid_path\\?*" } else { "/root/invalid_path" };
        let result = MemoryStore::new(invalid_path);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_write_and_read_ltm() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");

        let agent_id = Uuid::new_v4();
        let entry = MemoryEntry {
            timestamp: chrono::Utc::now(),
            content: "Test memory".to_string(),
            importance: 0.8,
        };

        store.write_ltm(agent_id, &entry).await.expect("Failed to write LTM");

        let entries = store.read_ltm(agent_id, 10).await.expect("Failed to read LTM");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Test memory");
    }

    #[tokio::test]
    async fn test_write_ltm_same_millisecond_no_collision() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");
        let agent_id = Uuid::new_v4();

        // Five memories that all share the SAME timestamp (the same millisecond an
        // agent could emit them in). With the old bare `ltm:{ts}` key these collided
        // and only one survived; the monotonic sequence suffix keeps all five.
        let ts = chrono::Utc::now();
        for i in 0..5 {
            store
                .write_ltm(
                    agent_id,
                    &MemoryEntry { timestamp: ts, content: format!("memory {i}"), importance: 0.5 },
                )
                .await
                .expect("write ltm");
        }

        let entries = store.read_ltm(agent_id, 100).await.expect("read ltm");
        assert_eq!(entries.len(), 5, "all 5 same-millisecond LTM writes must persist");
    }

    #[tokio::test]
    async fn test_query_ltm_substring_search() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");

        let agent_id = Uuid::new_v4();
        let base_time = chrono::Utc::now();
        let entries = vec![
            MemoryEntry {
                timestamp: base_time,
                content: "Visited the market today".to_string(),
                importance: 0.7,
            },
            MemoryEntry {
                timestamp: base_time + chrono::Duration::milliseconds(100),
                content: "Met Alice at the library".to_string(),
                importance: 0.8,
            },
            MemoryEntry {
                timestamp: base_time + chrono::Duration::milliseconds(200),
                content: "Weather was sunny".to_string(),
                importance: 0.5,
            },
        ];

        for entry in &entries {
            store.write_ltm(agent_id, entry).await.expect("Failed to write LTM");
        }

        // Query for "market"
        let results = store.query_ltm(agent_id, "market").await.expect("Failed to query LTM");
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("market"));

        // Query for "library"
        let results = store.query_ltm(agent_id, "library").await.expect("Failed to query LTM");
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("library"));

        // Case-insensitive query
        let results = store.query_ltm(agent_id, "ALICE").await.expect("Failed to query LTM");
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Alice"));

        // Query with no matches
        let results = store.query_ltm(agent_id, "nonexistent").await.expect("Failed to query LTM");
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_write_and_read_snapshot() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");

        let sim_id = Uuid::new_v4();
        let snapshot = WorldSnapshot {
            tick: 5,
            agents: std::collections::HashMap::new(),
            events: Vec::new(),
            variables: std::collections::HashMap::new(),
        };

        store
            .write_snapshot(sim_id, 5, &snapshot)
            .await
            .expect("Failed to write snapshot");

        let read_snapshot = store.read_snapshot(sim_id, 5).await.expect("Failed to read snapshot");

        assert_eq!(read_snapshot.tick, snapshot.tick);
    }

    #[tokio::test]
    async fn test_read_history_with_limit() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");

        let sim_id = Uuid::new_v4();
        let snapshot_template = WorldSnapshot {
            tick: 0,
            agents: std::collections::HashMap::new(),
            events: Vec::new(),
            variables: std::collections::HashMap::new(),
        };

        // Write 5 snapshots
        for tick in 0..5 {
            let mut snapshot = snapshot_template.clone();
            snapshot.tick = tick;
            store
                .write_snapshot(sim_id, tick, &snapshot)
                .await
                .expect("Failed to write snapshot");
        }

        // Read all history
        let all = store.read_history(sim_id).await.expect("Failed to read history");
        assert_eq!(all.len(), 5);

        // Read with limit
        let limited = store
            .read_history_limit(sim_id, 2)
            .await
            .expect("Failed to read history with limit");
        assert_eq!(limited.len(), 2);
    }

    #[tokio::test]
    async fn test_read_missing_snapshot() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");

        let sim_id = Uuid::new_v4();
        let result = store.read_snapshot(sim_id, 99).await;

        assert!(result.is_err());
        match result {
            Err(TeriError::Memory(msg)) => assert!(msg.contains("not found")),
            _ => panic!("Expected Memory error with 'not found' message"),
        }
    }

    #[tokio::test]
    async fn test_write_and_read_vec() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");

        let agent_id = Uuid::new_v4();
        let entry = VectorEntry {
            timestamp: chrono::Utc::now(),
            content: "Test vector memory".to_string(),
            embedding: vec![0.1, 0.2, 0.3, 0.4, 0.5],
            importance: 0.9,
        };

        store.write_vec(agent_id, &entry).await.expect("Failed to write vector");

        let entries = store.read_vec(agent_id, 10).await.expect("Failed to read vectors");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "Test vector memory");
        assert_eq!(entries[0].embedding, vec![0.1, 0.2, 0.3, 0.4, 0.5]);
        assert_eq!(entries[0].importance, 0.9);
    }

    // ===== query_vec_similarity tests =====

    #[tokio::test]
    async fn test_query_vec_similarity_empty_store_returns_empty() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");

        let agent_id = Uuid::new_v4();
        let query_embedding = vec![0.1, 0.2, 0.3];
        let result = store
            .query_vec_similarity(agent_id, &query_embedding, 5)
            .await
            .expect("empty store must succeed, not error");
        assert!(result.is_empty(), "empty store must return empty vec");
    }

    #[tokio::test]
    async fn test_query_vec_similarity_ranking() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");
        let agent_id = Uuid::new_v4();
        let base_time = chrono::Utc::now();

        // Three vectors:
        //   A = [1, 0, 0] — orthogonal to query [0, 1, 0] → similarity = 0
        //   B = [0, 1, 0] — identical to query → similarity = 1.0 (closest)
        //   C = [0.6, 0.8, 0.0] — partial overlap → similarity = 0.8
        let entries = vec![
            VectorEntry {
                timestamp: base_time,
                content: "A: orthogonal".to_string(),
                embedding: vec![1.0, 0.0, 0.0],
                importance: 0.5,
            },
            VectorEntry {
                timestamp: base_time + chrono::Duration::milliseconds(1),
                content: "B: identical".to_string(),
                embedding: vec![0.0, 1.0, 0.0],
                importance: 0.5,
            },
            VectorEntry {
                timestamp: base_time + chrono::Duration::milliseconds(2),
                content: "C: partial".to_string(),
                embedding: vec![0.6, 0.8, 0.0],
                importance: 0.5,
            },
        ];

        for e in &entries {
            store.write_vec(agent_id, e).await.expect("write vec");
        }

        let query = vec![0.0, 1.0, 0.0];
        let results = store
            .query_vec_similarity(agent_id, &query, 3)
            .await
            .expect("query must succeed");

        assert_eq!(results.len(), 3);
        // B must rank first (similarity ≈ 1.0)
        assert!(
            results[0].content.contains("B"),
            "B must rank first; got: {}",
            results[0].content
        );
        // C must rank second (similarity = 0.8)
        assert!(
            results[1].content.contains("C"),
            "C must rank second; got: {}",
            results[1].content
        );
        // A must rank last (similarity = 0.0)
        assert!(
            results[2].content.contains("A"),
            "A must rank last; got: {}",
            results[2].content
        );
    }

    #[tokio::test]
    async fn test_query_vec_similarity_top_k_limiting() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");
        let agent_id = Uuid::new_v4();
        let base_time = chrono::Utc::now();

        // Store 5 vectors with non-zero embeddings and well-separated timestamps.
        // i+1 avoids [0.0, 0.0, 0.0] which would be skipped (zero norm).
        for i in 0i64..5 {
            let entry = VectorEntry {
                timestamp: base_time + chrono::Duration::milliseconds(i * 100),
                content: format!("entry {i}"),
                embedding: vec![(i + 1) as f32, 0.0, 0.0],
                importance: 0.5,
            };
            store.write_vec(agent_id, &entry).await.expect("write");
        }

        // Request only 2
        let query = vec![1.0, 0.0, 0.0];
        let results = store
            .query_vec_similarity(agent_id, &query, 2)
            .await
            .expect("query must succeed");
        assert_eq!(results.len(), 2, "must return at most top_k=2");
    }

    #[tokio::test]
    async fn test_query_vec_similarity_top_k_ge_available_returns_all() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");
        let agent_id = Uuid::new_v4();
        // Use a fixed base so timestamps are well-separated (100ms apart) to avoid key collision.
        let base_time =
            chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp");

        // Store 3 vectors with non-zero embeddings and well-separated timestamps (100ms apart).
        // i+1 avoids the [0.0, 0.0] zero-norm case that would be silently skipped.
        for i in 0u32..3 {
            let entry = VectorEntry {
                timestamp: base_time + chrono::Duration::milliseconds(i64::from(i) * 100),
                content: format!("entry {i}"),
                embedding: vec![(i + 1) as f32, 0.0],
                importance: 0.5,
            };
            store.write_vec(agent_id, &entry).await.expect("write");
        }

        // Request more than available
        let query = vec![1.0, 0.0];
        let results = store
            .query_vec_similarity(agent_id, &query, 100)
            .await
            .expect("query must succeed");
        assert_eq!(results.len(), 3, "when top_k >= available, must return all");
    }

    #[tokio::test]
    async fn test_query_vec_similarity_dimension_mismatch_skipped() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");
        let agent_id = Uuid::new_v4();
        let base_time = chrono::Utc::now();

        // Store a 3-dim entry
        let entry_3d = VectorEntry {
            timestamp: base_time,
            content: "3-dim".to_string(),
            embedding: vec![1.0, 0.0, 0.0],
            importance: 0.5,
        };
        store.write_vec(agent_id, &entry_3d).await.expect("write 3d");

        // Store a 2-dim entry (correct dimension)
        let entry_2d = VectorEntry {
            timestamp: base_time + chrono::Duration::milliseconds(1),
            content: "2-dim".to_string(),
            embedding: vec![1.0, 0.0],
            importance: 0.5,
        };
        store.write_vec(agent_id, &entry_2d).await.expect("write 2d");

        // Query with 2-dim → 3-dim entry is skipped, 2-dim entry is returned
        let query = vec![1.0, 0.0];
        let results = store
            .query_vec_similarity(agent_id, &query, 10)
            .await
            .expect("dimension mismatch must not error");
        assert_eq!(results.len(), 1, "3-dim entry must be skipped; only 2-dim returned");
        assert!(results[0].content.contains("2-dim"));
    }

    #[tokio::test]
    async fn test_query_vec_similarity_identical_vector_similarity_near_one() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");
        let agent_id = Uuid::new_v4();

        let vec = vec![0.6, 0.8]; // already unit-normed: |v|=1.0
        let entry = VectorEntry {
            timestamp: chrono::Utc::now(),
            content: "identical".to_string(),
            embedding: vec.clone(),
            importance: 1.0,
        };
        store.write_vec(agent_id, &entry).await.expect("write");

        let results =
            store.query_vec_similarity(agent_id, &vec, 1).await.expect("query must succeed");
        assert_eq!(results.len(), 1);
        // We can't directly read the score but the identical vector must rank first (and only).
        assert_eq!(results[0].content, "identical");
    }

    // ===== Workstream B: write_vec_batch + clear_vec_namespace =====

    #[tokio::test]
    async fn test_write_vec_batch_no_collision_same_millisecond() {
        let temp_dir = TempDir::new().expect("temp dir");
        let store = MemoryStore::new(temp_dir.path()).expect("store");
        let ns = Uuid::new_v4();
        // All entries share the SAME timestamp → would collide under write_vec's vec:{ts} key.
        let ts = chrono::Utc::now();
        let entries: Vec<VectorEntry> = (0..5)
            .map(|i| VectorEntry {
                timestamp: ts,
                content: format!("entry {i}"),
                embedding: vec![(i + 1) as f32, 0.0],
                importance: 0.5,
            })
            .collect();
        let n = store.write_vec_batch(ns, &entries).await.expect("batch write");
        assert_eq!(n, 5);
        let read = store.read_vec(ns, 100).await.expect("read");
        assert_eq!(read.len(), 5, "all 5 entries persisted despite identical timestamp");
    }

    #[tokio::test]
    async fn test_clear_vec_namespace_removes_only_that_namespace() {
        let temp_dir = TempDir::new().expect("temp dir");
        let store = MemoryStore::new(temp_dir.path()).expect("store");
        let ns_a = Uuid::new_v4();
        let ns_b = Uuid::new_v4();
        let mk = |c: &str| VectorEntry {
            timestamp: chrono::Utc::now(),
            content: c.to_string(),
            embedding: vec![1.0, 0.0],
            importance: 0.5,
        };
        store.write_vec_batch(ns_a, &[mk("a1"), mk("a2")]).await.expect("write a");
        store.write_vec_batch(ns_b, &[mk("b1")]).await.expect("write b");

        let removed = store.clear_vec_namespace(ns_a).await.expect("clear a");
        assert_eq!(removed, 2, "both ns_a vectors removed");
        assert!(store.read_vec(ns_a, 100).await.expect("read a").is_empty());
        assert_eq!(store.read_vec(ns_b, 100).await.expect("read b").len(), 1, "ns_b untouched");
    }

    #[tokio::test]
    async fn test_clear_vec_namespace_empty_is_noop() {
        let temp_dir = TempDir::new().expect("temp dir");
        let store = MemoryStore::new(temp_dir.path()).expect("store");
        let removed = store.clear_vec_namespace(Uuid::new_v4()).await.expect("clear empty");
        assert_eq!(removed, 0);
    }

    // ===== GAP-OQ3-EMBED end-to-end: EmbeddingClient -> write_vec_text/semantic_recall =====

    /// Full text→vector→cosine path through the REAL embedding client (mocked HTTP, never a
    /// fake embedder): store two texts via `write_vec_text`, then `semantic_recall` a query and
    /// assert the semantically-aligned entry ranks first. The mock serves distinct unit vectors
    /// keyed by the request body, so ranking is determined by the generated embeddings — proving
    /// the generation half is wired to the search half.
    #[tokio::test]
    async fn test_semantic_recall_end_to_end_via_embedding_client() {
        use crate::config::LlmConfig;
        use crate::embedding::EmbeddingClient;
        use httpmock::prelude::*;

        let server = MockServer::start();
        // "fruit-red-apple" -> e1 = [1,0]
        server.mock(|when, then| {
            when.method(POST).path("/embeddings").body_contains("fruit-red-apple");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"object":"list","data":[{"object":"embedding","embedding":[1.0,0.0],"index":0}]}"#,
            );
        });
        // "fast-red-car" -> e2 = [0,1]
        server.mock(|when, then| {
            when.method(POST).path("/embeddings").body_contains("fast-red-car");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"object":"list","data":[{"object":"embedding","embedding":[0.0,1.0],"index":0}]}"#,
            );
        });
        // query "apple-query" -> aligned with the apple entry = [1,0]
        server.mock(|when, then| {
            when.method(POST).path("/embeddings").body_contains("apple-query");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"object":"list","data":[{"object":"embedding","embedding":[1.0,0.0],"index":0}]}"#,
            );
        });

        let cfg = LlmConfig {
            base_url: server.base_url(),
            api_key: String::new(),
            model: "unused".to_string(),
            embed_model: "all-MiniLM-L6-v2".to_string(),
            timeout_secs: 5,
            max_retries: 0,
            max_tokens: 2048,
            provider: crate::config::LlmProvider::Openai,
        };
        let embedder = EmbeddingClient::new(&cfg);

        let temp_dir = TempDir::new().expect("temp dir");
        let store = MemoryStore::new(temp_dir.path()).expect("store");
        let agent_id = Uuid::new_v4();

        store
            .write_vec_text(agent_id, &embedder, "fruit-red-apple", 0.9)
            .await
            .expect("store apple via embedding");
        store
            .write_vec_text(agent_id, &embedder, "fast-red-car", 0.5)
            .await
            .expect("store car via embedding");

        let hits = store
            .semantic_recall(agent_id, &embedder, "apple-query", 2)
            .await
            .expect("semantic recall");

        assert_eq!(hits.len(), 2, "both stored entries returned");
        assert_eq!(hits[0].content, "fruit-red-apple", "apple must rank first for an apple query");
        assert_eq!(hits[1].content, "fast-red-car");
    }
}
