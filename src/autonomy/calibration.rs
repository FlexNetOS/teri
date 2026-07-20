//! The **LEARN layer** (L4) of teri's autonomy loop — per-community confidence calibration.
//!
//! ## Why this exists
//!
//! Everywhere teri attaches a `confidence` to a prediction it pushes back to the community platform
//! ([`crate::seed::community::TopicSignal`] / [`ContributorTrajectory`] /
//! [`SpaceHealthRisk`](crate::seed::community::SpaceHealthRisk)), and on the report
//! ([`crate::report::PredictionReport::confidence`]), that number is **synthesized metadata** — the
//! model's self-assessment, not a calibrated probability. This module turns observed *outcomes*
//! (which actioned predictions actually came true) into a per-domain multiplier and applies it so
//! confidence drifts toward calibrated for communities where outcomes have been recorded.
//!
//! ## The heuristic — MUST stay in lock-step with pebesen
//!
//! pebesen's receiver computes the canonical calibration knob inline in
//! `IntelligenceStore::calibration` (`pebesen/crates/intelligence/src/lib.rs`):
//!
//! ```text
//!   accuracy           = accurate / scored          (None when scored == 0)
//!   confidence_adjustment = match accuracy {
//!       Some(acc) => (0.5 + acc).clamp(0.5, 1.5),
//!       None      => 1.0,                            // no evidence → neutral
//!   }
//! ```
//!
//! teri's [`confidence_weight_from_counts`] replicates that arithmetic **byte-for-byte** so the two
//! halves of the loop never disagree about what a given accuracy record is worth. The shared test
//! `weight_matches_pebesen_for_the_same_counts` asserts equality against the same formula over a
//! grid of counts. (pebesen does not expose the math as a standalone `SpaceCalibration::from_counts`
//! constructor — it is inline in `calibration()` — so parity is by *replicated arithmetic + a
//! cross-checking test*, not by importing across the repo boundary, which teri deliberately avoids:
//! `seed::community` takes **no cargo dependency on pebesen**.)
//!
//! ## No-downgrade guarantee
//!
//! A domain with **no recorded outcomes** has accuracy `None` → weight `1.0` → [`apply`] is the
//! identity. So until the operator/pebesen feeds real accuracy in via [`record_outcome`], every
//! calibrated confidence is *byte-identical* to today's synthesized value. Calibration is purely
//! additive and opt-in.
//!
//! [`apply`]: CommunityCalibration::apply
//! [`record_outcome`]: CommunityCalibration::record_outcome
//! [`ContributorTrajectory`]: crate::seed::community::ContributorTrajectory

use crate::error::{Result, TeriError};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// redb table: `domain_id -> serde_json(`[`CommunityCalibration`]`)`. One row per domain.
const CALIBRATION_TABLE: TableDefinition<&str, &[u8]> =
    TableDefinition::new("autonomy_calibration");

/// Compute the confidence weight from raw outcome counts, **identical** to pebesen's
/// `IntelligenceStore::calibration` arithmetic (`pebesen/crates/intelligence/src/lib.rs`):
/// `accuracy = accurate / scored`; weight = `(0.5 + accuracy).clamp(0.5, 1.5)`, or neutral `1.0`
/// when nothing has been scored. Factored out so the formula is single-sourced (DRY) and the
/// pebesen-parity test can assert directly against it.
///
/// * `scored` — outcomes with a known accurate/inaccurate result.
/// * `accurate` — of those, how many were accurate (`accurate <= scored` is the caller's invariant;
///   if violated the accuracy simply exceeds 1.0 and the clamp still bounds the weight at 1.5).
pub fn confidence_weight_from_counts(scored: u64, accurate: u64) -> f64 {
    // accuracy: None when no evidence (scored == 0) — mirrors pebesen's `if scored > 0`.
    let accuracy = if scored > 0 { Some(accurate as f64 / scored as f64) } else { None };
    match accuracy {
        Some(acc) => (0.5 + acc).clamp(0.5, 1.5),
        None => 1.0,
    }
}

