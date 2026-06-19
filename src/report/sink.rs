//! `ReportSink` — the progress/SSE surface for `generate_report`.
//!
//! Port of `progress_callback(stage, progress, message)` threading in
//! `backend/app/services/report_agent.py:1532-1765`.
//!
//! ## Design (h1, from architect finding u024-h-generate-report.md §1)
//!
//! `generate_report` emits [`ReportEvent`]s to a `&mut dyn ReportSink` at every
//! progress milestone.  Fan-out (jsonl + SSE + console) is the sink implementation's
//! concern; the orchestration is sink-agnostic.
//!
//! ## Concrete sinks
//!
//! | Sink | Where defined | Used by |
//! |------|---------------|---------|
//! | [`NullSink`] | here | parity tests (zero I/O) |
//! | `ChannelSink` / `SseSink` | U-027 (future) | HTTP SSE route |
//!
//! ## Serde contract
//!
//! [`ReportStage`] serializes to the **same lowercase strings** as Python's
//! `ReportStatus` enum values (`"pending"`, `"planning"`, `"generating"`,
//! `"completed"`, `"failed"`).  This is load-bearing: `progress.json` writes the
//! stage as a status string; the frontend reads it verbatim.

use serde::Serialize;

// ────────────────────────────────────────────────────────────────────────────
// ReportStage
// ────────────────────────────────────────────────────────────────────────────

/// Lifecycle stage of a `generate_report` run.
///
/// Serializes to the same lowercase status strings Python writes:
/// `"pending"`, `"planning"`, `"generating"`, `"completed"`, `"failed"`.
///
/// Mirrors `ReportStatus` in `src/report/mod.rs` (the at-rest `meta.json` enum)
/// but is intentionally separate — `ReportStage` is the **in-flight** stage
/// emitted by `generate_report`; `ReportStatus` is the persisted field on
/// `Report`.  They share the same wire values so `update_progress` can write
/// `stage.to_status_str()` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportStage {
    Pending,
    Planning,
    Generating,
    Completed,
    Failed,
}

