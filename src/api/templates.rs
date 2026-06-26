//! Prompt Templates viewer API (`GET /api/templates`).
//!
//! Exposes the LLM prompt templates that drive each of teri's five pipeline stages so the
//! web UI can render them. The templates were previously invisible: they are compiled into
//! the binary via `include_str!` (the `.jinja` files) or live as `const &str` system prompts,
//! and NO endpoint or frontend surfaced them. This module reads each source directly and
//! returns them as JSON grouped by stage.
//!
//! Sources (read directly, the SAME paths/consts the engine uses):
//! - jinja (via `include_str!`): `templates/persona_gen.jinja` (stage 2),
//!   `templates/agent_action.jinja` (stage 3), `templates/report_gen.jinja` (stage 4).
//! - system-prompt consts (the English-default `_EN` variant — each prompt now has an `_EN`/`_ZH`
//!   pair selected at runtime by `crate::i18n::localized`; the viewer shows the English reference):
//!   `crate::services::ontology::ONTOLOGY_SYSTEM_PROMPT_EN` (stage 1),
//!   `crate::report::{PLAN_SYSTEM_PROMPT_EN, SECTION_SYSTEM_PROMPT_TEMPLATE_EN}` (stage 4),
//!   `crate::report::CHAT_SYSTEM_PROMPT_TEMPLATE_EN` (stage 5).
//!
//! The prompt text is exposed in English (the `_EN` default; zh users get the `_ZH` body at
//! render time). Stage labels: 1 Graph Build, 2 Env Setup, 3 Simulation, 4 Report, 5 Interaction.

use std::sync::Arc;

use axum::{Router, response::Json, routing::get};
use serde::Serialize;
use serde_json::Value;

use crate::api::ApiState;

/// A single prompt template descriptor as surfaced to the UI.
#[derive(Debug, Clone, Serialize)]
pub struct TemplateDescriptor {
    /// Stable identifier (e.g. `persona_gen`).
    pub id: &'static str,
    /// Pipeline stage number (1..=5).
    pub stage: u8,
    /// Human-readable stage step label (e.g. `Env Setup`).
    pub step_label: &'static str,
    /// Template kind: `jinja`, `system_prompt`, or `user_prompt`.
    pub kind: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Origin path or const reference (informational; for jinja this is the real file path).
    pub source_path: &'static str,
    /// The raw template / prompt text.
    pub content: &'static str,
}

// Jinja templates — same `include_str!` paths the engine uses (see `agent/mod.rs`).
const PERSONA_GEN_JINJA: &str = include_str!("../../templates/persona_gen.jinja");
const AGENT_ACTION_JINJA: &str = include_str!("../../templates/agent_action.jinja");
const REPORT_GEN_JINJA: &str = include_str!("../../templates/report_gen.jinja");

/// Build the full ordered list of prompt templates grouped (1→5) by pipeline stage.
///
/// Returned in stage order so the UI can render groups without re-sorting.
pub fn all_templates() -> Vec<TemplateDescriptor> {
    vec![
        // ── Stage 1: Graph Build ──
        TemplateDescriptor {
            id: "ontology_system",
            stage: 1,
            step_label: "Graph Build",
            kind: "system_prompt",
            name: "Ontology generation system prompt",
            source_path: "src/services/ontology.rs::ONTOLOGY_SYSTEM_PROMPT_EN",
            content: crate::services::ontology::ONTOLOGY_SYSTEM_PROMPT_EN,
        },
        // ── Stage 2: Env Setup ──
        TemplateDescriptor {
            id: "persona_gen",
            stage: 2,
            step_label: "Env Setup",
            kind: "jinja",
            name: "Persona generation template",
            source_path: "templates/persona_gen.jinja",
            content: PERSONA_GEN_JINJA,
        },
        // ── Stage 3: Simulation ──
        TemplateDescriptor {
            id: "agent_action",
            stage: 3,
            step_label: "Simulation",
            kind: "jinja",
            name: "Agent action template",
            source_path: "templates/agent_action.jinja",
            content: AGENT_ACTION_JINJA,
        },
        // ── Stage 4: Report ──
        TemplateDescriptor {
            id: "report_gen",
            stage: 4,
            step_label: "Report",
            kind: "jinja",
            name: "Report generation template",
            source_path: "templates/report_gen.jinja",
            content: REPORT_GEN_JINJA,
        },
        TemplateDescriptor {
            id: "report_plan_system",
            stage: 4,
            step_label: "Report",
            kind: "system_prompt",
            name: "Report plan system prompt",
            source_path: "src/report/mod.rs::PLAN_SYSTEM_PROMPT_EN",
            content: crate::report::PLAN_SYSTEM_PROMPT_EN,
        },
        TemplateDescriptor {
            id: "report_section_system",
            stage: 4,
            step_label: "Report",
            kind: "system_prompt",
            name: "Report section system prompt",
            source_path: "src/report/mod.rs::SECTION_SYSTEM_PROMPT_TEMPLATE_EN",
            content: crate::report::SECTION_SYSTEM_PROMPT_TEMPLATE_EN,
        },
        // ── Stage 5: Interaction ──
        TemplateDescriptor {
            id: "report_chat_system",
            stage: 5,
            step_label: "Interaction",
            kind: "system_prompt",
            name: "Report chat system prompt",
            source_path: "src/report/mod.rs::CHAT_SYSTEM_PROMPT_TEMPLATE_EN",
            content: crate::report::CHAT_SYSTEM_PROMPT_TEMPLATE_EN,
        },
    ]
}

/// Build the `/templates` sub-router. Mirrors `graph_router`/`report_router` — a single
/// `.with_state(state)` so it composes under the `/api` nest in `server.rs`.
pub fn templates_router(state: Arc<ApiState>) -> Router {
    Router::new().route("/", get(list_templates_route)).with_state(state)
}

/// `GET /api/templates` — return all prompt templates grouped by stage as a JSON array.
async fn list_templates_route() -> Json<Value> {
    let templates = all_templates();
    Json(serde_json::to_value(&templates).expect("TemplateDescriptor is always serializable"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state() -> Arc<ApiState> {
        Arc::new(ApiState::new(crate::Config::build_test()))
    }

    #[test]
    fn all_templates_cover_five_stages_with_nonempty_content() {
        let templates = all_templates();
        assert!(templates.len() >= 6, "expected >=6 templates, got {}", templates.len());

        // Every template must carry non-empty content.
        for t in &templates {
            assert!(!t.content.trim().is_empty(), "template {} has empty content", t.id);
        }

        // All five pipeline stages must be represented.
        for stage in 1..=5u8 {
            assert!(templates.iter().any(|t| t.stage == stage), "no template covers stage {stage}");
        }
    }

    #[tokio::test]
    async fn endpoint_returns_all_templates_grouped_by_stage() {
        // Route through the real app to also verify the `/api/templates` wiring.
        let app = crate::server::create_app(test_state());
        let resp = app
            .oneshot(Request::builder().uri("/api/templates").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = body.as_array().expect("response is a JSON array");

        assert!(arr.len() >= 6, "expected >=6 templates, got {}", arr.len());

        // Cover all 5 stages, each item has non-empty content.
        let mut stages = std::collections::BTreeSet::new();
        for item in arr {
            let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("");
            assert!(!content.trim().is_empty(), "an item has empty content: {item:?}");
            stages.insert(item.get("stage").and_then(|v| v.as_u64()).unwrap());
        }
        assert_eq!(stages, [1, 2, 3, 4, 5].into_iter().collect());
    }
}