/// Per-domain running calibration stats: how predictions for one community have panned out.
///
/// Folded one outcome at a time via [`record_outcome`](CommunityCalibration::record_outcome); the
/// derived [`weight`](CommunityCalibration::weight) is the multiplier teri applies to that domain's
/// future confidence. Serializable so a [`CalibrationStore`] can persist it verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DomainCalibration {
    /// Actioned outcomes with a known accurate/inaccurate result.
    pub scored: u64,
    /// Of `scored`, how many were accurate.
    pub accurate: u64,
}

impl DomainCalibration {
    /// Fold one scored outcome into the stats.
    fn record(&mut self, accurate: bool) {
        self.scored += 1;
        if accurate {
            self.accurate += 1;
        }
    }

    /// Observed accuracy in `[0.0, 1.0]`, or `None` when nothing has been scored yet.
    pub fn accuracy(&self) -> Option<f64> {
        if self.scored > 0 { Some(self.accurate as f64 / self.scored as f64) } else { None }
    }

    /// The confidence multiplier for this domain (see [`confidence_weight_from_counts`]).
    pub fn weight(&self) -> f64 {
        confidence_weight_from_counts(self.scored, self.accurate)
    }
}

/// The whole loop's calibration state: `domain_id -> `[`DomainCalibration`]. The in-memory model the
/// orchestrator reads/writes; a [`CalibrationStore`] persists it. Unknown domains weight neutrally.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunityCalibration {
    /// Per-domain stats, keyed by `CommunityDomain::id`.
    pub domains: HashMap<String, DomainCalibration>,
}

impl CommunityCalibration {
    /// A fresh, empty calibration (every domain weights neutrally → [`apply`] is the identity).
    ///
    /// [`apply`]: CommunityCalibration::apply
    pub fn new() -> Self {
        Self::default()
    }

    /// **LEARN input.** Fold one actioned outcome for `domain_id` into the running stats. `accurate`
    /// is the operator's / pebesen's verdict on whether the prediction came true. This is the only
    /// way accuracy enters the loop — teri never fabricates it (see the connector note in the module
    /// docs / ledger).
    pub fn record_outcome(&mut self, domain_id: &str, accurate: bool) {
        self.domains.entry(domain_id.to_string()).or_default().record(accurate);
    }

    /// The confidence multiplier for `domain_id` — `1.0` (neutral) for a domain with no recorded
    /// outcomes, so an unknown domain never changes a prediction's confidence.
    pub fn weight(&self, domain_id: &str) -> f64 {
        self.domains.get(domain_id).map(DomainCalibration::weight).unwrap_or(1.0)
    }

    /// Observed accuracy for `domain_id`, or `None` when nothing has been scored.
    pub fn accuracy(&self, domain_id: &str) -> Option<f64> {
        self.domains.get(domain_id).and_then(DomainCalibration::accuracy)
    }

    /// Calibrate a raw (synthesized) confidence for `domain_id`:
    /// `calibrated = (raw * weight).clamp(0.0, 1.0)`.
    ///
    /// The clamp is load-bearing: a weight `> 1.0` (a consistently-accurate domain) must NOT push
    /// confidence above `1.0`, and a negative `raw` (shouldn't happen, but be defensive) is floored
    /// at `0.0`. With no recorded outcomes weight is `1.0`, so `apply` returns `raw` unchanged
    /// (modulo the no-op clamp) — the no-downgrade guarantee.
    pub fn apply(&self, domain_id: &str, raw_confidence: f64) -> f64 {
        (raw_confidence * self.weight(domain_id)).clamp(0.0, 1.0)
    }
}

/// Persistence seam for [`CommunityCalibration`]. Implementors round-trip the state faithfully so a
/// `save` then `load` (across a restart) restores the exact per-domain counts.
///
/// Mirrors the [`StateStore`](crate::autonomy::StateStore) idiom: an in-memory impl for tests and a
/// durable impl for production. The production impl is **redb**-backed
/// ([`RedbCalibrationStore`]) — the ledger specifies "persist in redb", redb is already teri's
/// persistence engine ([`crate::memory`]), and a single `domain_id -> stats` table is the natural
/// fit.
pub trait CalibrationStore: Send + Sync {
    /// Load the persisted calibration. A fresh store returns the default (empty) calibration.
    fn load(&self) -> Result<CommunityCalibration>;
    /// Persist the calibration, overwriting any prior checkpoint.
    fn save(&self, calibration: &CommunityCalibration) -> Result<()>;
}

