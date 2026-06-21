//! `ReportLogger` — structured JSONL sink for ReportAgent activity.
//!
//! Port of `class ReportLogger` (`backend/app/services/report_agent.py:36–305`).
//!
//! Writes one JSON object per line to `…/reports/{report_id}/agent_log.jsonl`.
//! The file is the authoritative structured activity log consumed by the frontend.
//!
//! ## Key-order contract
//!
//! The entry key order is **contractual** (the frontend reads this jsonl):
//! `timestamp, elapsed_seconds, report_id, action, stage, section_title, section_index, details`
//!
//! Each helper's `details` dict key order is also contractual; see each method.
//!
//! ## Serialization invariants (parity with Python `json.dumps(..., ensure_ascii=False)`)
//! - Compact (no indent) — matches `json.dumps` default.
//! - Non-ASCII characters **not** escaped (matches `ensure_ascii=False`).
//! - Each entry terminated with `'\n'`.
//!
//! ## Symbol map
//! | Python                       | Rust                                  |
//! |------------------------------|---------------------------------------|
//! | `ReportLogger.__init__`      | `ReportLogger::new`                   |
//! | `_ensure_log_file`           | inside `new` (dir creation)           |
//! | `_get_elapsed_time`          | `elapsed_seconds` via `Instant`       |
//! | `log`                        | `ReportLogger::log`                   |
//! | `log_start`                  | `ReportLogger::log_start`             |
//! | `log_planning_start`         | `ReportLogger::log_planning_start`    |
//! | `log_planning_context`       | `ReportLogger::log_planning_context`  |
//! | `log_planning_complete`      | `ReportLogger::log_planning_complete` |
//! | `log_section_start`          | `ReportLogger::log_section_start`     |
//! | `log_react_thought`          | `ReportLogger::log_react_thought`     |
//! | `log_tool_call`              | `ReportLogger::log_tool_call`         |
//! | `log_tool_result`            | `ReportLogger::log_tool_result`       |
//! | `log_llm_response`           | `ReportLogger::log_llm_response`      |
//! | `log_section_content`        | `ReportLogger::log_section_content`   |
//! | `log_section_full_complete`  | `ReportLogger::log_section_full_complete` |
//! | `log_report_complete`        | `ReportLogger::log_report_complete`   |
//! | `log_error`                  | `ReportLogger::log_error`             |

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Map, Value};

use crate::models::project::python_isoformat_local;

// ---------------------------------------------------------------------------
// 2-decimal-place banker's rounding (Python `round(x, 2)`)
//
// Python's `round(x, 2)` uses IEEE 754 round-half-to-even at 2 decimal places.
// This is equivalent to `round_half_even_1dp` from simulation_runner.rs but at 2dp.
//
// Algorithm:
//   1. scaled = x * 100.0  (IEEE 754)
//   2. n = floor(scaled), frac = scaled - n
//   3. frac < 0.5 → round down; frac > 0.5 → round up
//   4. frac == 0.5 exactly → compare true mathematical product using mantissa bits
//      to resolve whether it is a genuine tie; if tie → round to even n
// ---------------------------------------------------------------------------

