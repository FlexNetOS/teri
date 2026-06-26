//! sqlx/Postgres-backed [`PredictionStore`] (feature `postgres`).
//!
//! A durable backend for the prediction receiver, behaviorally identical to the
//! in-memory [`IntelligenceStore`](crate::IntelligenceStore) (the shared behavior
//! test runs the same assertions against both). Queries are sqlx **runtime**
//! queries (`sqlx::query`/`query_scalar`), not the compile-time-checked `query!`
//! macros, so the crate compiles in CI **without** a live database or an
//! offline `.sqlx` cache — the queries are validated at runtime against the real
//! schema created by [`PgStore::migrate`].

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::{
    IntelligenceError, Prediction, PredictionAction, PredictionKind, PredictionStore, Result,
    SpaceCalibration,
};

/// Idempotent DDL for the two receiver tables. Mirrors the column set named in
/// the `// SQLX SLOT:` markers on [`IntelligenceStore`](crate::IntelligenceStore).
pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS predictions (
    prediction_id UUID PRIMARY KEY,
    domain_id     TEXT             NOT NULL,
    topic_id      TEXT,
    kind          TEXT             NOT NULL,
    payload       JSONB            NOT NULL,
    confidence    DOUBLE PRECISION NOT NULL,
    ingested_at   TIMESTAMPTZ      NOT NULL
);
CREATE INDEX IF NOT EXISTS predictions_domain_idx ON predictions (domain_id);
CREATE INDEX IF NOT EXISTS predictions_topic_idx  ON predictions (topic_id);
CREATE TABLE IF NOT EXISTS prediction_actions (
    prediction_id UUID PRIMARY KEY REFERENCES predictions (prediction_id),
    actioned_at   TIMESTAMPTZ NOT NULL,
    accurate      BOOLEAN
);
";

/// A Postgres-backed prediction store. Cheap to clone (the inner `PgPool` is an
/// `Arc`), so it is suitable for axum app state just like the in-memory store.
#[derive(Debug, Clone)]
pub struct PgStore {
    pool: PgPool,
}

fn stor(e: sqlx::Error) -> IntelligenceError {
    IntelligenceError::Storage(e.to_string())
}

/// The `kind` discriminant column value for a payload (matches the serde tag).
fn kind_str(p: &PredictionKind) -> &'static str {
    match p {
        PredictionKind::TopicSignal(_) => "topic_signal",
        PredictionKind::ContributorTrajectory(_) => "contributor_trajectory",
        PredictionKind::SpaceHealthRisk(_) => "space_health_risk",
    }
}

fn row_to_prediction(row: &PgRow) -> Result<Prediction> {
    let prediction_id: Uuid = row.try_get("prediction_id").map_err(stor)?;
    let ingested_at: DateTime<Utc> = row.try_get("ingested_at").map_err(stor)?;
    let payload_json: serde_json::Value = row.try_get("payload").map_err(stor)?;
    let payload: PredictionKind = serde_json::from_value(payload_json)
        .map_err(|e| IntelligenceError::Storage(format!("payload decode: {e}")))?;
    Ok(Prediction {
        prediction_id,
        ingested_at,
        payload,
    })
}

impl PgStore {
    /// Wrap an existing pool (e.g. the one pebesen's `db` crate already owns).
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Connect a fresh pool to `database_url`.
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .map_err(stor)?;
        Ok(Self { pool })
    }

    /// Create the receiver tables if absent. Idempotent; call once at startup.
    pub async fn migrate(&self) -> Result<()> {
        sqlx::raw_sql(SCHEMA)
            .execute(&self.pool)
            .await
            .map_err(stor)?;
        Ok(())
    }
}

