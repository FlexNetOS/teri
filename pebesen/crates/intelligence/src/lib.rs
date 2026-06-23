//! Pebesen Intelligence — prediction RECEIVER for the teri ↔ pebesen seam.
//!
//! teri (the swarm-intelligence prediction engine) pushes predictions about the
//! community back to pebesen over HTTP/JSON (see teri's
//! `src/seed/community/pebesen.rs`). This crate is the pebesen-side receiver: it
//! owns the payload types, stores ingested predictions, records when an operator
//! *acts* on a prediction and whether it turned out accurate, and computes a
//! simple per-space calibration metric that the prediction loop can feed back to
//! teri to tune confidence.
//!
//! ## Decoupling
//!
//! This crate does **not** depend on teri. The seam is a wire contract, not a
//! code dependency: teri serializes its `TopicSignal` / `ContributorTrajectory` /
//! `SpaceHealthRisk`, and these mirror-types deserialize them. Either side can
//! evolve independently as long as the JSON shape is preserved.
//!
//! ## Storage
//!
//! [`IntelligenceStore`] starts as an in-memory, thread-safe store (`RwLock` over
//! `HashMap`s) — correct and dependency-light for a first integration. The
//! `// SQLX SLOT:` comments mark exactly where a `sqlx`/Postgres-backed
//! implementation would slot in (each store method maps to one or two SQL
//! statements against `predictions` / `prediction_actions` tables). When that
//! lands, `IntelligenceStore` becomes a trait with an in-memory and a Postgres
//! impl; the public API functions below stay unchanged.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Payload types — mirror teri's pushed predictions (no teri dependency)
// ─────────────────────────────────────────────────────────────────────────────

/// The kind of prediction teri pushed. Each variant mirrors one of teri's
/// feedback payloads. Tagged so a heterogeneous prediction stream round-trips
/// through JSON unambiguously.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PredictionKind {
    /// Per-topic momentum (mirrors teri `TopicSignal`).
    TopicSignal(TopicSignal),
    /// Per-contributor trajectory (mirrors teri `ContributorTrajectory`).
    ContributorTrajectory(ContributorTrajectory),
    /// Per-space health risk (mirrors teri `SpaceHealthRisk`).
    SpaceHealthRisk(SpaceHealthRisk),
}

/// Mirror of teri's `TopicSignal`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopicSignal {
    pub topic_id: String,
    pub domain_id: String,
    /// Predicted momentum, `[-1.0, 1.0]`.
    pub momentum: f64,
    /// teri's confidence, `[0.0, 1.0]`.
    pub confidence: f64,
    pub rationale: String,
}

/// Mirror of teri's `ContributorTrajectory`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContributorTrajectory {
    pub contributor_id: String,
    pub domain_id: String,
    pub trajectory: String,
    pub engagement_score: f64,
    pub confidence: f64,
}

/// Mirror of teri's `SpaceHealthRisk`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceHealthRisk {
    pub domain_id: String,
    pub risk: String,
    pub severity: f64,
    pub confidence: f64,
    pub detail: String,
}

impl PredictionKind {
    /// The space/domain this prediction is scoped to — the calibration key.
    pub fn domain_id(&self) -> &str {
        match self {
            PredictionKind::TopicSignal(s) => &s.domain_id,
            PredictionKind::ContributorTrajectory(t) => &t.domain_id,
            PredictionKind::SpaceHealthRisk(r) => &r.domain_id,
        }
    }

    /// The optional topic this prediction is scoped to (only topic signals are).
    pub fn topic_id(&self) -> Option<&str> {
        match self {
            PredictionKind::TopicSignal(s) => Some(&s.topic_id),
            _ => None,
        }
    }

    /// teri's confidence for this prediction.
    pub fn confidence(&self) -> f64 {
        match self {
            PredictionKind::TopicSignal(s) => s.confidence,
            PredictionKind::ContributorTrajectory(t) => t.confidence,
            PredictionKind::SpaceHealthRisk(r) => r.confidence,
        }
    }
}

/// A stored prediction: an ingested [`PredictionKind`] plus receiver-assigned
/// identity and ingest time. `prediction_id` is what an operator later references
/// when reporting an actioned outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prediction {
    pub prediction_id: Uuid,
    pub ingested_at: DateTime<Utc>,
    #[serde(flatten)]
    pub payload: PredictionKind,
}