/// In-memory calibration store for tests (and ephemeral runs). Holds the last saved state behind a
/// mutex so a `save`→`load` round-trip survives within a test without touching disk.
#[derive(Debug, Default)]
pub struct InMemoryCalibrationStore {
    inner: std::sync::Mutex<CommunityCalibration>,
}

impl InMemoryCalibrationStore {
    /// A fresh, empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl CalibrationStore for InMemoryCalibrationStore {
    fn load(&self) -> Result<CommunityCalibration> {
        Ok(self.inner.lock().expect("calibration mutex poisoned").clone())
    }

    fn save(&self, calibration: &CommunityCalibration) -> Result<()> {
        *self.inner.lock().expect("calibration mutex poisoned") = calibration.clone();
        Ok(())
    }
}

/// redb-backed calibration store: the production checkpoint. One row per domain in a single
/// [`CALIBRATION_TABLE`] (`domain_id -> serde_json(DomainCalibration)`), mirroring the redb idiom in
/// [`crate::memory::MemoryStore`]. Loading reconstructs the whole [`CommunityCalibration`] by
/// scanning the table; saving writes every domain row (a small per-domain map, so a full rewrite is
/// cheap and keeps the store + in-memory model trivially consistent).
pub struct RedbCalibrationStore {
    db: Arc<Database>,
}

impl RedbCalibrationStore {
    /// Open (or create) the calibration database at `path` (the file itself, e.g.
    /// `{data}/autonomy_calibration.redb`). The parent directory is created on demand; the table is
    /// initialized so the first `load` sees an empty-but-valid table.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                TeriError::Database(format!("calibration db dir {}: {e}", parent.display()))
            })?;
        }
        let db = Database::create(path)
            .map_err(|e| TeriError::Database(format!("open calibration redb: {e}")))?;
        // Initialize the table so an empty DB loads cleanly.
        let write_txn = db
            .begin_write()
            .map_err(|e| TeriError::Database(format!("calibration tx: {e}")))?;
        write_txn
            .open_table(CALIBRATION_TABLE)
            .map_err(|e| TeriError::Database(format!("calibration table: {e}")))?;
        write_txn
            .commit()
            .map_err(|e| TeriError::Database(format!("calibration commit: {e}")))?;
        Ok(Self { db: Arc::new(db) })
    }

    /// The default calibration DB path under a config's data root:
    /// `{upload_folder}/autonomy_calibration.redb`, co-located with the orchestrator's JSON state.
    pub fn for_config(config: &crate::config::Config) -> Result<Self> {
        Self::new(Path::new(&config.upload_folder).join("autonomy_calibration.redb"))
    }
}

