//! S4 (TASK-SEAM-2) — one behavior contract, run against every `PredictionStore`
//! backend so the in-memory and Postgres impls can never diverge.
//!
//! The in-memory store is exercised unconditionally. The Postgres store
//! (`pg::PgStore`, feature `postgres`) runs the SAME contract against a live
//! database when `TEST_DATABASE_URL` is set; otherwise it is skipped (logged),
//! mirroring the repo's network-gated test convention.

use pebesen_intelligence::{
    ContributorTrajectory, IntelligenceError, IntelligenceStore, PredictionKind, PredictionStore,
    SpaceHealthRisk, TopicSignal,
};
use uuid::Uuid;

fn topic_signal(domain: &str, topic: &str, confidence: f64) -> PredictionKind {
    PredictionKind::TopicSignal(TopicSignal {
        topic_id: topic.into(),
        domain_id: domain.into(),
        momentum: 0.4,
        confidence,
        rationale: "test".into(),
    })
}

fn health_risk(domain: &str, confidence: f64) -> PredictionKind {
    PredictionKind::SpaceHealthRisk(SpaceHealthRisk {
        domain_id: domain.into(),
        risk: "burnout".into(),
        severity: 0.7,
        confidence,
        detail: "test".into(),
    })
}

fn trajectory(domain: &str, contributor: &str) -> PredictionKind {
    PredictionKind::ContributorTrajectory(ContributorTrajectory {
        contributor_id: contributor.into(),
        domain_id: domain.into(),
        trajectory: "rising".into(),
        engagement_score: 0.8,
        confidence: 0.6,
    })
}

/// The full storage contract. Any `PredictionStore` must satisfy every assertion
/// identically. `domain` is parameterized so a shared live DB stays isolated
/// per test run.
async fn behavior_contract<S: PredictionStore>(store: S, domain: &str) {
    let other_domain = format!("{domain}-other");

    // --- ingest + scoping ---
    let p1 = store.ingest(topic_signal(domain, "t1", 0.9)).await.unwrap();
    let _p2 = store.ingest(trajectory(domain, "c1")).await.unwrap();
    let _p3 = store.ingest(health_risk(domain, 0.5)).await.unwrap();
    // A prediction in a different space must not leak into this one.
    store
        .ingest(topic_signal(&other_domain, "t9", 0.1))
        .await
        .unwrap();

    let in_space = store.list_by_space(domain).await.unwrap();
    assert_eq!(
        in_space.len(),
        3,
        "space {domain} must hold exactly its 3 predictions"
    );
    // Newest-first ordering.
    for w in in_space.windows(2) {
        assert!(
            w[0].ingested_at >= w[1].ingested_at,
            "list_by_space must be newest-first"
        );
    }

    // --- topic scoping (topic signals only) ---
    let by_topic = store.list_by_topic("t1").await.unwrap();
    assert_eq!(by_topic.len(), 1, "topic t1 must resolve its single signal");
    assert_eq!(by_topic[0].prediction_id, p1.prediction_id);

    // --- fail-closed on unknown id ---
    let unknown = store.record_action(Uuid::new_v4(), Some(true)).await;
    assert!(
        matches!(unknown, Err(IntelligenceError::UnknownPrediction(_))),
        "actioning an unknown prediction must fail-closed, got {unknown:?}"
    );

    // --- calibration: no scored outcomes yet → neutral ---
    let cal0 = store.calibration(domain).await.unwrap();
    assert_eq!(cal0.total_predictions, 3);
    assert_eq!(cal0.actioned, 0);
    assert_eq!(cal0.scored, 0);
    assert_eq!(cal0.accuracy, None);
    assert_eq!(cal0.confidence_adjustment, 1.0, "no evidence → neutral 1.0");

    // --- action + calibration movement ---
    store
        .record_action(p1.prediction_id, Some(true))
        .await
        .unwrap();
    let cal1 = store.calibration(domain).await.unwrap();
    assert_eq!(cal1.actioned, 1);
    assert_eq!(cal1.scored, 1);
    assert_eq!(cal1.accurate, 1);
    assert_eq!(cal1.accuracy, Some(1.0));
    assert_eq!(
        cal1.confidence_adjustment, 1.5,
        "all-accurate → +0.5 clamp ceiling"
    );

    // --- upsert: re-actioning the same id replaces the outcome (accurate→false) ---
    store
        .record_action(p1.prediction_id, Some(false))
        .await
        .unwrap();
    let cal2 = store.calibration(domain).await.unwrap();
    assert_eq!(cal2.actioned, 1, "re-actioning must not double-count");
    assert_eq!(cal2.scored, 1);
    assert_eq!(cal2.accurate, 0);
    assert_eq!(cal2.accuracy, Some(0.0));
    assert_eq!(
        cal2.confidence_adjustment, 0.5,
        "all-wrong → -0.5 clamp floor"
    );
}

#[tokio::test]
async fn in_memory_store_satisfies_contract() {
    behavior_contract(IntelligenceStore::new(), "space-mem").await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
async fn pg_store_satisfies_contract() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!(
            "SKIP pg_store_satisfies_contract: set TEST_DATABASE_URL to run against Postgres"
        );
        return;
    };
    let store = pebesen_intelligence::pg::PgStore::connect(&url)
        .await
        .expect("connect to TEST_DATABASE_URL");
    store.migrate().await.expect("migrate schema");
    // Unique domain per run so a shared DB doesn't accumulate cross-run state.
    let domain = format!("space-pg-{}", Uuid::new_v4());
    behavior_contract(store, &domain).await;
}
