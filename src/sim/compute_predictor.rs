//! Pluggable **learned effect predictors** for `ComputeWorld` (world-type #2).
//!
//! The deterministic rulebook ([`super::compute_world::deduce_effect`]) is the always-available
//! base. This module adds the *learned* layer as a trait so every candidate model —
//! the rulebook, a retrieval kNN, a future `ruv-fann`/`conformal-prediction` net, a future
//! `sona` ReasoningBank, or **a theory drawn from a paper** — lives behind ONE interface and is
//! scored by ONE harness ([`evaluate_predictor`]). That is the scientifically honest way to
//! "test a theory": swap the [`EffectPredictor`], run the same evaluation, compare.
//!
//! ## Honesty guard (deferral)
//! [`EffectPredictor::predict`] returns `Option` — a learned model that has **no signal** for a
//! command returns `None` and defers to the next predictor (ultimately the rulebook), rather than
//! fabricating an answer. This mirrors the rulebook's own "unknown ⇒ low confidence, no fabricated
//! effect" doctrine at the model layer.
//!
//! ## Graduation path (all behind this trait, no rework)
//! - tier-1 (here): [`RetrievalPredictor`] — self-contained kNN over observed `(command→outcome)`.
//! - tier-2: `ruvnet/ruv-fann` (parametric NN) wrapped by `conformal-prediction`
//!   (`StreamingConformalPredictor` = calibrated confidence + online feedback), or in-repo `sona`
//!   ReasoningBank (trajectory replay + distillation into the per-program `calibration` weight).
//! - tier-3: `ruvllm` local inference for the residual long tail.

use super::compute_world::{
    deduce_effect, ActualOutcome, ExitPrediction, FsChange, PredictedEffect,
};

/// A pluggable predictor of a command's raw effect (before the twin's state-elimination and
/// per-program calibration, which `ComputeWorld` applies on top). `Debug + Send + Sync` so a
/// `ComputeWorld` holding a `Box<dyn EffectPredictor>` stays `Debug` and thread-safe.
pub trait EffectPredictor: std::fmt::Debug + Send + Sync {
    /// Predict the effect of `command`. Return `None` to **defer** (no signal) — the caller
    /// falls back to the next predictor, ultimately the deterministic rulebook.
    fn predict(&self, command: &str) -> Option<PredictedEffect>;

    /// Learn from a real observed outcome. Default: a no-op (a stateless predictor ignores feedback).
    fn observe(&mut self, _command: &str, _actual: &ActualOutcome) {}

    /// Short tag identifying this predictor — stamped into `EffectProvenance.model`.
    fn model_tag(&self) -> &str;
}

/// The deterministic rulebook as an [`EffectPredictor`]. Never defers — it is the base of the
/// stack, so `predict` always returns `Some` (the rulebook always has an answer, even if it is
/// an honest `Unknown` at low confidence).
#[derive(Debug, Default, Clone, Copy)]
pub struct RulebookPredictor;

impl EffectPredictor for RulebookPredictor {
    fn predict(&self, command: &str) -> Option<PredictedEffect> {
        Some(deduce_effect(command))
    }
    fn model_tag(&self) -> &str {
        "baseline-rulebook/v0"
    }
}

/// Dimensionality of the deterministic command embedding used for kNN retrieval.
const EMBED_DIM: usize = 64;
/// Cosine-similarity floor: below this the nearest neighbor is too dissimilar to trust, so the
/// retrieval predictor DEFERS to the rulebook (honesty guard).
const RETRIEVAL_THRESHOLD: f32 = 0.6;
/// How many nearest neighbors vote on the outcome.
const RETRIEVAL_K: usize = 3;

/// One learned memory: the embedding of a command paired with the effect actually observed.
#[derive(Debug, Clone)]
struct Memory {
    embedding: [f32; EMBED_DIM],
    effect: PredictedEffect,
}

/// A self-contained **retrieval predictor**: it remembers `(command → observed effect)` and
/// predicts a new command by a k-nearest-neighbor vote over a cheap, deterministic bag-of-tokens
/// embedding. It **learns** on [`observe`](EffectPredictor::observe) (each real outcome becomes a
/// memory) and **defers** when no stored command is similar enough — so it resolves the rulebook's
/// `Unknown`s once it has seen them, without ever fabricating for the truly unseen.
///
/// This is the in-crate realization of the `ruvector-temporal-coherence` kNN pattern (kept
/// self-contained so teri stays a clean peer repo — no cross-repo dependency).
#[derive(Debug, Default)]
pub struct RetrievalPredictor {
    memory: Vec<Memory>,
}

