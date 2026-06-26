//! End-to-end + smoke tests for the live teri ↔ pebesen prediction loop.
//!
//! These drive the **real** pebesen intelligence receiver (`pebesen_intelligence::http::router`,
//! started in-process on an ephemeral port) against teri's real `PebesenFeedback`/`PebesenAdapter`
//! over actual HTTP — no database required (the receiver's store is in-memory). The ingest
//! direction (adapter reads pebesen's DB-backed read API) is exercised against an `httpmock`
//! stand-in for pebesen's spaces/streams/topics endpoints.
//!
//! Coverage:
//!   * SMOKE          — receiver boots, `/health` is 200 and self-identifies.
//!   * FEEDBACK loop  — teri pushes all three prediction kinds; receiver stores + scopes them.
//!   * ACTION/CALIB   — operator actions a prediction; calibration moves; unknown id 404s.
//!   * INGEST         — adapter pulls pebesen signal and renders a SeedDocument.
//!   * ROUND TRIP     — signal in (adapter) → prediction out (feedback) → lands scoped correctly.

use httpmock::{Method::GET, MockServer};
use pebesen_intelligence::{IntelligenceStore, http::router};
use serde_json::json;
use teri::seed::community::pebesen::{PebesenAdapter, PebesenFeedback};
use teri::seed::community::{
    CommunityAdapter, CommunityFeedback, ContributorTrajectory, SpaceHealthRisk, TopicSignal,
};

/// Start the real intelligence receiver on an ephemeral port. Returns the base URL
/// and a handle to the SAME store the router uses (for direct assertions).
async fn spawn_receiver() -> (String, IntelligenceStore) {
    let store = IntelligenceStore::new();
    let app = router(store.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral port");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve receiver");
    });
    (format!("http://{addr}"), store)
}

