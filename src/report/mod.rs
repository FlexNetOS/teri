use crate::error::{Result, TeriError};
use crate::i18n::{get_language_instruction, t};
use crate::llm::{ChatMessage, ChatOptions, LlmClient};
use crate::services::zep_tools::ReportTools;
use crate::sim::SimulationResult;
use async_stream::try_stream;
use futures::{Stream, StreamExt};
use minijinja::{Environment, context};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use uuid::Uuid;

const REPORT_TEMPLATE: &str = r#"You are a prediction analysis system that synthesizes simulation results into insightful reports.

## User Query
{{ query }}

## Simulation Summary
- Total Ticks: {{ total_ticks }}
- Unique Agents: {{ agent_count }}
- Total Events: {{ total_events }}

## Key Events from Simulation
{% for event in key_events %}
- Tick {{ event.tick }}: {{ event.description }} ({{ event.actor }})
{% endfor %}

## Agent Activity
{% for agent in agents %}
- {{ agent.name }}: {{ agent.action_count }} actions, State: {{ agent.final_state }}
{% endfor %}

## Task
Analyze the simulation to answer the user's query. Provide a structured prediction report.

Generate a JSON object with the following structure:
```json
{
    "summary": "string - 2-3 sentence synthesis of what happened and what it predicts for the query",
    "timeline": [
        {
            "tick": number,
            "description": "string - what happened at this tick that was significant",
            "significance": 0.0-1.0
        }
    ],
    "agent_highlights": [
        {
            "agent_id": "uuid string",
            "agent_name": "string",
            "summary": "string - 1-2 sentences about this agent's role and impact"
        }
    ],
    "confidence": 0.0-1.0
}
```

Respond with only the JSON object:
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub tick: u32,
    pub description: String,
    pub significance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHighlight {
    pub agent_id: Uuid,
    pub agent_name: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionReport {
    pub id: Uuid,
    pub summary: String,
    pub timeline: Vec<TimelineEvent>,
    pub agent_highlights: Vec<AgentHighlight>,
    pub confidence: f32,
    pub raw_query: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// Sub-cycle (d): PLAN_SYSTEM_PROMPT + PLAN_USER_PROMPT_TEMPLATE
// Ported VERBATIM from report_agent.py:552-611 (Chinese text is behavioral
// — the model conditions on it).
// ============================================================================

const PLAN_SYSTEM_PROMPT: &str = r#"你是一个「未来预测报告」的撰写专家，拥有对模拟世界的「上帝视角」——你可以洞察模拟中每一位Agent的行为、言论和互动。

【核心理念】
我们构建了一个模拟世界，并向其中注入了特定的「模拟需求」作为变量。模拟世界的演化结果，就是对未来可能发生情况的预测。你正在观察的不是"实验数据"，而是"未来的预演"。

【你的任务】
撰写一份「未来预测报告」，回答：
1. 在我们设定的条件下，未来发生了什么？
2. 各类Agent（人群）是如何反应和行动？
3. 这个模拟揭示了哪些值得关注的未来趋势和风险？

【报告定位】
- ✅ 这是一份基于模拟的未来预测报告，揭示"如果这样，未来会怎样"
- ✅ 聚焦于预测结果：事件走向、群体反应、涌现现象、潜在风险
- ✅ 模拟世界中的Agent言行就是对未来人群行为的预测
- ❌ 不是对现实世界现状的分析
- ❌ 不是泛泛而谈的舆情综述

【章节数量限制】
- 最少2个章节，最多5个章节
- 不需要子章节，每个章节直接撰写完整内容
- 内容要精炼，聚焦于核心预测发现
- 章节结构由你根据预测结果自主设计

请输出JSON格式的报告大纲，格式如下：
{
    "title": "报告标题",
    "summary": "报告摘要（一句话概括核心预测发现）",
    "sections": [
        {
            "title": "章节标题",
            "description": "章节内容描述"
        }
    ]
}

注意：sections数组最少2个，最多5个元素！"#;

const PLAN_USER_PROMPT_TEMPLATE: &str = r#"【预测场景设定】
我们向模拟世界注入的变量（模拟需求）：{simulation_requirement}

【模拟世界规模】
- 参与模拟的实体数量: {total_nodes}
- 实体间产生的关系数量: {total_edges}
- 实体类型分布: {entity_types}
- 活跃Agent数量: {total_entities}

【模拟预测到的部分未来事实样本】
{related_facts_json}

请以「上帝视角」审视这个未来预演：
1. 在我们设定的条件下，未来呈现出了什么样的状态？
2. 各类人群（Agent）是如何反应和行动的？
3. 这个模拟揭示了哪些值得关注的未来趋势？

根据预测结果，设计最合适的报告章节结构。

【再次提醒】报告章节数量：最少2个，最多5个，内容要精炼聚焦于核心预测发现。"#;

/// Progress callback type for ReACT pipeline methods.
///
/// Called at key milestones: `(stage: &str, pct: u32, message: &str)`.
/// Mirrors Python's `Optional[Callable]` progress_callback parameter.
pub type ProgressCallback<'a> = dyn Fn(&str, u32, &str) + 'a;

// ============================================================================
// Sub-cycle (a): Report data model
// Ported from report_agent.py:389-467 — ReportStatus, ReportSection,
// ReportOutline, Report.
// ============================================================================

/// Report status enum.
///
/// Port of `ReportStatus(str, Enum)` (`report_agent.py:389`).
/// Serde lowercase matches the Python `.value` strings exactly
/// ("pending"/"planning"/"generating"/"completed"/"failed").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportStatus {
    Pending,
    Planning,
    Generating,
    Completed,
    Failed,
}

/// A single section of a report.
///
/// Port of `ReportSection` dataclass (`report_agent.py:399`).
/// Fields: title (required), content (defaults to "").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSection {
    pub title: String,
    pub content: String,
}

