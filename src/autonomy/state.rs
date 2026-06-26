//! Durable orchestrator state + the [`StateStore`] persistence seam.
//!
//! Continuity (AGENTIC-STORY §4): the loop checkpoints its per-domain state so a restart resumes
//! mid-cycle — an unchanged signal after a restart must NOT re-trigger a run. The state is a map
//! `domain_id -> `[`DomainState`] persisted behind the [`StateStore`] trait: tests use the
//! in-memory [`InMemoryStateStore`]; production uses the JSON-file-backed [`JsonFileStateStore`]
//! (a small, human-inspectable checkpoint under the config data dir — redb is overkill for a
//! per-domain map this small, and a plain JSON file is trivially auditable).

use crate::autonomy::SignalFingerprint;
use crate::error::{Result, TeriError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// Per-domain persisted state: the last fingerprint we acted on, when we last ran, and a one-line
/// summary of the last outcome. The fingerprint is the debounce key (continuity); the rest is
/// audit-trail context the [`crate::autonomy::TickReport`] and any UI can surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainState {
    /// Fingerprint of the last signal that triggered (or was reconciled to) a run.
    pub last_fingerprint: SignalFingerprint,
    /// When the last run for this domain started (None until the first run).
    pub last_run_at: Option<DateTime<Utc>>,
    /// One-line summary of the last outcome (report id + summary head), for the audit trail.
    pub last_outcome_summary: Option<String>,
}

/// The whole orchestrator's durable state: `domain_id -> `[`DomainState`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorState {
    /// Per-domain checkpoint, keyed by `CommunityDomain::id`.
    pub domains: HashMap<String, DomainState>,
}

impl OrchestratorState {
    /// The fingerprint last acted on for `domain_id`, if any.
    pub fn last_fingerprint(&self, domain_id: &str) -> Option<&SignalFingerprint> {
        self.domains.get(domain_id).map(|d| &d.last_fingerprint)
    }

    /// Record that a run completed for `domain_id` at `at` with the given fingerprint + summary.
    pub fn record_run(
        &mut self,
        domain_id: &str,
        fingerprint: SignalFingerprint,
        at: DateTime<Utc>,
        outcome_summary: Option<String>,
    ) {
        self.domains.insert(
            domain_id.to_string(),
            DomainState {
                last_fingerprint: fingerprint,
                last_run_at: Some(at),
                last_outcome_summary: outcome_summary,
            },
        );
    }
}

/// Persistence seam for [`OrchestratorState`]. Implementors must round-trip the state faithfully
/// so a `save` then `load` (across a process restart) restores the exact debounce keys.
pub trait StateStore: Send + Sync {
    /// Load the persisted state. A fresh store (nothing saved yet) returns the default empty state.
    fn load(&self) -> Result<OrchestratorState>;
    /// Persist the state, overwriting any prior checkpoint.
    fn save(&self, state: &OrchestratorState) -> Result<()>;
}

/// In-memory state store for tests (and ephemeral runs). Holds the last saved state behind a mutex
/// so the same instance survives a `save`→`load` round-trip within a test, without touching disk.
#[derive(Debug, Default)]
pub struct InMemoryStateStore {
    inner: Mutex<OrchestratorState>,
}

impl InMemoryStateStore {
    /// A fresh, empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl StateStore for InMemoryStateStore {
    fn load(&self) -> Result<OrchestratorState> {
        Ok(self.inner.lock().expect("autonomy state mutex poisoned").clone())
    }

    fn save(&self, state: &OrchestratorState) -> Result<()> {
        *self.inner.lock().expect("autonomy state mutex poisoned") = state.clone();
        Ok(())
    }
}

/// JSON-file state store: the production checkpoint. Writes a single human-inspectable JSON file;
/// a missing file (first run) loads as the default empty state. Writes go through a temp file +
/// atomic rename so a crash mid-write never corrupts the checkpoint (fail-closed continuity).
pub struct JsonFileStateStore {
    path: PathBuf,
}

impl JsonFileStateStore {
    /// Construct a store backed by the file at `path` (created on first `save`).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The default checkpoint path under a config's data root: `{upload_folder}/autonomy_state.json`.
    /// Co-located with the staged seeds so all autonomy state lives under one root.
    pub fn for_config(config: &crate::config::Config) -> Self {
        Self::new(std::path::Path::new(&config.upload_folder).join("autonomy_state.json"))
    }
}

impl StateStore for JsonFileStateStore {
    fn load(&self) -> Result<OrchestratorState> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| TeriError::Serialization(format!("autonomy state parse: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(OrchestratorState::default()),
            Err(e) => Err(TeriError::Io(e)),
        }
    }

    fn save(&self, state: &OrchestratorState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                TeriError::Io(std::io::Error::other(format!("autonomy state dir: {e}")))
            })?;
        }
        let json = serde_json::to_vec_pretty(state)
            .map_err(|e| TeriError::Serialization(format!("autonomy state serialize: {e}")))?;
        // Atomic write: temp file beside the target, then rename.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json).map_err(TeriError::Io)?;
        std::fs::rename(&tmp, &self.path).map_err(TeriError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::community::{CommunitySignal, CommunityTopic};

    fn fp(active: u64) -> SignalFingerprint {
        SignalFingerprint::of(&CommunitySignal {
            domain_id: "d1".to_string(),
            domain_slug: "dom".to_string(),
            contributor_count: 1,
            topic_count: 2,
            active_topic_count: active,
            recent_topics: vec![CommunityTopic {
                id: "t1".to_string(),
                stream_id: "s1".to_string(),
                name: "t".to_string(),
                status: "open".to_string(),
                created_at: None,
                last_active: None,
            }],
            captured_at: Utc::now(),
        })
    }

    #[test]
    fn record_run_updates_fingerprint_and_metadata() {
        let mut st = OrchestratorState::default();
        assert!(st.last_fingerprint("d1").is_none());
        let now = Utc::now();
        st.record_run("d1", fp(1), now, Some("report rep_1".to_string()));
        assert_eq!(st.last_fingerprint("d1"), Some(&fp(1)));
        let d = st.domains.get("d1").unwrap();
        assert_eq!(d.last_run_at, Some(now));
        assert_eq!(d.last_outcome_summary.as_deref(), Some("report rep_1"));
    }

    #[test]
    fn in_memory_store_round_trips() {
        let store = InMemoryStateStore::new();
        assert_eq!(store.load().unwrap(), OrchestratorState::default());
        let mut st = OrchestratorState::default();
        st.record_run("d1", fp(1), Utc::now(), None);
        store.save(&st).unwrap();
        assert_eq!(store.load().unwrap(), st);
    }

    #[test]
    fn json_file_store_round_trips_and_missing_loads_default() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("autonomy_state.json");
        let store = JsonFileStateStore::new(&path);
        // Missing file → default empty state.
        assert_eq!(store.load().unwrap(), OrchestratorState::default());

        let mut st = OrchestratorState::default();
        st.record_run("d1", fp(2), Utc::now(), Some("s".to_string()));
        store.save(&st).unwrap();
        assert!(path.exists());

        // A FRESH store at the same path (simulating a process restart) reloads the exact state.
        let reopened = JsonFileStateStore::new(&path);
        assert_eq!(reopened.load().unwrap(), st);
    }
}
