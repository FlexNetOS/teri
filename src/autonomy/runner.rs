//! The PREDICT half of the loop, behind a trait so the orchestrator is testable with no LLM.
//!
//! * [`Job`] — a decided `(seed, query)` unit of prediction work for one domain.
//! * [`JobOutcome`] — the summarized result of running a job.
//! * [`PredictionJobRunner`] — the trait the orchestrator drives; production is
//!   [`PipelineJobRunner`] (writes the seed to a temp file under the config data dir and calls
//!   `pipeline::run_pipeline` with the provider-selected LLM), tests inject a fake.

use crate::config::Config;
use crate::error::{Result, TeriError};
use crate::seed::SeedDocument;
use crate::seed::community::{CommunityDomain, CommunitySignal};
use async_trait::async_trait;

/// A decided unit of prediction work: the seed material to run on + the prediction query.
///
/// The `seed_document` is produced by `CommunityAdapter::to_seed_document(domain)` (the SENSE→seed
/// half); the `query` is produced by the DECIDE policy. `domain` is retained so the runner and the
/// audit trail can attribute the run + outcome to the originating community.
#[derive(Debug, Clone)]
pub struct Job {
    /// The domain (community) this job predicts on.
    pub domain: CommunityDomain,
    /// The seed material rendered from the domain's signal.
    pub seed_document: SeedDocument,
    /// The prediction question (from the DECIDE policy).
    pub query: String,
}

impl Job {
    /// Stable id of the domain this job targets — the key the orchestrator state is keyed by.
    pub fn domain_id(&self) -> &str {
        &self.domain.id
    }
}

/// The summarized result of running a [`Job`] — the report-level facts the orchestrator records in
/// its state and the [`crate::autonomy::TickReport`]. Deliberately a flat summary (not the full
/// `PipelineOutcome`) so the fake runner is trivial and the audit trail stays compact.
#[derive(Debug, Clone, PartialEq)]
pub struct JobOutcome {
    /// The domain this outcome is for.
    pub domain_id: String,
    /// The produced report id.
    pub report_id: String,
    /// The report outline summary, when one was produced.
    pub report_summary: Option<String>,
    /// Graph size after the run (provenance for the prediction).
    pub graph_node_count: usize,
    pub graph_edge_count: usize,
}

/// Runs a decided [`Job`] and returns its [`JobOutcome`]. The seam between the DECIDE layer and the
/// PREDICT engine: production calls the real pipeline, tests inject a recorder.
///
/// `#[async_trait(?Send)]`: the production [`PipelineJobRunner`] delegates to
/// `pipeline::run_pipeline`, whose future is **not** `Send` (the five-stage composition holds
/// non-`Send` progress callbacks across awaits — `main.rs` awaits it directly, never `spawn`s it).
/// The orchestrator therefore drives runs on a single task (`buffer_unordered` for bounded
/// concurrency without crossing threads), exactly as `teri run` does. The trait *object* is still
/// `Send + Sync` (both impls are); only the returned future is thread-local.
#[async_trait(?Send)]
pub trait PredictionJobRunner: Send + Sync {
    /// Run one prediction job to completion.
    async fn run(&self, job: &Job) -> Result<JobOutcome>;
}

/// Production runner: writes the job's seed to a temp file under the config data dir and drives the
/// real five-stage pipeline (`pipeline::run_pipeline`) with the provider-selected LLM.
///
/// The backend-honesty guard is NOT bypassed here — `run_pipeline` is the same composition `teri
/// run` uses, so a real run inherits the guard via the caller's preflight. (Tests never construct
/// this; they inject a fake.) The runner holds the `Config` it needs for the data dir + provider.
pub struct PipelineJobRunner {
    config: Config,
}

impl PipelineJobRunner {
    /// Construct from teri's loaded config.
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// The directory autonomy runs stage their temp seed files under, beside the other persisted
    /// state. Rooted at the upload folder's parent so it shares the config's data root; created on
    /// demand. Mirrors the e2e test pattern of writing the seed text to a file the pipeline reads.
    fn seed_staging_dir(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.config.upload_folder).join("autonomy_seeds")
    }
}

#[async_trait(?Send)]
impl PredictionJobRunner for PipelineJobRunner {
    async fn run(&self, job: &Job) -> Result<JobOutcome> {
        // Stage the seed text to a file the pipeline ingests (run_pipeline takes a path/URL, not a
        // SeedDocument — see tests/community_pipeline_e2e.rs for this exact pattern).
        let dir = self.seed_staging_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| TeriError::Seed(format!("autonomy seed staging dir: {e}")))?;
        let seed_path = dir.join(format!("{}.txt", job.seed_document.id));
        std::fs::write(&seed_path, &job.seed_document.raw_text)
            .map_err(|e| TeriError::Seed(format!("write autonomy seed file: {e}")))?;
        let seed_str = seed_path
            .to_str()
            .ok_or_else(|| TeriError::Seed("non-UTF8 autonomy seed path".to_string()))?;

        tracing::info!(
            domain_id = job.domain_id(),
            seed = seed_str,
            query = %job.query,
            "autonomy.predict: starting pipeline run for changed domain"
        );

        let llm = crate::api::build_provider_llm(&self.config);
        // `--agents` is entity-derived inside the pipeline; pass a sane request count.
        let outcome =
            crate::pipeline::run_pipeline(&self.config, llm, seed_str, &job.query, 10).await?;

        Ok(JobOutcome {
            domain_id: job.domain.id.clone(),
            report_id: outcome.report_id,
            report_summary: outcome.report_summary,
            graph_node_count: outcome.graph_node_count,
            graph_edge_count: outcome.graph_edge_count,
        })
    }
}

/// Convenience for callers that hold a signal: build a [`Job`] from a domain's seed document + the
/// DECIDE policy's query for `(domain, signal)`. Factored here so the orchestrator and any future
/// `teri autonomy` subcommand build jobs the one canonical way (DRY).
pub fn build_job(
    domain: &CommunityDomain,
    signal: &CommunitySignal,
    seed_document: SeedDocument,
    policy: &dyn crate::autonomy::DecidePolicy,
) -> Job {
    Job { domain: domain.clone(), query: policy.query_for(domain, signal), seed_document }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autonomy::DefaultDecidePolicy;
    use chrono::Utc;
    use std::collections::HashMap;

    fn domain() -> CommunityDomain {
        CommunityDomain {
            id: "d1".to_string(),
            slug: "dom".to_string(),
            name: "Dom".to_string(),
            description: None,
            visibility: "public".to_string(),
            member_count: 1,
        }
    }

    fn signal() -> CommunitySignal {
        CommunitySignal {
            domain_id: "d1".to_string(),
            domain_slug: "dom".to_string(),
            contributor_count: 1,
            topic_count: 2,
            active_topic_count: 1,
            recent_topics: vec![],
            captured_at: Utc::now(),
        }
    }

    fn seed_doc() -> SeedDocument {
        SeedDocument {
            id: uuid::Uuid::new_v4(),
            raw_text: "seed body".to_string(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn build_job_pairs_the_seed_with_the_policy_query() {
        let d = domain();
        let s = signal();
        let job = build_job(&d, &s, seed_doc(), &DefaultDecidePolicy::default());
        assert_eq!(job.domain_id(), "d1");
        assert_eq!(job.seed_document.raw_text, "seed body");
        // The query came from the policy, grounded in the domain.
        assert!(job.query.contains("Dom"));
        assert!(job.query.ends_with('?'));
    }
}