impl ReportSection {
    /// Create a new section with the given title and empty content.
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), content: String::new() }
    }

    /// Convert to dict matching Python `to_dict()` (`report_agent.py:404`).
    /// Key order: title, content.
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("title".into(), self.title.clone().into());
        m.insert("content".into(), self.content.clone().into());
        m
    }

    /// Convert to Markdown matching Python `to_markdown(level=2)` (`report_agent.py:410`).
    ///
    /// Python: `f"{'#' * level} {self.title}\n\n"` + optional content.
    pub fn to_markdown(&self, level: usize) -> String {
        let hashes = "#".repeat(level.max(1));
        let mut md = format!("{} {}\n\n", hashes, self.title);
        if !self.content.is_empty() {
            md.push_str(&self.content);
            md.push_str("\n\n");
        }
        md
    }
}

/// Report outline: title, summary, and ordered sections.
///
/// Port of `ReportOutline` dataclass (`report_agent.py:419`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportOutline {
    pub title: String,
    pub summary: String,
    pub sections: Vec<ReportSection>,
}

impl ReportOutline {
    /// Convert to dict matching Python `to_dict()` (`report_agent.py:425`).
    /// Key order: title, summary, sections.
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("title".into(), self.title.clone().into());
        m.insert("summary".into(), self.summary.clone().into());
        let sections: Vec<serde_json::Value> =
            self.sections.iter().map(|s| serde_json::Value::Object(s.to_dict())).collect();
        m.insert("sections".into(), serde_json::Value::Array(sections));
        m
    }

    /// Convert to Markdown matching Python `to_markdown()` (`report_agent.py:432`).
    ///
    /// Python: `f"# {self.title}\n\n"` + `f"> {self.summary}\n\n"` + each section.
    pub fn to_markdown(&self) -> String {
        let mut md = format!("# {}\n\n", self.title);
        md.push_str(&format!("> {}\n\n", self.summary));
        for section in &self.sections {
            md.push_str(&section.to_markdown(2));
        }
        md
    }
}

/// Complete report.
///
/// Port of `Report` dataclass (`report_agent.py:442`).
/// Field order matches Python `to_dict()` key order (report_id, simulation_id,
/// graph_id, simulation_requirement, status, outline, markdown_content,
/// created_at, completed_at, error) — serde_json preserves struct declaration order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub report_id: String,
    pub simulation_id: String,
    pub graph_id: String,
    pub simulation_requirement: String,
    pub status: ReportStatus,
    pub outline: Option<ReportOutline>,
    pub markdown_content: String,
    pub created_at: String,
    pub completed_at: String,
    pub error: Option<String>,
}

impl Report {
    /// Convert to dict matching Python `to_dict()` (`report_agent.py:455`).
    ///
    /// Key order: report_id, simulation_id, graph_id, simulation_requirement,
    /// status (lowercase string), outline (dict or null), markdown_content,
    /// created_at, completed_at, error (null if None).
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("report_id".into(), self.report_id.clone().into());
        m.insert("simulation_id".into(), self.simulation_id.clone().into());
        m.insert("graph_id".into(), self.graph_id.clone().into());
        m.insert("simulation_requirement".into(), self.simulation_requirement.clone().into());
        // Python: `self.status.value` (the lowercase string)
        let status_str = serde_json::to_value(&self.status)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "pending".to_string());
        m.insert("status".into(), status_str.into());
        m.insert(
            "outline".into(),
            match &self.outline {
                Some(o) => serde_json::Value::Object(o.to_dict()),
                None => serde_json::Value::Null,
            },
        );
        m.insert("markdown_content".into(), self.markdown_content.clone().into());
        m.insert("created_at".into(), self.created_at.clone().into());
        m.insert("completed_at".into(), self.completed_at.clone().into());
        m.insert(
            "error".into(),
            match &self.error {
                Some(e) => e.clone().into(),
                None => serde_json::Value::Null,
            },
        );
        m
    }
}

// ============================================================================
// ReportAgent — stateful struct (sub-cycle d)
//
// `new()` returns a ZST-compatible value (all-empty strings) so the existing
// template assoc-fn call sites and Default impl continue to compile unchanged.
// `new_react(...)` is the ReACT constructor; the 3 fields carry per-run context.
// ============================================================================

pub struct ReportAgent {
    /// Opaque graph label. [≠] Zep graph_id server semantics are inexpressible;
    /// teri binds &KnowledgeGraph directly. Retained for Report.graph_id serialization.
    pub graph_id: String,
    pub simulation_id: String,
    pub simulation_requirement: String,
}

impl ReportAgent {
    /// Create a value usable by existing template assoc-fn call sites.
    ///
    /// All fields are empty strings. Template assoc-fns (`generate`, `generate_stream`,
    /// `create_empty_report`) are `fn(…)` not `&self` methods, so they never read these
    /// fields — adding them is a pure extension, no breakage.
    pub fn new() -> Self {
        Self {
            graph_id: String::new(),
            simulation_id: String::new(),
            simulation_requirement: String::new(),
        }
    }

