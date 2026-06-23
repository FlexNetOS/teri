//! pebesen HTTP adapter — concrete [`CommunityAdapter`] / [`CommunityFeedback`].
//!
//! These clients talk to pebesen over plain HTTP (reqwest); teri takes **no cargo
//! dependency** on the pebesen crates, so the seam stays decoupled and any other
//! platform can implement the same traits.
//!
//! ## Route map (pebesen REST)
//!
//! pebesen's handlers (`pebesen/crates/api/src/{spaces,streams,topics}.rs`) route
//! spaces by **slug**, streams nested under a space slug, and topics nested under a
//! stream id. The adapter mirrors that shape:
//!
//! | normalized call                | pebesen route                                  | source DTO            |
//! |--------------------------------|------------------------------------------------|-----------------------|
//! | `fetch_domains`                | `GET  /api/spaces`                             | `SpaceDTO[]`          |
//! | `fetch_contributors(domain)`   | `GET  /api/spaces/{slug}/members`             | `PaginatedMembers`    |
//! | `fetch_topics(domain)`         | `GET  /api/spaces/{slug}/streams` →           | `StreamDTO[]` then    |
//! |                                | `GET  /api/streams/{stream_id}/topics?status=all` | `TopicDTO[]`     |
//! | `fetch_signal(domain)`         | derived from contributors + topics            | (aggregate)           |
//!
//! ## Feedback map (pebesen intelligence receiver)
//!
//! Predictions are POSTed to the platform's intelligence endpoints, which the
//! `pebesen-intelligence` crate backs:
//!
//! | normalized call                  | pebesen route                                |
//! |----------------------------------|----------------------------------------------|
//! | `push_topic_signals`             | `POST /api/intelligence/topic-signals`       |
//! | `push_contributor_trajectories`  | `POST /api/intelligence/contributor-trajectories` |
//! | `push_health_risks`              | `POST /api/intelligence/space-health-risks`  |
//!
//! `base_url` is configurable so the same client targets local dev, a staging
//! deployment, or a test [`httpmock`] server. An optional bearer token is sent on
//! every request for pebesen's auth-gated routes.

use super::{
    CommunityAdapter, CommunityContributor, CommunityDomain, CommunityFeedback, CommunitySignal,
    CommunityTopic, ContributorTrajectory, SpaceHealthRisk, TopicSignal,
};
use crate::error::{Result, TeriError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Default number of most-recently-active topics carried in a [`CommunitySignal`].
const DEFAULT_RECENT_TOPIC_LIMIT: usize = 10;

// ─────────────────────────────────────────────────────────────────────────────
// Wire DTOs — shapes pebesen's API actually serializes (mirrored, not imported)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SpaceDto {
    id: String,
    slug: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    visibility: String,
    #[serde(default)]
    member_count: i64,
}

#[derive(Debug, Deserialize)]
struct UserDto {
    id: String,
    username: String,
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct MemberDto {
    user: UserDto,
    role: String,
    #[serde(default)]
    joined_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct PaginatedMembersDto {
    members: Vec<MemberDto>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDto {
    id: String,
}

#[derive(Debug, Deserialize)]
struct TopicDto {
    id: String,
    stream_id: String,
    name: String,
    status: String,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    last_active: Option<DateTime<Utc>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// PebesenAdapter — ingest
// ─────────────────────────────────────────────────────────────────────────────

/// HTTP client implementing [`CommunityAdapter`] against a pebesen deployment.
#[derive(Debug, Clone)]
pub struct PebesenAdapter {
    base_url: String,
    auth_token: Option<String>,
    http: reqwest::Client,
    recent_topic_limit: usize,
}

impl PebesenAdapter {
    /// Construct an adapter targeting `base_url` (e.g. `http://localhost:8080`).
    /// Trailing slashes are trimmed so route joins are unambiguous.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            auth_token: None,
            http: reqwest::Client::new(),
            recent_topic_limit: DEFAULT_RECENT_TOPIC_LIMIT,
        }
    }

    /// Attach a bearer token sent on every request (pebesen auth-gated routes).
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    /// Override how many recent topics a [`CommunitySignal`] carries.
    pub fn with_recent_topic_limit(mut self, limit: usize) -> Self {
        self.recent_topic_limit = limit;
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Issue a GET and deserialize JSON, mapping transport/status/parse failures
    /// into [`TeriError::Http`] / [`TeriError::Api`] so the seam reports honestly.
    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let mut req = self.http.get(self.url(path));
        if let Some(token) = &self.auth_token {
            req = req.bearer_auth(token);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| TeriError::Http(format!("pebesen GET {path} failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(TeriError::Api(format!(
                "pebesen GET {path} returned status {}",
                resp.status()
            )));
        }
        resp.json::<T>()
            .await
            .map_err(|e| TeriError::Api(format!("pebesen GET {path} bad JSON: {e}")))
    }

    /// All topics in a domain, fetched per-stream. Status `all` is requested so the
    /// signal reflects open + closed + archived; `active_topic_count` is derived
    /// from the per-topic status, not the query.
    async fn fetch_topics_inner(&self, domain: &CommunityDomain) -> Result<Vec<CommunityTopic>> {
        let streams: Vec<StreamDto> =
            self.get_json(&format!("/api/spaces/{}/streams", domain.slug)).await?;

        let mut topics = Vec::new();
        for stream in streams {
            let stream_topics: Vec<TopicDto> =
                self.get_json(&format!("/api/streams/{}/topics?status=all", stream.id)).await?;
            for t in stream_topics {
                topics.push(CommunityTopic {
                    id: t.id,
                    stream_id: t.stream_id,
                    name: t.name,
                    status: t.status,
                    created_at: t.created_at,
                    last_active: t.last_active,
                });
            }
        }
        Ok(topics)
    }
}

impl SpaceDto {
    fn into_domain(self) -> CommunityDomain {
        CommunityDomain {
            id: self.id,
            slug: self.slug,
            name: self.name,
            description: self.description,
            visibility: self.visibility,
            member_count: self.member_count.max(0) as u64,
        }
    }
}

#[async_trait]
impl CommunityAdapter for PebesenAdapter {
    async fn fetch_domains(&self) -> Result<Vec<CommunityDomain>> {
        let spaces: Vec<SpaceDto> = self.get_json("/api/spaces").await?;
        Ok(spaces.into_iter().map(SpaceDto::into_domain).collect())
    }

