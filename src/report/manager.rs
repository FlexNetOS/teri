//! ReportManager — persistence, assembly, and heading normalization.
//!
//! Port of `class ReportManager` in `backend/app/services/report_agent.py` lines 1884–2572.
//!
//! # Folder layout (per-report)
//! ```text
//! {upload_folder}/reports/
//!   {report_id}/
//!     meta.json          — Report fields (to_dict serialization)
//!     outline.json       — ReportOutline (to_dict)
//!     progress.json      — generation progress
//!     section_01.md      — section 1 (1-indexed, 2-digit zero-padded)
//!     section_02.md      — section 2
//!     …
//!     full_report.md     — assembled complete report
//!     agent_log.jsonl    — newline-delimited JSON log (one entry per line)
//!     console_log.txt    — plain text console log lines
//!
//! # Back-compat (old flat-file format)
//! Old format stored `{report_id}.json` and `{report_id}.md` directly under `reports/`.
//! `get_report`, `get_report_by_simulation`, `list_reports`, and `delete_report` all
//! handle that fallback path, matching Python's observable behavior.

use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::Value;

use crate::report::{Report, ReportOutline, ReportSection, ReportStatus};

// ────────────────────────────────────────────────────────────────────────────
// ReportManager
// ────────────────────────────────────────────────────────────────────────────

/// Manages report persistence, retrieval, and assembly.
///
/// Port of `ReportManager` (`report_agent.py:1884`).
///
/// # Construction
/// ```no_run
/// use teri::report::manager::ReportManager;
/// let mgr = ReportManager::new("./uploads");
/// ```
///
/// `upload_folder` is the value of the `UPLOAD_FOLDER` env var (teri config).
/// Python: `REPORTS_DIR = os.path.join(Config.UPLOAD_FOLDER, 'reports')`.
/// teri: caller-constructed per DECISION-11 ("caller-constructs handles").
pub struct ReportManager {
    /// Equivalent to Python's `REPORTS_DIR`.
    pub reports_dir: PathBuf,
}

#[allow(clippy::collapsible_if)]
impl ReportManager {
    /// Create a manager rooted at `{upload_folder}/reports`.
    pub fn new(upload_folder: impl AsRef<Path>) -> Self {
        Self { reports_dir: upload_folder.as_ref().join("reports") }
    }

    /// Return the `upload_folder` root (the **parent** of `reports_dir`).
    ///
    /// `generate_report` (h2) needs `upload_folder` to construct `ReportLogger::new(id, folder)`
    /// and `ReportConsoleLogger::new(id, folder)` — both take `upload_folder`, not `reports_dir`.
    /// Exposing this accessor avoids threading a second path argument through `generate_report`.
    ///
    /// Returns `None` only if `reports_dir` has no parent (impossible in practice).
    pub fn upload_folder(&self) -> Option<&Path> {
        self.reports_dir.parent()
    }

    // ── Path helpers ────────────────────────────────────────────────────────

    /// `{reports_dir}/{report_id}/`
    fn get_report_folder(&self, report_id: &str) -> PathBuf {
        self.reports_dir.join(report_id)
    }

    /// `{reports_dir}/{report_id}/meta.json`
    fn get_report_path(&self, report_id: &str) -> PathBuf {
        self.get_report_folder(report_id).join("meta.json")
    }

    /// `{reports_dir}/{report_id}/full_report.md`
    fn get_report_markdown_path(&self, report_id: &str) -> PathBuf {
        self.get_report_folder(report_id).join("full_report.md")
    }

    /// `{reports_dir}/{report_id}/outline.json`
    fn get_outline_path(&self, report_id: &str) -> PathBuf {
        self.get_report_folder(report_id).join("outline.json")
    }

    /// `{reports_dir}/{report_id}/progress.json`
    fn get_progress_path(&self, report_id: &str) -> PathBuf {
        self.get_report_folder(report_id).join("progress.json")
    }

    /// `{reports_dir}/{report_id}/section_{NN:02}.md`  (1-indexed, 2-digit)
    fn get_section_path(&self, report_id: &str, section_index: usize) -> PathBuf {
        self.get_report_folder(report_id)
            .join(format!("section_{:02}.md", section_index))
    }

    /// `{reports_dir}/{report_id}/agent_log.jsonl`
    fn get_agent_log_path(&self, report_id: &str) -> PathBuf {
        self.get_report_folder(report_id).join("agent_log.jsonl")
    }

    /// `{reports_dir}/{report_id}/console_log.txt`
    fn get_console_log_path(&self, report_id: &str) -> PathBuf {
        self.get_report_folder(report_id).join("console_log.txt")
    }

    // ── Directory helpers ───────────────────────────────────────────────────