    /// Create a ReACT-mode agent bound to a specific run's identifiers.
    ///
    /// Port of `ReportAgent.__init__` (`report_agent.py:1085-1131`).
    pub fn new_react(
        graph_id: impl Into<String>,
        simulation_id: impl Into<String>,
        simulation_requirement: impl Into<String>,
    ) -> Self {
        Self {
            graph_id: graph_id.into(),
            simulation_id: simulation_id.into(),
            simulation_requirement: simulation_requirement.into(),
        }
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (d): plan_outline
    //
    // Port of `ReportAgent.plan_outline(progress_callback)` (report_agent.py:1137-1219).
    //
    // Flow (all branches preserved):
    //   1. progress(planning, 0, analyzingRequirements)
    //   2. get_simulation_context(graph_id, simulation_requirement, limit=0→30)
    //   3. progress(planning, 30, generatingOutline)
    //   4. build system_prompt = PLAN_SYSTEM_PROMPT + "\n\n" + get_language_instruction()
    //   5. build user_prompt via PLAN_USER_PROMPT_TEMPLATE substitution:
    //        entity_types → Python str(list) repr  (e.g. "['A', 'B']")
    //        related_facts_json → serde_json pretty, first 10 facts, non-ASCII unescaped
    //   6. chat_json([system, user], temperature=0.3) → serde_json::Value
    //   7. progress(planning, 80, parsingOutline)
    //   8. parse sections + outline (title default "模拟分析报告", summary default "")
    //   9. progress(planning, 100, outlinePlanComplete); return outline
    //   EXCEPT: return 3-section fallback on ANY error (byte-identical strings)
    // -----------------------------------------------------------------------

    /// Plan the report outline using the LLM.
    ///
    /// Returns a `ReportOutline` (happy-path or the 3-section fallback on any error).
    ///
    /// # Arguments
    /// * `tools` — `ReportTools` bound to the graph; provides `get_simulation_context`.
    /// * `llm`   — LLM client; `chat_json` is called with temperature=0.3.
    /// * `progress` — optional callback: `(stage: &str, pct: u32, msg: &str)`.
    pub async fn plan_outline<L: LlmClient>(
        &self,
        tools: &ReportTools<'_, L>,
        llm: &L,
        progress: Option<&ProgressCallback<'_>>,
    ) -> ReportOutline {
        // Step 1: progress(0)
        if let Some(cb) = progress {
            cb("planning", 0, &t("progress.analyzingRequirements"));
        }

        // Step 2: get simulation context (limit=0 → ReportTools maps 0→30)
        let context = tools.get_simulation_context(&self.graph_id, &self.simulation_requirement, 0);

        // Step 3: progress(30)
        if let Some(cb) = progress {
            cb("planning", 30, &t("progress.generatingOutline"));
        }

        // Step 4: build system prompt
        let system_prompt = format!("{}\n\n{}", PLAN_SYSTEM_PROMPT, get_language_instruction());

        // Step 5: build user prompt
        let user_prompt = match Self::build_plan_user_prompt(&self.simulation_requirement, &context)
        {
            Ok(p) => p,
            Err(_) => {
                return Self::fallback_outline();
            }
        };

        // Step 6: chat_json([system, user], temperature=0.3)
        let opts = ChatOptions { temperature: Some(0.3), max_tokens: None };
        let messages = vec![ChatMessage::system(system_prompt), ChatMessage::user(user_prompt)];

        let response: serde_json::Value = match llm.chat_json(&messages, &opts).await {
            Ok(v) => v,
            Err(_) => {
                return Self::fallback_outline();
            }
        };

        // Step 7: progress(80)
        if let Some(cb) = progress {
            cb("planning", 80, &t("progress.parsingOutline"));
        }

        // Step 8: parse outline from response
        let sections: Vec<ReportSection> = response
            .get("sections")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|s| ReportSection {
                        title: s.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        content: String::new(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let outline = ReportOutline {
            title: response
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("模拟分析报告")
                .to_string(),
            summary: response.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            sections,
        };

        // Step 9: progress(100)
        if let Some(cb) = progress {
            cb("planning", 100, &t("progress.outlinePlanComplete"));
        }

        outline
    }

    /// Build the PLAN_USER_PROMPT_TEMPLATE substitution.
    ///
    /// Factored out for testability (golden prompt tests in sub-cycle d tests).
    ///
    /// # entity_types formatting (hard requirement)
    /// Python: `{entity_types}` in the template receives
    /// `list(context['graph_statistics'].get('entity_types', {}).keys())` → a Python list
    /// which formats as `str(list)` = `['A', 'B']` (square brackets, each key
    /// single-quoted, ", " separator; empty → `[]`).
    ///
    /// # related_facts_json formatting
    /// Python: `json.dumps(facts[:10], ensure_ascii=False, indent=2)`
    /// serde_json pretty-prints with 2-space indent and does NOT escape non-ASCII,
    /// matching `ensure_ascii=False`.
    ///
    /// [≠] watch: serde_json pretty-print uses the same indent=2 as Python's
    /// `json.dumps(indent=2)` for arrays, but the final newline and trailing comma
    /// behavior may differ in pathological cases. Normal arrays match exactly.
    fn build_plan_user_prompt(
        simulation_requirement: &str,
        context: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<String> {
        let stats = context
            .get("graph_statistics")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        let total_nodes = stats.get("total_nodes").and_then(|v| v.as_i64()).unwrap_or(0);
        let total_edges = stats.get("total_edges").and_then(|v| v.as_i64()).unwrap_or(0);

        // entity_types: Python str(list(keys())) repr
        let entity_types_repr = {
            let keys: Vec<String> = stats
                .get("entity_types")
                .and_then(|v| v.as_object())
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default();
            python_list_repr(&keys)
        };

        let total_entities = context.get("total_entities").and_then(|v| v.as_i64()).unwrap_or(0);

        // related_facts_json: json.dumps(facts[:10], ensure_ascii=False, indent=2)
        let related_facts_json = {
            let facts: Vec<serde_json::Value> = context
                .get("related_facts")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().take(10).cloned().collect())
                .unwrap_or_default();
            // serde_json::to_string_pretty does not escape non-ASCII (matches ensure_ascii=False)
            serde_json::to_string_pretty(&facts)
                .map_err(|e| TeriError::Report(format!("JSON serialization failed: {e}")))?
        };

        let prompt = PLAN_USER_PROMPT_TEMPLATE
            .replace("{simulation_requirement}", simulation_requirement)
            .replace("{total_nodes}", &total_nodes.to_string())
            .replace("{total_edges}", &total_edges.to_string())
            .replace("{entity_types}", &entity_types_repr)
            .replace("{total_entities}", &total_entities.to_string())
            .replace("{related_facts_json}", &related_facts_json);

        Ok(prompt)
    }

    /// The 3-section fallback outline returned on any `plan_outline` error.
    ///
    /// Byte-identical strings to Python `report_agent.py:1211-1218`.
    fn fallback_outline() -> ReportOutline {
        ReportOutline {
            title: "未来预测报告".to_string(),
            summary: "基于模拟预测的未来趋势与风险分析".to_string(),
            sections: vec![
                ReportSection::new("预测场景与核心发现"),
                ReportSection::new("人群行为预测分析"),
                ReportSection::new("趋势展望与风险提示"),
            ],
        }
    }

    // -----------------------------------------------------------------------
    // Existing template family (Y, UNCHANGED — assoc fns, not &self methods)
    // -----------------------------------------------------------------------

    pub fn create_empty_report(query: String) -> PredictionReport {
        PredictionReport {
            id: Uuid::new_v4(),
            summary: String::new(),
            timeline: Vec::new(),
            agent_highlights: Vec::new(),
            confidence: 0.0,
            raw_query: query,
            created_at: chrono::Utc::now(),
        }
    }

    pub async fn generate_stream<L: LlmClient + ?Sized>(
        result: &SimulationResult,
        query: &str,
        llm: &L,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<PredictionReport>> + Send>>> {
        let env = Environment::new();
        let template = env
            .template_from_str(REPORT_TEMPLATE)
            .map_err(|e| TeriError::Report(format!("Template parsing error: {}", e)))?;

        let key_events = Self::extract_key_events(result);
        let agents = Self::summarize_agents(result);
        let total_ticks = result.final_snapshot().map(|s| s.tick).unwrap_or(0);
        let total_events: usize = result.history.iter().map(|s| s.events.len()).sum();

        let ctx = context! {
            query => query,
            total_ticks => total_ticks,
            agent_count => agents.len(),
            total_events => total_events,
            key_events => key_events,
            agents => agents,
        };

        let prompt = template
            .render(ctx)
            .map_err(|e| TeriError::Report(format!("Failed to render report template: {}", e)))?;

        let mut stream = llm.stream(&prompt).await?;
        let query_owned = query.to_string();

        let result_stream = try_stream! {
            let mut buffer = String::new();

            // Yield initial partial report to ensure ≥2 chunks
            yield PredictionReport {
                id: Uuid::new_v4(),
                summary: String::from("[Generating...]"),
                timeline: Vec::new(),
                agent_highlights: Vec::new(),
                confidence: 0.0,
                raw_query: query_owned.clone(),
                created_at: chrono::Utc::now(),
            };

            // Stream text chunks and accumulate
            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
                buffer.push_str(&chunk);

                // Try to parse complete JSON when buffer is large enough
                if buffer.len() > 100 && buffer.contains("}")
                    && let Ok(response) = serde_json::from_str::<serde_json::Value>(&buffer)
                    && let Some(report) = Self::parse_report_from_json(&response, &query_owned) {
                    yield report;
                    return;
                }
            }

            // Final parsing with complete buffer
            if let Ok(response) = serde_json::from_str::<serde_json::Value>(&buffer)
                && let Some(report) = Self::parse_report_from_json(&response, &query_owned) {
                yield report;
                return;
            }

            // If we get here, return error
            Err(TeriError::Report("Failed to parse streaming response".to_string()))?;
        };

        Ok(Box::pin(result_stream))
    }

    fn parse_report_from_json(
        response: &serde_json::Value,
        query: &str,
    ) -> Option<PredictionReport> {
        let summary = response.get("summary")?.as_str()?.to_string();
        let timeline = response
            .get("timeline")
            .and_then(|v| v.as_array())?
            .iter()
            .filter_map(|v| {
                let tick = v.get("tick")?.as_u64()? as u32;
                let description = v.get("description")?.as_str()?.to_string();
                let significance = v.get("significance")?.as_f64()? as f32;
                Some(TimelineEvent { tick, description, significance })
            })
            .collect();

        let agent_highlights = response
            .get("agent_highlights")
            .and_then(|v| v.as_array())?
            .iter()
            .filter_map(|v| {
                let agent_id = v.get("agent_id")?.as_str()?.parse::<Uuid>().ok()?;
                let agent_name = v.get("agent_name")?.as_str()?.to_string();
                let summary = v.get("summary")?.as_str()?.to_string();
                Some(AgentHighlight { agent_id, agent_name, summary })
            })
            .collect();

        let confidence = response.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

        Some(PredictionReport {
            id: Uuid::new_v4(),
            summary,
            timeline,
            agent_highlights,
            confidence,
            raw_query: query.to_string(),
            created_at: chrono::Utc::now(),
        })
    }

    pub async fn generate<L: LlmClient + ?Sized>(
        result: &SimulationResult,
        query: &str,
        llm: &L,
    ) -> Result<PredictionReport> {
        let env = Environment::new();
        let template = env
            .template_from_str(REPORT_TEMPLATE)
            .map_err(|e| TeriError::Report(format!("Template parsing error: {}", e)))?;

        let key_events = Self::extract_key_events(result);
        let agents = Self::summarize_agents(result);
        let total_ticks = result.final_snapshot().map(|s| s.tick).unwrap_or(0);
        let total_events: usize = result.history.iter().map(|s| s.events.len()).sum();

        let ctx = context! {
            query => query,
            total_ticks => total_ticks,
            agent_count => agents.len(),
            total_events => total_events,
            key_events => key_events,
            agents => agents,
        };

        let prompt = template
            .render(ctx)
            .map_err(|e| TeriError::Report(format!("Failed to render report template: {}", e)))?;

        let response = llm.complete_json::<serde_json::Value>(&prompt).await?;

        Self::parse_report_from_json(&response, query).ok_or_else(|| {
            TeriError::Report("Failed to parse LLM response into report".to_string())
        })
    }

    fn extract_key_events(result: &SimulationResult) -> Vec<serde_json::Value> {
        let mut events = Vec::new();
        for snapshot in &result.history {
            for event in &snapshot.events {
                let actor = event.agent_id.to_string();
                let description = format!("{}", event.action);
                events.push(serde_json::json!({
                    "tick": snapshot.tick,
                    "description": description,
                    "actor": actor,
                }));
            }
        }
        events.sort_by_key(|e| e.get("tick").and_then(|v| v.as_u64()).unwrap_or(0));
        events.into_iter().take(10).collect()
    }

    fn summarize_agents(result: &SimulationResult) -> Vec<serde_json::Value> {
        let mut agent_map: std::collections::HashMap<Uuid, (String, usize, String)> =
            std::collections::HashMap::new();

        for snapshot in &result.history {
            for (id, agent) in &snapshot.agents {
                let entry = agent_map
                    .entry(*id)
                    .or_insert_with(|| (agent.name.clone(), 0, agent.state.clone()));
                entry.1 += 1;
                entry.2 = agent.state.clone();
            }
        }

        let mut agents: Vec<_> = agent_map
            .into_iter()
            .map(|(id, (name, action_count, final_state))| {
                serde_json::json!({
                    "agent_id": id.to_string(),
                    "name": name,
                    "action_count": action_count,
                    "final_state": final_state,
                })
            })
            .collect();

        agents.sort_by(|a, b| {
            let a_count = a.get("action_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let b_count = b.get("action_count").and_then(|v| v.as_u64()).unwrap_or(0);
            b_count.cmp(&a_count)
        });

        agents
    }
}

impl Default for ReportAgent {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Internal helpers
// ============================================================================

/// Produce Python's `str(list_of_strings)` representation.
///
/// Python formats a list of strings as: `['A', 'B', 'C']`
/// — square brackets, each string single-quoted, `", "` separator, empty → `[]`.
///
/// This is the exact format that appears in the PLAN_USER_PROMPT_TEMPLATE
/// `{entity_types}` slot. The model conditions on this representation.
fn python_list_repr(items: &[String]) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    let inner: Vec<String> = items.iter().map(|s| format!("'{s}'")).collect();
    format!("[{}]", inner.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Action, AgentSnapshot, Event, WorldSnapshot};

    #[test]
    fn test_timeline_event_creation() {
        let event = TimelineEvent {
            tick: 5,
            description: "Something happened".to_string(),
            significance: 0.8,
        };

        assert_eq!(event.tick, 5);
        assert_eq!(event.significance, 0.8);
    }

    #[test]
    fn test_agent_highlight_creation() {
        let highlight = AgentHighlight {
            agent_id: Uuid::new_v4(),
            agent_name: "Alice".to_string(),
            summary: "Alice was very active".to_string(),
        };

        assert_eq!(highlight.agent_name, "Alice");
    }

    #[test]
    fn test_prediction_report_creation() {
        let report = ReportAgent::create_empty_report("What will happen?".to_string());
        assert_eq!(report.raw_query, "What will happen?");
        assert!(report.summary.is_empty());
    }

    #[test]
    fn test_extract_key_events_from_simulation() {
        let agent_id = Uuid::new_v4();
        let mut agents = std::collections::HashMap::new();
        agents.insert(
            agent_id,
            AgentSnapshot { id: agent_id, name: "Alice".to_string(), state: "Active".to_string() },
        );

        let event = Event {
            agent_id,
            action: Action::Speak("Hello world".to_string()),
            timestamp: chrono::Utc::now(),
        };

        let snapshot = WorldSnapshot {
            tick: 1,
            agents,
            events: vec![event],
            variables: std::collections::HashMap::new(),
        };

        let result = SimulationResult { id: Uuid::new_v4(), history: vec![snapshot] };

        let events = ReportAgent::extract_key_events(&result);
        assert!(!events.is_empty());
        assert_eq!(events[0]["tick"], 1);
    }

    #[test]
    fn test_summarize_agents_from_simulation() {
        let agent_id = Uuid::new_v4();
        let mut agents = std::collections::HashMap::new();
        agents.insert(
            agent_id,
            AgentSnapshot { id: agent_id, name: "Bob".to_string(), state: "Idle".to_string() },
        );

        let snapshot = WorldSnapshot {
            tick: 1,
            agents,
            events: vec![],
            variables: std::collections::HashMap::new(),
        };

        let result = SimulationResult { id: Uuid::new_v4(), history: vec![snapshot] };

        let agent_summaries = ReportAgent::summarize_agents(&result);
        assert!(!agent_summaries.is_empty());
        assert_eq!(agent_summaries[0]["name"], "Bob");
    }

    // Mock LLM client for streaming tests
    struct MockStreamingLlm {
        chunks: Vec<String>,
    }

    #[async_trait::async_trait]
    impl LlmClient for MockStreamingLlm {
        async fn complete(&self, _prompt: &str) -> Result<String> {
            Err(TeriError::Llm("Not implemented".to_string()))
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _prompt: &str) -> Result<T> {
            Err(TeriError::Llm("Not implemented".to_string()))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
            let chunks = self.chunks.clone();
            let stream = try_stream! {
                for chunk in chunks {
                    yield chunk;
                }
            };
            Ok(Box::pin(stream))
        }
        async fn chat(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (d): plan_outline tests
    // -----------------------------------------------------------------------

    /// Mock LLM that returns a fixed JSON value from chat_json.
    struct MockChatJsonLlm {
        response: serde_json::Value,
    }

    #[async_trait::async_trait]
    impl LlmClient for MockChatJsonLlm {
        async fn complete(&self, _prompt: &str) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _prompt: &str) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> Result<T> {
            serde_json::from_value(self.response.clone())
                .map_err(|e| TeriError::Llm(format!("mock parse: {e}")))
        }
    }

    /// Mock LLM that always fails chat_json — used to test fallback path.
    struct FailingChatJsonLlm;

    #[async_trait::async_trait]
    impl LlmClient for FailingChatJsonLlm {
        async fn complete(&self, _prompt: &str) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _prompt: &str) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("deliberate failure".into()))
        }
    }

    // Helper: build an empty KnowledgeGraph + ReportTools fixture.
    fn make_tools_fixture<'g, L: LlmClient>(
        graph: &'g crate::graph::KnowledgeGraph,
        llm: &'g L,
    ) -> crate::services::zep_tools::ReportTools<'g, L> {
        crate::services::zep_tools::ReportTools::new(graph, llm)
    }

