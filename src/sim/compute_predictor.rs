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

// ===========================================================================================
// TheoryPredictor — Conic–Harmonic resonance memory (R. E. Grant, "Conic-Harmonic Theory of
// Everything"). A FAITHFUL, computable implementation of the theory's resonance-memory operator,
// applied to command→effect prediction so it can be scored head-to-head by `evaluate_predictor`.
//
// Mapping (theory → code), from consciousness_operator.md + math_primitives.md:
//   - Memory retrieval = "phase-space reactivation": a cue projects onto the nearest stable node.
//   - Similarity = the coherence inner product ⟨ρ₁|ρ₂⟩ = ∫ ρ₁ρ₂√|g| — a METRIC-WEIGHTED inner
//     product, not a flat cosine. The metric weight is the sech-power keyboard: for station k,
//     √|g|ₖ ~ sech²(θₖ) = 1/cosh²(arsinh(k/2)) = 1/(1 + k²/4).
//   - Tokens embed as nodal coherence densities ρ: a sech-shaped bump (sech is the theory's
//     suppressor s = sech(θ₆)) centered on the token's metallic-rapidity station, so similar
//     commands share overlapping harmonic components and RESONATE.
//   - Confidence = |λ|² (eigenmode-selection probability): the squared coherence amplitude.
// This directly tests the theory's falsifiable prediction P10 ("memory retrieval follows
// hyperbolic distance"): if the conic-metric kernel predicts command outcomes better than the
// flat-cosine RetrievalPredictor, P10 is corroborated in a new domain; if not, falsified here.
// ===========================================================================================

/// Number of metallic-rapidity registers (stations n = 1..=THEORY_DIM) in the harmonic embedding.
const THEORY_DIM: usize = 48;
/// Width (in registers) of a token's sech coherence bump.
const THEORY_SIGMA: f32 = 1.0;
/// Resonance floor below which the predictor defers to the rulebook (honesty guard).
const THEORY_THRESHOLD: f32 = 0.5;
/// Nearest neighbors that vote.
const THEORY_K: usize = 3;

/// arsinh(x) = ln(x + sqrt(x²+1)).
fn arsinh(x: f32) -> f32 {
    (x + (x * x + 1.0).sqrt()).ln()
}

/// The conic keyboard metric weight at station k (1-indexed): √|g|ₖ ~ sech²(θₖ), where
/// θₖ = arsinh(k/2) is the metallic rapidity (`math_primitives.md`). Lower stations carry more
/// weight — the sech-power suppression that makes the inner product *harmonic*, not Euclidean.
fn metric_weight(k: usize) -> f32 {
    let theta = arsinh(k as f32 / 2.0);
    let sech = 1.0 / theta.cosh();
    sech * sech
}