/// An actioned event: the operator acted on a prediction and (optionally) knows
/// whether it was accurate. `accurate: None` means "acted, outcome not yet known";
/// `Some(true)`/`Some(false)` feed the calibration metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PredictionAction {
    pub prediction_id: Uuid,
    pub actioned_at: DateTime<Utc>,
    pub accurate: Option<bool>,
}

/// Per-space calibration metric: how teri's predictions for a space are panning
/// out, so the loop can nudge confidence. Computed over actioned predictions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceCalibration {
    pub domain_id: String,
    /// Predictions ingested for this space.
    pub total_predictions: usize,
    /// Predictions that have been actioned.
    pub actioned: usize,
    /// Actioned predictions with a known accurate/inaccurate outcome.
    pub scored: usize,
    /// Of `scored`, how many were accurate.
    pub accurate: usize,
    /// `accurate / scored` in `[0.0, 1.0]`; `None` when nothing scored yet.
    pub accuracy: Option<f64>,
    /// Suggested multiplier teri should apply to future confidence for this space,
    /// derived from observed accuracy. `1.0` when there is no evidence yet.
    ///
    /// Heuristic: `0.5 + accuracy` clamped to `[0.5, 1.5]` — accuracy of 0.5
    /// (chance) leaves confidence unchanged; consistently accurate predictions
    /// scale confidence up, consistently wrong ones scale it down. This is the
    /// minimal honest calibration knob, NOT a calibrated probability model.
    pub confidence_adjustment: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum IntelligenceError {
    /// An actioned event referenced a prediction id the store has never seen.
    #[error("unknown prediction id: {0}")]
    UnknownPrediction(Uuid),
    /// The internal lock was poisoned (a holder panicked). Surfaced rather than
    /// re-panicking so callers (e.g. axum handlers) can map it to a 500.
    #[error("intelligence store lock poisoned")]
    LockPoisoned,
}

type Result<T> = std::result::Result<T, IntelligenceError>;

// ─────────────────────────────────────────────────────────────────────────────
// Store
// ─────────────────────────────────────────────────────────────────────────────

/// Thread-safe, in-memory store of received predictions and their actioned
/// outcomes. Cloneable handle (cheap `Arc` clone) suitable for axum app state.
///
/// SQLX SLOT: replace the two `RwLock<HashMap<…>>` fields with a `sqlx::PgPool`
/// and back each method with SQL against `predictions(prediction_id, domain_id,
/// topic_id, kind, payload jsonb, confidence, ingested_at)` and
/// `prediction_actions(prediction_id, actioned_at, accurate)`.
#[derive(Debug, Clone, Default)]
pub struct IntelligenceStore {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    predictions: RwLock<HashMap<Uuid, Prediction>>,
    actions: RwLock<HashMap<Uuid, PredictionAction>>,
}

impl IntelligenceStore {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingest a single prediction, assigning it an id + ingest time. Returns the
    /// stored [`Prediction`] (with its `prediction_id`) so the caller can echo the
    /// id back to teri / an operator.
    ///
    /// SQLX SLOT: `INSERT INTO predictions (...) VALUES (...) RETURNING prediction_id`.
    pub fn ingest(&self, payload: PredictionKind) -> Result<Prediction> {
        let prediction = Prediction {
            prediction_id: Uuid::new_v4(),
            ingested_at: Utc::now(),
            payload,
        };
        self.inner
            .predictions
            .write()
            .map_err(|_| IntelligenceError::LockPoisoned)?
            .insert(prediction.prediction_id, prediction.clone());
        Ok(prediction)
    }

    /// Ingest a batch of predictions. Convenience over [`ingest`](Self::ingest)
    /// matching teri's batched `push_*` calls.
    pub fn ingest_batch(
        &self,
        payloads: impl IntoIterator<Item = PredictionKind>,
    ) -> Result<Vec<Prediction>> {
        payloads.into_iter().map(|p| self.ingest(p)).collect()
    }