    #[tokio::test]
    async fn test_plan_outline_happy_path() {
        // mock returns title/summary/2 sections
        let mock_response = serde_json::json!({
            "title": "T",
            "summary": "S",
            "sections": [
                {"title": "A"},
                {"title": "B"}
            ]
        });
        let llm = MockChatJsonLlm { response: mock_response };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let agent = ReportAgent::new_react("g1", "sim1", "req1");

        let outline = agent.plan_outline(&tools, &llm, None).await;

        assert_eq!(outline.title, "T");
        assert_eq!(outline.summary, "S");
        assert_eq!(outline.sections.len(), 2);
        assert_eq!(outline.sections[0].title, "A");
        assert_eq!(outline.sections[1].title, "B");
        // content must be empty (Python sets content="" for each section)
        assert!(outline.sections[0].content.is_empty());
        assert!(outline.sections[1].content.is_empty());
    }

    #[tokio::test]
    async fn test_plan_outline_defaults_on_empty_sections() {
        // mock returns no title/summary and empty sections list
        let mock_response = serde_json::json!({
            "sections": []
        });
        let llm = MockChatJsonLlm { response: mock_response };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let agent = ReportAgent::new_react("g1", "sim1", "req1");

        let outline = agent.plan_outline(&tools, &llm, None).await;

        // Default title (Python: response.get("title", "模拟分析报告"))
        assert_eq!(outline.title, "模拟分析报告");
        // Default summary (Python: response.get("summary", ""))
        assert_eq!(outline.summary, "");
        assert_eq!(outline.sections.len(), 0);
    }