    /// Ensure `reports_dir` exists (mkdir -p).
    fn ensure_reports_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.reports_dir)
    }

    /// Ensure `reports_dir/{report_id}/` exists and return the path.
    ///
    /// Made `pub` so `generate_report` (h2) can call it explicitly to mirror
    /// Python's `ReportManager._ensure_report_folder(report_id)` first-call before
    /// any `update_progress`/`save_report` writes.  Zero blast radius — internal impl.
    pub fn ensure_report_folder(&self, report_id: &str) -> io::Result<PathBuf> {
        let folder = self.get_report_folder(report_id);
        fs::create_dir_all(&folder)?;
        Ok(folder)
    }

    // ── Console log ─────────────────────────────────────────────────────────

    /// Read console log lines with optional `from_line` offset.
    ///
    /// Port of `ReportManager.get_console_log` (`report_agent.py:1957`).
    ///
    /// Returns:
    /// ```json
    /// { "logs": [...], "total_lines": N, "from_line": K, "has_more": false }
    /// ```
    ///
    /// If the file does not exist, returns the dict with all-zero/empty values.
    pub fn get_console_log(
        &self,
        report_id: &str,
        from_line: usize,
    ) -> serde_json::Map<String, Value> {
        let log_path = self.get_console_log_path(report_id);

        if !log_path.exists() {
            let mut m = serde_json::Map::new();
            m.insert("logs".into(), Value::Array(vec![]));
            m.insert("total_lines".into(), Value::Number(0.into()));
            m.insert("from_line".into(), Value::Number(0.into()));
            m.insert("has_more".into(), Value::Bool(false));
            return m;
        }

        let file = match fs::File::open(&log_path) {
            Ok(f) => f,
            Err(_) => {
                let mut m = serde_json::Map::new();
                m.insert("logs".into(), Value::Array(vec![]));
                m.insert("total_lines".into(), Value::Number(0.into()));
                m.insert("from_line".into(), Value::Number(from_line.into()));
                m.insert("has_more".into(), Value::Bool(false));
                return m;
            }
        };

        let reader = io::BufReader::new(file);
        let mut logs: Vec<Value> = Vec::new();
        let mut total_lines: usize = 0;

        for (i, line) in reader.lines().enumerate() {
            total_lines = i + 1;
            // BufReader.lines() already strips the trailing newline, but we trim
            // for exact parity with Python `line.rstrip('\n\r')`.
            if i >= from_line {
                if let Ok(l) = line {
                    logs.push(Value::String(l.trim_end_matches(['\n', '\r']).to_string()));
                }
            }
        }

        let mut m = serde_json::Map::new();
        m.insert("logs".into(), Value::Array(logs));
        m.insert("total_lines".into(), Value::Number(total_lines.into()));
        m.insert("from_line".into(), Value::Number(from_line.into()));
        m.insert("has_more".into(), Value::Bool(false));
        m
    }

    /// Return all console log lines as a `Vec<String>`.
    ///
    /// Port of `ReportManager.get_console_log_stream` (`report_agent.py:2004`).
    pub fn get_console_log_stream(&self, report_id: &str) -> Vec<String> {
        let result = self.get_console_log(report_id, 0);
        result
            .get("logs")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default()
    }

    // ── Agent log ───────────────────────────────────────────────────────────

    /// Read structured agent log entries with optional `from_line` offset.
    ///
    /// Port of `ReportManager.get_agent_log` (`report_agent.py:2018`).
    ///
    /// Each line of `agent_log.jsonl` is parsed as JSON; lines that fail to
    /// parse are silently skipped (matching Python's `except json.JSONDecodeError: continue`).
    ///
    /// Returns:
    /// ```json
    /// { "logs": [...], "total_lines": N, "from_line": K, "has_more": false }
    /// ```
    pub fn get_agent_log(
        &self,
        report_id: &str,
        from_line: usize,
    ) -> serde_json::Map<String, Value> {
        let log_path = self.get_agent_log_path(report_id);

        if !log_path.exists() {
            let mut m = serde_json::Map::new();
            m.insert("logs".into(), Value::Array(vec![]));
            m.insert("total_lines".into(), Value::Number(0.into()));
            m.insert("from_line".into(), Value::Number(0.into()));
            m.insert("has_more".into(), Value::Bool(false));
            return m;
        }

        let file = match fs::File::open(&log_path) {
            Ok(f) => f,
            Err(_) => {
                let mut m = serde_json::Map::new();
                m.insert("logs".into(), Value::Array(vec![]));
                m.insert("total_lines".into(), Value::Number(0.into()));
                m.insert("from_line".into(), Value::Number(from_line.into()));
                m.insert("has_more".into(), Value::Bool(false));
                return m;
            }
        };

        let reader = io::BufReader::new(file);
        let mut logs: Vec<Value> = Vec::new();
        let mut total_lines: usize = 0;

        for (i, line) in reader.lines().enumerate() {
            total_lines = i + 1;
            if i >= from_line {
                if let Ok(l) = line {
                    let trimmed = l.trim().to_string();
                    // Skip lines that fail to parse (Python: `except json.JSONDecodeError: continue`)
                    if let Ok(entry) = serde_json::from_str::<Value>(&trimmed) {
                        logs.push(entry);
                    }
                }
            }
        }

        let mut m = serde_json::Map::new();
        m.insert("logs".into(), Value::Array(logs));
        m.insert("total_lines".into(), Value::Number(total_lines.into()));
        m.insert("from_line".into(), Value::Number(from_line.into()));
        m.insert("has_more".into(), Value::Bool(false));
        m
    }

    /// Return all agent log entries as a `Vec<Value>`.
    ///
    /// Port of `ReportManager.get_agent_log_stream` (`report_agent.py:2066`).
    pub fn get_agent_log_stream(&self, report_id: &str) -> Vec<Value> {
        let result = self.get_agent_log(report_id, 0);
        result.get("logs").and_then(|v| v.as_array()).cloned().unwrap_or_default()
    }

    // ── Outline ─────────────────────────────────────────────────────────────

    /// Save report outline to `outline.json`.
    ///
    /// Port of `ReportManager.save_outline` (`report_agent.py:2080`).
    /// JSON: `json.dump(outline.to_dict(), f, ensure_ascii=False, indent=2)`.
    pub fn save_outline(&self, report_id: &str, outline: &ReportOutline) -> io::Result<()> {
        self.ensure_report_folder(report_id)?;
        let json = serde_json::to_string_pretty(&Value::Object(outline.to_dict()))
            .map_err(io::Error::other)?;
        fs::write(self.get_outline_path(report_id), json.as_bytes())
    }

    // ── Section ─────────────────────────────────────────────────────────────

    /// Save a single section to `section_{NN:02}.md`.
    ///
    /// Port of `ReportManager.save_section` (`report_agent.py:2094`).
    ///
    /// The section content is cleaned by `_clean_section_content` before writing.
    /// Returns the path written to.
    pub fn save_section(
        &self,
        report_id: &str,
        section_index: usize,
        section: &ReportSection,
    ) -> io::Result<PathBuf> {
        self.ensure_report_folder(report_id)?;

        let cleaned_content = self.clean_section_content(&section.content, &section.title);
        let mut md_content = format!("## {}\n\n", section.title);
        if !cleaned_content.is_empty() {
            md_content.push_str(&cleaned_content);
            md_content.push_str("\n\n");
        }

        let file_path = self.get_section_path(report_id, section_index);
        fs::write(&file_path, md_content.as_bytes())?;
        Ok(file_path)
    }

    /// Clean section content.
    ///
    /// Port of `ReportManager._clean_section_content` (`report_agent.py:2131`).
    ///
    /// # Algorithm (byte-faithful)
    /// 1. Strip the content string.
    /// 2. Split on `'\n'`.
    /// 3. Per line, match heading regex `^(#{1,6})\s+(.+)$` against the stripped line.
    ///    - If it is a heading AND within the first 5 lines (`i < 5`) AND the heading
    ///      title equals `section_title` (or equals when spaces are removed): skip it
    ///      and set `skip_next_empty = true`.
    ///    - Otherwise (any heading): push `**{title_text}**` then an empty line; continue.
    /// 4. If `skip_next_empty` is true and the current stripped line is empty: skip it,
    ///    reset flag.
    /// 5. Otherwise: push the original line.
    /// 6. Pop leading empty lines.
    /// 7. Pop leading separator lines (`---`, `***`, `___`) AND their trailing empties.
    /// 8. Join with `'\n'`.
    pub fn clean_section_content(&self, content: &str, section_title: &str) -> String {
        if content.is_empty() {
            return content.to_string();
        }

        let content = content.trim();
        let lines: Vec<&str> = content.split('\n').collect();
        let mut cleaned_lines: Vec<String> = Vec::new();
        let mut skip_next_empty = false;

        let heading_re = Regex::new(r"^(#{1,6})\s+(.+)$").unwrap();

        for (i, &line) in lines.iter().enumerate() {
            let stripped = line.trim();

            if let Some(cap) = heading_re.captures(stripped) {
                let title_text = cap[2].trim();

                // Within first 5 lines: check for duplicate of section_title
                if i < 5 {
                    let titles_match = title_text == section_title
                        || title_text.replace(' ', "") == section_title.replace(' ', "");
                    if titles_match {
                        skip_next_empty = true;
                        continue;
                    }
                }

                // All headings → bold + blank line
                cleaned_lines.push(format!("**{}**", title_text));
                cleaned_lines.push(String::new());
                continue;
            }

            // If previous line was a skipped duplicate heading and this line is empty: skip it
            if skip_next_empty && stripped.is_empty() {
                skip_next_empty = false;
                continue;
            }

            skip_next_empty = false;
            cleaned_lines.push(line.to_string());
        }

        // Remove leading empty lines
        while !cleaned_lines.is_empty() && cleaned_lines[0].trim().is_empty() {
            cleaned_lines.remove(0);
        }

        // Remove leading separator lines (---/***/___) and their trailing empties
        while !cleaned_lines.is_empty() && matches!(cleaned_lines[0].trim(), "---" | "***" | "___")
        {
            cleaned_lines.remove(0);
            while !cleaned_lines.is_empty() && cleaned_lines[0].trim().is_empty() {
                cleaned_lines.remove(0);
            }
        }

        cleaned_lines.join("\n")
    }

    // ── Progress ────────────────────────────────────────────────────────────

    /// Update report generation progress in `progress.json`.
    ///
    /// Port of `ReportManager.update_progress` (`report_agent.py:2199`).
    ///
    /// JSON: `json.dump(progress_data, f, ensure_ascii=False, indent=2)`.
    ///
    /// `progress` is `i32` (not `u32`) because Python's failed path writes `-1`
    /// (`report_agent.py:1753`: `update_progress(report_id, "failed", -1, …)`).
    /// Widened from `u32` in sub-cycle h1 (parity bug fix).
    #[allow(clippy::too_many_arguments)]
    pub fn update_progress(
        &self,
        report_id: &str,
        status: &str,
        progress: i32,
        message: &str,
        current_section: Option<&str>,
        completed_sections: Option<&[String]>,
    ) -> io::Result<()> {
        self.ensure_report_folder(report_id)?;

        let mut m = serde_json::Map::new();
        m.insert("status".into(), Value::String(status.to_string()));
        // `i32` → `serde_json::Number` via `From<i32>` (not `From<u32>`).
        // This allows -1 to serialize as the JSON integer -1, matching Python.
        m.insert("progress".into(), Value::Number(serde_json::Number::from(progress)));
        m.insert("message".into(), Value::String(message.to_string()));
        m.insert(
            "current_section".into(),
            current_section.map(|s| Value::String(s.to_string())).unwrap_or(Value::Null),
        );
        m.insert(
            "completed_sections".into(),
            Value::Array(
                completed_sections
                    .unwrap_or(&[])
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
        m.insert(
            "updated_at".into(),
            // Python `datetime.now().isoformat()` is NAIVE (no offset). Reuse the project's
            // shared `python_isoformat_local()` helper (also used by U-023 action_logger) so
            // the timestamp string matches Python's naive isoformat — not an offset-suffixed
            // rfc3339. Field is write-only/non-contractual, but keep the format faithful.
            Value::String(crate::models::project::python_isoformat_local()),
        );

        let json = serde_json::to_string_pretty(&Value::Object(m)).map_err(io::Error::other)?;
        fs::write(self.get_progress_path(report_id), json.as_bytes())
    }

    /// Read the progress JSON for a report.
    ///
    /// Port of `ReportManager.get_progress` (`report_agent.py:2228`).
    ///
    /// Returns `None` if the file does not exist.
    pub fn get_progress(&self, report_id: &str) -> Option<serde_json::Map<String, Value>> {
        let path = self.get_progress_path(report_id);
        if !path.exists() {
            return None;
        }
        let data = fs::read_to_string(&path).ok()?;
        serde_json::from_str::<Value>(&data)
            .ok()
            .and_then(|v| if let Value::Object(m) = v { Some(m) } else { None })
    }

    // ── Generated sections ─────────────────────────────────────────────────

    /// List all saved section files for a report.
    ///
    /// Port of `ReportManager.get_generated_sections` (`report_agent.py:2239`).
    ///
    /// Returns a sorted list of `{ "filename", "section_index", "content" }` maps.
    /// Returns an empty list if the folder doesn't exist.
    pub fn get_generated_sections(&self, report_id: &str) -> Vec<serde_json::Map<String, Value>> {
        let folder = self.get_report_folder(report_id);
        if !folder.exists() {
            return vec![];
        }

        let mut entries: Vec<(String, usize, String)> = Vec::new();

        let read_dir = match fs::read_dir(&folder) {
            Ok(d) => d,
            Err(_) => return vec![],
        };

        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("section_") && name.ends_with(".md") {
                // Parse section index: section_01.md → 1
                let stem = name.trim_end_matches(".md"); // "section_01"
                let parts: Vec<&str> = stem.split('_').collect();
                if parts.len() >= 2 {
                    if let Ok(idx) = parts[1].parse::<usize>() {
                        let content = fs::read_to_string(entry.path()).unwrap_or_default();
                        entries.push((name, idx, content));
                    }
                }
            }
        }

        // Sort by filename (matches Python's `sorted(os.listdir(folder))`)
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        entries
            .into_iter()
            .map(|(filename, section_index, content)| {
                let mut m = serde_json::Map::new();
                m.insert("filename".into(), Value::String(filename));
                m.insert("section_index".into(), Value::Number(section_index.into()));
                m.insert("content".into(), Value::String(content));
                m
            })
            .collect()
    }

    /// U-027 (c) GAP-A: read the on-disk `full_report.md` content if it exists.
    ///
    /// The `/<report_id>/download` route (`report.py:414-433`) serves the on-disk
    /// markdown file when present, else a temp file written from `report.markdown_content`.
    /// This pub wrapper exposes the on-disk read (the private `get_report_markdown_path`
    /// stays internal); `None` → the route falls back to `report.markdown_content`. Both
    /// branches yield the same `.md` attachment bytes (save_report writes
    /// `full_report.md` = `markdown_content`).
    pub fn read_report_markdown(&self, report_id: &str) -> Option<String> {
        let path = self.get_report_markdown_path(report_id);
        if path.exists() { fs::read_to_string(&path).ok() } else { None }
    }

    /// U-027 (c) GAP-B: read a single section's `(filename, content)` if it exists.
    ///
    /// Port of `get_single_section`'s file read (`report.py:676-694`): reads
    /// `section_{NN:02}.md` from the report folder. `None` → the route returns 404
    /// `sectionNotFound`. The private `get_section_path` stays internal.
    pub fn get_single_section(
        &self,
        report_id: &str,
        section_index: usize,
    ) -> Option<(String, String)> {
        let path = self.get_section_path(report_id, section_index);
        if !path.exists() {
            return None;
        }
        let content = fs::read_to_string(&path).ok()?;
        Some((format!("section_{:02}.md", section_index), content))
    }

    // ── Full report assembly ─────────────────────────────────────────────────

    /// Assemble the full report from saved section files.
    ///
    /// Port of `ReportManager.assemble_full_report` (`report_agent.py:2270`).
    ///
    /// Reads all `section_NN.md` files, concatenates them after a header block,
    /// runs `_post_process_report` heading normalization, saves to `full_report.md`,
    /// and returns the final markdown string.
    pub fn assemble_full_report(
        &self,
        report_id: &str,
        outline: &ReportOutline,
    ) -> io::Result<String> {
        // Build report header
        let mut md_content = format!("# {}\n\n", outline.title);
        md_content.push_str(&format!("> {}\n\n", outline.summary));
        md_content.push_str("---\n\n");

        // Read all section files in order
        let sections = self.get_generated_sections(report_id);
        for section_info in &sections {
            if let Some(content) = section_info.get("content").and_then(|v| v.as_str()) {
                md_content.push_str(content);
            }
        }

        // Post-process: heading normalization, dup-heading removal, blank collapse
        md_content = self.post_process_report(&md_content, outline);

        // Save full_report.md
        let full_path = self.get_report_markdown_path(report_id);
        fs::write(&full_path, md_content.as_bytes())?;

        Ok(md_content)
    }

    /// Post-process assembled report content.
    ///
    /// Port of `ReportManager._post_process_report` (`report_agent.py:2300`).
    ///
    /// # Algorithm (byte-faithful)
    ///
    /// 1. Split content on `'\n'`.
    /// 2. Collect outline section titles into a set.
    /// 3. While-loop over lines with index `i`:
    ///     - Match heading regex `^(#{1,6})\s+(.+)$` on the stripped line.
    ///     - **Duplicate detection**: scan the last up-to-5 `processed_lines`
    ///       for a heading with the same title → if dup, skip heading and all
    ///       following blank lines (`i++` while blank).
    ///     - **Level handling**: level 1 → keep if outline.title, promote to `##` if
    ///       section title, else bold; level 2 → keep if section/outline title, else bold;
    ///       level >= 3 → bold + empty line.
    ///     - `---` right after a heading: skip it.
    ///     - Blank right after a heading: keep at most one.
    ///     - Other lines: push as-is.
    /// 4. Final pass: collapse runs of blank lines to **at most 2**.
    pub fn post_process_report(&self, content: &str, outline: &ReportOutline) -> String {
        let lines: Vec<&str> = content.split('\n').collect();
        let mut processed_lines: Vec<String> = Vec::new();
        let mut prev_was_heading = false;

        // Collect outline section titles into a set
        let section_titles: std::collections::HashSet<&str> =
            outline.sections.iter().map(|s| s.title.as_str()).collect();

        let heading_re = Regex::new(r"^(#{1,6})\s+(.+)$").unwrap();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let stripped = line.trim();

            if let Some(cap) = heading_re.captures(stripped) {
                let level = cap[1].len();
                let title = cap[2].trim();

                // Duplicate detection: scan last up-to-5 processed_lines for same title heading
                let is_duplicate = {
                    let start = processed_lines.len().saturating_sub(5);
                    processed_lines[start..].iter().any(|prev_line| {
                        let prev_stripped = prev_line.trim();
                        heading_re
                            .captures(prev_stripped)
                            .map(|prev_cap| prev_cap[2].trim() == title)
                            .unwrap_or(false)
                    })
                };

                if is_duplicate {
                    // Skip duplicate heading and all following blank lines
                    i += 1;
                    while i < lines.len() && lines[i].trim().is_empty() {
                        i += 1;
                    }
                    continue;
                }

                // Level handling
                match level {
                    1 => {
                        if title == outline.title {
                            // Keep the report main title
                            processed_lines.push(line.to_string());
                            prev_was_heading = true;
                        } else if section_titles.contains(title) {
                            // Section title incorrectly used #, fix to ##
                            processed_lines.push(format!("## {}", title));
                            prev_was_heading = true;
                        } else {
                            // Other level-1 → bold
                            processed_lines.push(format!("**{}**", title));
                            processed_lines.push(String::new());
                            prev_was_heading = false;
                        }
                    }
                    2 => {
                        if section_titles.contains(title) || title == outline.title {
                            // Keep section heading
                            processed_lines.push(line.to_string());
                            prev_was_heading = true;
                        } else {
                            // Non-section level-2 → bold
                            processed_lines.push(format!("**{}**", title));
                            processed_lines.push(String::new());
                            prev_was_heading = false;
                        }
                    }
                    _ => {
                        // level >= 3 → bold
                        processed_lines.push(format!("**{}**", title));
                        processed_lines.push(String::new());
                        prev_was_heading = false;
                    }
                }

                i += 1;
                continue;
            } else if stripped == "---" && prev_was_heading {
                // Skip separator immediately after a heading
                i += 1;
                continue;
            } else if stripped.is_empty() && prev_was_heading {
                // After a heading: keep at most one blank line
                if processed_lines.last().map(|l| !l.trim().is_empty()).unwrap_or(true) {
                    processed_lines.push(line.to_string());
                }
                prev_was_heading = false;
            } else {
                processed_lines.push(line.to_string());
                prev_was_heading = false;
            }

            i += 1;
        }

        // Final pass: collapse runs of blank lines to at most 2
        let mut result_lines: Vec<String> = Vec::new();
        let mut empty_count: usize = 0;
        for line in processed_lines {
            if line.trim().is_empty() {
                empty_count += 1;
                if empty_count <= 2 {
                    result_lines.push(line);
                }
            } else {
                empty_count = 0;
                result_lines.push(line);
            }
        }

        result_lines.join("\n")
    }

    // ── Save / Get report ───────────────────────────────────────────────────

    /// Save report metadata and full markdown to disk.
    ///
    /// Port of `ReportManager.save_report` (`report_agent.py:2426`).
    ///
    /// Writes:
    /// - `meta.json` (from `report.to_dict()`)
    /// - `outline.json` if `report.outline` is `Some`
    /// - `full_report.md` if `report.markdown_content` is non-empty
    pub fn save_report(&self, report: &Report) -> io::Result<()> {
        self.ensure_report_folder(&report.report_id)?;

        // Save meta.json
        let json = serde_json::to_string_pretty(&Value::Object(report.to_dict()))
            .map_err(io::Error::other)?;
        fs::write(self.get_report_path(&report.report_id), json.as_bytes())?;

        // Save outline.json if present
        if let Some(outline) = &report.outline {
            self.save_outline(&report.report_id, outline)?;
        }

        // Save full_report.md if content is present
        if !report.markdown_content.is_empty() {
            fs::write(
                self.get_report_markdown_path(&report.report_id),
                report.markdown_content.as_bytes(),
            )?;
        }

        Ok(())
    }

    /// Load a report by ID.
    ///
    /// Port of `ReportManager.get_report` (`report_agent.py:2446`).
    ///
    /// # Fallback (back-compat)
    /// If `{reports_dir}/{id}/meta.json` does not exist, tries the old flat-file
    /// path `{reports_dir}/{id}.json` before returning `None`.
    ///
    /// If `markdown_content` in meta.json is empty, attempts to read it from
    /// `full_report.md`.
    pub fn get_report(&self, report_id: &str) -> Option<Report> {
        let mut path = self.get_report_path(report_id);

        if !path.exists() {
            // Back-compat: old flat {id}.json
            let old_path = self.reports_dir.join(format!("{}.json", report_id));
            if old_path.exists() {
                path = old_path;
            } else {
                return None;
            }
        }

        let data_str = fs::read_to_string(&path).ok()?;
        let data: serde_json::Map<String, Value> = serde_json::from_str(&data_str).ok()?;

        // Reconstruct outline from dict
        let outline = data.get("outline").and_then(|v| {
            let outline_data = v.as_object()?;
            let title = outline_data.get("title")?.as_str()?.to_string();
            let summary = outline_data.get("summary")?.as_str()?.to_string();
            let sections: Vec<ReportSection> = outline_data
                .get("sections")
                .and_then(|sv| sv.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| {
                            let title = s.get("title")?.as_str()?.to_string();
                            let content =
                                s.get("content").and_then(|c| c.as_str()).unwrap_or("").to_string();
                            Some(ReportSection { title, content })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(ReportOutline { title, summary, sections })
        });

        // markdown_content: read from dict, fall back to full_report.md
        let markdown_content = {
            let from_dict =
                data.get("markdown_content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if from_dict.is_empty() {
                let full_report_path = self.get_report_markdown_path(report_id);
                if full_report_path.exists() {
                    fs::read_to_string(&full_report_path).unwrap_or_default()
                } else {
                    from_dict
                }
            } else {
                from_dict
            }
        };

        let status = data
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_value::<ReportStatus>(Value::String(s.to_string())).ok())
            .unwrap_or(ReportStatus::Pending);

        Some(Report {
            report_id: data.get("report_id")?.as_str()?.to_string(),
            simulation_id: data.get("simulation_id")?.as_str()?.to_string(),
            graph_id: data.get("graph_id")?.as_str()?.to_string(),
            simulation_requirement: data.get("simulation_requirement")?.as_str()?.to_string(),
            status,
            outline,
            markdown_content,
            created_at: data.get("created_at").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            completed_at: data
                .get("completed_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            error: data
                .get("error")
                .and_then(|v| if v.is_null() { None } else { v.as_str().map(|s| s.to_string()) }),
        })
    }

    /// Find a report by simulation ID.
    ///
    /// Port of `ReportManager.get_report_by_simulation` (`report_agent.py:2499`).
    ///
    /// Scans `reports_dir` for all entries (both new-format folders and old-format
    /// `.json` files) and returns the first report whose `simulation_id` matches.
    pub fn get_report_by_simulation(&self, simulation_id: &str) -> Option<Report> {
        self.ensure_reports_dir().ok()?;

        let read_dir = fs::read_dir(&self.reports_dir).ok()?;

        for entry in read_dir.flatten() {
            let item_path = entry.path();
            let item_name = entry.file_name().to_string_lossy().to_string();

            if item_path.is_dir() {
                // New format: folder
                let report = self.get_report(&item_name);
                if let Some(r) = report {
                    if r.simulation_id == simulation_id {
                        return Some(r);
                    }
                }
            } else if item_name.ends_with(".json") {
                // Old format: flat JSON file
                let report_id = item_name.trim_end_matches(".json");
                let report = self.get_report(report_id);
                if let Some(r) = report {
                    if r.simulation_id == simulation_id {
                        return Some(r);
                    }
                }
            }
        }

        None
    }

    /// List all reports, optionally filtered by simulation ID.
    ///
    /// Port of `ReportManager.list_reports` (`report_agent.py:2520`).
    ///
    /// Results are sorted by `created_at` descending (newest first).
    /// Capped at `limit` entries.
    pub fn list_reports(&self, simulation_id: Option<&str>, limit: usize) -> Vec<Report> {
        if self.ensure_reports_dir().is_err() {
            return vec![];
        }

        let read_dir = match fs::read_dir(&self.reports_dir) {
            Ok(d) => d,
            Err(_) => return vec![],
        };

        let mut reports: Vec<Report> = Vec::new();

        for entry in read_dir.flatten() {
            let item_path = entry.path();
            let item_name = entry.file_name().to_string_lossy().to_string();

            let maybe_report = if item_path.is_dir() {
                self.get_report(&item_name)
            } else if item_name.ends_with(".json") {
                let report_id = item_name.trim_end_matches(".json");
                self.get_report(report_id)
            } else {
                None
            };

            if let Some(report) = maybe_report {
                let matches = simulation_id.map(|sid| report.simulation_id == sid).unwrap_or(true);
                if matches {
                    reports.push(report);
                }
            }
        }

        // Sort by created_at descending (Python: `reports.sort(key=lambda r: r.created_at, reverse=True)`)
        reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        reports.truncate(limit);
        reports
    }

    /// Delete a report.
    ///
    /// Port of `ReportManager.delete_report` (`report_agent.py:2547`).
    ///
    /// New format: removes the entire report folder (`shutil.rmtree`).
    /// Old format: removes `{id}.json` and/or `{id}.md` from `reports_dir`.
    ///
    /// Returns `true` if anything was deleted.
    pub fn delete_report(&self, report_id: &str) -> bool {
        let folder_path = self.get_report_folder(report_id);

        // New format: delete the whole folder
        if folder_path.exists() && folder_path.is_dir() {
            if fs::remove_dir_all(&folder_path).is_ok() {
                return true;
            }
        }

        // Old format: delete flat files
        let mut deleted = false;
        let old_json = self.reports_dir.join(format!("{}.json", report_id));
        let old_md = self.reports_dir.join(format!("{}.md", report_id));

        if old_json.exists() && fs::remove_file(&old_json).is_ok() {
            deleted = true;
        }
        if old_md.exists() && fs::remove_file(&old_md).is_ok() {
            deleted = true;
        }

        deleted
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{ReportOutline, ReportSection, ReportStatus};

    fn temp_mgr() -> (ReportManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mgr = ReportManager::new(dir.path());
        (mgr, dir)
    }

    // ── _clean_section_content goldens ──────────────────────────────────────

    /// A heading whose title matches section_title within the first 5 lines is dropped.
    #[test]
    fn test_clean_section_content_drops_dup_title_heading() {
        let mgr = ReportManager::new("/tmp/unused");
        let content = "## My Section\n\nSome body text.";
        let result = mgr.clean_section_content(content, "My Section");
        // The heading "## My Section" is dropped (it's in the first 5 lines and matches)
        assert!(!result.contains("## My Section"), "should have dropped dup heading");
        assert!(result.contains("Some body text."));
    }

    /// Heading whose title does NOT match section_title → converted to bold.
    #[test]
    fn test_clean_section_content_other_heading_to_bold() {
        let mgr = ReportManager::new("/tmp/unused");
        let content = "### Sub heading\n\nBody.";
        let result = mgr.clean_section_content(content, "Different Title");
        assert!(result.contains("**Sub heading**"), "heading should become bold");
        assert!(!result.contains("###"), "should not have any markdown heading markers");
    }

    /// Leading separator lines (---, ***, ___) are stripped along with their trailing blanks.
    #[test]
    fn test_clean_section_content_strips_leading_separator() {
        let mgr = ReportManager::new("/tmp/unused");
        let content = "---\n\nActual content.";
        let result = mgr.clean_section_content(content, "Title");
        assert!(!result.starts_with("---"), "should strip leading separator");
        assert!(result.contains("Actual content."));
    }

    /// Heading in first 5 lines with space-normalized match is dropped.
    #[test]
    fn test_clean_section_content_space_normalized_dup() {
        let mgr = ReportManager::new("/tmp/unused");
        let content = "## My Section\n\nBody.";
        // section_title has extra spaces — spaces removed before comparison
        let result = mgr.clean_section_content(content, "MySection");
        assert!(!result.contains("## My Section"), "space-normalized dup should be dropped");
    }

    /// Leading empty lines after processing are removed.
    #[test]
    fn test_clean_section_content_strips_leading_blanks() {
        let mgr = ReportManager::new("/tmp/unused");
        let content = "\n\nReal content.";
        let result = mgr.clean_section_content(content, "Title");
        assert!(!result.starts_with('\n'), "leading blank lines should be stripped");
        assert!(result.starts_with("Real content."));
    }

    /// Content with no headings passes through unchanged (minus strip).
    #[test]
    fn test_clean_section_content_no_headings_passthrough() {
        let mgr = ReportManager::new("/tmp/unused");
        let content = "Plain text content.\nAnother line.";
        let result = mgr.clean_section_content(content, "Title");
        assert_eq!(result, "Plain text content.\nAnother line.");
    }

    /// Heading beyond first 5 lines is not treated as a duplicate, but still converted to bold.
    #[test]
    fn test_clean_section_content_heading_after_5_lines_to_bold() {
        let mgr = ReportManager::new("/tmp/unused");
        // 5 blank lines then the heading that would match title
        let content = "line1\nline2\nline3\nline4\nline5\n## My Section\n\nBody.";
        let result = mgr.clean_section_content(content, "My Section");
        // At i=5 (6th line), no longer within first 5, so heading → bold regardless
        assert!(result.contains("**My Section**"), "heading at i>=5 must be bolded");
        assert!(!result.contains("## My Section"), "should not keep heading syntax");
    }

    // ── _post_process_report goldens ────────────────────────────────────────

    fn make_outline() -> ReportOutline {
        ReportOutline {
            title: "Report Title".to_string(),
            summary: "Summary.".to_string(),
            sections: vec![
                ReportSection { title: "Section A".to_string(), content: String::new() },
                ReportSection { title: "Section B".to_string(), content: String::new() },
            ],
        }
    }

    /// Level-1 heading that equals outline.title is kept as-is.
    #[test]
    fn test_post_process_level1_title_kept() {
        let mgr = ReportManager::new("/tmp/unused");
        let outline = make_outline();
        let content = "# Report Title\n\nBody.";
        let result = mgr.post_process_report(content, &outline);
        assert!(result.contains("# Report Title"), "main title should be preserved");
    }

    /// Level-1 heading that matches a section title is promoted to ##.
    #[test]
    fn test_post_process_level1_section_title_promoted() {
        let mgr = ReportManager::new("/tmp/unused");
        let outline = make_outline();
        let content = "# Report Title\n\n# Section A\n\nBody.";
        let result = mgr.post_process_report(content, &outline);
        assert!(result.contains("## Section A"), "section title as # should become ##");
        // Check that it is NOT a level-1 heading (i.e., not "# Section A" without a leading #)
        // We check that the line starts with "## " not "# " (single hash)
        let has_solo_hash =
            result.lines().any(|l| l == "# Section A" || l.starts_with("# Section A "));
        assert!(!has_solo_hash, "# Section A should be promoted to ##, not remain as level-1");
    }

    /// Level-1 heading that is neither outline.title nor section title → bold.
    #[test]
    fn test_post_process_level1_other_to_bold() {
        let mgr = ReportManager::new("/tmp/unused");
        let outline = make_outline();
        let content = "# Some Other Title\n\nBody.";
        let result = mgr.post_process_report(content, &outline);
        assert!(result.contains("**Some Other Title**"), "other level-1 → bold");
    }

    /// Level-2 heading that is a section title is kept.
    #[test]
    fn test_post_process_level2_section_kept() {
        let mgr = ReportManager::new("/tmp/unused");
        let outline = make_outline();
        let content = "## Section A\n\nBody.";
        let result = mgr.post_process_report(content, &outline);
        assert!(result.contains("## Section A"), "section title ## should be kept");
    }

    /// Level-2 heading that is NOT a section title → bold.
    #[test]
    fn test_post_process_level2_non_section_to_bold() {
        let mgr = ReportManager::new("/tmp/unused");
        let outline = make_outline();
        let content = "## Random Heading\n\nBody.";
        let result = mgr.post_process_report(content, &outline);
        assert!(result.contains("**Random Heading**"), "non-section ## → bold");
        assert!(!result.contains("## Random Heading"), "should not keep as heading");
    }

    /// Level-3+ headings always → bold.
    #[test]
    fn test_post_process_level3_to_bold() {
        let mgr = ReportManager::new("/tmp/unused");
        let outline = make_outline();
        let content = "### Deep Heading\n\nBody.";
        let result = mgr.post_process_report(content, &outline);
        assert!(result.contains("**Deep Heading**"), "### should become bold");
        assert!(!result.contains("###"), "should not have ### markers");
    }

    /// Duplicate heading within 5-line window is skipped (along with following blanks).
    #[test]
    fn test_post_process_dup_heading_skipped() {
        let mgr = ReportManager::new("/tmp/unused");
        let outline = make_outline();
        // ## Section A appears twice within 5 lines
        let content = "## Section A\n\n## Section A\n\nBody.";
        let result = mgr.post_process_report(content, &outline);
        // Count occurrences of "## Section A" in result — should be exactly 1
        let count = result.matches("## Section A").count();
        assert_eq!(count, 1, "duplicate heading should be removed; count={count}");
    }

    /// `---` immediately after a heading is skipped.
    #[test]
    fn test_post_process_separator_after_heading_skipped() {
        let mgr = ReportManager::new("/tmp/unused");
        let outline = make_outline();
        let content = "## Section A\n---\n\nBody.";
        let result = mgr.post_process_report(content, &outline);
        // The --- line should not appear right after the heading
        let lines: Vec<&str> = result.lines().collect();
        let heading_pos = lines.iter().position(|l| l.trim() == "## Section A");
        if let Some(pos) = heading_pos {
            // The next non-empty line after the heading should not be "---"
            let next_non_empty = lines[pos + 1..].iter().find(|l| !l.trim().is_empty());
            assert_ne!(next_non_empty, Some(&"---"), "--- after heading should be skipped");
        }
    }

    /// Runs of blank lines are collapsed to at most 2.
    #[test]
    fn test_post_process_blank_collapse_to_2() {
        let mgr = ReportManager::new("/tmp/unused");
        let outline = make_outline();
        let content = "Line A\n\n\n\n\nLine B";
        let result = mgr.post_process_report(content, &outline);
        // Should not have 3 consecutive blank lines
        assert!(
            !result.contains("\n\n\n\n"),
            "should not have 4 consecutive newlines (3 blanks)"
        );
    }

    // ── save_outline + get round-trip ────────────────────────────────────────

    #[test]
    fn test_save_outline_round_trip() {
        let (mgr, _dir) = temp_mgr();
        let outline = ReportOutline {
            title: "Test Report".to_string(),
            summary: "A summary.".to_string(),
            sections: vec![
                ReportSection { title: "Intro".to_string(), content: "intro content".to_string() },
                ReportSection { title: "Body".to_string(), content: String::new() },
            ],
        };

        mgr.save_outline("report1", &outline).expect("save_outline");

        // Read back and verify
        let path = mgr.get_outline_path("report1");
        assert!(path.exists(), "outline.json should exist");
        let data: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(data["title"], "Test Report");
        assert_eq!(data["summary"], "A summary.");
        assert_eq!(data["sections"][0]["title"], "Intro");
        assert_eq!(data["sections"][1]["title"], "Body");
    }

    // ── save_section ────────────────────────────────────────────────────────

    #[test]
    fn test_save_section_writes_section_01() {
        let (mgr, _dir) = temp_mgr();
        let section = ReportSection {
            title: "Introduction".to_string(),
            content: "Some content here.".to_string(),
        };
        let path = mgr.save_section("rep1", 1, &section).expect("save_section");
        assert!(
            path.to_string_lossy().ends_with("section_01.md"),
            "filename should be section_01.md"
        );
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("## Introduction\n\n"), "should start with ## heading");
        assert!(written.contains("Some content here."));
    }

    #[test]
    fn test_save_section_zero_pads_index() {
        let (mgr, _dir) = temp_mgr();
        let section = ReportSection { title: "Ch10".to_string(), content: "content".to_string() };
        let path = mgr.save_section("rep1", 10, &section).expect("save_section");
        assert!(path.to_string_lossy().ends_with("section_10.md"));
    }

    // ── update_progress / get_progress round-trip ───────────────────────────

    #[test]
    fn test_update_get_progress_round_trip() {
        let (mgr, _dir) = temp_mgr();
        mgr.update_progress(
            "rep1",
            "generating",
            50,
            "Half done",
            Some("Section A"),
            Some(&["Section X".to_string()]),
        )
        .expect("update_progress");

        let progress = mgr.get_progress("rep1").expect("get_progress should return Some");
        assert_eq!(progress.get("status").and_then(|v| v.as_str()), Some("generating"));
        assert_eq!(progress.get("progress").and_then(|v| v.as_u64()), Some(50));
        assert_eq!(progress.get("message").and_then(|v| v.as_str()), Some("Half done"));
        assert_eq!(progress.get("current_section").and_then(|v| v.as_str()), Some("Section A"));
        let completed = progress.get("completed_sections").and_then(|v| v.as_array()).unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].as_str(), Some("Section X"));
    }

    #[test]
    fn test_get_progress_missing_file_returns_none() {
        let (mgr, _dir) = temp_mgr();
        assert!(mgr.get_progress("nonexistent").is_none());
    }

    // ── save_report / get_report round-trip (meta.json) ─────────────────────

    fn make_report(id: &str) -> Report {
        Report {
            report_id: id.to_string(),
            simulation_id: "sim1".to_string(),
            graph_id: "g1".to_string(),
            simulation_requirement: "some requirement".to_string(),
            status: ReportStatus::Completed,
            outline: Some(ReportOutline {
                title: "My Report".to_string(),
                summary: "Summary.".to_string(),
                sections: vec![ReportSection { title: "Sec1".to_string(), content: String::new() }],
            }),
            markdown_content: "# My Report\n\nBody.".to_string(),
            created_at: "2024-01-01T00:00:00".to_string(),
            completed_at: "2024-01-01T01:00:00".to_string(),
            error: None,
        }
    }

    #[test]
    fn test_save_report_get_report_round_trip() {
        let (mgr, _dir) = temp_mgr();
        let report = make_report("rep1");
        mgr.save_report(&report).expect("save_report");

        let loaded = mgr.get_report("rep1").expect("get_report should return Some");
        assert_eq!(loaded.report_id, "rep1");
        assert_eq!(loaded.simulation_id, "sim1");
        assert_eq!(loaded.graph_id, "g1");
        assert_eq!(loaded.simulation_requirement, "some requirement");
        assert_eq!(loaded.status, ReportStatus::Completed);
        assert!(loaded.outline.is_some());
        assert_eq!(loaded.outline.as_ref().unwrap().title, "My Report");
        assert_eq!(loaded.markdown_content, "# My Report\n\nBody.");
        assert!(loaded.error.is_none());
    }

    /// Old-format `{id}.json` flat file fallback in get_report.
    #[test]
    fn test_get_report_old_format_fallback() {
        let (mgr, _dir) = temp_mgr();
        // Create old-format file directly under reports_dir
        fs::create_dir_all(&mgr.reports_dir).unwrap();
        let old_path = mgr.reports_dir.join("old_report.json");
        let report = make_report("old_report");
        let json = serde_json::to_string_pretty(&Value::Object(report.to_dict())).unwrap();
        fs::write(&old_path, json.as_bytes()).unwrap();

        // get_report should find it via the old-format fallback
        let loaded = mgr.get_report("old_report").expect("should find old-format report");
        assert_eq!(loaded.report_id, "old_report");
    }

    /// get_report returns None for a completely unknown ID.
    #[test]
    fn test_get_report_missing_returns_none() {
        let (mgr, _dir) = temp_mgr();
        assert!(mgr.get_report("nonexistent").is_none());
    }

    /// markdown_content falls back to full_report.md when meta.json has empty string.
    #[test]
    fn test_get_report_markdown_fallback_from_full_report_md() {
        let (mgr, _dir) = temp_mgr();
        // Build a report with empty markdown_content in meta.json
        let mut report = make_report("rep_md_fallback");
        report.markdown_content = String::new();
        mgr.save_report(&report).expect("save_report");

        // Now write full_report.md separately
        let md_path = mgr.get_report_markdown_path("rep_md_fallback");
        fs::write(&md_path, b"# Loaded from file").unwrap();

        let loaded = mgr.get_report("rep_md_fallback").expect("should load");
        assert_eq!(loaded.markdown_content, "# Loaded from file");
    }

    // ── assemble_full_report golden ──────────────────────────────────────────

    #[test]
    fn test_assemble_full_report_two_sections() {
        let (mgr, _dir) = temp_mgr();
        let outline = ReportOutline {
            title: "Full Report".to_string(),
            summary: "Full summary.".to_string(),
            sections: vec![
                ReportSection { title: "Sec A".to_string(), content: String::new() },
                ReportSection { title: "Sec B".to_string(), content: String::new() },
            ],
        };

        // Save two section files
        let sec1 = ReportSection { title: "Sec A".to_string(), content: "Content A.".to_string() };
        let sec2 = ReportSection { title: "Sec B".to_string(), content: "Content B.".to_string() };
        mgr.save_section("rep_full", 1, &sec1).unwrap();
        mgr.save_section("rep_full", 2, &sec2).unwrap();

        let md = mgr.assemble_full_report("rep_full", &outline).expect("assemble");
        assert!(md.contains("# Full Report"), "should contain main title");
        assert!(md.contains("> Full summary."), "should contain summary blockquote");
        assert!(md.contains("## Sec A"), "should contain section A heading");
        assert!(md.contains("Content A."), "should contain section A content");
        assert!(md.contains("## Sec B"), "should contain section B heading");
        assert!(md.contains("Content B."), "should contain section B content");

        // Verify full_report.md was written
        let full_path = mgr.get_report_markdown_path("rep_full");
        assert!(full_path.exists(), "full_report.md should be saved");
        let on_disk = fs::read_to_string(&full_path).unwrap();
        assert_eq!(on_disk, md, "file content should match returned string");
    }

    // ── list_reports ─────────────────────────────────────────────────────────

    #[test]
    fn test_list_reports_returns_all() {
        let (mgr, _dir) = temp_mgr();
        let r1 = make_report("list_rep1");
        let mut r2 = make_report("list_rep2");
        r2.created_at = "2024-02-01T00:00:00".to_string();

        mgr.save_report(&r1).unwrap();
        mgr.save_report(&r2).unwrap();

        let reports = mgr.list_reports(None, 50);
        assert_eq!(reports.len(), 2);
        // Sorted by created_at descending — r2 (2024-02) before r1 (2024-01)
        assert_eq!(reports[0].report_id, "list_rep2");
        assert_eq!(reports[1].report_id, "list_rep1");
    }

    #[test]
    fn test_list_reports_filtered_by_simulation_id() {
        let (mgr, _dir) = temp_mgr();
        let r1 = make_report("f_rep1");
        let mut r2 = make_report("f_rep2");
        r2.simulation_id = "sim_other".to_string();
        mgr.save_report(&r1).unwrap();
        mgr.save_report(&r2).unwrap();

        let reports = mgr.list_reports(Some("sim1"), 50);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].report_id, "f_rep1");
    }

    #[test]
    fn test_list_reports_limit() {
        let (mgr, _dir) = temp_mgr();
        for i in 0..5 {
            let mut r = make_report(&format!("lim_rep{}", i));
            r.created_at = format!("2024-01-0{}T00:00:00", i + 1);
            mgr.save_report(&r).unwrap();
        }
        let reports = mgr.list_reports(None, 3);
        assert_eq!(reports.len(), 3);
    }

    // ── delete_report ────────────────────────────────────────────────────────

    #[test]
    fn test_delete_report_removes_folder() {
        let (mgr, _dir) = temp_mgr();
        let report = make_report("del_rep1");
        mgr.save_report(&report).unwrap();
        let folder = mgr.get_report_folder("del_rep1");
        assert!(folder.exists(), "folder should exist before delete");

        let deleted = mgr.delete_report("del_rep1");
        assert!(deleted, "should return true");
        assert!(!folder.exists(), "folder should be gone after delete");
    }

    #[test]
    fn test_delete_report_old_format() {
        let (mgr, _dir) = temp_mgr();
        fs::create_dir_all(&mgr.reports_dir).unwrap();
        let old_json = mgr.reports_dir.join("old_del.json");
        let old_md = mgr.reports_dir.join("old_del.md");
        fs::write(&old_json, b"{}").unwrap();
        fs::write(&old_md, b"# md").unwrap();

        let deleted = mgr.delete_report("old_del");
        assert!(deleted);
        assert!(!old_json.exists());
        assert!(!old_md.exists());
    }

    #[test]
    fn test_delete_report_nonexistent_returns_false() {
        let (mgr, _dir) = temp_mgr();
        // ensure reports_dir exists so we don't error on read
        fs::create_dir_all(&mgr.reports_dir).unwrap();
        assert!(!mgr.delete_report("ghost"));
    }

    // ── get_agent_log pagination ─────────────────────────────────────────────

    #[test]
    fn test_get_agent_log_from_line_pagination() {
        let (mgr, _dir) = temp_mgr();
        mgr.ensure_report_folder("log_rep").unwrap();
        let log_path = mgr.get_agent_log_path("log_rep");
        // Write 3 JSON lines
        fs::write(&log_path, b"{\"action\":\"a0\"}\n{\"action\":\"a1\"}\n{\"action\":\"a2\"}\n")
            .unwrap();

        let result = mgr.get_agent_log("log_rep", 1);
        assert_eq!(result.get("total_lines").and_then(|v| v.as_u64()), Some(3));
        assert_eq!(result.get("from_line").and_then(|v| v.as_u64()), Some(1));
        let logs = result.get("logs").and_then(|v| v.as_array()).unwrap();
        assert_eq!(logs.len(), 2, "should return lines 1 and 2 (0-indexed)");
        assert_eq!(logs[0].get("action").and_then(|v| v.as_str()), Some("a1"));
        assert_eq!(logs[1].get("action").and_then(|v| v.as_str()), Some("a2"));
        assert_eq!(result.get("has_more").and_then(|v| v.as_bool()), Some(false));
    }

    #[test]
    fn test_get_agent_log_missing_file_shape() {
        let (mgr, _dir) = temp_mgr();
        let result = mgr.get_agent_log("no_report", 0);
        assert_eq!(result.get("total_lines").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(result.get("from_line").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(result.get("has_more").and_then(|v| v.as_bool()), Some(false));
        let logs = result.get("logs").and_then(|v| v.as_array()).unwrap();
        assert!(logs.is_empty());
    }

    #[test]
    fn test_get_agent_log_skips_invalid_json_lines() {
        let (mgr, _dir) = temp_mgr();
        mgr.ensure_report_folder("log_rep2").unwrap();
        let log_path = mgr.get_agent_log_path("log_rep2");
        fs::write(&log_path, b"{\"action\":\"ok\"}\nNOT JSON\n{\"action\":\"ok2\"}\n").unwrap();

        let result = mgr.get_agent_log("log_rep2", 0);
        let logs = result.get("logs").and_then(|v| v.as_array()).unwrap();
        // "NOT JSON" line is silently skipped
        assert_eq!(logs.len(), 2);
        // total_lines counts ALL lines including bad ones
        assert_eq!(result.get("total_lines").and_then(|v| v.as_u64()), Some(3));
    }

    // ── get_console_log pagination ───────────────────────────────────────────

    #[test]
    fn test_get_console_log_from_line_pagination() {
        let (mgr, _dir) = temp_mgr();
        mgr.ensure_report_folder("con_rep").unwrap();
        let log_path = mgr.get_console_log_path("con_rep");
        fs::write(&log_path, b"line0\nline1\nline2\n").unwrap();

        let result = mgr.get_console_log("con_rep", 2);
        assert_eq!(result.get("total_lines").and_then(|v| v.as_u64()), Some(3));
        assert_eq!(result.get("from_line").and_then(|v| v.as_u64()), Some(2));
        let logs = result.get("logs").and_then(|v| v.as_array()).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].as_str(), Some("line2"));
    }

    #[test]
    fn test_get_console_log_missing_file_shape() {
        let (mgr, _dir) = temp_mgr();
        let result = mgr.get_console_log("no_rep", 0);
        assert_eq!(result.get("total_lines").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(result.get("has_more").and_then(|v| v.as_bool()), Some(false));
        let logs = result.get("logs").and_then(|v| v.as_array()).unwrap();
        assert!(logs.is_empty());
    }

    #[test]
    fn test_get_console_log_stream_returns_all_lines() {
        let (mgr, _dir) = temp_mgr();
        mgr.ensure_report_folder("con_rep2").unwrap();
        fs::write(mgr.get_console_log_path("con_rep2"), b"a\nb\nc\n").unwrap();
        let lines = mgr.get_console_log_stream("con_rep2");
        assert_eq!(lines, vec!["a", "b", "c"]);
    }

    // ── get_agent_log_stream ─────────────────────────────────────────────────

    #[test]
    fn test_get_agent_log_stream_returns_all_entries() {
        let (mgr, _dir) = temp_mgr();
        mgr.ensure_report_folder("als_rep").unwrap();
        let log_path = mgr.get_agent_log_path("als_rep");
        fs::write(&log_path, b"{\"x\":1}\n{\"x\":2}\n").unwrap();
        let entries = mgr.get_agent_log_stream("als_rep");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("x").and_then(|v| v.as_u64()), Some(1));
    }

    // ── get_generated_sections ───────────────────────────────────────────────

    #[test]
    fn test_get_generated_sections_sorted() {
        let (mgr, _dir) = temp_mgr();
        // Write section_02 before section_01 to verify sorting
        let sec2 = ReportSection { title: "B".to_string(), content: "B content".to_string() };
        let sec1 = ReportSection { title: "A".to_string(), content: "A content".to_string() };
        mgr.save_section("gsec_rep", 2, &sec2).unwrap();
        mgr.save_section("gsec_rep", 1, &sec1).unwrap();

        let sections = mgr.get_generated_sections("gsec_rep");
        assert_eq!(sections.len(), 2);
        // Should be sorted: section_01 first, section_02 second
        assert_eq!(sections[0].get("section_index").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(sections[1].get("section_index").and_then(|v| v.as_u64()), Some(2));
    }

    #[test]
    fn test_get_generated_sections_missing_folder_returns_empty() {
        let (mgr, _dir) = temp_mgr();
        let sections = mgr.get_generated_sections("no_folder");
        assert!(sections.is_empty());
    }

    // ── get_report_by_simulation ─────────────────────────────────────────────

    #[test]
    fn test_get_report_by_simulation() {
        let (mgr, _dir) = temp_mgr();
        let mut r = make_report("by_sim_rep");
        r.simulation_id = "target_sim".to_string();
        mgr.save_report(&r).unwrap();

        let found = mgr.get_report_by_simulation("target_sim");
        assert!(found.is_some());
        assert_eq!(found.unwrap().report_id, "by_sim_rep");
    }

    #[test]
    fn test_get_report_by_simulation_not_found() {
        let (mgr, _dir) = temp_mgr();
        let r = make_report("nosim_rep");
        mgr.save_report(&r).unwrap();
        assert!(mgr.get_report_by_simulation("nonexistent_sim").is_none());
    }

    // ── h1 new tests: update_progress i32 widening ──────────────────────────

    /// Parity bug fix (h1): Python's failed path writes `update_progress(..., "failed", -1, ...)`.
    /// Teri previously narrowed to `u32`; this test proves -1 now round-trips correctly.
    #[test]
    fn test_update_progress_negative_one_failed_path() {
        let (mgr, _dir) = temp_mgr();
        mgr.update_progress("rep_fail", "failed", -1, "Generation failed", None, None)
            .expect("update_progress failed path");

        let progress = mgr.get_progress("rep_fail").expect("get_progress should return Some");
        assert_eq!(progress.get("status").and_then(|v| v.as_str()), Some("failed"));
        // The key assertion: progress.json must contain the integer -1, not some mangled u32.
        assert_eq!(
            progress.get("progress").and_then(|v| v.as_i64()),
            Some(-1),
            "progress.json 'progress' key must be -1 on the failed path"
        );
        assert_eq!(progress.get("message").and_then(|v| v.as_str()), Some("Generation failed"));
        // current_section and completed_sections must be present (null / empty) as Python writes them.
        assert!(progress.get("current_section").map(|v| v.is_null()).unwrap_or(false));
        let completed = progress
            .get("completed_sections")
            .and_then(|v| v.as_array())
            .expect("completed_sections array");
        assert!(completed.is_empty());
    }

    /// Normal 0..100 progress values still serialize correctly after i32 widening.
    #[test]
    fn test_update_progress_normal_range_still_works() {
        let (mgr, _dir) = temp_mgr();
        for pct in [0_i32, 15, 50, 95, 100] {
            mgr.update_progress("rep_pct", "generating", pct, "msg", None, None)
                .expect("update_progress");
            let p = mgr.get_progress("rep_pct").unwrap();
            assert_eq!(
                p.get("progress").and_then(|v| v.as_i64()),
                Some(pct as i64),
                "progress {pct} must round-trip"
            );
        }
    }

    // ── h1 new tests: upload_folder() accessor ───────────────────────────────

    #[test]
    fn test_upload_folder_returns_parent_of_reports_dir() {
        let dir = std::env::temp_dir().join("teri_test_upload_folder");
        let mgr = ReportManager::new(&dir);
        // upload_folder() must return the original `dir`, not `dir/reports`.
        let uf = mgr.upload_folder().expect("upload_folder should be Some");
        assert_eq!(uf, dir.as_path());
    }

    // ── h1 new tests: ensure_report_folder now pub ───────────────────────────

    #[test]
    fn test_ensure_report_folder_is_pub_and_creates_dir() {
        let (mgr, _dir) = temp_mgr();
        let folder = mgr.ensure_report_folder("rep_pub").expect("ensure_report_folder");
        assert!(folder.exists(), "folder must be created by ensure_report_folder");
        assert!(folder.is_dir());
        // Idempotent: calling again must not fail.
        mgr.ensure_report_folder("rep_pub").expect("idempotent ensure_report_folder");
    }
}