impl CalibrationStore for RedbCalibrationStore {
    fn load(&self) -> Result<CommunityCalibration> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| TeriError::Database(format!("calibration read tx: {e}")))?;
        let table = read_txn
            .open_table(CALIBRATION_TABLE)
            .map_err(|e| TeriError::Database(format!("calibration table: {e}")))?;
        let mut domains = HashMap::new();
        let iter = table
            .iter()
            .map_err(|e| TeriError::Database(format!("calibration iter: {e}")))?;
        for item in iter {
            let (k, v) =
                item.map_err(|e| TeriError::Database(format!("calibration iter item: {e}")))?;
            let stats: DomainCalibration = serde_json::from_slice(v.value())
                .map_err(|e| TeriError::Serialization(format!("calibration parse: {e}")))?;
            domains.insert(k.value().to_string(), stats);
        }
        Ok(CommunityCalibration { domains })
    }

    fn save(&self, calibration: &CommunityCalibration) -> Result<()> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| TeriError::Database(format!("calibration write tx: {e}")))?;
        {
            let mut table = write_txn
                .open_table(CALIBRATION_TABLE)
                .map_err(|e| TeriError::Database(format!("calibration table: {e}")))?;
            // Drop rows that are no longer present, then upsert every current domain. The map is
            // small (one row per domain), so a full reconcile keeps disk == memory exactly.
            let stale: Vec<String> = {
                let mut keys = Vec::new();
                let iter = table
                    .iter()
                    .map_err(|e| TeriError::Database(format!("calibration iter: {e}")))?;
                for item in iter {
                    let (k, _) = item
                        .map_err(|e| TeriError::Database(format!("calibration iter item: {e}")))?;
                    let key = k.value().to_string();
                    if !calibration.domains.contains_key(&key) {
                        keys.push(key);
                    }
                }
                keys
            };
            for key in stale {
                table
                    .remove(key.as_str())
                    .map_err(|e| TeriError::Database(format!("calibration remove: {e}")))?;
            }
            for (domain_id, stats) in &calibration.domains {
                let bytes = serde_json::to_vec(stats)
                    .map_err(|e| TeriError::Serialization(format!("calibration serialize: {e}")))?;
                table
                    .insert(domain_id.as_str(), bytes.as_slice())
                    .map_err(|e| TeriError::Database(format!("calibration insert: {e}")))?;
            }
        }
        write_txn
            .commit()
            .map_err(|e| TeriError::Database(format!("calibration commit: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // (a) no evidence → weight 1.0; apply is the identity.
    #[test]
    fn no_evidence_is_neutral_and_apply_is_identity() {
        let cal = CommunityCalibration::new();
        assert_eq!(cal.weight("unknown"), 1.0);
        assert_eq!(cal.accuracy("unknown"), None);
        // apply returns raw unchanged for every value in range.
        for raw in [0.0, 0.25, 0.5, 0.8, 1.0] {
            assert_eq!(cal.apply("unknown", raw), raw, "neutral weight must be identity");
        }
    }

    // (b) all-accurate → weight 1.5 (ceiling); apply scales up but clamps at 1.0.
    #[test]
    fn all_accurate_hits_ceiling_and_clamps() {
        let mut cal = CommunityCalibration::new();
        for _ in 0..7 {
            cal.record_outcome("d1", true);
        }
        assert_eq!(cal.accuracy("d1"), Some(1.0));
        assert_eq!(cal.weight("d1"), 1.5, "accuracy 1.0 → 0.5+1.0 = 1.5 ceiling");
        // 0.5 * 1.5 = 0.75 (scaled up, under the clamp).
        assert!((cal.apply("d1", 0.5) - 0.75).abs() < 1e-12);
        // 0.8 * 1.5 = 1.2 → clamped to 1.0 (weight > 1 can't exceed 1.0).
        assert_eq!(cal.apply("d1", 0.8), 1.0, "calibrated confidence is clamped at 1.0");
    }

    // (c) all-wrong → weight 0.5 (floor).
    #[test]
    fn all_wrong_hits_floor() {
        let mut cal = CommunityCalibration::new();
        for _ in 0..4 {
            cal.record_outcome("d1", false);
        }
        assert_eq!(cal.accuracy("d1"), Some(0.0));
        assert_eq!(cal.weight("d1"), 0.5, "accuracy 0.0 → 0.5+0.0 = 0.5 floor");
        // 0.8 * 0.5 = 0.4 (scaled down).
        assert!((cal.apply("d1", 0.8) - 0.4).abs() < 1e-12);
    }

    // (d) mixed accuracy → weight equals (0.5 + accuracy).clamp.
    #[test]
    fn mixed_accuracy_matches_clamped_formula() {
        let mut cal = CommunityCalibration::new();
        // 3 accurate of 4 → accuracy 0.75 → weight 1.25.
        cal.record_outcome("d1", true);
        cal.record_outcome("d1", true);
        cal.record_outcome("d1", true);
        cal.record_outcome("d1", false);
        assert_eq!(cal.accuracy("d1"), Some(0.75));
        let expected = (0.5 + 0.75_f64).clamp(0.5, 1.5);
        assert_eq!(cal.weight("d1"), expected);
        assert_eq!(cal.weight("d1"), 1.25);
        // 1 accurate of 2 (chance) → accuracy 0.5 → weight 1.0 (unchanged).
        cal.record_outcome("d2", true);
        cal.record_outcome("d2", false);
        assert_eq!(cal.weight("d2"), 1.0, "chance accuracy leaves confidence unchanged");
        assert_eq!(cal.apply("d2", 0.6), 0.6);
    }

    // (f) the weight formula equals pebesen's `IntelligenceStore::calibration` arithmetic.
    //
    // pebesen does not export the math as a standalone constructor; it is inline as
    // `match accuracy { Some(acc) => (0.5 + acc).clamp(0.5, 1.5), None => 1.0 }` over
    // `accuracy = accurate / scored` (None when scored == 0). We replicate that EXACT expression
    // here and assert teri's `confidence_weight_from_counts` equals it over a grid of counts, so the
    // two halves of the loop provably agree. (Cross-ref: pebesen/crates/intelligence/src/lib.rs.)
    #[test]
    fn weight_matches_pebesen_for_the_same_counts() {
        fn pebesen_confidence_adjustment(scored: u64, accurate: u64) -> f64 {
            let accuracy = if scored > 0 { Some(accurate as f64 / scored as f64) } else { None };
            match accuracy {
                Some(acc) => (0.5 + acc).clamp(0.5, 1.5),
                None => 1.0,
            }
        }
        for scored in 0..=10u64 {
            for accurate in 0..=scored {
                assert_eq!(
                    confidence_weight_from_counts(scored, accurate),
                    pebesen_confidence_adjustment(scored, accurate),
                    "teri weight must equal pebesen's for scored={scored} accurate={accurate}"
                );
            }
        }
    }

    // (e) in-memory store persist → reload round-trip preserves stats.
    #[test]
    fn in_memory_store_round_trips() {
        let store = InMemoryCalibrationStore::new();
        assert_eq!(store.load().unwrap(), CommunityCalibration::default());
        let mut cal = CommunityCalibration::new();
        cal.record_outcome("d1", true);
        cal.record_outcome("d1", false);
        cal.record_outcome("d2", true);
        store.save(&cal).unwrap();
        assert_eq!(store.load().unwrap(), cal);
    }

    // (e) redb store persist → reload (fresh handle = restart) preserves stats.
    #[test]
    fn redb_store_round_trips_across_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("autonomy_calibration.redb");

        let mut cal = CommunityCalibration::new();
        cal.record_outcome("d1", true);
        cal.record_outcome("d1", true);
        cal.record_outcome("d1", false);
        cal.record_outcome("d2", false);
        {
            let store = RedbCalibrationStore::new(&path).unwrap();
            // Fresh DB loads empty.
            assert_eq!(store.load().unwrap(), CommunityCalibration::default());
            store.save(&cal).unwrap();
        }
        assert!(path.exists(), "redb checkpoint written to disk");
        // A FRESH store handle at the same path (simulating a restart) reloads the exact stats.
        let reopened = RedbCalibrationStore::new(&path).unwrap();
        let loaded = reopened.load().unwrap();
        assert_eq!(loaded, cal);
        assert_eq!(loaded.weight("d1"), confidence_weight_from_counts(3, 2));
        assert_eq!(loaded.weight("d2"), 0.5);
    }

    // redb store reconciles deletions (a domain dropped from memory is removed on save).
    #[test]
    fn redb_store_reconciles_removed_domains() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("autonomy_calibration.redb");
        let store = RedbCalibrationStore::new(&path).unwrap();

        let mut cal = CommunityCalibration::new();
        cal.record_outcome("keep", true);
        cal.record_outcome("drop", true);
        store.save(&cal).unwrap();
        assert_eq!(store.load().unwrap().domains.len(), 2);

        // Drop one domain and re-save; the reload must not resurrect it.
        cal.domains.remove("drop");
        store.save(&cal).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.domains.len(), 1);
        assert!(loaded.domains.contains_key("keep"));
        assert!(!loaded.domains.contains_key("drop"));
    }
}