#[async_trait]
impl PredictionStore for PgStore {
    async fn ingest(&self, payload: PredictionKind) -> Result<Prediction> {
        // Identity + ingest time are assigned here (not by the DB) so semantics
        // match the in-memory store exactly.
        let prediction = Prediction {
            prediction_id: Uuid::new_v4(),
            ingested_at: Utc::now(),
            payload,
        };
        let payload_json = serde_json::to_value(&prediction.payload)
            .map_err(|e| IntelligenceError::Storage(format!("payload encode: {e}")))?;
        sqlx::query(
            "INSERT INTO predictions \
             (prediction_id, domain_id, topic_id, kind, payload, confidence, ingested_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(prediction.prediction_id)
        .bind(prediction.payload.domain_id())
        .bind(prediction.payload.topic_id())
        .bind(kind_str(&prediction.payload))
        .bind(&payload_json)
        .bind(prediction.payload.confidence())
        .bind(prediction.ingested_at)
        .execute(&self.pool)
        .await
        .map_err(stor)?;
        Ok(prediction)
    }

    async fn list_by_space(&self, domain_id: &str) -> Result<Vec<Prediction>> {
        let rows = sqlx::query(
            "SELECT prediction_id, ingested_at, payload FROM predictions \
             WHERE domain_id = $1 ORDER BY ingested_at DESC",
        )
        .bind(domain_id)
        .fetch_all(&self.pool)
        .await
        .map_err(stor)?;
        rows.iter().map(row_to_prediction).collect()
    }

    async fn list_by_topic(&self, topic_id: &str) -> Result<Vec<Prediction>> {
        let rows = sqlx::query(
            "SELECT prediction_id, ingested_at, payload FROM predictions \
             WHERE topic_id = $1 ORDER BY ingested_at DESC",
        )
        .bind(topic_id)
        .fetch_all(&self.pool)
        .await
        .map_err(stor)?;
        rows.iter().map(row_to_prediction).collect()
    }

    async fn record_action(
        &self,
        prediction_id: Uuid,
        accurate: Option<bool>,
    ) -> Result<PredictionAction> {
        // Fail-closed: the prediction must exist (mirrors the in-memory guard).
        let exists: Option<Uuid> =
            sqlx::query_scalar("SELECT prediction_id FROM predictions WHERE prediction_id = $1")
                .bind(prediction_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(stor)?;
        if exists.is_none() {
            return Err(IntelligenceError::UnknownPrediction(prediction_id));
        }
        let action = PredictionAction {
            prediction_id,
            actioned_at: Utc::now(),
            accurate,
        };
        sqlx::query(
            "INSERT INTO prediction_actions (prediction_id, actioned_at, accurate) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (prediction_id) \
             DO UPDATE SET actioned_at = EXCLUDED.actioned_at, accurate = EXCLUDED.accurate",
        )
        .bind(action.prediction_id)
        .bind(action.actioned_at)
        .bind(action.accurate)
        .execute(&self.pool)
        .await
        .map_err(stor)?;
        Ok(action)
    }

    async fn calibration(&self, domain_id: &str) -> Result<SpaceCalibration> {
        // Single aggregate join — the arithmetic is delegated to
        // SpaceCalibration::from_counts so it can never diverge from in-memory.
        // COUNT(a.prediction_id) = actioned; COUNT(a.accurate) = scored (NULLs
        // excluded); FILTER counts the accurate ones.
        let row = sqlx::query(
            "SELECT \
               COUNT(*)                                          AS total, \
               COUNT(a.prediction_id)                            AS actioned, \
               COUNT(a.accurate)                                 AS scored, \
               COUNT(*) FILTER (WHERE a.accurate IS TRUE)        AS accurate \
             FROM predictions p \
             LEFT JOIN prediction_actions a ON a.prediction_id = p.prediction_id \
             WHERE p.domain_id = $1",
        )
        .bind(domain_id)
        .fetch_one(&self.pool)
        .await
        .map_err(stor)?;
        let total: i64 = row.try_get("total").map_err(stor)?;
        let actioned: i64 = row.try_get("actioned").map_err(stor)?;
        let scored: i64 = row.try_get("scored").map_err(stor)?;
        let accurate: i64 = row.try_get("accurate").map_err(stor)?;
        Ok(SpaceCalibration::from_counts(
            domain_id,
            total as usize,
            actioned as usize,
            scored as usize,
            accurate as usize,
        ))
    }
}