fn topic_signal(domain: &str, topic: &str, confidence: f64) -> TopicSignal {
    TopicSignal {
        topic_id: topic.to_string(),
        domain_id: domain.to_string(),
        momentum: 0.4,
        confidence,
        rationale: "rising engagement".to_string(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smoke_receiver_health() {
    let (base, _store) = spawn_receiver().await;
    let resp = reqwest::get(format!("{base}/health")).await.unwrap();
    assert_eq!(resp.status(), 200, "health must be 200");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "pebesen-intelligence");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn feedback_loop_pushes_all_kinds_over_http() {
    let (base, store) = spawn_receiver().await;
    let feedback = PebesenFeedback::new(&base);

    // teri pushes all three prediction kinds, all scoped to "space-1".
    feedback
        .push_topic_signals(vec![
            topic_signal("space-1", "t1", 0.8),
            topic_signal("space-1", "t2", 0.6),
        ])
        .await
        .unwrap();
    feedback
        .push_contributor_trajectories(vec![ContributorTrajectory {
            contributor_id: "u1".to_string(),
            domain_id: "space-1".to_string(),
            trajectory: "rising".to_string(),
            engagement_score: 0.9,
            confidence: 0.7,
        }])
        .await
        .unwrap();
    feedback
        .push_health_risks(vec![SpaceHealthRisk {
            domain_id: "space-1".to_string(),
            risk: "activity_decline".to_string(),
            severity: 0.4,
            confidence: 0.5,
            detail: "slowing".to_string(),
        }])
        .await
        .unwrap();

    // Asserted two ways: directly on the store, and over the receiver's read HTTP API.
    assert_eq!(store.list_by_space("space-1").unwrap().len(), 4);

    let listed: Vec<serde_json::Value> =
        reqwest::get(format!("{base}/api/intelligence/spaces/space-1/predictions"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(listed.len(), 4, "HTTP read endpoint returns all 4");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_push_is_noop() {
    let (base, store) = spawn_receiver().await;
    let feedback = PebesenFeedback::new(&base);
    feedback.push_topic_signals(vec![]).await.unwrap();
    assert_eq!(store.list_by_space("space-1").unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn action_then_calibration_over_http() {
    let (base, store) = spawn_receiver().await;
    let feedback = PebesenFeedback::new(&base);
    feedback
        .push_topic_signals(vec![topic_signal("space-cal", "t1", 0.8)])
        .await
        .unwrap();

    let pred_id = store.list_by_space("space-cal").unwrap()[0].prediction_id;
    let client = reqwest::Client::new();

    // Operator actions the prediction as accurate.
    let resp = client
        .post(format!("{base}/api/intelligence/predictions/{pred_id}/action"))
        .json(&json!({ "accurate": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Calibration now reflects one accurate outcome → confidence_adjustment 1.5 (upper clamp).
    let cal: serde_json::Value =
        reqwest::get(format!("{base}/api/intelligence/spaces/space-cal/calibration"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(cal["scored"], 1);
    assert_eq!(cal["accurate"], 1);
    assert_eq!(cal["confidence_adjustment"], 1.5);

    // Fail-closed: actioning an unknown prediction id is a 404.
    let bad = client
        .post(format!("{base}/api/intelligence/predictions/{}/action", uuid::Uuid::new_v4()))
        .json(&json!({ "accurate": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 404);
}

/// Stand-in for pebesen's DB-backed read API (spaces/streams/topics/members) so the
/// adapter can be exercised end-to-end without a live postgres.
fn mock_pebesen_read_api() -> MockServer {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/api/spaces");
        then.status(200).json_body(json!([{
            "id": "11111111-1111-1111-1111-111111111111",
            "slug": "rust-lang",
            "name": "Rust Lang",
            "description": "All things Rust",
            "visibility": "public",
            "member_count": 3
        }]));
    });
    server.mock(|when, then| {
        when.method(GET).path("/api/spaces/rust-lang/members");
        then.status(200).json_body(json!({
            "members": [
                {"user": {"id": "u1", "username": "alice", "display_name": "Alice"}, "role": "owner"},
                {"user": {"id": "u2", "username": "bob", "display_name": "Bob"}, "role": "editor"}
            ],
            "next_cursor": null
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/api/spaces/rust-lang/streams");
        then.status(200).json_body(json!([{ "id": "stream-1" }]));
    });
    server.mock(|when, then| {
        when.method(GET).path("/api/streams/stream-1/topics");
        then.status(200).json_body(json!([
            {"id": "tp1", "stream_id": "stream-1", "name": "async runtime", "status": "open",
             "last_active": "2026-06-20T10:00:00Z"},
            {"id": "tp2", "stream_id": "stream-1", "name": "old thread", "status": "closed",
             "last_active": "2026-06-01T10:00:00Z"}
        ]));
    });
    server
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ingest_signal_to_seed_document() {
    let server = mock_pebesen_read_api();
    let adapter = PebesenAdapter::new(server.base_url());

    let domains = adapter.fetch_domains().await.unwrap();
    assert_eq!(domains.len(), 1);
    assert_eq!(domains[0].slug, "rust-lang");

    let signal = adapter.fetch_signal(&domains[0]).await.unwrap();
    assert_eq!(signal.topic_count, 2);
    assert_eq!(signal.active_topic_count, 1, "only the 'open' topic is active");

    let seed = adapter.to_seed_document(&domains[0]).await.unwrap();
    assert_eq!(seed.metadata.get("community_platform").unwrap(), "pebesen");
    assert_eq!(seed.metadata.get("domain_slug").unwrap(), "rust-lang");
    assert_eq!(seed.metadata.get("active_topic_count").unwrap(), "1");
    assert!(seed.raw_text.contains("Rust Lang"));
    assert!(seed.raw_text.contains("async runtime"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_round_trip_signal_in_prediction_out() {
    // INGEST: pull a domain + signal from the (mocked) pebesen read API.
    let read_api = mock_pebesen_read_api();
    let adapter = PebesenAdapter::new(read_api.base_url());
    let domains = adapter.fetch_domains().await.unwrap();
    let signal = adapter.fetch_signal(&domains[0]).await.unwrap();
    let domain_id = signal.domain_id.clone();
    assert!(!signal.recent_topics.is_empty());

    // PREDICT (stand-in): derive a topic prediction from the freshest topic.
    let freshest = &signal.recent_topics[0];
    let prediction = TopicSignal {
        topic_id: freshest.id.clone(),
        domain_id: domain_id.clone(),
        momentum: 0.6,
        confidence: 0.75,
        rationale: format!("'{}' is the most recently active topic", freshest.name),
    };

    // FEEDBACK: push it to the real receiver and confirm it lands scoped to the same domain.
    let (base, store) = spawn_receiver().await;
    PebesenFeedback::new(&base).push_topic_signals(vec![prediction]).await.unwrap();

    let stored = store.list_by_space(&domain_id).unwrap();
    assert_eq!(stored.len(), 1, "the prediction derived from ingested signal landed");
    let cal = store.calibration(&domain_id).unwrap();
    assert_eq!(cal.total_predictions, 1);
    assert_eq!(cal.confidence_adjustment, 1.0, "no actioned outcome yet → neutral");
}
