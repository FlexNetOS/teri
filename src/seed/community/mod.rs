//! Community signal seam — teri ↔ pebesen integration.
//!
//! This module defines the normalized, transport-agnostic contract between teri
//! (the prediction engine) and a community platform (pebesen — a Zulip-like
//! spaces/streams/topics/messages/users system). It is the code-level realization
//! of the seam specced in [`ARCHITECTURE.md`] §3.
//!
//! Two directions:
//!
//! * **Ingest** ([`CommunityAdapter`]) — teri pulls community *signal*
//!   (domains, contributors, topic/message activity) and folds it into a
//!   [`crate::seed::SeedDocument`] so the five-stage pipeline can consume it like
//!   any other seed source.
//! * **Feedback** ([`CommunityFeedback`]) — teri pushes *predictions* back to the
//!   community platform (per-topic momentum signals, contributor trajectories,
//!   space-health risks) where the platform's `intelligence` layer receives them
//!   and (later) calibrates confidence against actioned outcomes.
//!
//! The normalized types are deliberately decoupled from pebesen's wire DTOs: teri
//! takes **no cargo dependency** on the pebesen crates. The concrete
//! [`pebesen::PebesenAdapter`] / [`pebesen::PebesenFeedback`] talk plain HTTP and
//! map pebesen's REST JSON onto these types, so any other community platform can
//! be wired by implementing the same two traits.

pub mod pebesen;

use crate::error::Result;
use crate::seed::SeedDocument;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// Ingest side — normalized community signal flowing INTO teri
// ─────────────────────────────────────────────────────────────────────────────

/// A community *domain* — the top-level grouping teri reasons over. Maps onto a
/// pebesen **space** (`spaces` table: slug/name/visibility/description).
///
/// `slug` is the stable, human-readable key pebesen routes by; `id` is the
/// opaque platform identifier. Both are retained so feedback can be addressed by
/// whichever the platform prefers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunityDomain {
    /// Opaque platform id (pebesen space `id`, a UUID rendered as string).
    pub id: String,
    /// Stable routing key (pebesen space `slug`).
    pub slug: String,
    /// Human-readable name.
    pub name: String,
    /// Optional long-form description.
    pub description: Option<String>,
    /// Visibility class as reported by the platform (`public` / `private` / `secret`).
    pub visibility: String,
    /// Number of members, when the platform reports it.
    pub member_count: u64,
}

/// A participant in a domain. Maps onto a pebesen **space member**
/// (`memberships` joined with `users`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunityContributor {
    /// Opaque platform user id.
    pub id: String,
    /// Login handle.
    pub username: String,
    /// Display name.
    pub display_name: String,
    /// Role in the domain (`owner` / `admin` / `editor` / `viewer`).
    pub role: String,
    /// When the contributor joined the domain.
    pub joined_at: Option<DateTime<Utc>>,
}

/// A discussion topic within a domain. Maps onto a pebesen **topic**
/// (`topics` table), denormalized with the owning stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommunityTopic {
    /// Opaque platform topic id.
    pub id: String,
    /// Id of the stream this topic lives under (pebesen `stream_id`).
    pub stream_id: String,
    /// Topic name.
    pub name: String,
    /// Lifecycle status (`open` / `closed` / `archived`).
    pub status: String,
    /// When the topic was created.
    pub created_at: Option<DateTime<Utc>>,
    /// Last activity timestamp — the recency signal for momentum.
    pub last_active: Option<DateTime<Utc>>,
}

/// Aggregate, point-in-time signal for a whole domain. This is the compact
/// summary teri ingests as a seed: how big the community is, how active it is,
/// and which topics carry the current momentum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommunitySignal {
    /// Domain this signal describes (id form).
    pub domain_id: String,
    /// Domain slug (carried for human-readable seed text).
    pub domain_slug: String,
    /// Total contributors observed.
    pub contributor_count: u64,
    /// Total topics observed across the domain.
    pub topic_count: u64,
    /// Topics still `open` (the live surface).
    pub active_topic_count: u64,
    /// The most-recently-active topics, freshest first (bounded slice).
    pub recent_topics: Vec<CommunityTopic>,
    /// When this snapshot was taken.
    pub captured_at: DateTime<Utc>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Feedback side — predictions flowing OUT of teri back to the platform
// ─────────────────────────────────────────────────────────────────────────────

/// A prediction about a single topic's near-future momentum. Pushed to the
/// platform's intelligence layer keyed by `topic_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopicSignal {
    /// Topic the prediction is about.
    pub topic_id: String,
    /// Domain (space) the topic belongs to — lets the receiver scope by space.
    pub domain_id: String,
    /// Predicted momentum, normalized to `[-1.0, 1.0]` (decline ↔ surge).
    pub momentum: f64,
    /// teri's confidence in this signal, `[0.0, 1.0]` (synthesized metadata).
    pub confidence: f64,
    /// Short human-readable rationale for surfacing in the platform UI.
    pub rationale: String,
}