    /// All predictions scoped to a space, newest first.
    ///
    /// SQLX SLOT: `SELECT ... FROM predictions WHERE domain_id = $1 ORDER BY ingested_at DESC`.
    pub fn list_by_space(&self, domain_id: &str) -> Result<Vec<Prediction>> {
        let preds = self
            .inner
            .predictions
            .read()
            .map_err(|_| IntelligenceError::LockPoisoned)?;
        let mut out: Vec<Prediction> = preds
            .values()
            .filter(|p| p.payload.domain_id() == domain_id)
            .cloned()
            .collect();
        out.sort_by(|a, b| b.ingested_at.cmp(&a.ingested_at));
        Ok(out)
    }

    /// All topic-signal predictions for a given topic, newest first.
    ///
    /// SQLX SLOT: `SELECT ... FROM predictions WHERE topic_id = $1 ORDER BY ingested_at DESC`.
    pub fn list_by_topic(&self, topic_id: &str) -> Result<Vec<Prediction>> {
        let preds = self
            .inner
            .predictions
            .read()
            .map_err(|_| IntelligenceError::LockPoisoned)?;
        let mut out: Vec<Prediction> = preds
            .values()
            .filter(|p| p.payload.topic_id() == Some(topic_id))
            .cloned()
            .collect();
        out.sort_by(|a, b| b.ingested_at.cmp(&a.ingested_at));
        Ok(out)
    }

    /// Record that an operator acted on a prediction, with an optional accuracy
    /// outcome. Errors if the prediction id is unknown (fail-closed: we never
    /// record outcomes for predictions we never received).
    ///
    /// SQLX SLOT: `INSERT INTO prediction_actions (...) ON CONFLICT (prediction_id)
    /// DO UPDATE SET actioned_at = EXCLUDED.actioned_at, accurate = EXCLUDED.accurate`
    /// guarded by an existence check on `predictions`.
    pub fn record_action(
        &self,
        prediction_id: Uuid,
        accurate: Option<bool>,
    ) -> Result<PredictionAction> {
        // Fail-closed: the prediction must exist.
        {
            let preds = self
                .inner
                .predictions
                .read()
                .map_err(|_| IntelligenceError::LockPoisoned)?;
            if !preds.contains_key(&prediction_id) {
                return Err(IntelligenceError::UnknownPrediction(prediction_id));
            }
        }
        let action = PredictionAction {
            prediction_id,
            actioned_at: Utc::now(),
            accurate,
        };
        self.inner
            .actions
            .write()
            .map_err(|_| IntelligenceError::LockPoisoned)?
            .insert(prediction_id, action.clone());
        Ok(action)
    }

