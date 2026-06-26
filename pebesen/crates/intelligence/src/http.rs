//! HTTP receiver for the teri → pebesen prediction seam.
//!
//! [`router`] mounts the endpoints teri's `PebesenFeedback` pushes to (and a few
//! read endpoints an operator / the calibration loop consumes), all backed by an
//! [`IntelligenceStore`]. This is the pebesen-side half of the live loop — no DB
//! required (the in-memory store is the default). The pebesen `api` crate can nest
//! this router under its main app once that router exists; `pebesen-bin` serves it
//! standalone today.
//!
//! Wire contract (matches `teri/src/seed/community/pebesen.rs`):
//! ```text
//! POST /api/intelligence/topic-signals            body [TopicSignal]
//! POST /api/intelligence/contributor-trajectories body [ContributorTrajectory]
//! POST /api/intelligence/space-health-risks       body [SpaceHealthRisk]
//! POST /api/intelligence/predictions/:id/action   body {accurate?: bool}
//! GET  /api/intelligence/spaces/:domain_id/predictions
//! GET  /api/intelligence/spaces/:domain_id/calibration
//! GET  /health
//! ```

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    ContributorTrajectory, IntelligenceError, IntelligenceStore, Prediction, SpaceCalibration,
    SpaceHealthRisk, TopicSignal, receive_contributor_trajectories, receive_space_health_risks,
    receive_topic_signals, report_actioned, space_calibration,
};

/// Build the intelligence receiver router backed by `store`.
pub fn router(store: IntelligenceStore) -> Router {
    Router::new()
        .route("/health", get(health))
        .route(
            "/api/intelligence/topic-signals",
            post(ingest_topic_signals),
        )
        .route(
            "/api/intelligence/contributor-trajectories",
            post(ingest_contributor_trajectories),
        )
        .route(
            "/api/intelligence/space-health-risks",
            post(ingest_space_health_risks),
        )
        .route(
            "/api/intelligence/predictions/:id/action",
            post(action_prediction),
        )
        .route(
            "/api/intelligence/spaces/:domain_id/predictions",
            get(list_space_predictions),
        )
        .route(
            "/api/intelligence/spaces/:domain_id/calibration",
            get(space_calibration_route),
        )
        .with_state(store)
}

/// Map store errors to HTTP responses (fail-closed): unknown id → 404, lock
/// poisoning → 500. Body mirrors teri's `{success:false, error}` envelope.
impl IntoResponse for IntelligenceError {
    fn into_response(self) -> Response {
        let status = match self {
            IntelligenceError::UnknownPrediction(_) => StatusCode::NOT_FOUND,
            IntelligenceError::LockPoisoned | IntelligenceError::Storage(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        let body = Json(serde_json::json!({ "success": false, "error": self.to_string() }));
        (status, body).into_response()
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "pebesen-intelligence" }))
}

async fn ingest_topic_signals(
    State(store): State<IntelligenceStore>,
    Json(signals): Json<Vec<TopicSignal>>,
) -> Result<Json<Vec<Prediction>>, IntelligenceError> {
    Ok(Json(receive_topic_signals(&store, signals)?))
}

async fn ingest_contributor_trajectories(
    State(store): State<IntelligenceStore>,
    Json(trajectories): Json<Vec<ContributorTrajectory>>,
) -> Result<Json<Vec<Prediction>>, IntelligenceError> {
    Ok(Json(receive_contributor_trajectories(
        &store,
        trajectories,
    )?))
}

async fn ingest_space_health_risks(
    State(store): State<IntelligenceStore>,
    Json(risks): Json<Vec<SpaceHealthRisk>>,
) -> Result<Json<Vec<Prediction>>, IntelligenceError> {
    Ok(Json(receive_space_health_risks(&store, risks)?))
}

/// Body of `POST /predictions/:id/action`. `accurate` is optional: omitted/`null`
/// means "actioned, outcome not yet known".
#[derive(Debug, Deserialize, Default)]
pub struct ActionBody {
    #[serde(default)]
    pub accurate: Option<bool>,
}

async fn action_prediction(
    State(store): State<IntelligenceStore>,
    Path(id): Path<Uuid>,
    body: Option<Json<ActionBody>>,
) -> Result<Json<crate::PredictionAction>, IntelligenceError> {
    let accurate = body.unwrap_or_default().0.accurate;
    Ok(Json(report_actioned(&store, id, accurate)?))
}

async fn list_space_predictions(
    State(store): State<IntelligenceStore>,
    Path(domain_id): Path<String>,
) -> Result<Json<Vec<Prediction>>, IntelligenceError> {
    Ok(Json(store.list_by_space(&domain_id)?))
}

async fn space_calibration_route(
    State(store): State<IntelligenceStore>,
    Path(domain_id): Path<String>,
) -> Result<Json<SpaceCalibration>, IntelligenceError> {
    Ok(Json(space_calibration(&store, &domain_id)?))
}
