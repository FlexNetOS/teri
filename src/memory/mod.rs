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
        let key = format!("agent:{agent_id}:ltm:{ts}");
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
        let key = format!("agent:{agent_id}:vec:{ts}");
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

    pub async fn query_vec_similarity(
        &self,
        _agent_id: Uuid,
        _query_embedding: &[f32],
        _top_k: usize,
    ) -> Result<Vec<VectorEntry>> {
        Err(TeriError::Memory("vector similarity search not yet implemented".to_string()))
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

    #[tokio::test]
    async fn test_query_vec_similarity_returns_not_implemented() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.redb");
        let store = MemoryStore::new(&db_path).expect("Failed to create memory store");

        let agent_id = Uuid::new_v4();
        let query_embedding = vec![0.1, 0.2, 0.3];
        let result = store.query_vec_similarity(agent_id, &query_embedding, 5).await;

        assert!(result.is_err());
        match result {
            Err(TeriError::Memory(msg)) => {
                assert!(msg.contains("not yet implemented"));
            }
            _ => panic!("Expected Memory error with 'not yet implemented' message"),
        }
    }
}