    #[tokio::test]
    async fn test_plan_outline_fallback_on_llm_error() {
        let llm = FailingChatJsonLlm;
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let agent = ReportAgent::new_react("g1", "sim1", "req1");

        let outline = agent.plan_outline(&tools, &llm, None).await;

        // Byte-identical to Python report_agent.py:1211-1218
        assert_eq!(outline.title, "未来预测报告");
        assert_eq!(outline.summary, "基于模拟预测的未来趋势与风险分析");
        assert_eq!(outline.sections.len(), 3);
        assert_eq!(outline.sections[0].title, "预测场景与核心发现");
        assert_eq!(outline.sections[1].title, "人群行为预测分析");
        assert_eq!(outline.sections[2].title, "趋势展望与风险提示");
    }

    #[tokio::test]
    async fn test_plan_outline_progress_emissions() {
        let mock_response = serde_json::json!({
            "title": "Report",
            "summary": "Sum",
            "sections": [{"title": "S1"}, {"title": "S2"}]
        });
        let llm = MockChatJsonLlm { response: mock_response };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let agent = ReportAgent::new_react("g1", "sim1", "req1");

        // Collect progress emissions
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, u32, String)>::new()));
        let calls_clone = calls.clone();
        let cb = move |stage: &str, pct: u32, msg: &str| {
            calls_clone.lock().unwrap().push((stage.to_string(), pct, msg.to_string()));
        };

        agent.plan_outline(&tools, &llm, Some(&cb)).await;

        let emissions = calls.lock().unwrap().clone();
        // Must have exactly 4 progress calls at 0/30/80/100 all with stage="planning"
        assert_eq!(emissions.len(), 4, "expected 4 progress emissions, got {}", emissions.len());
        assert_eq!(emissions[0].0, "planning");
        assert_eq!(emissions[0].1, 0);
        assert_eq!(emissions[1].1, 30);
        assert_eq!(emissions[2].1, 80);
        assert_eq!(emissions[3].1, 100);
        // Verify i18n messages are non-empty (keys resolve in zh locale default)
        assert!(!emissions[0].2.is_empty(), "i18n key for pct=0 resolved to empty");
        assert!(!emissions[3].2.is_empty(), "i18n key for pct=100 resolved to empty");
    }

    #[tokio::test]
    async fn test_plan_outline_fallback_no_progress_after_failure() {
        // Verify fallback path still emits 0 and 30 before the error,
        // then skips 80 and 100 (Python's except skips both inner callbacks).
        // Python: progress(0) → context → progress(30) → chat_json → EXCEPT:
        //   skips progress(80) and progress(100) and returns fallback.
        let llm = FailingChatJsonLlm;
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let agent = ReportAgent::new_react("g1", "sim1", "req1");

        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));
        let calls_clone = calls.clone();
        let cb = move |_stage: &str, pct: u32, _msg: &str| {
            calls_clone.lock().unwrap().push(pct);
        };

        let outline = agent.plan_outline(&tools, &llm, Some(&cb)).await;

        // Must be the fallback
        assert_eq!(outline.title, "未来预测报告");

        // Python emits 0 and 30 before the try block that chat_json sits in,
        // then within except it skips 80 and 100.
        let emitted = calls.lock().unwrap().clone();
        assert!(emitted.contains(&0), "expected pct=0 even on error path");
        assert!(emitted.contains(&30), "expected pct=30 even on error path");
        assert!(!emitted.contains(&80), "pct=80 should NOT be emitted on error path");
        assert!(!emitted.contains(&100), "pct=100 should NOT be emitted on error path");
    }

    #[test]
    fn test_python_list_repr_empty() {
        assert_eq!(python_list_repr(&[]), "[]");
    }

    #[test]
    fn test_python_list_repr_single() {
        let items = vec!["Person".to_string()];
        assert_eq!(python_list_repr(&items), "['Person']");
    }

    #[test]
    fn test_python_list_repr_multiple() {
        let items = vec!["Person".to_string(), "Organization".to_string()];
        assert_eq!(python_list_repr(&items), "['Person', 'Organization']");
    }

    #[test]
    fn test_build_plan_user_prompt_entity_types_format() {
        let mut stats_obj = serde_json::Map::new();
        let mut entity_types_map = serde_json::Map::new();
        entity_types_map.insert("Person".into(), 3.into());
        entity_types_map.insert("Organization".into(), 2.into());
        stats_obj.insert("total_nodes".into(), 5.into());
        stats_obj.insert("total_edges".into(), 10.into());
        stats_obj.insert("entity_types".into(), serde_json::Value::Object(entity_types_map));

        let mut ctx = serde_json::Map::new();
        ctx.insert("graph_statistics".into(), serde_json::Value::Object(stats_obj));
        ctx.insert(
            "related_facts".into(),
            serde_json::Value::Array(vec!["fact1".into(), "fact2".into()]),
        );
        ctx.insert("total_entities".into(), 5.into());

        let prompt = ReportAgent::build_plan_user_prompt("test requirement", &ctx).unwrap();

        // entity_types slot must contain the Python list repr
        assert!(
            prompt.contains("['Person', 'Organization']")
                || prompt.contains("['Organization', 'Person']"),
            "entity_types list repr not found in prompt: {}",
            &prompt[..300.min(prompt.len())]
        );

        // related_facts_json must contain the 2 facts in pretty JSON
        assert!(prompt.contains("\"fact1\""), "fact1 not in prompt");
        assert!(prompt.contains("\"fact2\""), "fact2 not in prompt");
    }

    #[test]
    fn test_build_plan_user_prompt_related_facts_truncated_to_10() {
        let facts: Vec<serde_json::Value> = (0..15).map(|i| format!("fact_{i}").into()).collect();
        let mut ctx = serde_json::Map::new();
        let mut stats = serde_json::Map::new();
        stats.insert("total_nodes".into(), 0.into());
        stats.insert("total_edges".into(), 0.into());
        stats.insert("entity_types".into(), serde_json::Value::Object(serde_json::Map::new()));
        ctx.insert("graph_statistics".into(), serde_json::Value::Object(stats));
        ctx.insert("related_facts".into(), serde_json::Value::Array(facts));
        ctx.insert("total_entities".into(), 0.into());

        let prompt = ReportAgent::build_plan_user_prompt("req", &ctx).unwrap();

        // fact_10..fact_14 must NOT appear (only first 10 taken)
        assert!(!prompt.contains("fact_10"), "fact_10 should not be in prompt (only first 10)");
        assert!(prompt.contains("fact_9"), "fact_9 should be in prompt");
    }

    #[test]
    fn test_report_section_to_dict_key_order() {
        let s = ReportSection { title: "T".into(), content: "C".into() };
        let d = s.to_dict();
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, vec!["title", "content"]);
    }

    #[test]
    fn test_report_outline_to_dict_key_order() {
        let o = ReportOutline {
            title: "T".into(),
            summary: "S".into(),
            sections: vec![ReportSection::new("A")],
        };
        let d = o.to_dict();
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, vec!["title", "summary", "sections"]);
    }

    #[test]
    fn test_report_section_to_markdown() {
        let s = ReportSection { title: "Hello".into(), content: "Body text.".into() };
        let md = s.to_markdown(2);
        assert_eq!(md, "## Hello\n\nBody text.\n\n");
    }

    #[test]
    fn test_report_outline_to_markdown() {
        let o = ReportOutline {
            title: "Report".into(),
            summary: "Summary line.".into(),
            sections: vec![ReportSection { title: "S1".into(), content: "Content.".into() }],
        };
        let md = o.to_markdown();
        assert!(md.starts_with("# Report\n\n> Summary line.\n\n"));
        assert!(md.contains("## S1\n\nContent.\n\n"));
    }

    #[test]
    fn test_report_status_serde_lowercase() {
        // Verify serde values match Python's .value strings
        assert_eq!(serde_json::to_string(&ReportStatus::Pending).unwrap(), "\"pending\"");
        assert_eq!(serde_json::to_string(&ReportStatus::Planning).unwrap(), "\"planning\"");
        assert_eq!(serde_json::to_string(&ReportStatus::Generating).unwrap(), "\"generating\"");
        assert_eq!(serde_json::to_string(&ReportStatus::Completed).unwrap(), "\"completed\"");
        assert_eq!(serde_json::to_string(&ReportStatus::Failed).unwrap(), "\"failed\"");
    }

    #[test]
    fn test_report_to_dict_key_order() {
        let r = Report {
            report_id: "r1".into(),
            simulation_id: "s1".into(),
            graph_id: "g1".into(),
            simulation_requirement: "req".into(),
            status: ReportStatus::Pending,
            outline: None,
            markdown_content: "".into(),
            created_at: "".into(),
            completed_at: "".into(),
            error: None,
        };
        let d = r.to_dict();
        let keys: Vec<&str> = d.keys().map(|k| k.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "report_id",
                "simulation_id",
                "graph_id",
                "simulation_requirement",
                "status",
                "outline",
                "markdown_content",
                "created_at",
                "completed_at",
                "error",
            ]
        );
    }

    #[test]
    fn test_new_react_fields() {
        let agent = ReportAgent::new_react("graph1", "sim1", "do something");
        assert_eq!(agent.graph_id, "graph1");
        assert_eq!(agent.simulation_id, "sim1");
        assert_eq!(agent.simulation_requirement, "do something");
    }

    #[test]
    fn test_new_returns_empty_fields() {
        let agent = ReportAgent::new();
        assert!(agent.graph_id.is_empty());
        assert!(agent.simulation_id.is_empty());
        assert!(agent.simulation_requirement.is_empty());
    }

    #[tokio::test]
    async fn test_generate_stream_yields_multiple_chunks() {
        let agent_id = Uuid::new_v4();
        let mut agents = std::collections::HashMap::new();
        agents.insert(
            agent_id,
            AgentSnapshot { id: agent_id, name: "Alice".to_string(), state: "Active".to_string() },
        );

        let event = Event {
            agent_id,
            action: Action::Speak("Hello world".to_string()),
            timestamp: chrono::Utc::now(),
        };

        let snapshot = WorldSnapshot {
            tick: 1,
            agents,
            events: vec![event],
            variables: std::collections::HashMap::new(),
        };

        let result = SimulationResult { id: Uuid::new_v4(), history: vec![snapshot] };

        // Mock streaming response - split JSON across chunks
        let json_response = serde_json::json!({
            "summary": "Test prediction about simulation",
            "timeline": [
                {"tick": 1, "description": "Event occurred", "significance": 0.8}
            ],
            "agent_highlights": [
                {"agent_id": agent_id.to_string(), "agent_name": "Alice", "summary": "Alice was key"}
            ],
            "confidence": 0.75
        });

        let chunks = vec![json_response.to_string()];

        let mock_llm = MockStreamingLlm { chunks };
        let mut stream = ReportAgent::generate_stream(&result, "What happened?", &mock_llm)
            .await
            .expect("Failed to create stream");

        let mut chunk_count = 0;
        let mut last_report: Option<PredictionReport> = None;

        while let Some(report_result) = stream.next().await {
            let report = report_result.expect("Stream chunk failed");
            chunk_count += 1;
            last_report = Some(report);
        }

        assert!(
            chunk_count >= 2,
            "Expected at least 2 chunks from streaming, got {}",
            chunk_count
        );
        assert!(last_report.is_some(), "Expected final report");

        let final_report = last_report.unwrap();
        assert_eq!(final_report.raw_query, "What happened?");
        assert!(!final_report.summary.is_empty());
    }
}