impl RetrievalPredictor {
    /// A fresh predictor with an empty memory.
    #[must_use]
    pub fn new() -> Self {
        Self { memory: Vec::new() }
    }

    /// Number of learned memories.
    #[must_use]
    pub fn len(&self) -> usize {
        self.memory.len()
    }

    /// Whether the predictor has learned nothing yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.memory.is_empty()
    }
}

/// Deterministic bag-of-tokens embedding: hash each whitespace token into a bucket and count,
/// then L2-normalize. Deterministic (no RNG) so predictions are reproducible.
fn embed(command: &str) -> [f32; EMBED_DIM] {
    let mut v = [0.0_f32; EMBED_DIM];
    for token in command.split_whitespace() {
        // FNV-1a over the token bytes → a stable bucket.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in token.bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        v[(hash as usize) % EMBED_DIM] += 1.0;
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// Cosine similarity of two L2-normalized embeddings (= their dot product).
fn cosine(a: &[f32; EMBED_DIM], b: &[f32; EMBED_DIM]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

impl EffectPredictor for RetrievalPredictor {
    fn predict(&self, command: &str) -> Option<PredictedEffect> {
        if self.memory.is_empty() {
            return None;
        }
        let query = embed(command);

        // Rank memories by similarity (descending); take the top-K above threshold.
        let mut scored: Vec<(f32, &Memory)> = self
            .memory
            .iter()
            .map(|m| (cosine(&query, &m.embedding), m))
            .filter(|(sim, _)| *sim >= RETRIEVAL_THRESHOLD)
            .collect();
        if scored.is_empty() {
            return None; // nothing similar enough → defer to the rulebook (honesty guard)
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(RETRIEVAL_K);

        // Majority vote on the exit outcome across the voters; confidence = mean voter similarity.
        let voters = scored.len() as f32;
        let successes = scored
            .iter()
            .filter(|(_, m)| matches!(m.effect.predicted_exit, ExitPrediction::Success))
            .count();
        let mean_sim = scored.iter().map(|(s, _)| *s).sum::<f32>() / voters;

        // Base the structural effect (fs changes, risk) on the nearest neighbor, but set the exit
        // by the vote so a mixed history honestly widens uncertainty.
        let (_, nearest) = scored[0];
        let mut effect = nearest.effect.clone();
        effect.command = command.to_string();
        let majority_success = successes * 2 >= scored.len();
        effect.predicted_exit = if majority_success {
            ExitPrediction::Success
        } else {
            ExitPrediction::Failure {
                reason: "learned: this command has failed in observed history".to_string(),
            }
        };
        // Confidence = neighbor agreement (how similar) × vote agreement (how unanimous).
        let agreement = (successes.max(scored.len() - successes)) as f32 / voters;
        effect.confidence = (mean_sim * agreement).clamp(0.0, 1.0);
        effect.rationale = format!(
            "retrieved {} similar observed command(s); {}/{} succeeded (mean similarity {:.2})",
            scored.len(),
            successes,
            scored.len(),
            mean_sim
        );
        effect.provenance.model = self.model_tag().to_string();
        effect
            .provenance
            .evidence
            .push(format!("knn_voters={} mean_sim={mean_sim:.3}", scored.len()));
        Some(effect)
    }

    fn observe(&mut self, command: &str, actual: &ActualOutcome) {
        // Store what REALLY happened: take the rulebook's structural deduction but overwrite the
        // exit with the observed truth, so future retrievals reflect reality (resolving Unknowns).
        let mut effect = deduce_effect(command);
        effect.predicted_exit = if actual.succeeded {
            ExitPrediction::Success
        } else {
            ExitPrediction::Failure {
                reason: "observed failure".to_string(),
            }
        };
        // If the rulebook could not predict fs changes (Unknown command), a bare observation still
        // teaches the exit — that alone resolves the rulebook's biggest blind spot.
        let _: &[FsChange] = &effect.fs_changes;
        self.memory.push(Memory {
            embedding: embed(command),
            effect,
        });
    }

    fn model_tag(&self) -> &str {
        "retrieval-knn/v0"
    }
}

/// A predictor's score over a labeled dataset — the "test the theory" verdict.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PredictorScore {
    /// Number of labeled commands scored.
    pub n: usize,
    /// Fraction whose predicted success/failure matched reality, in `[0, 1]`.
    pub accuracy: f32,
    /// Mean Brier score of the success probability, in `[0, 1]` — LOWER is better-calibrated.
    pub brier: f32,
}

/// Score a predictor against labeled `(command, ActualOutcome)` data. A `None` prediction falls
/// back to the rulebook (as the real stack does), so this scores the *effective* predictor.
///
/// This is the theory-testing rig: any [`EffectPredictor`] — retrieval, a future `ruv-fann` net,
/// or a `TheoryPredictor` built from a paper's equations — is measured on identical data, so their
/// accuracy and calibration are directly comparable.
#[must_use]
pub fn evaluate_predictor(
    predictor: &dyn EffectPredictor,
    dataset: &[(String, ActualOutcome)],
) -> PredictorScore {
    if dataset.is_empty() {
        return PredictorScore { n: 0, accuracy: 0.0, brier: 0.0 };
    }
    let mut correct = 0.0_f32;
    let mut brier_sum = 0.0_f32;
    for (command, actual) in dataset {
        let effect = predictor
            .predict(command)
            .unwrap_or_else(|| deduce_effect(command));
        // Map the prediction to a success probability in [0, 1].
        let p_success = match effect.predicted_exit {
            ExitPrediction::Success => effect.confidence,
            ExitPrediction::Failure { .. } => 1.0 - effect.confidence,
            ExitPrediction::Unknown => 0.5,
        };
        let y = if actual.succeeded { 1.0 } else { 0.0 };
        if (p_success >= 0.5) == actual.succeeded {
            correct += 1.0;
        }
        brier_sum += (p_success - y) * (p_success - y);
    }
    let n = dataset.len();
    PredictorScore {
        n,
        accuracy: correct / n as f32,
        brier: brier_sum / n as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(succeeded: bool) -> ActualOutcome {
        ActualOutcome { succeeded }
    }

    #[test]
    fn rulebook_predictor_never_defers() {
        let p = RulebookPredictor;
        assert!(p.predict("mkdir x").is_some());
        assert!(p.predict("frobnicate --quux").is_some(), "even an unknown gets an honest answer");
        assert_eq!(p.model_tag(), "baseline-rulebook/v0");
    }

    #[test]
    fn retrieval_defers_when_cold() {
        // With no memory, the learned predictor has NO signal → defers to the rulebook.
        let p = RetrievalPredictor::new();
        assert!(p.is_empty());
        assert!(p.predict("anything").is_none());
    }

    #[test]
    fn retrieval_resolves_an_unknown_after_observing_it() {
        // The rulebook returns Unknown for `frobnicate`. After observing it succeed a few times,
        // the retrieval predictor should predict Success for it — learning what the rulebook can't.
        assert_eq!(deduce_effect("frobnicate --x").predicted_exit, ExitPrediction::Unknown);

        let mut p = RetrievalPredictor::new();
        for _ in 0..3 {
            p.observe("frobnicate --x", &outcome(true));
        }
        let pred = p.predict("frobnicate --x").expect("learned a prediction");
        assert_eq!(pred.predicted_exit, ExitPrediction::Success);
        assert!(pred.confidence > 0.0);
        assert_eq!(pred.provenance.model, "retrieval-knn/v0");
        assert!(pred.rationale.contains("retrieved"));
    }

    #[test]
    fn retrieval_learns_a_failure() {
        let mut p = RetrievalPredictor::new();
        for _ in 0..3 {
            p.observe("deploy --prod", &outcome(false));
        }
        let pred = p.predict("deploy --prod").expect("learned");
        assert!(matches!(pred.predicted_exit, ExitPrediction::Failure { .. }));
    }

    #[test]
    fn retrieval_defers_for_a_dissimilar_command() {
        // Learned only about `cargo build`; asked about a totally different command → defer.
        let mut p = RetrievalPredictor::new();
        p.observe("cargo build --release", &outcome(true));
        assert!(
            p.predict("rm -rf /some/deep/unrelated/path").is_none(),
            "a dissimilar command has no learned signal → defer to the rulebook"
        );
    }

    #[test]
    fn eval_harness_scores_a_learned_predictor_above_the_cold_rulebook() {
        // Dataset: an unknown command that always succeeds. The rulebook only ever says Unknown
        // (p=0.5 → Brier 0.25); a retrieval predictor trained on it should score strictly better.
        let data: Vec<(String, ActualOutcome)> =
            (0..8).map(|_| ("frobnicate --run".to_string(), outcome(true))).collect();

        let cold = evaluate_predictor(&RulebookPredictor, &data);
        assert!((cold.brier - 0.25).abs() < 1e-6, "rulebook Unknown scores Brier 0.25");

        let mut learned = RetrievalPredictor::new();
        for _ in 0..4 {
            learned.observe("frobnicate --run", &outcome(true));
        }
        let warm = evaluate_predictor(&learned, &data);
        assert!(warm.accuracy >= cold.accuracy);
        assert!(warm.brier < cold.brier, "learning improves calibration: {} !< {}", warm.brier, cold.brier);
    }
}
