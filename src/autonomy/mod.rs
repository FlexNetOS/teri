//! Autonomy orchestrator — the **DECIDE layer** of teri's autonomy loop (L2/L5).
//!
//! This is the supervising loop that sits *above* `pipeline.rs` and makes teri self-driving.
//! Per `docs/AGENTIC-STORY.md` §"What must be built for L2–L5": "a supervising loop … watches
//! adapters, debounces signal deltas into jobs, schedules runs, enforces a compute budget, writes
//! results to the backlog. The orchestrator calls the engine as a library." It never reaches inside
//! the five stages — it drives `CommunityAdapter` (SENSE) and `pipeline::run_pipeline` (PREDICT,
//! via the [`PredictionJobRunner`] seam) only.
//!
//! ## One supervised cycle ([`Orchestrator::tick`])
//!
//! ```text
//!   SENSE      fetch_domains → fetch_signal per domain   (CommunityAdapter)
//!     │
//!   FINGERPRINT + DEBOUNCE
//!     │        skip domains whose SignalFingerprint == last-seen (no meaningful delta)
//!     │
//!   DECIDE     for each CHANGED domain: to_seed_document(domain) + policy.query_for(...)
//!     │        → a Job { domain, seed_document, query }
//!     │
//!   BUDGET     run up to Budget.max_runs_per_tick jobs; DEFER the rest (logged, never silent)
//!     │
//!   PREDICT    run the (budgeted) jobs via the PredictionJobRunner, ≤ Budget.max_concurrent
//!     │        at once; one domain's error never aborts the others
//!     │
//!   PERSIST    update + save OrchestratorState (continuity: a restart won't re-run an unchanged
//!     │        signal)
//!     ▼
//!   TickReport (sensed / changed / ran / deferred / outcomes / errors) — the witnessed result
//! ```
//!
//! Every decision emits a structured `tracing` event — the **witnessed audit trail** required by
//! AGENTIC-STORY ("every autonomous action is witnessed").
//!
//! ## Design for testability
//!
//! The orchestrator is generic over a [`PredictionJobRunner`] and holds a `dyn StateStore` + a
//! `dyn CommunityAdapter` + a `dyn DecidePolicy`, so tests inject a fake runner (records jobs,
//! returns canned outcomes — NO LLM, NO network) and an in-memory state store. The unit of test is
//! [`Orchestrator::tick`]; a thin [`Orchestrator::run_forever`] loops it on an interval.
//!
//! ## What's left for S13 (TASK-AUTO-2 — calibration)
//!
//! This slice builds SENSE→DECIDE→PREDICT + budget/continuity/audit. The L3 ACT push
//! (`CommunityFeedback`) and the L4 calibration loop are deliberately out of scope; see the
//! `// SEAM(S13):` markers below for exactly where actioned-outcome feedback and per-community
//! confidence calibration hook in.

mod fingerprint;
mod policy;
mod runner;
mod state;

pub use fingerprint::SignalFingerprint;
pub use policy::{DEFAULT_HORIZON_DAYS, DecidePolicy, DefaultDecidePolicy};
pub use runner::{Job, JobOutcome, PipelineJobRunner, PredictionJobRunner, build_job};
pub use state::{
    DomainState, InMemoryStateStore, JsonFileStateStore, OrchestratorState, StateStore,
};

use crate::error::Result;
use crate::seed::community::CommunityAdapter;
use chrono::Utc;
use futures::stream::StreamExt;
use std::sync::Arc;

/// Compute budget for a single [`Orchestrator::tick`]. The orchestrator MUST NEVER exceed it: when
/// more domains changed than `max_runs_per_tick`, it runs up to the cap and reports the rest as
/// *deferred* (never silently truncated — owner value / AGENTIC-STORY guardrail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Maximum prediction runs started in one tick. Excess changed domains are deferred.
    pub max_runs_per_tick: usize,
    /// Maximum prediction runs in flight at once (concurrency cap; ≥ 1).
    pub max_concurrent: usize,
}