/// A prediction about a contributor's trajectory within a domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContributorTrajectory {
    /// Contributor the prediction is about.
    pub contributor_id: String,
    /// Domain (space) the trajectory is scoped to.
    pub domain_id: String,
    /// Predicted direction (`rising` / `steady` / `declining` / `at_risk`).
    pub trajectory: String,
    /// Engagement score forecast, `[0.0, 1.0]`.
    pub engagement_score: f64,
    /// teri's confidence, `[0.0, 1.0]`.
    pub confidence: f64,
}

/// A predicted health risk for a whole domain/space (e.g. activity collapse,
/// moderation load, contributor churn).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceHealthRisk {
    /// Domain (space) the risk is scoped to.
    pub domain_id: String,
    /// Risk category (`activity_decline` / `churn` / `moderation_load` / …).
    pub risk: String,
    /// Severity, `[0.0, 1.0]` (higher = more severe).
    pub severity: f64,
    /// teri's confidence, `[0.0, 1.0]`.
    pub confidence: f64,
    /// Human-readable detail for the platform UI.
    pub detail: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Traits — the seam contract
// ─────────────────────────────────────────────────────────────────────────────

/// Pulls normalized community signal from a platform into teri's seed stage.
///
/// Implementors are typically thin HTTP clients (see [`pebesen::PebesenAdapter`]).
/// All methods return [`crate::error::Result`] so transport/parse failures flow
/// through teri's unified [`crate::error::TeriError`].
#[async_trait]
pub trait CommunityAdapter: Send + Sync {
    /// Enumerate the domains (spaces) teri should reason over.
    async fn fetch_domains(&self) -> Result<Vec<CommunityDomain>>;

    /// Enumerate the contributors (members) of a domain.
    async fn fetch_contributors(
        &self,
        domain: &CommunityDomain,
    ) -> Result<Vec<CommunityContributor>>;

    /// Compute the aggregate point-in-time [`CommunitySignal`] for a domain.
    async fn fetch_signal(&self, domain: &CommunityDomain) -> Result<CommunitySignal>;

    /// Enumerate the topics within a domain.
    async fn fetch_topics(&self, domain: &CommunityDomain) -> Result<Vec<CommunityTopic>>;

    /// Render a domain's signal into a teri [`SeedDocument`] ready for the pipeline.
    ///
    /// A default implementation is provided that composes the other methods and
    /// produces a deterministic, human-readable text body; implementors may
    /// override it if the platform exposes a richer native digest.
    async fn to_seed_document(&self, domain: &CommunityDomain) -> Result<SeedDocument> {
        let signal = self.fetch_signal(domain).await?;
        let contributors = self.fetch_contributors(domain).await?;
        Ok(signal_to_seed_document(domain, &signal, &contributors))
    }
}

/// Pushes teri's predictions back to a community platform's intelligence layer.
#[async_trait]
pub trait CommunityFeedback: Send + Sync {
    /// Push per-topic momentum signals.
    async fn push_topic_signals(&self, signals: Vec<TopicSignal>) -> Result<()>;

    /// Push per-contributor trajectory predictions.
    async fn push_contributor_trajectories(
        &self,
        trajectories: Vec<ContributorTrajectory>,
    ) -> Result<()>;

    /// Push per-space health-risk predictions.
    async fn push_health_risks(&self, risks: Vec<SpaceHealthRisk>) -> Result<()>;
}

