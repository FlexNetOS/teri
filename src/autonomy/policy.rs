//! The **DECIDE policy** — turns a sensed [`CommunityDomain`] + [`CommunitySignal`] into the
//! *prediction question* the pipeline will answer.
//!
//! Per `docs/AGENTIC-STORY.md` the DECIDE layer is "a policy that turns a signal delta into a
//! `(seed, query)` job". The **seed** half is produced by the adapter
//! (`CommunityAdapter::to_seed_document`); the **query** half is produced here. The policy is a
//! trait so an operator can override the default templated question (e.g. swap in a
//! domain-specific or model-tuned prompt) without touching the orchestrator.

use crate::seed::community::{CommunityDomain, CommunitySignal};

/// The default forecast horizon (days) baked into the templated prediction question.
pub const DEFAULT_HORIZON_DAYS: u32 = 30;

/// Decides the prediction *query* for a domain whose signal changed.
///
/// Implementors map `(domain, signal)` → a natural-language prediction question. Kept tiny and
/// synchronous: it is pure policy, no I/O.
pub trait DecidePolicy: Send + Sync {
    /// Produce the prediction query for this domain's changed signal.
    fn query_for(&self, domain: &CommunityDomain, signal: &CommunitySignal) -> String;
}

/// The default DECIDE policy: a templated engagement/health-trend question over a fixed horizon,
/// grounded in the domain name and the live-topic count so the question is specific to the domain
/// that actually changed (not a generic boilerplate prompt).
#[derive(Debug, Clone)]
pub struct DefaultDecidePolicy {
    /// Forecast horizon in days, interpolated into the question.
    pub horizon_days: u32,
}

impl Default for DefaultDecidePolicy {
    fn default() -> Self {
        Self { horizon_days: DEFAULT_HORIZON_DAYS }
    }
}

impl DefaultDecidePolicy {
    /// Construct with an explicit horizon.
    pub fn with_horizon(horizon_days: u32) -> Self {
        Self { horizon_days }
    }
}

impl DecidePolicy for DefaultDecidePolicy {
    fn query_for(&self, domain: &CommunityDomain, signal: &CommunitySignal) -> String {
        format!(
            "How will engagement and community health in \"{}\" trend over the next {} days, \
             given {} active topics among {} total and {} contributors?",
            domain.name,
            self.horizon_days,
            signal.active_topic_count,
            signal.topic_count,
            signal.contributor_count,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn domain() -> CommunityDomain {
        CommunityDomain {
            id: "d1".to_string(),
            slug: "rust-lang".to_string(),
            name: "Rust Lang".to_string(),
            description: None,
            visibility: "public".to_string(),
            member_count: 10,
        }
    }

    fn signal() -> CommunitySignal {
        CommunitySignal {
            domain_id: "d1".to_string(),
            domain_slug: "rust-lang".to_string(),
            contributor_count: 7,
            topic_count: 12,
            active_topic_count: 4,
            recent_topics: vec![],
            captured_at: Utc::now(),
        }
    }

    #[test]
    fn default_policy_produces_a_sane_grounded_query() {
        let q = DefaultDecidePolicy::default().query_for(&domain(), &signal());
        // Grounded in the domain and the live signal numbers, asks about a trend over a horizon.
        assert!(q.contains("Rust Lang"));
        assert!(q.contains("30 days"));
        assert!(q.contains("4 active topics"));
        assert!(q.contains("12 total"));
        assert!(q.contains("7 contributors"));
        assert!(q.to_lowercase().contains("trend"));
        // A real question, not an empty/degenerate string.
        assert!(q.ends_with('?'));
        assert!(q.len() > 40);
    }

    #[test]
    fn horizon_is_configurable() {
        let q = DefaultDecidePolicy::with_horizon(7).query_for(&domain(), &signal());
        assert!(q.contains("7 days"));
        assert!(!q.contains("30 days"));
    }
}