    async fn fetch_contributors(
        &self,
        domain: &CommunityDomain,
    ) -> Result<Vec<CommunityContributor>> {
        let mut contributors = Vec::new();
        let mut cursor: Option<String> = None;
        // Walk pebesen's cursor pagination to completion.
        loop {
            let path = match &cursor {
                Some(c) => format!("/api/spaces/{}/members?cursor={c}", domain.slug),
                None => format!("/api/spaces/{}/members", domain.slug),
            };
            let page: PaginatedMembersDto = self.get_json(&path).await?;
            for m in page.members {
                contributors.push(CommunityContributor {
                    id: m.user.id,
                    username: m.user.username,
                    display_name: m.user.display_name,
                    role: m.role,
                    joined_at: m.joined_at,
                });
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        Ok(contributors)
    }

    async fn fetch_topics(&self, domain: &CommunityDomain) -> Result<Vec<CommunityTopic>> {
        self.fetch_topics_inner(domain).await
    }

    async fn fetch_signal(&self, domain: &CommunityDomain) -> Result<CommunitySignal> {
        let contributors = self.fetch_contributors(domain).await?;
        let mut topics = self.fetch_topics_inner(domain).await?;

        let topic_count = topics.len() as u64;
        let active_topic_count =
            topics.iter().filter(|t| t.status.eq_ignore_ascii_case("open")).count() as u64;

        // Recent topics, freshest first. Topics without a timestamp sort last.
        topics.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        topics.truncate(self.recent_topic_limit);

        Ok(CommunitySignal {
            domain_id: domain.id.clone(),
            domain_slug: domain.slug.clone(),
            contributor_count: contributors.len() as u64,
            topic_count,
            active_topic_count,
            recent_topics: topics,
            captured_at: Utc::now(),
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PebesenFeedback — push predictions back
// ─────────────────────────────────────────────────────────────────────────────

/// HTTP client implementing [`CommunityFeedback`] against pebesen's intelligence
/// receiver endpoints.
#[derive(Debug, Clone)]
pub struct PebesenFeedback {
    base_url: String,
    auth_token: Option<String>,
    http: reqwest::Client,
}

impl PebesenFeedback {
    /// Construct a feedback client targeting `base_url`.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            auth_token: None,
            http: reqwest::Client::new(),
        }
    }

    /// Attach a bearer token sent on every push.
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// POST a JSON body, treating any non-2xx as a hard error (predictions must
    /// not be silently dropped). Empty pushes short-circuit without a request.
    async fn post_json<T: serde::Serialize>(&self, path: &str, body: &[T]) -> Result<()> {
        if body.is_empty() {
            return Ok(());
        }
        let mut req = self.http.post(self.url(path)).json(body);
        if let Some(token) = &self.auth_token {
            req = req.bearer_auth(token);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| TeriError::Http(format!("pebesen POST {path} failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(TeriError::Api(format!(
                "pebesen POST {path} returned status {}",
                resp.status()
            )));
        }
        Ok(())
    }
}

#[async_trait]
impl CommunityFeedback for PebesenFeedback {
    async fn push_topic_signals(&self, signals: Vec<TopicSignal>) -> Result<()> {
        self.post_json("/api/intelligence/topic-signals", &signals).await
    }

    async fn push_contributor_trajectories(
        &self,
        trajectories: Vec<ContributorTrajectory>,
    ) -> Result<()> {
        self.post_json("/api/intelligence/contributor-trajectories", &trajectories)
            .await
    }

    async fn push_health_risks(&self, risks: Vec<SpaceHealthRisk>) -> Result<()> {
        self.post_json("/api/intelligence/space-health-risks", &risks).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use serde_json::json;

    fn domain() -> CommunityDomain {
        CommunityDomain {
            id: "space-1".to_string(),
            slug: "rust-lang".to_string(),
            name: "Rust Lang".to_string(),
            description: Some("All things Rust".to_string()),
            visibility: "public".to_string(),
            member_count: 2,
        }
    }

    #[tokio::test]
    async fn fetch_domains_maps_spaces() {
        let server = MockServer::start_async().await;
        let _m = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/spaces");
                then.status(200).json_body(json!([
                    {
                        "id": "space-1",
                        "slug": "rust-lang",
                        "name": "Rust Lang",
                        "description": "All things Rust",
                        "visibility": "public",
                        "created_at": "2024-01-01T00:00:00Z",
                        "member_count": 42
                    }
                ]));
            })
            .await;

        let adapter = PebesenAdapter::new(server.base_url());
        let domains = adapter.fetch_domains().await.expect("fetch_domains ok");
        assert_eq!(domains.len(), 1);
        assert_eq!(domains[0].slug, "rust-lang");
        assert_eq!(domains[0].name, "Rust Lang");
        assert_eq!(domains[0].member_count, 42);
        assert_eq!(domains[0].visibility, "public");
    }

    #[tokio::test]
    async fn fetch_contributors_walks_pagination() {
        let server = MockServer::start_async().await;
        // First page returns a next_cursor; second page closes it out.
        // `matches` disambiguates the two mocks by cursor presence (httpmock 0.7
        // has no `query_param_missing`; an ambiguous match would error).
        let _p1 = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/spaces/rust-lang/members").matches(|req| {
                    req.query_params
                        .as_ref()
                        .map(|p| p.iter().all(|(k, _)| k != "cursor"))
                        .unwrap_or(true)
                });
                then.status(200).json_body(json!({
                    "members": [
                        {"user": {"id": "u1", "username": "ferris", "display_name": "Ferris"},
                         "role": "owner", "joined_at": "2024-01-01T00:00:00Z"}
                    ],
                    "next_cursor": "50"
                }));
            })
            .await;
        let _p2 = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/api/spaces/rust-lang/members")
                    .query_param("cursor", "50");
                then.status(200).json_body(json!({
                    "members": [
                        {"user": {"id": "u2", "username": "corro", "display_name": "Corro"},
                         "role": "editor", "joined_at": null}
                    ],
                    "next_cursor": null
                }));
            })
            .await;

        let adapter = PebesenAdapter::new(server.base_url());
        let contributors =
            adapter.fetch_contributors(&domain()).await.expect("fetch_contributors ok");
        assert_eq!(contributors.len(), 2);
        assert_eq!(contributors[0].username, "ferris");
        assert_eq!(contributors[0].role, "owner");
        assert_eq!(contributors[1].id, "u2");
        assert!(contributors[1].joined_at.is_none());
    }

    #[tokio::test]
    async fn fetch_signal_aggregates_streams_and_topics() {
        let server = MockServer::start_async().await;
        // members
        let _members = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/spaces/rust-lang/members");
                then.status(200).json_body(json!({
                    "members": [
                        {"user": {"id": "u1", "username": "ferris", "display_name": "Ferris"},
                         "role": "owner", "joined_at": null}
                    ],
                    "next_cursor": null
                }));
            })
            .await;
        // streams
        let _streams = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/spaces/rust-lang/streams");
                then.status(200).json_body(json!([
                    {"id": "stream-1", "space_id": "space-1", "name": "general",
                     "description": null, "visibility": "public",
                     "created_at": "2024-01-01T00:00:00Z"}
                ]));
            })
            .await;
        // topics under stream-1
        let _topics = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/streams/stream-1/topics");
                then.status(200).json_body(json!([
                    {"id": "t1", "stream_id": "stream-1", "name": "async traits",
                     "status": "open", "created_by": null,
                     "created_at": "2024-01-01T00:00:00Z",
                     "last_active": "2024-03-01T00:00:00Z"},
                    {"id": "t2", "stream_id": "stream-1", "name": "old thread",
                     "status": "archived", "created_by": null,
                     "created_at": "2024-01-01T00:00:00Z",
                     "last_active": "2024-02-01T00:00:00Z"}
                ]));
            })
            .await;

        let adapter = PebesenAdapter::new(server.base_url());
        let signal = adapter.fetch_signal(&domain()).await.expect("fetch_signal ok");

        assert_eq!(signal.contributor_count, 1);
        assert_eq!(signal.topic_count, 2);
        assert_eq!(signal.active_topic_count, 1); // only the "open" one
        assert_eq!(signal.recent_topics.len(), 2);
        // Freshest first: t1 (March) before t2 (February).
        assert_eq!(signal.recent_topics[0].id, "t1");
        assert_eq!(signal.recent_topics[1].id, "t2");
    }

    #[tokio::test]
    async fn to_seed_document_default_impl_renders_signal() {
        let server = MockServer::start_async().await;
        let _members = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/spaces/rust-lang/members");
                then.status(200).json_body(json!({
                    "members": [
                        {"user": {"id": "u1", "username": "ferris", "display_name": "Ferris"},
                         "role": "owner", "joined_at": null}
                    ],
                    "next_cursor": null
                }));
            })
            .await;
        let _streams = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/spaces/rust-lang/streams");
                then.status(200).json_body(json!([
                    {"id": "stream-1", "space_id": "space-1", "name": "general",
                     "description": null, "visibility": "public",
                     "created_at": "2024-01-01T00:00:00Z"}
                ]));
            })
            .await;
        let _topics = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/streams/stream-1/topics");
                then.status(200).json_body(json!([
                    {"id": "t1", "stream_id": "stream-1", "name": "async traits",
                     "status": "open", "created_by": null,
                     "created_at": "2024-01-01T00:00:00Z",
                     "last_active": "2024-03-01T00:00:00Z"}
                ]));
            })
            .await;

        let adapter = PebesenAdapter::new(server.base_url());
        let doc = adapter.to_seed_document(&domain()).await.expect("to_seed_document ok");

        assert_eq!(doc.metadata.get("source").unwrap(), "community");
        assert_eq!(doc.metadata.get("domain_slug").unwrap(), "rust-lang");
        assert_eq!(doc.metadata.get("topic_count").unwrap(), "1");
        assert!(doc.raw_text.contains("Rust Lang"));
        assert!(doc.raw_text.contains("async traits"));
        assert!(doc.raw_text.contains("@ferris"));
    }

    #[tokio::test]
    async fn fetch_domains_propagates_http_error() {
        let server = MockServer::start_async().await;
        let _m = server
            .mock_async(|when, then| {
                when.method(GET).path("/api/spaces");
                then.status(500).body("boom");
            })
            .await;
        let adapter = PebesenAdapter::new(server.base_url());
        let res = adapter.fetch_domains().await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn push_topic_signals_posts_payload() {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/intelligence/topic-signals").json_body(json!([
                    {"topic_id": "t1", "domain_id": "space-1", "momentum": 0.5,
                     "confidence": 0.9, "rationale": "rising"}
                ]));
                then.status(202);
            })
            .await;

        let feedback = PebesenFeedback::new(server.base_url());
        feedback
            .push_topic_signals(vec![TopicSignal {
                topic_id: "t1".to_string(),
                domain_id: "space-1".to_string(),
                momentum: 0.5,
                confidence: 0.9,
                rationale: "rising".to_string(),
            }])
            .await
            .expect("push ok");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn push_empty_is_noop_no_request() {
        // No mock registered: if a request were made, it would 404 and error.
        let server = MockServer::start_async().await;
        let feedback = PebesenFeedback::new(server.base_url());
        feedback.push_health_risks(vec![]).await.expect("empty push is a no-op");
    }

    #[tokio::test]
    async fn push_non_2xx_is_error() {
        let server = MockServer::start_async().await;
        let _m = server
            .mock_async(|when, then| {
                when.method(POST).path("/api/intelligence/space-health-risks");
                then.status(500);
            })
            .await;
        let feedback = PebesenFeedback::new(server.base_url());
        let res = feedback
            .push_health_risks(vec![SpaceHealthRisk {
                domain_id: "space-1".to_string(),
                risk: "activity_decline".to_string(),
                severity: 0.7,
                confidence: 0.6,
                detail: "halved".to_string(),
            }])
            .await;
        assert!(res.is_err());
    }
}