impl Default for Budget {
    fn default() -> Self {
        // Conservative defaults: a few runs per tick, modest concurrency.
        Self { max_runs_per_tick: 4, max_concurrent: 2 }
    }
}

impl Budget {
    /// Normalize to safe values: `max_concurrent` is clamped to `[1, max_runs_per_tick]` so the
    /// concurrency cap can never be zero (deadlock) nor exceed the per-tick cap (pointless).
    fn normalized(self) -> Self {
        let max_runs_per_tick = self.max_runs_per_tick;
        let max_concurrent = self.max_concurrent.clamp(1, max_runs_per_tick.max(1));
        Self { max_runs_per_tick, max_concurrent }
    }
}

/// A domain whose signal changed but whose job was NOT run this tick because the budget was
/// exhausted — surfaced so deferral is observable, never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredDomain {
    /// The domain id that was deferred.
    pub domain_id: String,
    /// Why it was deferred (today always budget; an enum-free string keeps the report flat).
    pub reason: String,
}

/// A per-domain error captured during a tick — recorded, not propagated, so one domain's failure
/// (SENSE, seed-build, or PREDICT) never aborts the others (per-domain error isolation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainError {
    /// The domain id (or `"<fetch_domains>"` when the SENSE enumeration itself failed).
    pub domain_id: String,
    /// The stage the error occurred in (`sense` / `decide` / `predict`).
    pub stage: String,
    /// The error message.
    pub message: String,
}

/// The witnessed result of one [`Orchestrator::tick`] — the audit record of what the DECIDE layer
/// decided and did this cycle.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickReport {
    /// Domain ids successfully sensed this tick.
    pub sensed: Vec<String>,
    /// Domain ids whose fingerprint changed since last-seen (candidates for a run).
    pub changed: Vec<String>,
    /// Outcomes of the jobs actually run this tick.
    pub ran: Vec<JobOutcome>,
    /// Changed domains deferred because the budget was exhausted.
    pub deferred: Vec<DeferredDomain>,
    /// Per-domain errors (isolated; the tick still completed for the other domains).
    pub errors: Vec<DomainError>,
}

impl TickReport {
    /// `true` if the tick completed with no per-domain errors.
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }
}

/// The DECIDE-layer orchestrator. Generic over the [`PredictionJobRunner`] so production drives the
/// real pipeline and tests inject a fake. Holds the SENSE adapter, the DECIDE policy, the durable
/// state store, the budget, and the runner.
pub struct Orchestrator<R: PredictionJobRunner> {
    adapter: Arc<dyn CommunityAdapter>,
    policy: Arc<dyn DecidePolicy>,
    store: Arc<dyn StateStore>,
    runner: Arc<R>,
    budget: Budget,
}