/// Build a deterministic [`SeedDocument`] from a domain's signal + contributors.
///
/// Factored out of [`CommunityAdapter::to_seed_document`] so the rendering is one
/// canonical, unit-testable function (DRY) rather than duplicated per adapter.
/// The body is plain text — exactly the shape the seed stage's `text_processor`
/// already chunks — and the metadata carries the structured counts so downstream
/// stages (and tests) can assert on them without re-parsing prose.
pub fn signal_to_seed_document(
    domain: &CommunityDomain,
    signal: &CommunitySignal,
    contributors: &[CommunityContributor],
) -> SeedDocument {
    use std::fmt::Write as _;

    let mut body = String::new();
    let _ = writeln!(body, "# Community signal: {} ({})", domain.name, domain.slug);
    if let Some(desc) = &domain.description {
        let _ = writeln!(body, "{desc}");
    }
    let _ = writeln!(body);
    let _ = writeln!(body, "Visibility: {}", domain.visibility);
    let _ = writeln!(body, "Members: {}", domain.member_count);
    let _ = writeln!(body, "Contributors observed: {}", signal.contributor_count);
    let _ = writeln!(
        body,
        "Topics: {} total, {} active",
        signal.topic_count, signal.active_topic_count
    );
    let _ = writeln!(body, "Captured at: {}", signal.captured_at.to_rfc3339());

    if !contributors.is_empty() {
        let _ = writeln!(body, "\n## Contributors");
        for c in contributors {
            let _ = writeln!(body, "- {} (@{}) — {}", c.display_name, c.username, c.role);
        }
    }

    if !signal.recent_topics.is_empty() {
        let _ = writeln!(body, "\n## Recent topics (freshest first)");
        for t in &signal.recent_topics {
            let last =
                t.last_active.map(|d| d.to_rfc3339()).unwrap_or_else(|| "unknown".to_string());
            let _ = writeln!(body, "- [{}] {} (last active {})", t.status, t.name, last);
        }
    }

    let mut metadata: HashMap<String, String> = HashMap::new();
    metadata.insert("source".to_string(), "community".to_string());
    metadata.insert("community_platform".to_string(), "pebesen".to_string());
    metadata.insert("domain_id".to_string(), domain.id.clone());
    metadata.insert("domain_slug".to_string(), domain.slug.clone());
    metadata.insert("domain_name".to_string(), domain.name.clone());
    metadata.insert("contributor_count".to_string(), signal.contributor_count.to_string());
    metadata.insert("topic_count".to_string(), signal.topic_count.to_string());
    metadata.insert("active_topic_count".to_string(), signal.active_topic_count.to_string());
    metadata.insert("recent_topic_count".to_string(), signal.recent_topics.len().to_string());

    SeedDocument { id: uuid::Uuid::new_v4(), raw_text: body, metadata, created_at: Utc::now() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain() -> CommunityDomain {
        CommunityDomain {
            id: "11111111-1111-1111-1111-111111111111".to_string(),
            slug: "rust-lang".to_string(),
            name: "Rust Lang".to_string(),
            description: Some("All things Rust".to_string()),
            visibility: "public".to_string(),
            member_count: 3,
        }
    }

    #[test]
    fn signal_to_seed_document_carries_structured_metadata() {
        let d = domain();
        let signal = CommunitySignal {
            domain_id: d.id.clone(),
            domain_slug: d.slug.clone(),
            contributor_count: 2,
            topic_count: 5,
            active_topic_count: 3,
            recent_topics: vec![CommunityTopic {
                id: "t1".to_string(),
                stream_id: "s1".to_string(),
                name: "async traits".to_string(),
                status: "open".to_string(),
                created_at: None,
                last_active: Some(Utc::now()),
            }],
            captured_at: Utc::now(),
        };
        let contributors = vec![CommunityContributor {
            id: "u1".to_string(),
            username: "ferris".to_string(),
            display_name: "Ferris".to_string(),
            role: "owner".to_string(),
            joined_at: None,
        }];

        let doc = signal_to_seed_document(&d, &signal, &contributors);

        // Structured metadata (no prose re-parsing needed downstream).
        assert_eq!(doc.metadata.get("source").unwrap(), "community");
        assert_eq!(doc.metadata.get("community_platform").unwrap(), "pebesen");
        assert_eq!(doc.metadata.get("domain_slug").unwrap(), "rust-lang");
        assert_eq!(doc.metadata.get("contributor_count").unwrap(), "2");
        assert_eq!(doc.metadata.get("topic_count").unwrap(), "5");
        assert_eq!(doc.metadata.get("active_topic_count").unwrap(), "3");
        assert_eq!(doc.metadata.get("recent_topic_count").unwrap(), "1");

        // Human-readable body mentions the salient facts.
        assert!(doc.raw_text.contains("Rust Lang"));
        assert!(doc.raw_text.contains("async traits"));
        assert!(doc.raw_text.contains("@ferris"));
        assert!(!doc.raw_text.is_empty());
    }

    #[test]
    fn types_roundtrip_through_serde() {
        let sig = TopicSignal {
            topic_id: "t1".to_string(),
            domain_id: "d1".to_string(),
            momentum: 0.42,
            confidence: 0.8,
            rationale: "rising mentions".to_string(),
        };
        let json = serde_json::to_string(&sig).unwrap();
        let back: TopicSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(sig, back);

        let risk = SpaceHealthRisk {
            domain_id: "d1".to_string(),
            risk: "activity_decline".to_string(),
            severity: 0.6,
            confidence: 0.5,
            detail: "topic creation rate halved".to_string(),
        };
        let back2: SpaceHealthRisk =
            serde_json::from_str(&serde_json::to_string(&risk).unwrap()).unwrap();
        assert_eq!(risk, back2);
    }
}