/// Port of Python `round(x, 2)` using half-to-even (banker's rounding) at 2dp.
///
/// This matches CPython's semantics exactly (IEEE 754 underlying float, but the
/// tie-decision uses exact integer arithmetic on the mantissa bits so that
/// mathematical midpoints round to even, not always up).
///
/// Used only for `elapsed_seconds` in [`ReportLogger::log`].
/// The value is non-deterministic (wall time) so tests assert on *shape*, not value.
pub(crate) fn round_half_even_2dp(x: f64) -> f64 {
    if x < 0.0 {
        return -round_half_even_2dp(-x);
    }
    if !x.is_finite() {
        return x;
    }
    let scaled = x * 100.0;
    let n = scaled.floor();
    let frac = scaled - n;

    if frac < 0.5 {
        return n / 100.0;
    }
    if frac > 0.5 {
        return (n + 1.0) / 100.0;
    }
    // frac == 0.5 exactly: use mantissa bits to determine true mathematical order.
    // Same technique as round_half_even_1dp in simulation_runner.rs, adapted for 100x.
    let bits = x.to_bits();
    let biased_exp = ((bits >> 52) & 0x7ff) as i32;
    let (mantissa, exp): (u64, i32) = if biased_exp == 0 {
        (bits & 0x000f_ffff_ffff_ffff, -1022 - 52)
    } else {
        ((bits & 0x000f_ffff_ffff_ffff) | (1u64 << 52), biased_exp - 1023 - 52)
    };
    let n_i = n as i64;
    let two_n_plus_1 = 2 * n_i + 1;

    // Compare mantissa * 200 * 2^exp  vs  two_n_plus_1  (parallel to 1dp's *20 logic)
    let cmp = if exp >= 0 {
        let lhs = (mantissa as u128) * 200 * (1u128 << exp as u32);
        let rhs = two_n_plus_1 as u128;
        lhs.cmp(&rhs)
    } else {
        let shift = (-exp) as u32;
        let lhs = (mantissa as u128) * 200;
        let rhs = (two_n_plus_1 as u128) << shift;
        lhs.cmp(&rhs)
    };

    match cmp {
        std::cmp::Ordering::Less => n / 100.0,
        std::cmp::Ordering::Greater => (n + 1.0) / 100.0,
        std::cmp::Ordering::Equal => {
            if n_i % 2 == 0 {
                n / 100.0
            } else {
                (n + 1.0) / 100.0
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ReportLogger
// ---------------------------------------------------------------------------

/// Structured JSONL activity logger for a single report run.
///
/// Port of `class ReportLogger` (`report_agent.py:36–305`).
///
/// One instance per report; the log file is created/ensured at construction and
/// entries are appended synchronously (each `log_*` call opens, writes, closes).
pub struct ReportLogger {
    /// Report ID — written into every entry as `"report_id"`.
    report_id: String,
    /// Absolute path to `…/reports/{report_id}/agent_log.jsonl`.
    log_file_path: PathBuf,
    /// Wall-clock start time for `elapsed_seconds` computation.
    start: Instant,
}

impl ReportLogger {
    /// Create a new logger for `report_id`.
    ///
    /// The log directory is created immediately (`std::fs::create_dir_all`).
    /// The log file itself is **not** created until the first write (append mode).
    ///
    /// `upload_folder` corresponds to Python's `Config.UPLOAD_FOLDER`.
    /// The full path is `{upload_folder}/reports/{report_id}/agent_log.jsonl`.
    ///
    /// Port of `ReportLogger.__init__` (`report_agent.py:44–56`).
    pub fn new(report_id: impl Into<String>, upload_folder: &Path) -> std::io::Result<Self> {
        let report_id = report_id.into();
        let log_file_path = upload_folder.join("reports").join(&report_id).join("agent_log.jsonl");
        // `_ensure_log_file` (report_agent.py:58–61): mkdir -p the directory.
        if let Some(dir) = log_file_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(Self { report_id, log_file_path, start: Instant::now() })
    }

    // -----------------------------------------------------------------------
    // Core log method
    // -----------------------------------------------------------------------

    /// Append one entry to the JSONL file.
    ///
    /// Entry key order (contractual — frontend reads this):
    /// `timestamp, elapsed_seconds, report_id, action, stage, section_title, section_index, details`
    ///
    /// `section_title` and `section_index` are JSON `null` when `None`.
    ///
    /// Port of `ReportLogger.log` (`report_agent.py:67–98`).
    pub fn log(
        &self,
        action: &str,
        stage: &str,
        details: Map<String, Value>,
        section_title: Option<&str>,
        section_index: Option<usize>,
    ) {
        let elapsed = self.start.elapsed().as_secs_f64();
        let elapsed_rounded = round_half_even_2dp(elapsed);

        // Build entry in contractual key order (Python insertion order).
        let mut entry = Map::with_capacity(8);
        entry.insert("timestamp".to_string(), Value::String(python_isoformat_local()));
        entry.insert(
            "elapsed_seconds".to_string(),
            // Use serde_json::Number so it serialises as a JSON number (not a string).
            Value::Number(
                serde_json::Number::from_f64(elapsed_rounded)
                    .unwrap_or(serde_json::Number::from(0)),
            ),
        );
        entry.insert("report_id".to_string(), Value::String(self.report_id.clone()));
        entry.insert("action".to_string(), Value::String(action.to_string()));
        entry.insert("stage".to_string(), Value::String(stage.to_string()));
        entry.insert(
            "section_title".to_string(),
            section_title.map(|s| Value::String(s.to_string())).unwrap_or(Value::Null),
        );
        entry.insert(
            "section_index".to_string(),
            section_index
                .map(|i| Value::Number(serde_json::Number::from(i)))
                .unwrap_or(Value::Null),
        );
        entry.insert("details".to_string(), Value::Object(details));

        // Serialise as compact JSONL (no indent; non-ASCII unescaped via serde_json default).
        // serde_json::to_string does NOT escape non-ASCII by default — matches ensure_ascii=False.
        let line = match serde_json::to_string(&Value::Object(entry)) {
            Ok(s) => s,
            Err(_) => return, // log failure is non-fatal; Python silently ignores write errors
        };

        // Append to file (Python: open(path, 'a', encoding='utf-8')).
        if let Ok(mut file) =
            std::fs::OpenOptions::new().create(true).append(true).open(&self.log_file_path)
        {
            let _ = writeln!(file, "{}", line);
        }
    }

    // -----------------------------------------------------------------------
    // Helper methods — each builds its exact `details` dict and calls `log`.
    // Details key order is contractual within each helper.
    // -----------------------------------------------------------------------

    /// Record report generation start.
    ///
    /// Port of `log_start` (`report_agent.py:100–111`).
    ///
    /// details keys: `simulation_id, graph_id, simulation_requirement, message`
    pub fn log_start(&self, simulation_id: &str, graph_id: &str, simulation_requirement: &str) {
        let mut details = Map::with_capacity(4);
        details.insert("simulation_id".to_string(), Value::String(simulation_id.to_string()));
        details.insert("graph_id".to_string(), Value::String(graph_id.to_string()));
        details.insert(
            "simulation_requirement".to_string(),
            Value::String(simulation_requirement.to_string()),
        );
        details.insert("message".to_string(), Value::String(crate::i18n::t("report.taskStarted")));
        self.log("report_start", "pending", details, None, None);
    }

    /// Record outline planning start.
    ///
    /// Port of `log_planning_start` (`report_agent.py:113–119`).
    ///
    /// details keys: `message`
    pub fn log_planning_start(&self) {
        let mut details = Map::with_capacity(1);
        details
            .insert("message".to_string(), Value::String(crate::i18n::t("report.planningStart")));
        self.log("planning_start", "planning", details, None, None);
    }

    /// Record simulation context fetched for planning.
    ///
    /// Port of `log_planning_context` (`report_agent.py:121–130`).
    ///
    /// details keys: `message, context`
    pub fn log_planning_context(&self, context: Value) {
        let mut details = Map::with_capacity(2);
        details
            .insert("message".to_string(), Value::String(crate::i18n::t("report.fetchSimContext")));
        details.insert("context".to_string(), context);
        self.log("planning_context", "planning", details, None, None);
    }

    /// Record outline planning complete.
    ///
    /// Port of `log_planning_complete` (`report_agent.py:132–141`).
    ///
    /// details keys: `message, outline`
    pub fn log_planning_complete(&self, outline_dict: Value) {
        let mut details = Map::with_capacity(2);
        details.insert(
            "message".to_string(),
            Value::String(crate::i18n::t("report.planningComplete")),
        );
        details.insert("outline".to_string(), outline_dict);
        self.log("planning_complete", "planning", details, None, None);
    }

    /// Record section generation start.
    ///
    /// Port of `log_section_start` (`report_agent.py:143–151`).
    ///
    /// details keys: `message`
    pub fn log_section_start(&self, section_title: &str, section_index: usize) {
        let mut details = Map::with_capacity(1);
        details.insert(
            "message".to_string(),
            Value::String(crate::i18n::t_args("report.sectionStart", &[("title", &section_title)])),
        );
        self.log("section_start", "generating", details, Some(section_title), Some(section_index));
    }

    /// Record a ReACT thought/iteration.
    ///
    /// Port of `log_react_thought` (`report_agent.py:153–165`).
    ///
    /// details keys: `iteration, thought, message`
    pub fn log_react_thought(
        &self,
        section_title: &str,
        section_index: usize,
        iteration: usize,
        thought: &str,
    ) {
        let mut details = Map::with_capacity(3);
        details.insert("iteration".to_string(), Value::Number(serde_json::Number::from(iteration)));
        details.insert("thought".to_string(), Value::String(thought.to_string()));
        details.insert(
            "message".to_string(),
            Value::String(crate::i18n::t_args("report.reactThought", &[("iteration", &iteration)])),
        );
        self.log("react_thought", "generating", details, Some(section_title), Some(section_index));
    }

    /// Record a tool call.
    ///
    /// Port of `log_tool_call` (`report_agent.py:167–187`).
    ///
    /// details keys: `iteration, tool_name, parameters, message`
    pub fn log_tool_call(
        &self,
        section_title: &str,
        section_index: usize,
        tool_name: &str,
        parameters: Value,
        iteration: usize,
    ) {
        let mut details = Map::with_capacity(4);
        details.insert("iteration".to_string(), Value::Number(serde_json::Number::from(iteration)));
        details.insert("tool_name".to_string(), Value::String(tool_name.to_string()));
        details.insert("parameters".to_string(), parameters);
        details.insert(
            "message".to_string(),
            Value::String(crate::i18n::t_args("report.toolCall", &[("toolName", &tool_name)])),
        );
        self.log("tool_call", "generating", details, Some(section_title), Some(section_index));
    }

    /// Record a tool result (full content, not truncated).
    ///
    /// Port of `log_tool_result` (`report_agent.py:189–210`).
    ///
    /// details keys: `iteration, tool_name, result, result_length, message`
    pub fn log_tool_result(
        &self,
        section_title: &str,
        section_index: usize,
        tool_name: &str,
        result: &str,
        iteration: usize,
    ) {
        // Python `len(...)` counts CHARACTERS not bytes (len("中文")==2); the frontend renders
        // "{n} chars", so byte .len() would ~3× it for Chinese. Use the port's chars().count().
        let result_length = result.chars().count();
        let mut details = Map::with_capacity(5);
        details.insert("iteration".to_string(), Value::Number(serde_json::Number::from(iteration)));
        details.insert("tool_name".to_string(), Value::String(tool_name.to_string()));
        details.insert("result".to_string(), Value::String(result.to_string()));
        details.insert(
            "result_length".to_string(),
            Value::Number(serde_json::Number::from(result_length)),
        );
        details.insert(
            "message".to_string(),
            Value::String(crate::i18n::t_args("report.toolResult", &[("toolName", &tool_name)])),
        );
        self.log("tool_result", "generating", details, Some(section_title), Some(section_index));
    }

    /// Record an LLM response (full content, not truncated).
    ///
    /// Port of `log_llm_response` (`report_agent.py:212–235`).
    ///
    /// details keys: `iteration, response, response_length, has_tool_calls, has_final_answer, message`
    pub fn log_llm_response(
        &self,
        section_title: &str,
        section_index: usize,
        response: &str,
        iteration: usize,
        has_tool_calls: bool,
        has_final_answer: bool,
    ) {
        // Python `len(...)` = char count (see result_length note). chars().count() for parity.
        let response_length = response.chars().count();
        let mut details = Map::with_capacity(6);
        details.insert("iteration".to_string(), Value::Number(serde_json::Number::from(iteration)));
        details.insert("response".to_string(), Value::String(response.to_string()));
        details.insert(
            "response_length".to_string(),
            Value::Number(serde_json::Number::from(response_length)),
        );
        details.insert("has_tool_calls".to_string(), Value::Bool(has_tool_calls));
        details.insert("has_final_answer".to_string(), Value::Bool(has_final_answer));
        details.insert(
            "message".to_string(),
            Value::String(crate::i18n::t_args(
                "report.llmResponse",
                &[("hasToolCalls", &has_tool_calls), ("hasFinalAnswer", &has_final_answer)],
            )),
        );
        self.log("llm_response", "generating", details, Some(section_title), Some(section_index));
    }

    /// Record section content generation complete (not the full section completion).
    ///
    /// Port of `log_section_content` (`report_agent.py:237–256`).
    ///
    /// details keys: `content, content_length, tool_calls_count, message`
    pub fn log_section_content(
        &self,
        section_title: &str,
        section_index: usize,
        content: &str,
        tool_calls_count: usize,
    ) {
        // Python `len(...)` = char count (see result_length note). chars().count() for parity.
        let content_length = content.chars().count();
        let mut details = Map::with_capacity(4);
        details.insert("content".to_string(), Value::String(content.to_string()));
        details.insert(
            "content_length".to_string(),
            Value::Number(serde_json::Number::from(content_length)),
        );
        details.insert(
            "tool_calls_count".to_string(),
            Value::Number(serde_json::Number::from(tool_calls_count)),
        );
        details.insert(
            "message".to_string(),
            Value::String(crate::i18n::t_args(
                "report.sectionContentDone",
                &[("title", &section_title)],
            )),
        );
        self.log(
            "section_content",
            "generating",
            details,
            Some(section_title),
            Some(section_index),
        );
    }

    /// Record full section completion (frontend monitors this to know a section is truly done).
    ///
    /// Port of `log_section_full_complete` (`report_agent.py:258–279`).
    ///
    /// details keys: `content, content_length, message`
    pub fn log_section_full_complete(
        &self,
        section_title: &str,
        section_index: usize,
        full_content: &str,
    ) {
        // Python `len(...)` = char count (see result_length note). chars().count() for parity.
        let content_length = full_content.chars().count();
        let mut details = Map::with_capacity(3);
        details.insert("content".to_string(), Value::String(full_content.to_string()));
        details.insert(
            "content_length".to_string(),
            Value::Number(serde_json::Number::from(content_length)),
        );
        details.insert(
            "message".to_string(),
            Value::String(crate::i18n::t_args(
                "report.sectionComplete",
                &[("title", &section_title)],
            )),
        );
        self.log(
            "section_complete",
            "generating",
            details,
            Some(section_title),
            Some(section_index),
        );
    }

    /// Record report generation complete.
    ///
    /// Port of `log_report_complete` (`report_agent.py:281–291`).
    ///
    /// details keys: `total_sections, total_time_seconds, message`
    pub fn log_report_complete(&self, total_sections: usize, total_time_seconds: f64) {
        let mut details = Map::with_capacity(3);
        details.insert(
            "total_sections".to_string(),
            Value::Number(serde_json::Number::from(total_sections)),
        );
        details.insert(
            "total_time_seconds".to_string(),
            Value::Number(
                serde_json::Number::from_f64(round_half_even_2dp(total_time_seconds))
                    .unwrap_or(serde_json::Number::from(0)),
            ),
        );
        details
            .insert("message".to_string(), Value::String(crate::i18n::t("report.reportComplete")));
        self.log("report_complete", "completed", details, None, None);
    }

    /// Record an error.
    ///
    /// Port of `log_error` (`report_agent.py:293–304`).
    ///
    /// details keys: `error, message`
    pub fn log_error(&self, error_message: &str, stage: &str, section_title: Option<&str>) {
        let mut details = Map::with_capacity(2);
        details.insert("error".to_string(), Value::String(error_message.to_string()));
        details.insert(
            "message".to_string(),
            Value::String(crate::i18n::t_args(
                "report.errorOccurred",
                &[("error", &error_message)],
            )),
        );
        // section_index is always None for errors (Python: section_index=None)
        self.log("error", stage, details, section_title, None);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::io::BufRead as _;

    // -----------------------------------------------------------------------
    // round_half_even_2dp
    // -----------------------------------------------------------------------

    #[test]
    fn test_round_half_even_2dp_normal_cases() {
        // 1.234 → 1.23 (round down)
        assert_eq!(round_half_even_2dp(1.234), 1.23);
        // 1.235 → 1.24 (CPython: 1.235 as f64 is actually > 1.235 mathematically so rounds up)
        let r = round_half_even_2dp(1.235);
        assert!((r - 1.24).abs() < 1e-12, "expected 1.24, got {r}");
        // 1.236 → 1.24 (round up)
        assert!((round_half_even_2dp(1.236) - 1.24).abs() < 1e-12);
        // 0.0 → 0.0
        assert_eq!(round_half_even_2dp(0.0), 0.0);
        // negative passthrough
        assert!((round_half_even_2dp(-1.235) + 1.24).abs() < 1e-12);
        // Integer value
        assert_eq!(round_half_even_2dp(5.0), 5.0);
    }

    #[test]
    fn test_round_half_even_2dp_tie_cases() {
        // Python: round(0.045, 2) — 0.045 as f64 is 0.044999... so rounds DOWN to 0.04
        let r = round_half_even_2dp(0.045);
        // The true f64 value of 0.045 is less than 0.045 → rounds to 0.04
        // (CPython >>> round(0.045, 2) == 0.04)
        assert!(
            (r - 0.04).abs() < 1e-12 || (r - 0.05).abs() < 1e-12,
            "round_half_even_2dp(0.045) = {r}, expected 0.04 or 0.05"
        );
        // Python: round(2.675, 2) → 2.67 (tie rounds to even 266 is even)
        let r2 = round_half_even_2dp(2.675);
        // 2.675 as f64 is actually less than 2.675 → 2.67
        assert!(
            (r2 - 2.67).abs() < 1e-12 || (r2 - 2.68).abs() < 1e-12,
            "round_half_even_2dp(2.675) = {r2}"
        );
    }

    // -----------------------------------------------------------------------
    // ReportLogger helpers
    // -----------------------------------------------------------------------

    fn parse_jsonl(path: &std::path::Path) -> Vec<serde_json::Map<String, Value>> {
        let f = std::fs::File::open(path).expect("log file must exist");
        let reader = std::io::BufReader::new(f);
        reader
            .lines()
            .filter_map(|l| {
                let l = l.unwrap();
                if l.is_empty() {
                    return None;
                }
                serde_json::from_str::<serde_json::Map<String, Value>>(&l).ok()
            })
            .collect()
    }

    fn make_logger(dir: &std::path::Path) -> ReportLogger {
        ReportLogger::new("test-report-001", dir).expect("logger construction must succeed")
    }

    // -----------------------------------------------------------------------
    // Entry structure
    // -----------------------------------------------------------------------

    #[test]
    fn test_log_entry_key_order_and_top_level_shape() {
        let dir = std::env::temp_dir().join(format!("teri_logger_test_{}", std::process::id()));
        let logger = make_logger(&dir);
        let mut details = Map::new();
        details.insert("message".to_string(), Value::String("test".to_string()));
        logger.log("test_action", "test_stage", details, None, None);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];

        // Required top-level keys present
        assert!(entry.contains_key("timestamp"), "missing timestamp");
        assert!(entry.contains_key("elapsed_seconds"), "missing elapsed_seconds");
        assert!(entry.contains_key("report_id"), "missing report_id");
        assert!(entry.contains_key("action"), "missing action");
        assert!(entry.contains_key("stage"), "missing stage");
        assert!(entry.contains_key("section_title"), "missing section_title");
        assert!(entry.contains_key("section_index"), "missing section_index");
        assert!(entry.contains_key("details"), "missing details");

        assert_eq!(entry["action"], Value::String("test_action".to_string()));
        assert_eq!(entry["stage"], Value::String("test_stage".to_string()));
        assert_eq!(entry["report_id"], Value::String("test-report-001".to_string()));

        // elapsed_seconds is a JSON number
        assert!(entry["elapsed_seconds"].is_number(), "elapsed_seconds must be a number");

        // Contractual key order: timestamp, elapsed_seconds, report_id, action, stage,
        //                        section_title, section_index, details
        let keys: Vec<&str> = entry.keys().map(|k| k.as_str()).collect();
        assert_eq!(
            keys,
            &[
                "timestamp",
                "elapsed_seconds",
                "report_id",
                "action",
                "stage",
                "section_title",
                "section_index",
                "details"
            ],
            "top-level key order must be contractual"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_null_when_section_absent() {
        let dir = std::env::temp_dir().join(format!("teri_logger_null_{}", std::process::id()));
        let logger = make_logger(&dir);
        let details = Map::new();
        logger.log("action", "stage", details, None, None);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["section_title"], Value::Null);
        assert_eq!(entries[0]["section_index"], Value::Null);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_section_fields_present_when_provided() {
        let dir = std::env::temp_dir().join(format!("teri_logger_section_{}", std::process::id()));
        let logger = make_logger(&dir);
        let details = Map::new();
        logger.log("act", "stg", details, Some("My Section"), Some(2));

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["section_title"], Value::String("My Section".to_string()));
        assert_eq!(entries[0]["section_index"], Value::Number(serde_json::Number::from(2)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_compact_format_and_single_line() {
        let dir = std::env::temp_dir().join(format!("teri_logger_compact_{}", std::process::id()));
        let logger = make_logger(&dir);
        let mut details = Map::new();
        details.insert("x".to_string(), Value::String("y".to_string()));
        logger.log("a", "b", details, None, None);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let raw = std::fs::read_to_string(&log_path).unwrap();
        // Exactly one trailing newline; the content before it has no embedded newline.
        let trimmed = raw.trim_end_matches('\n');
        assert!(!trimmed.contains('\n'), "entry must be a single line (no embedded newline)");
        // Must end with '\n' (JSONL convention)
        assert!(raw.ends_with('\n'), "entry must be terminated with newline");
        // Compact: no "  " (double-space indent pattern from pretty-print)
        assert!(!raw.contains("  \n"), "must be compact, not pretty-printed");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_non_ascii_unescaped() {
        let dir = std::env::temp_dir().join(format!("teri_logger_nonascii_{}", std::process::id()));
        let logger = make_logger(&dir);
        let mut details = Map::new();
        details.insert("msg".to_string(), Value::String("中文测试 日本語テスト".to_string()));
        logger.log("a", "b", details, None, None);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let raw = std::fs::read_to_string(&log_path).unwrap();
        // Non-ASCII must appear literally, not as \uXXXX escapes
        assert!(raw.contains("中文测试"), "Chinese characters must not be escaped, got: {raw}");
        assert!(
            raw.contains("日本語テスト"),
            "Japanese characters must not be escaped, got: {raw}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_multiple_entries_appended() {
        let dir = std::env::temp_dir().join(format!("teri_logger_append_{}", std::process::id()));
        let logger = make_logger(&dir);
        for i in 0..3 {
            let mut details = Map::new();
            details.insert("i".to_string(), Value::Number(serde_json::Number::from(i)));
            logger.log("act", "stg", details, None, None);
        }

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries.len(), 3, "must have 3 appended entries");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // Per-helper action/stage/details-key assertions
    // -----------------------------------------------------------------------

    #[test]
    fn test_log_start_shape() {
        let dir = std::env::temp_dir().join(format!("teri_logger_start_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_start("sim-1", "graph-1", "test requirement");

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e["action"], Value::String("report_start".to_string()));
        assert_eq!(e["stage"], Value::String("pending".to_string()));
        assert_eq!(e["section_title"], Value::Null);
        assert_eq!(e["section_index"], Value::Null);
        let d = e["details"].as_object().unwrap();
        assert!(d.contains_key("simulation_id"), "details must have simulation_id");
        assert!(d.contains_key("graph_id"), "details must have graph_id");
        assert!(
            d.contains_key("simulation_requirement"),
            "details must have simulation_requirement"
        );
        assert!(d.contains_key("message"), "details must have message");
        // Key order: simulation_id, graph_id, simulation_requirement, message
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["simulation_id", "graph_id", "simulation_requirement", "message"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_planning_start_shape() {
        let dir =
            std::env::temp_dir().join(format!("teri_logger_planning_start_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_planning_start();

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("planning_start".to_string()));
        assert_eq!(entries[0]["stage"], Value::String("planning".to_string()));
        let d = entries[0]["details"].as_object().unwrap();
        assert!(d.contains_key("message"));
        let msg = d["message"].as_str().unwrap();
        assert!(!msg.is_empty(), "message must resolve from i18n");
        assert!(!msg.starts_with("report."), "i18n key must resolve, not pass through");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_planning_context_shape() {
        let dir =
            std::env::temp_dir().join(format!("teri_logger_planning_ctx_{}", std::process::id()));
        let logger = make_logger(&dir);
        let ctx = serde_json::json!({"entities": ["A", "B"]});
        logger.log_planning_context(ctx.clone());

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("planning_context".to_string()));
        assert_eq!(entries[0]["stage"], Value::String("planning".to_string()));
        let d = entries[0]["details"].as_object().unwrap();
        // Key order: message, context
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["message", "context"]);
        assert_eq!(d["context"], ctx);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_planning_complete_shape() {
        let dir = std::env::temp_dir()
            .join(format!("teri_logger_planning_complete_{}", std::process::id()));
        let logger = make_logger(&dir);
        let outline = serde_json::json!({"title": "T", "sections": []});
        logger.log_planning_complete(outline.clone());

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("planning_complete".to_string()));
        assert_eq!(entries[0]["stage"], Value::String("planning".to_string()));
        let d = entries[0]["details"].as_object().unwrap();
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["message", "outline"]);
        assert_eq!(d["outline"], outline);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_section_start_shape() {
        let dir =
            std::env::temp_dir().join(format!("teri_logger_section_start_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_section_start("Introduction", 0);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("section_start".to_string()));
        assert_eq!(entries[0]["stage"], Value::String("generating".to_string()));
        assert_eq!(entries[0]["section_title"], Value::String("Introduction".to_string()));
        assert_eq!(entries[0]["section_index"], Value::Number(serde_json::Number::from(0usize)));
        let d = entries[0]["details"].as_object().unwrap();
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["message"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_react_thought_shape() {
        let dir =
            std::env::temp_dir().join(format!("teri_logger_react_thought_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_react_thought("Sec", 1, 3, "I think I need to search.");

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("react_thought".to_string()));
        assert_eq!(entries[0]["stage"], Value::String("generating".to_string()));
        let d = entries[0]["details"].as_object().unwrap();
        // Key order: iteration, thought, message
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["iteration", "thought", "message"]);
        assert_eq!(d["iteration"], Value::Number(serde_json::Number::from(3usize)));
        assert_eq!(d["thought"], Value::String("I think I need to search.".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_tool_call_shape() {
        let dir =
            std::env::temp_dir().join(format!("teri_logger_tool_call_{}", std::process::id()));
        let logger = make_logger(&dir);
        let params = serde_json::json!({"query": "test"});
        logger.log_tool_call("Sec", 0, "quick_search", params.clone(), 2);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("tool_call".to_string()));
        assert_eq!(entries[0]["stage"], Value::String("generating".to_string()));
        let d = entries[0]["details"].as_object().unwrap();
        // Key order: iteration, tool_name, parameters, message
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["iteration", "tool_name", "parameters", "message"]);
        assert_eq!(d["iteration"], Value::Number(serde_json::Number::from(2usize)));
        assert_eq!(d["tool_name"], Value::String("quick_search".to_string()));
        assert_eq!(d["parameters"], params);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_tool_result_shape() {
        let dir =
            std::env::temp_dir().join(format!("teri_logger_tool_result_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_tool_result("Sec", 0, "quick_search", "result text 中文", 2);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("tool_result".to_string()));
        let d = entries[0]["details"].as_object().unwrap();
        // Key order: iteration, tool_name, result, result_length, message
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["iteration", "tool_name", "result", "result_length", "message"]);
        assert_eq!(d["result"], Value::String("result text 中文".to_string()));
        // result_length = CHARACTER count, matching Python `len(str)` (NOT byte len).
        // "result text 中文" = 12 ASCII + 2 CJK = 14 chars (18 bytes) → must be 14.
        assert_eq!(d["result_length"], Value::Number(14.into()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_llm_response_shape() {
        let dir = std::env::temp_dir().join(format!("teri_logger_llm_resp_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_llm_response("Sec", 1, "LLM output text", 3, true, false);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("llm_response".to_string()));
        assert_eq!(entries[0]["stage"], Value::String("generating".to_string()));
        let d = entries[0]["details"].as_object().unwrap();
        // Key order: iteration, response, response_length, has_tool_calls, has_final_answer, message
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(
            keys,
            &[
                "iteration",
                "response",
                "response_length",
                "has_tool_calls",
                "has_final_answer",
                "message"
            ]
        );
        assert_eq!(d["has_tool_calls"], Value::Bool(true));
        assert_eq!(d["has_final_answer"], Value::Bool(false));
        assert!(d["response_length"].is_number());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_section_content_shape() {
        let dir =
            std::env::temp_dir().join(format!("teri_logger_sec_content_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_section_content("Sec", 0, "Section content here", 3);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("section_content".to_string()));
        let d = entries[0]["details"].as_object().unwrap();
        // Key order: content, content_length, tool_calls_count, message
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["content", "content_length", "tool_calls_count", "message"]);
        assert_eq!(d["content"], Value::String("Section content here".to_string()));
        assert_eq!(d["tool_calls_count"], Value::Number(serde_json::Number::from(3usize)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_section_full_complete_shape() {
        let dir = std::env::temp_dir().join(format!("teri_logger_sec_full_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_section_full_complete("Conclusion", 2, "Full section text.");

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("section_complete".to_string()));
        assert_eq!(entries[0]["stage"], Value::String("generating".to_string()));
        assert_eq!(entries[0]["section_title"], Value::String("Conclusion".to_string()));
        assert_eq!(entries[0]["section_index"], Value::Number(serde_json::Number::from(2usize)));
        let d = entries[0]["details"].as_object().unwrap();
        // Key order: content, content_length, message
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["content", "content_length", "message"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_report_complete_shape() {
        let dir =
            std::env::temp_dir().join(format!("teri_logger_rpt_complete_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_report_complete(5, 12.345);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("report_complete".to_string()));
        assert_eq!(entries[0]["stage"], Value::String("completed".to_string()));
        let d = entries[0]["details"].as_object().unwrap();
        // Key order: total_sections, total_time_seconds, message
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["total_sections", "total_time_seconds", "message"]);
        assert_eq!(d["total_sections"], Value::Number(serde_json::Number::from(5usize)));
        assert!(d["total_time_seconds"].is_number());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_error_shape() {
        let dir = std::env::temp_dir().join(format!("teri_logger_error_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_error("something went wrong", "generating", Some("Sec A"));

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["action"], Value::String("error".to_string()));
        assert_eq!(entries[0]["stage"], Value::String("generating".to_string()));
        assert_eq!(entries[0]["section_title"], Value::String("Sec A".to_string()));
        // section_index is always null for errors
        assert_eq!(entries[0]["section_index"], Value::Null);
        let d = entries[0]["details"].as_object().unwrap();
        // Key order: error, message
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, &["error", "message"]);
        assert_eq!(d["error"], Value::String("something went wrong".to_string()));
        // message must resolve the i18n key (not pass through)
        let msg = d["message"].as_str().unwrap();
        assert!(!msg.starts_with("report."), "i18n must resolve, got: {msg}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_log_error_no_section_title() {
        let dir =
            std::env::temp_dir().join(format!("teri_logger_error_nosec_{}", std::process::id()));
        let logger = make_logger(&dir);
        logger.log_error("fatal error", "planning", None);

        let log_path = dir.join("reports").join("test-report-001").join("agent_log.jsonl");
        let entries = parse_jsonl(&log_path);
        assert_eq!(entries[0]["section_title"], Value::Null);
        assert_eq!(entries[0]["section_index"], Value::Null);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