/// Embed a command as a nodal coherence density ρ over the metallic-rapidity registers: each
/// token deposits a sech bump centered on its station, so tokens on nearby stations overlap
/// (resonate). Deterministic (FNV hash for the station), no RNG.
fn harmonic_embed(command: &str) -> Vec<f32> {
    let mut rho = vec![0.0_f32; THEORY_DIM];
    for token in command.split_whitespace() {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in token.bytes() {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let center = (hash as usize) % THEORY_DIM;
        for (k, cell) in rho.iter_mut().enumerate() {
            // sech bump: sech(d) = 1/cosh(d); d = register distance / width. sech is the theory's
            // suppressor, so coherence falls off harmonically from the token's station.
            let d = (k as f32 - center as f32) / THEORY_SIGMA;
            *cell += 1.0 / d.cosh();
        }
    }
    rho
}

/// The coherence inner product ⟨ρ₁|ρ₂⟩ normalized to `[0,1]` (the resonance amplitude λ): a
/// metric-weighted cosine using the conic keyboard metric [`metric_weight`].
fn resonance(a: &[f32], b: &[f32]) -> f32 {
    let mut num = 0.0_f32;
    let mut na = 0.0_f32;
    let mut nb = 0.0_f32;
    for k in 0..a.len() {
        let g = metric_weight(k + 1);
        num += a[k] * b[k] * g;
        na += a[k] * a[k] * g;
        nb += b[k] * b[k] * g;
    }
    let den = (na.sqrt()) * (nb.sqrt());
    if den > 0.0 {
        (num / den).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// One resonance memory: a stored coherence pattern paired with the effect actually observed.
#[derive(Debug, Clone)]
struct ResonanceMemory {
    rho: Vec<f32>,
    effect: PredictedEffect,
}

/// The **Conic–Harmonic resonance-memory predictor**. Same retrieval shape as
/// [`RetrievalPredictor`], but the theory's operator throughout: a harmonic (sech / metallic-
/// rapidity) coherence embedding, the metric-weighted resonance inner product for similarity, and
/// `|λ|²` confidence. The scientific artifact under test against retrieval + the rulebook.
#[derive(Debug, Default)]
pub struct TheoryPredictor {
    memory: Vec<ResonanceMemory>,
}

impl TheoryPredictor {
    #[must_use]
    pub fn new() -> Self {
        Self { memory: Vec::new() }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.memory.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.memory.is_empty()
    }
}

impl EffectPredictor for TheoryPredictor {
    fn predict(&self, command: &str) -> Option<PredictedEffect> {
        if self.memory.is_empty() {
            return None;
        }
        let cue = harmonic_embed(command);
        let mut scored: Vec<(f32, &ResonanceMemory)> = self
            .memory
            .iter()
            .map(|m| (resonance(&cue, &m.rho), m))
            .filter(|(lam, _)| *lam >= THEORY_THRESHOLD)
            .collect();
        if scored.is_empty() {
            return None; // no stable node resonates with the cue → defer (honesty guard)
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(THEORY_K);

        let voters = scored.len() as f32;
        let successes = scored
            .iter()
            .filter(|(_, m)| matches!(m.effect.predicted_exit, ExitPrediction::Success))
            .count();
        let mean_lambda = scored.iter().map(|(l, _)| *l).sum::<f32>() / voters;

        let (_, nearest) = scored[0];
        let mut effect = nearest.effect.clone();
        effect.command = command.to_string();
        let majority_success = successes * 2 >= scored.len();
        effect.predicted_exit = if majority_success {
            ExitPrediction::Success
        } else {
            ExitPrediction::Failure {
                reason: "resonance: nearest stable node is a failure mode".to_string(),
            }
        };
        // |λ|² eigenmode-selection probability × vote agreement.
        let agreement = (successes.max(scored.len() - successes)) as f32 / voters;
        effect.confidence = (mean_lambda * mean_lambda * agreement).clamp(0.0, 1.0);
        effect.rationale = format!(
            "conic-harmonic resonance: reactivated {} node(s), {}/{} succeeded (λ={:.2}, |λ|²={:.2})",
            scored.len(),
            successes,
            scored.len(),
            mean_lambda,
            mean_lambda * mean_lambda
        );
        effect.provenance.model = self.model_tag().to_string();
        effect
            .provenance
            .evidence
            .push(format!("resonance_lambda={mean_lambda:.3} voters={}", scored.len()));
        Some(effect)
    }

    fn observe(&mut self, command: &str, actual: &ActualOutcome) {
        let mut effect = deduce_effect(command);
        effect.predicted_exit = if actual.succeeded {
            ExitPrediction::Success
        } else {
            ExitPrediction::Failure {
                reason: "observed failure".to_string(),
            }
        };
        self.memory.push(ResonanceMemory {
            rho: harmonic_embed(command),
            effect,
        });
    }

    fn model_tag(&self) -> &str {
        "conic-harmonic-resonance/v0"
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
    fn theory_predictor_defers_when_cold() {
        let t = TheoryPredictor::new();
        assert!(t.is_empty());
        assert!(t.predict("anything").is_none());
    }

    #[test]
    fn theory_predictor_resolves_an_unknown_via_resonance() {
        let mut t = TheoryPredictor::new();
        for _ in 0..3 {
            t.observe("orchestrate cluster", &outcome(true));
        }
        let p = t.predict("orchestrate cluster").expect("learned via resonance");
        assert_eq!(p.predicted_exit, ExitPrediction::Success);
        assert_eq!(p.provenance.model, "conic-harmonic-resonance/v0");
        assert!(p.rationale.contains("resonance"));
        assert!(p.confidence > 0.0);
    }

    #[test]
    fn head_to_head_theory_vs_retrieval_vs_rulebook() {
        // A family of custom (rulebook-BLIND) commands sharing a token, all succeeding. Train both
        // learned predictors on the first three; evaluate all three predictors on the whole family
        // (incl. two HELD-OUT members) to probe generalization. Print the real numbers — this is
        // the honest test of the theory's P10 (retrieval on the conic metric), not a rigged demo.
        let family = [
            "deploybot alpha",
            "deploybot beta",
            "deploybot gamma",
            "deploybot delta",
            "deploybot epsilon",
        ];
        let dataset: Vec<(String, ActualOutcome)> =
            family.iter().map(|c| (c.to_string(), outcome(true))).collect();

        let mut retrieval = RetrievalPredictor::new();
        let mut theory = TheoryPredictor::new();
        for c in &family[..3] {
            for _ in 0..2 {
                retrieval.observe(c, &outcome(true));
                theory.observe(c, &outcome(true));
            }
        }

        let s_rule = evaluate_predictor(&RulebookPredictor, &dataset);
        let s_retr = evaluate_predictor(&retrieval, &dataset);
        let s_theo = evaluate_predictor(&theory, &dataset);
        eprintln!("\n=== predictor head-to-head (n={}; accuracy ↑ better, Brier ↓ better) ===", s_rule.n);
        eprintln!("  rulebook  : acc={:.3}  brier={:.3}", s_rule.accuracy, s_rule.brier);
        eprintln!("  retrieval : acc={:.3}  brier={:.3}", s_retr.accuracy, s_retr.brier);
        eprintln!("  theory    : acc={:.3}  brier={:.3}  (conic-harmonic resonance)", s_theo.accuracy, s_theo.brier);

        // Both learned predictors must beat the blind rulebook; the theory-vs-retrieval margin is
        // reported, not asserted (the point is to MEASURE, honestly, which generalizes better).
        assert!(s_retr.brier < s_rule.brier, "retrieval must beat the blind rulebook");
        assert!(s_theo.brier < s_rule.brier, "theory must beat the blind rulebook");
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