    /// Compute the [`SpaceCalibration`] for a space from its actioned predictions.
    ///
    /// SQLX SLOT: a single aggregate query joining `predictions` to
    /// `prediction_actions` filtered by `domain_id`, counting totals / actioned /
    /// scored / accurate. The arithmetic below is identical.
    pub fn calibration(&self, domain_id: &str) -> Result<SpaceCalibration> {
        let preds = self
            .inner
            .predictions
            .read()
            .map_err(|_| IntelligenceError::LockPoisoned)?;
        let actions = self
            .inner
            .actions
            .read()
            .map_err(|_| IntelligenceError::LockPoisoned)?;

        let space_preds: Vec<&Prediction> = preds
            .values()
            .filter(|p| p.payload.domain_id() == domain_id)
            .collect();

        let total_predictions = space_preds.len();
        let mut actioned = 0usize;
        let mut scored = 0usize;
        let mut accurate = 0usize;

        for p in &space_preds {
            if let Some(a) = actions.get(&p.prediction_id) {
                actioned += 1;
                if let Some(acc) = a.accurate {
                    scored += 1;
                    if acc {
                        accurate += 1;
                    }
                }
            }
        }

        let accuracy = if scored > 0 {
            Some(accurate as f64 / scored as f64)
        } else {
            None
        };

        // Confidence adjustment: 0.5 + accuracy, clamped to [0.5, 1.5]. No
        // evidence (no scored outcomes) → neutral 1.0.
        let confidence_adjustment = match accuracy {
            Some(acc) => (0.5 + acc).clamp(0.5, 1.5),
            None => 1.0,
        };

        Ok(SpaceCalibration {
            domain_id: domain_id.to_string(),
            total_predictions,
            actioned,
            scored,
            accurate,
            accuracy,
            confidence_adjustment,
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API — the functions pebesen's `api` crate would call
// ─────────────────────────────────────────────────────────────────────────────

/// Receive a batch of teri topic-signal predictions.
///
/// The pebesen `api` crate would call this from a `POST /api/intelligence/topic-signals`
/// handler with `State(IntelligenceStore)` and the deserialized body.
pub fn receive_topic_signals(
    store: &IntelligenceStore,
    signals: Vec<TopicSignal>,
) -> Result<Vec<Prediction>> {
    store.ingest_batch(signals.into_iter().map(PredictionKind::TopicSignal))
}

/// Receive a batch of teri contributor-trajectory predictions.
pub fn receive_contributor_trajectories(
    store: &IntelligenceStore,
    trajectories: Vec<ContributorTrajectory>,
) -> Result<Vec<Prediction>> {
    store.ingest_batch(
        trajectories
            .into_iter()
            .map(PredictionKind::ContributorTrajectory),
    )
}

/// Receive a batch of teri space-health-risk predictions.
pub fn receive_space_health_risks(
    store: &IntelligenceStore,
    risks: Vec<SpaceHealthRisk>,
) -> Result<Vec<Prediction>> {
    store.ingest_batch(risks.into_iter().map(PredictionKind::SpaceHealthRisk))
}

/// Report that an operator actioned a prediction (with optional accuracy).
/// Backs a `POST /api/intelligence/predictions/{id}/action` handler.
pub fn report_actioned(
    store: &IntelligenceStore,
    prediction_id: Uuid,
    accurate: Option<bool>,
) -> Result<PredictionAction> {
    store.record_action(prediction_id, accurate)
}

/// Read the calibration metric teri's loop should feed back into confidence.
/// Backs a `GET /api/intelligence/spaces/{domain_id}/calibration` handler.
pub fn space_calibration(store: &IntelligenceStore, domain_id: &str) -> Result<SpaceCalibration> {
    store.calibration(domain_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn topic_signal(domain: &str, topic: &str, confidence: f64) -> TopicSignal {
        TopicSignal {
            topic_id: topic.to_string(),
            domain_id: domain.to_string(),
            momentum: 0.3,
            confidence,
            rationale: "rising".to_string(),
        }
    }

    #[test]
    fn ingest_and_list_by_space_and_topic() {
        let store = IntelligenceStore::new();
        let preds = receive_topic_signals(
            &store,
            vec![
                topic_signal("space-1", "t1", 0.8),
                topic_signal("space-1", "t2", 0.6),
                topic_signal("space-2", "t3", 0.5),
            ],
        )
        .unwrap();
        assert_eq!(preds.len(), 3);

        assert_eq!(store.list_by_space("space-1").unwrap().len(), 2);
        assert_eq!(store.list_by_space("space-2").unwrap().len(), 1);
        assert_eq!(store.list_by_topic("t1").unwrap().len(), 1);
        assert_eq!(store.list_by_topic("nope").unwrap().len(), 0);
    }

    #[test]
    fn mixed_prediction_kinds_scope_correctly() {
        let store = IntelligenceStore::new();
        receive_topic_signals(&store, vec![topic_signal("space-1", "t1", 0.7)]).unwrap();
        receive_contributor_trajectories(
            &store,
            vec![ContributorTrajectory {
                contributor_id: "u1".to_string(),
                domain_id: "space-1".to_string(),
                trajectory: "rising".to_string(),
                engagement_score: 0.9,
                confidence: 0.7,
            }],
        )
        .unwrap();
        receive_space_health_risks(
            &store,
            vec![SpaceHealthRisk {
                domain_id: "space-1".to_string(),
                risk: "activity_decline".to_string(),
                severity: 0.4,
                confidence: 0.5,
                detail: "slowing".to_string(),
            }],
        )
        .unwrap();

        // All three are scoped to space-1.
        assert_eq!(store.list_by_space("space-1").unwrap().len(), 3);
        // Only the topic signal has a topic id.
        assert_eq!(store.list_by_topic("t1").unwrap().len(), 1);
    }

    #[test]
    fn record_action_unknown_prediction_errors() {
        let store = IntelligenceStore::new();
        let err = store.record_action(Uuid::new_v4(), Some(true)).unwrap_err();
        assert!(matches!(err, IntelligenceError::UnknownPrediction(_)));
    }

    #[test]
    fn actioned_events_drive_calibration() {
        let store = IntelligenceStore::new();
        let preds = receive_topic_signals(
            &store,
            vec![
                topic_signal("space-1", "t1", 0.8),
                topic_signal("space-1", "t2", 0.6),
                topic_signal("space-1", "t3", 0.5),
                topic_signal("space-1", "t4", 0.5),
            ],
        )
        .unwrap();

        // Action all four: two accurate, one inaccurate, one actioned-but-unknown.
        report_actioned(&store, preds[0].prediction_id, Some(true)).unwrap();
        report_actioned(&store, preds[1].prediction_id, Some(true)).unwrap();
        report_actioned(&store, preds[2].prediction_id, Some(false)).unwrap();
        report_actioned(&store, preds[3].prediction_id, None).unwrap();

        let cal = space_calibration(&store, "space-1").unwrap();
        assert_eq!(cal.total_predictions, 4);
        assert_eq!(cal.actioned, 4);
        assert_eq!(cal.scored, 3); // the None one is not scored
        assert_eq!(cal.accurate, 2);
        assert_eq!(cal.accuracy, Some(2.0 / 3.0));
        // 0.5 + 0.666… = 1.166…, within clamp.
        assert!((cal.confidence_adjustment - (0.5 + 2.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn calibration_neutral_with_no_evidence() {
        let store = IntelligenceStore::new();
        receive_topic_signals(&store, vec![topic_signal("space-1", "t1", 0.8)]).unwrap();
        // No actions recorded.
        let cal = space_calibration(&store, "space-1").unwrap();
        assert_eq!(cal.total_predictions, 1);
        assert_eq!(cal.actioned, 0);
        assert_eq!(cal.scored, 0);
        assert_eq!(cal.accuracy, None);
        assert_eq!(cal.confidence_adjustment, 1.0);
    }

    #[test]
    fn calibration_clamps_extremes() {
        let store = IntelligenceStore::new();
        // All accurate → accuracy 1.0 → 0.5 + 1.0 = 1.5 (upper clamp).
        let preds =
            receive_topic_signals(&store, vec![topic_signal("space-up", "t1", 0.5)]).unwrap();
        report_actioned(&store, preds[0].prediction_id, Some(true)).unwrap();
        assert_eq!(
            space_calibration(&store, "space-up")
                .unwrap()
                .confidence_adjustment,
            1.5
        );

        // All inaccurate → accuracy 0.0 → 0.5 + 0.0 = 0.5 (lower clamp).
        let preds2 =
            receive_topic_signals(&store, vec![topic_signal("space-down", "t1", 0.5)]).unwrap();
        report_actioned(&store, preds2[0].prediction_id, Some(false)).unwrap();
        assert_eq!(
            space_calibration(&store, "space-down")
                .unwrap()
                .confidence_adjustment,
            0.5
        );
    }

    #[test]
    fn prediction_roundtrips_through_json() {
        let store = IntelligenceStore::new();
        let pred = store
            .ingest(PredictionKind::TopicSignal(topic_signal(
                "space-1", "t1", 0.8,
            )))
            .unwrap();
        let json = serde_json::to_string(&pred).unwrap();
        let back: Prediction = serde_json::from_str(&json).unwrap();
        assert_eq!(pred, back);
        // The flattened tagged payload survives.
        assert!(json.contains("\"kind\":\"topic_signal\""));
    }

    #[test]
    fn confidence_accessor_reads_each_variant() {
        let ts = PredictionKind::TopicSignal(topic_signal("d", "t", 0.42));
        assert_eq!(ts.confidence(), 0.42);
        let ct = PredictionKind::ContributorTrajectory(ContributorTrajectory {
            contributor_id: "u".into(),
            domain_id: "d".into(),
            trajectory: "steady".into(),
            engagement_score: 0.5,
            confidence: 0.33,
        });
        assert_eq!(ct.confidence(), 0.33);
    }
}