impl ReportStage {
    /// Return the lowercase string written to `progress.json` `"status"` field.
    ///
    /// Matches `ReportStatus` wire values so `manager.update_progress(…, stage.to_status_str(), …)`
    /// writes the correct string without an extra conversion layer.
    pub fn to_status_str(self) -> &'static str {
        match self {
            ReportStage::Pending => "pending",
            ReportStage::Planning => "planning",
            ReportStage::Generating => "generating",
            ReportStage::Completed => "completed",
            ReportStage::Failed => "failed",
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// ReportEvent
// ────────────────────────────────────────────────────────────────────────────

/// One progress/lifecycle event emitted by `generate_report`.
///
/// Mirrors Python's `progress_callback(stage, progress, message)` plus the
/// structured fields that the jsonl and SSE consumers need.
///
/// Fields match Python exactly:
/// - `progress` is `i32` (not `u32`) because the failed path writes `-1`
///   (`report_agent.py:1753`).
/// - `section_content` is carried so the SSE sink can stream each section's
///   markdown the moment it is generated (§3 save-immediately pattern).
#[derive(Debug, Clone, Serialize)]
pub struct ReportEvent {
    /// Current lifecycle stage (lowercase on the wire).
    pub stage: ReportStage,
    /// 0–100 on the happy path; **-1 on the failed path** (Python: `progress: int`).
    pub progress: i32,
    /// i18n-resolved progress message (same as `progress_callback` `message` arg).
    pub message: String,
    /// Section title when this event is about a specific section, else `None`.
    pub section_title: Option<String>,
    /// 0-based section index when this event is about a specific section, else `None`.
    pub section_index: Option<usize>,
    /// Freshly-generated section markdown on a `SectionComplete` event, else `None`.
    ///
    /// Carries the section payload so U-027 can stream it live (§3 save-immediately).
    pub section_content: Option<String>,
    /// The report id this event belongs to.
    pub report_id: String,
}

// ────────────────────────────────────────────────────────────────────────────
// ReportSink trait
// ────────────────────────────────────────────────────────────────────────────

/// A consumer of report progress events.
///
/// `generate_report` emits to ONE sink; fan-out (jsonl + SSE + console) is
/// the sink implementation's concern.
///
/// `Send` is required so the sink can be held across `.await` points in the
/// async orchestration.
pub trait ReportSink: Send {
    /// Receive one progress event.  Implementations MUST NOT panic; any I/O
    /// errors should be logged internally and swallowed (progress delivery is
    /// best-effort, not load-bearing for the report pipeline).
    fn event(&mut self, ev: &ReportEvent);
}

// ────────────────────────────────────────────────────────────────────────────
// NullSink — the zero-I/O parity-test default
// ────────────────────────────────────────────────────────────────────────────

/// A `ReportSink` that discards every event.
///
/// Used as the default sink in parity tests so `generate_report` can be
/// exercised without spinning up a real SSE channel or file sink.
pub struct NullSink;

impl ReportSink for NullSink {
    #[inline]
    fn event(&mut self, _ev: &ReportEvent) {}
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    // ── ReportStage serde ─────────────────────────────────────────────────

    #[test]
    fn test_report_stage_serde_lowercase() {
        // Each variant must produce the exact lowercase Python status string.
        let cases = [
            (ReportStage::Pending, "\"pending\""),
            (ReportStage::Planning, "\"planning\""),
            (ReportStage::Generating, "\"generating\""),
            (ReportStage::Completed, "\"completed\""),
            (ReportStage::Failed, "\"failed\""),
        ];
        for (stage, expected_json) in cases {
            let json = serde_json::to_string(&stage).expect("serialize stage");
            assert_eq!(json, expected_json, "stage {:?} serde mismatch", stage);
        }
    }

    #[test]
    fn test_report_stage_to_status_str() {
        assert_eq!(ReportStage::Pending.to_status_str(), "pending");
        assert_eq!(ReportStage::Planning.to_status_str(), "planning");
        assert_eq!(ReportStage::Generating.to_status_str(), "generating");
        assert_eq!(ReportStage::Completed.to_status_str(), "completed");
        assert_eq!(ReportStage::Failed.to_status_str(), "failed");
    }

    // ── ReportEvent serde ─────────────────────────────────────────────────

    #[test]
    fn test_report_event_serde_fields() {
        let ev = ReportEvent {
            stage: ReportStage::Generating,
            progress: 50,
            message: "Half done".to_string(),
            section_title: Some("Introduction".to_string()),
            section_index: Some(0),
            section_content: Some("# Intro\n\nContent.".to_string()),
            report_id: "report_abc123def456".to_string(),
        };
        let json = serde_json::to_string(&ev).expect("serialize event");
        let v: Value = serde_json::from_str(&json).expect("deserialize event");

        assert_eq!(v["stage"].as_str(), Some("generating"));
        assert_eq!(v["progress"].as_i64(), Some(50));
        assert_eq!(v["message"].as_str(), Some("Half done"));
        assert_eq!(v["section_title"].as_str(), Some("Introduction"));
        assert_eq!(v["section_index"].as_u64(), Some(0));
        assert_eq!(v["section_content"].as_str(), Some("# Intro\n\nContent."));
        assert_eq!(v["report_id"].as_str(), Some("report_abc123def456"));
    }

    #[test]
    fn test_report_event_negative_progress_failed_path() {
        // The Python failed path writes progress=-1 (report_agent.py:1753).
        // ReportEvent must accept and serialize -1 without truncation.
        let ev = ReportEvent {
            stage: ReportStage::Failed,
            progress: -1,
            message: "Generation failed".to_string(),
            section_title: None,
            section_index: None,
            section_content: None,
            report_id: "report_xyz".to_string(),
        };
        let json = serde_json::to_string(&ev).expect("serialize failed event");
        let v: Value = serde_json::from_str(&json).expect("deserialize failed event");
        assert_eq!(v["progress"].as_i64(), Some(-1), "progress -1 must round-trip");
        assert_eq!(v["stage"].as_str(), Some("failed"));
        assert!(v["section_title"].is_null());
        assert!(v["section_content"].is_null());
    }

    #[test]
    fn test_report_event_none_optional_fields_serialize_null() {
        let ev = ReportEvent {
            stage: ReportStage::Completed,
            progress: 100,
            message: "Done".to_string(),
            section_title: None,
            section_index: None,
            section_content: None,
            report_id: "report_done".to_string(),
        };
        let v: Value = serde_json::to_value(&ev).expect("to_value");
        // Optional fields are present as null (not omitted) — matches Python dict serialization.
        assert!(v["section_title"].is_null());
        assert!(v["section_index"].is_null());
        assert!(v["section_content"].is_null());
    }

    // ── NullSink ──────────────────────────────────────────────────────────

    #[test]
    fn test_null_sink_is_no_op() {
        let mut sink = NullSink;
        let ev = ReportEvent {
            stage: ReportStage::Pending,
            progress: 0,
            message: "Starting".to_string(),
            section_title: None,
            section_index: None,
            section_content: None,
            report_id: "rep_test".to_string(),
        };
        // Must not panic; calling through trait object exercises dyn dispatch.
        let sink_dyn: &mut dyn ReportSink = &mut sink;
        sink_dyn.event(&ev);
    }

    #[test]
    fn test_null_sink_multiple_events() {
        let mut sink = NullSink;
        for i in 0..5_i32 {
            let ev = ReportEvent {
                stage: ReportStage::Generating,
                progress: i * 20,
                message: format!("Step {}", i),
                section_title: None,
                section_index: Some(i as usize),
                section_content: None,
                report_id: "rep_multi".to_string(),
            };
            sink.event(&ev);
        }
        // Reaching here means NullSink handled all events without panic or I/O.
    }

    // ── ReportStage round-trip (all 5 values) ─────────────────────────────

    #[test]
    fn test_report_stage_all_five_variants_round_trip() {
        // Every variant survives a serde_json round-trip via Value.
        let all = [
            ReportStage::Pending,
            ReportStage::Planning,
            ReportStage::Generating,
            ReportStage::Completed,
            ReportStage::Failed,
        ];
        let expected_strings = ["pending", "planning", "generating", "completed", "failed"];
        for (stage, expected) in all.iter().zip(expected_strings.iter()) {
            let v = serde_json::to_value(stage).expect("to_value");
            assert_eq!(v.as_str(), Some(*expected), "stage {:?} round-trip", stage);
        }
    }
}