impl<R: PredictionJobRunner + 'static> Orchestrator<R> {
    /// Construct an orchestrator. `budget` is normalized (see [`Budget::normalized`]).
    pub fn new(
        adapter: Arc<dyn CommunityAdapter>,
        policy: Arc<dyn DecidePolicy>,
        store: Arc<dyn StateStore>,
        runner: Arc<R>,
        budget: Budget,
    ) -> Self {
        Self { adapter, policy, store, runner, budget: budget.normalized() }
    }

    /// Convenience constructor with the [`DefaultDecidePolicy`].
    pub fn with_default_policy(
        adapter: Arc<dyn CommunityAdapter>,
        store: Arc<dyn StateStore>,
        runner: Arc<R>,
        budget: Budget,
    ) -> Self {
        Self::new(adapter, Arc::new(DefaultDecidePolicy::default()), store, runner, budget)
    }

    /// Run one supervised cycle. See the module docs for the SENSE→DECIDE→PREDICT→PERSIST shape.
    ///
    /// Errors are isolated per domain into the [`TickReport`]; the only way `tick` returns `Err` is
    /// if the durable state cannot be loaded or saved (a continuity failure that must not be
    /// silently swallowed). Everything else is recorded and the tick proceeds.
    pub async fn tick(&self) -> Result<TickReport> {
        let mut report = TickReport::default();
        let mut state = self.store.load()?;

        // ── SENSE: enumerate domains ─────────────────────────────────────────
        let domains = match self.adapter.fetch_domains().await {
            Ok(d) => d,
            Err(e) => {
                tracing::error!(error = %e, "autonomy.sense: fetch_domains failed; tick is a no-op");
                report.errors.push(DomainError {
                    domain_id: "<fetch_domains>".to_string(),
                    stage: "sense".to_string(),
                    message: e.to_string(),
                });
                return Ok(report);
            }
        };
        tracing::info!(domains = domains.len(), "autonomy.sense: enumerated domains");

        // ── SENSE per domain → FINGERPRINT → DEBOUNCE → DECIDE (build jobs) ───
        // Collect the jobs for CHANGED domains, paired with their new fingerprint (so PERSIST can
        // record exactly what was acted on). Per-domain errors are isolated into the report.
        let mut pending: Vec<(Job, SignalFingerprint)> = Vec::new();
        for domain in &domains {
            let signal = match self.adapter.fetch_signal(domain).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(domain_id = %domain.id, error = %e, "autonomy.sense: fetch_signal failed; skipping domain");
                    report.errors.push(DomainError {
                        domain_id: domain.id.clone(),
                        stage: "sense".to_string(),
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            report.sensed.push(domain.id.clone());

            let fingerprint = SignalFingerprint::of(&signal);
            // DEBOUNCE: unchanged fingerprint since last acted-on → skip (no run).
            if state.last_fingerprint(&domain.id) == Some(&fingerprint) {
                tracing::debug!(domain_id = %domain.id, fingerprint = fingerprint.as_str(), "autonomy.debounce: signal unchanged; skipping");
                continue;
            }
            tracing::info!(domain_id = %domain.id, fingerprint = fingerprint.as_str(), "autonomy.decide: signal changed; building job");
            report.changed.push(domain.id.clone());

            // DECIDE: seed (from the adapter) + query (from the policy) → a Job.
            let seed_document = match self.adapter.to_seed_document(domain).await {
                Ok(doc) => doc,
                Err(e) => {
                    tracing::warn!(domain_id = %domain.id, error = %e, "autonomy.decide: to_seed_document failed; skipping domain");
                    report.errors.push(DomainError {
                        domain_id: domain.id.clone(),
                        stage: "decide".to_string(),
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            let job = build_job(domain, &signal, seed_document, self.policy.as_ref());
            pending.push((job, fingerprint));
        }

        // ── BUDGET: split changed jobs into run-now vs deferred (never silent) ─
        let budget = self.budget; // already normalized in the ctor
        let (to_run, deferred) = if pending.len() > budget.max_runs_per_tick {
            let split = pending.split_off(budget.max_runs_per_tick);
            (pending, split)
        } else {
            (pending, Vec::new())
        };
        for (job, _) in &deferred {
            tracing::warn!(
                domain_id = job.domain_id(),
                max_runs_per_tick = budget.max_runs_per_tick,
                "autonomy.budget: deferring changed domain (per-tick budget exhausted)"
            );
            report.deferred.push(DeferredDomain {
                domain_id: job.domain_id().to_string(),
                reason: format!("budget: max_runs_per_tick={}", budget.max_runs_per_tick),
            });
        }

        // ── PREDICT: run the budgeted jobs, ≤ max_concurrent at a time ────────
        // buffer_unordered bounds in-flight runs to the concurrency cap. Each future returns a
        // Result so a single run's failure is isolated (recorded below) rather than aborting the
        // batch. We pair the result back with the fingerprint to PERSIST exactly what ran.
        let runner = Arc::clone(&self.runner);
        let results: Vec<(String, SignalFingerprint, Result<JobOutcome>)> =
            futures::stream::iter(to_run.into_iter().map(|(job, fp)| {
                let runner = Arc::clone(&runner);
                async move {
                    let domain_id = job.domain_id().to_string();
                    let outcome = runner.run(&job).await;
                    (domain_id, fp, outcome)
                }
            }))
            .buffer_unordered(budget.max_concurrent)
            .collect()
            .await;

        // ── PERSIST: record successful runs in the durable state; isolate failures ─
        let now = Utc::now();
        for (domain_id, fingerprint, outcome) in results {
            match outcome {
                Ok(o) => {
                    let summary = autonomy_outcome_summary(&o);
                    tracing::info!(
                        domain_id = %domain_id,
                        report_id = %o.report_id,
                        nodes = o.graph_node_count,
                        edges = o.graph_edge_count,
                        "autonomy.predict: run completed; recording in state"
                    );
                    // Continuity: advance the debounce key ONLY for domains whose run succeeded, so
                    // a failed run is retried next tick (its fingerprint is not advanced).
                    state.record_run(&domain_id, fingerprint, now, Some(summary));
                    // SEAM(S13 — ACT/LEARN): this is where the L3 CommunityFeedback push of
                    // report-derived TopicSignals/SpaceHealthRisks would fire, and where the L4
                    // calibration loop would later fold actioned/accurate outcomes into a
                    // per-community confidence weight. Out of scope for TASK-AUTO-1.
                    report.ran.push(o);
                }
                Err(e) => {
                    tracing::error!(domain_id = %domain_id, error = %e, "autonomy.predict: run failed; NOT advancing fingerprint (will retry)");
                    report.errors.push(DomainError {
                        domain_id,
                        stage: "predict".to_string(),
                        message: e.to_string(),
                    });
                }
            }
        }

        // ── PERSIST: checkpoint the state (continuity / resume) ───────────────
        self.store.save(&state)?;
        tracing::info!(
            sensed = report.sensed.len(),
            changed = report.changed.len(),
            ran = report.ran.len(),
            deferred = report.deferred.len(),
            errors = report.errors.len(),
            "autonomy.tick: cycle complete"
        );

        Ok(report)
    }

    /// Loop [`Orchestrator::tick`] on a fixed interval, forever. The unit of behavior/test is
    /// `tick`; this is a thin convenience for an unattended deployment. A tick that returns `Err`
    /// (a continuity failure) is logged and the loop continues (it will retry next interval); the
    /// loop only ends if cancelled by the caller dropping the future.
    pub async fn run_forever(&self, interval: std::time::Duration) -> ! {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            if let Err(e) = self.tick().await {
                tracing::error!(error = %e, "autonomy.run_forever: tick failed (continuity error); continuing");
            }
        }
    }
}

/// One-line audit summary of an outcome for the persisted state: report id + the head of the
/// summary. Factored out so the format is single-sourced (DRY).
fn autonomy_outcome_summary(o: &JobOutcome) -> String {
    let head = o
        .report_summary
        .as_deref()
        .map(|s| {
            let trimmed = s.trim();
            if trimmed.chars().count() > 120 {
                let cut: String = trimmed.chars().take(120).collect();
                format!("{cut}…")
            } else {
                trimmed.to_string()
            }
        })
        .unwrap_or_default();
    if head.is_empty() {
        format!("report {}", o.report_id)
    } else {
        format!("report {} — {head}", o.report_id)
    }
}

// NOTE (CLI wiring, intentionally out of scope for S12): a `teri autonomy` subcommand would attach
// in `src/main.rs` beside `run`/`serve` — build the provider LLM-backed `PipelineJobRunner` +
// `JsonFileStateStore::for_config(&config)` + a concrete `CommunityAdapter`
// (`seed::community::pebesen::PebesenAdapter`), then call `tick()` once (cron mode) or
// `run_forever(interval)` (daemon mode). It would preflight the backend exactly like `run`/`serve`
// (the PipelineJobRunner inherits the guard via `run_pipeline`). Kept a pure library module here.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TeriError;
    use crate::seed::SeedDocument;
    use crate::seed::community::{
        CommunityContributor, CommunityDomain, CommunitySignal, CommunityTopic,
        signal_to_seed_document,
    };
    use async_trait::async_trait;
    use std::sync::Mutex as StdMutex;

    // ── Fake SENSE adapter ───────────────────────────────────────────────────
    // Returns a scripted set of domains and, per domain, the signal currently set for it. Tests
    // mutate the per-domain signal between ticks to simulate a community changing (or not).
    #[derive(Default)]
    struct FakeAdapter {
        domains: Vec<CommunityDomain>,
        signals: StdMutex<std::collections::HashMap<String, CommunitySignal>>,
        // domain ids whose fetch_signal should fail (per-domain SENSE error isolation test).
        fail_signal_for: StdMutex<std::collections::HashSet<String>>,
    }

    impl FakeAdapter {
        fn new(domains: Vec<CommunityDomain>) -> Self {
            Self { domains, ..Default::default() }
        }
        fn set_signal(&self, signal: CommunitySignal) {
            self.signals.lock().unwrap().insert(signal.domain_id.clone(), signal);
        }
        fn fail_signal(&self, domain_id: &str) {
            self.fail_signal_for.lock().unwrap().insert(domain_id.to_string());
        }
    }

    #[async_trait]
    impl CommunityAdapter for FakeAdapter {
        async fn fetch_domains(&self) -> Result<Vec<CommunityDomain>> {
            Ok(self.domains.clone())
        }
        async fn fetch_contributors(
            &self,
            _domain: &CommunityDomain,
        ) -> Result<Vec<CommunityContributor>> {
            Ok(vec![])
        }
        async fn fetch_signal(&self, domain: &CommunityDomain) -> Result<CommunitySignal> {
            if self.fail_signal_for.lock().unwrap().contains(&domain.id) {
                return Err(TeriError::Seed(format!("scripted SENSE failure for {}", domain.id)));
            }
            self.signals
                .lock()
                .unwrap()
                .get(&domain.id)
                .cloned()
                .ok_or_else(|| TeriError::Seed(format!("no scripted signal for {}", domain.id)))
        }
        async fn fetch_topics(&self, _domain: &CommunityDomain) -> Result<Vec<CommunityTopic>> {
            Ok(vec![])
        }
        // Use the canonical seed renderer so jobs carry a real community-derived seed.
        async fn to_seed_document(&self, domain: &CommunityDomain) -> Result<SeedDocument> {
            let signal = self.fetch_signal(domain).await?;
            Ok(signal_to_seed_document(domain, &signal, &[]))
        }
    }

    // ── Fake PREDICT runner ──────────────────────────────────────────────────
    // Records every job it received (so tests assert WHAT was run) and returns a canned outcome.
    // Optionally fails for a configured domain id (PREDICT error-isolation test). NO LLM.
    #[derive(Default)]
    struct FakeRunner {
        received: StdMutex<Vec<Job>>,
        fail_for: StdMutex<std::collections::HashSet<String>>,
    }
    impl FakeRunner {
        fn fail(&self, domain_id: &str) {
            self.fail_for.lock().unwrap().insert(domain_id.to_string());
        }
        fn received(&self) -> Vec<Job> {
            self.received.lock().unwrap().clone()
        }
    }
    #[async_trait(?Send)]
    impl PredictionJobRunner for FakeRunner {
        async fn run(&self, job: &Job) -> Result<JobOutcome> {
            self.received.lock().unwrap().push(job.clone());
            if self.fail_for.lock().unwrap().contains(job.domain_id()) {
                return Err(TeriError::Sim(format!(
                    "scripted PREDICT failure for {}",
                    job.domain_id()
                )));
            }
            Ok(JobOutcome {
                domain_id: job.domain.id.clone(),
                report_id: format!("rep_{}", job.domain.id),
                report_summary: Some(format!("forecast for {}", job.domain.name)),
                graph_node_count: 3,
                graph_edge_count: 2,
            })
        }
    }

    fn domain(id: &str) -> CommunityDomain {
        CommunityDomain {
            id: id.to_string(),
            slug: format!("slug-{id}"),
            name: format!("Domain {id}"),
            description: None,
            visibility: "public".to_string(),
            member_count: 5,
        }
    }

    fn signal(domain_id: &str, active: u64) -> CommunitySignal {
        CommunitySignal {
            domain_id: domain_id.to_string(),
            domain_slug: format!("slug-{domain_id}"),
            contributor_count: 3,
            topic_count: 6,
            active_topic_count: active,
            recent_topics: vec![CommunityTopic {
                id: "t1".to_string(),
                stream_id: "s1".to_string(),
                name: "topic".to_string(),
                status: "open".to_string(),
                created_at: None,
                last_active: Some(Utc::now()),
            }],
            captured_at: Utc::now(),
        }
    }

    fn orchestrator(
        adapter: Arc<FakeAdapter>,
        runner: Arc<FakeRunner>,
        store: Arc<dyn StateStore>,
        budget: Budget,
    ) -> Orchestrator<FakeRunner> {
        Orchestrator::with_default_policy(adapter, store, runner, budget)
    }

    // (b) changed signal → exactly one job with the right (seed-derived, query) + state updated.
    #[tokio::test]
    async fn changed_signal_runs_exactly_one_job_and_updates_state() {
        let adapter = Arc::new(FakeAdapter::new(vec![domain("d1")]));
        adapter.set_signal(signal("d1", 2));
        let runner = Arc::new(FakeRunner::default());
        let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
        let orch = orchestrator(
            Arc::clone(&adapter),
            Arc::clone(&runner),
            Arc::clone(&store),
            Budget::default(),
        );

        let report = orch.tick().await.unwrap();

        assert_eq!(report.sensed, vec!["d1".to_string()]);
        assert_eq!(report.changed, vec!["d1".to_string()]);
        assert_eq!(report.ran.len(), 1);
        assert!(report.deferred.is_empty());
        assert!(report.is_clean());

        // Exactly one job, carrying a community-derived seed + the policy query.
        let jobs = runner.received();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].domain_id(), "d1");
        assert!(
            jobs[0].seed_document.raw_text.contains("Domain d1"),
            "seed is community-derived"
        );
        assert!(
            jobs[0].query.contains("Domain d1") && jobs[0].query.ends_with('?'),
            "policy query"
        );

        // State advanced to the new fingerprint.
        let st = store.load().unwrap();
        assert_eq!(st.last_fingerprint("d1"), Some(&SignalFingerprint::of(&signal("d1", 2))));
        assert!(st.domains["d1"].last_outcome_summary.as_ref().unwrap().contains("rep_d1"));
    }

    // (a) unchanged signal → debounced, no job (second tick with the same signal).
    #[tokio::test]
    async fn unchanged_signal_is_debounced_no_job() {
        let adapter = Arc::new(FakeAdapter::new(vec![domain("d1")]));
        adapter.set_signal(signal("d1", 2));
        let runner = Arc::new(FakeRunner::default());
        let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
        let orch = orchestrator(
            Arc::clone(&adapter),
            Arc::clone(&runner),
            Arc::clone(&store),
            Budget::default(),
        );

        // First tick runs it; second tick (same signal) must debounce.
        let r1 = orch.tick().await.unwrap();
        assert_eq!(r1.ran.len(), 1);
        let r2 = orch.tick().await.unwrap();
        assert_eq!(r2.sensed, vec!["d1".to_string()]);
        assert!(r2.changed.is_empty(), "unchanged signal must not be 'changed'");
        assert!(r2.ran.is_empty(), "unchanged signal must not run a job");
        assert_eq!(runner.received().len(), 1, "runner driven exactly once across both ticks");
    }

    // (c) budget cap → runs ≤ cap, the rest reported as deferred (never silently dropped).
    #[tokio::test]
    async fn budget_caps_runs_and_reports_the_rest_deferred() {
        let domains: Vec<CommunityDomain> = (0..5).map(|i| domain(&format!("d{i}"))).collect();
        let adapter = Arc::new(FakeAdapter::new(domains));
        for i in 0..5 {
            adapter.set_signal(signal(&format!("d{i}"), 1));
        }
        let runner = Arc::new(FakeRunner::default());
        let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
        let budget = Budget { max_runs_per_tick: 2, max_concurrent: 2 };
        let orch =
            orchestrator(Arc::clone(&adapter), Arc::clone(&runner), Arc::clone(&store), budget);

        let report = orch.tick().await.unwrap();

        assert_eq!(report.sensed.len(), 5);
        assert_eq!(report.changed.len(), 5, "all 5 changed");
        assert_eq!(report.ran.len(), 2, "budget caps runs at 2");
        assert_eq!(report.deferred.len(), 3, "the other 3 are deferred, not dropped");
        assert_eq!(runner.received().len(), 2, "the runner saw exactly the budgeted 2");
        // Deferred reasons are recorded (audit trail).
        assert!(report.deferred.iter().all(|d| d.reason.contains("max_runs_per_tick")));
        // Ran + deferred together account for every changed domain (nothing lost).
        let mut accounted: std::collections::HashSet<String> =
            report.ran.iter().map(|o| o.domain_id.clone()).collect();
        accounted.extend(report.deferred.iter().map(|d| d.domain_id.clone()));
        assert_eq!(accounted.len(), 5);
    }

    // (d) state persist → reload → unchanged signal does NOT re-run (continuity across restart).
    #[tokio::test]
    async fn persisted_state_prevents_rerun_after_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("autonomy_state.json");

        let adapter = Arc::new(FakeAdapter::new(vec![domain("d1")]));
        adapter.set_signal(signal("d1", 2));

        // First orchestrator instance (process A): runs the job and persists state to disk.
        {
            let runner = Arc::new(FakeRunner::default());
            let store: Arc<dyn StateStore> = Arc::new(JsonFileStateStore::new(&state_path));
            let orch =
                orchestrator(Arc::clone(&adapter), Arc::clone(&runner), store, Budget::default());
            let r = orch.tick().await.unwrap();
            assert_eq!(r.ran.len(), 1);
        }
        assert!(state_path.exists(), "state checkpoint written to disk");

        // Second orchestrator instance (process B, a "restart"): SAME unchanged signal, FRESH
        // store reading the SAME file → must debounce (continuity), running NO new job.
        {
            let runner = Arc::new(FakeRunner::default());
            let store: Arc<dyn StateStore> = Arc::new(JsonFileStateStore::new(&state_path));
            let orch =
                orchestrator(Arc::clone(&adapter), Arc::clone(&runner), store, Budget::default());
            let r = orch.tick().await.unwrap();
            assert!(r.changed.is_empty(), "restart must not see an unchanged signal as changed");
            assert!(r.ran.is_empty(), "restart must NOT re-run an unchanged signal");
            assert_eq!(runner.received().len(), 0, "no job after restart on unchanged signal");
        }
    }

    // (e) per-domain error isolation: one SENSE failure + one PREDICT failure don't abort the rest.
    #[tokio::test]
    async fn per_domain_errors_are_isolated() {
        let adapter = Arc::new(FakeAdapter::new(vec![
            domain("ok"),
            domain("sense_fail"),
            domain("predict_fail"),
        ]));
        adapter.set_signal(signal("ok", 1));
        adapter.set_signal(signal("predict_fail", 1));
        // "sense_fail" has no scripted signal AND is marked to fail fetch_signal.
        adapter.fail_signal("sense_fail");

        let runner = Arc::new(FakeRunner::default());
        runner.fail("predict_fail");
        let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
        let orch = orchestrator(
            Arc::clone(&adapter),
            Arc::clone(&runner),
            Arc::clone(&store),
            Budget::default(),
        );

        let report = orch.tick().await.unwrap();

        // The healthy domain ran to completion despite the two failures.
        assert_eq!(report.ran.len(), 1);
        assert_eq!(report.ran[0].domain_id, "ok");
        // Two isolated errors: one SENSE, one PREDICT.
        assert_eq!(report.errors.len(), 2);
        assert!(report.errors.iter().any(|e| e.domain_id == "sense_fail" && e.stage == "sense"));
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.domain_id == "predict_fail" && e.stage == "predict")
        );
        assert!(!report.is_clean());

        // The failed PREDICT domain's fingerprint was NOT advanced → it will retry next tick.
        let st = store.load().unwrap();
        assert!(
            st.last_fingerprint("predict_fail").is_none(),
            "failed run must not advance state"
        );
        assert!(st.last_fingerprint("ok").is_some(), "successful run advanced state");
    }

    // A failed PREDICT run is retried on the next tick (because its fingerprint wasn't advanced).
    #[tokio::test]
    async fn failed_run_is_retried_next_tick() {
        let adapter = Arc::new(FakeAdapter::new(vec![domain("d1")]));
        adapter.set_signal(signal("d1", 1));
        let runner = Arc::new(FakeRunner::default());
        runner.fail("d1");
        let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
        let orch = orchestrator(
            Arc::clone(&adapter),
            Arc::clone(&runner),
            Arc::clone(&store),
            Budget::default(),
        );

        let r1 = orch.tick().await.unwrap();
        assert_eq!(r1.errors.len(), 1);
        let r2 = orch.tick().await.unwrap();
        // Same (unchanged) signal, but the prior run FAILED so it's still "changed" and retried.
        assert_eq!(r2.changed, vec!["d1".to_string()]);
        assert_eq!(runner.received().len(), 2, "the failed run was retried");
    }

    // (f)+(g) The DECIDE policy produces a sane query AND the TickReport shape is as documented.
    #[tokio::test]
    async fn tick_report_shape_and_policy_query() {
        let adapter = Arc::new(FakeAdapter::new(vec![domain("d1")]));
        adapter.set_signal(signal("d1", 3));
        let runner = Arc::new(FakeRunner::default());
        let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
        let orch = orchestrator(
            Arc::clone(&adapter),
            Arc::clone(&runner),
            Arc::clone(&store),
            Budget::default(),
        );

        let report = orch.tick().await.unwrap();
        // TickReport shape (g).
        assert_eq!(report.sensed, vec!["d1".to_string()]);
        assert_eq!(report.changed, vec!["d1".to_string()]);
        assert_eq!(report.ran.len(), 1);
        assert_eq!(report.ran[0].report_id, "rep_d1");
        assert!(report.deferred.is_empty());
        assert!(report.errors.is_empty());

        // DECIDE policy query (f): grounded, asks a trend question over a horizon.
        let q = &runner.received()[0].query;
        assert!(q.contains("Domain d1"));
        assert!(q.to_lowercase().contains("trend"));
        assert!(q.contains("30 days"));
        assert!(q.contains("3 active topics"));
    }

    #[test]
    fn budget_normalizes_concurrency() {
        // Zero concurrency is clamped up to 1 (no deadlock).
        let b = Budget { max_runs_per_tick: 4, max_concurrent: 0 }.normalized();
        assert_eq!(b.max_concurrent, 1);
        // Concurrency above the per-tick cap is clamped down (pointless otherwise).
        let b = Budget { max_runs_per_tick: 2, max_concurrent: 9 }.normalized();
        assert_eq!(b.max_concurrent, 2);
    }
}
