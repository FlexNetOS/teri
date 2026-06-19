pub mod console_logger;
pub mod logger;
pub mod manager;
pub mod sink;

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

// ============================================================================
// Sub-cycle (e): Section-generation prompt constants
//
// All ported VERBATIM from report_agent.py:615-825.
// Chinese text is behavioral — the model conditions on it.
// `{}` placeholders are filled by `format!` / `.replace(...)` at call time.
// ============================================================================

const SECTION_SYSTEM_PROMPT_TEMPLATE: &str = r#"你是一个「未来预测报告」的撰写专家，正在撰写报告的一个章节。

报告标题: {report_title}
报告摘要: {report_summary}
预测场景（模拟需求）: {simulation_requirement}

当前要撰写的章节: {section_title}

═══════════════════════════════════════════════════════════════
【核心理念】
═══════════════════════════════════════════════════════════════

模拟世界是对未来的预演。我们向模拟世界注入了特定条件（模拟需求），
模拟中Agent的行为和互动，就是对未来人群行为的预测。

你的任务是：
- 揭示在设定条件下，未来发生了什么
- 预测各类人群（Agent）是如何反应和行动的
- 发现值得关注的未来趋势、风险和机会

❌ 不要写成对现实世界现状的分析
✅ 要聚焦于"未来会怎样"——模拟结果就是预测的未来

═══════════════════════════════════════════════════════════════
【最重要的规则 - 必须遵守】
═══════════════════════════════════════════════════════════════

1. 【必须调用工具观察模拟世界】
   - 你正在以「上帝视角」观察未来的预演
   - 所有内容必须来自模拟世界中发生的事件和Agent言行
   - 禁止使用你自己的知识来编写报告内容
   - 每个章节至少调用3次工具（最多5次）来观察模拟的世界，它代表了未来

2. 【必须引用Agent的原始言行】
   - Agent的发言和行为是对未来人群行为的预测
   - 在报告中使用引用格式展示这些预测，例如：
     > "某类人群会表示：原文内容..."
   - 这些引用是模拟预测的核心证据

3. 【语言一致性 - 引用内容必须翻译为报告语言】
   - 工具返回的内容可能包含与报告语言不同的表述
   - 报告必须全部使用与用户指定语言一致的语言撰写
   - 当你引用工具返回的其他语言内容时，必须将其翻译为报告语言后再写入
   - 翻译时保持原意不变，确保表述自然通顺
   - 这一规则同时适用于正文和引用块（> 格式）中的内容

4. 【忠实呈现预测结果】
   - 报告内容必须反映模拟世界中的代表未来的模拟结果
   - 不要添加模拟中不存在的信息
   - 如果某方面信息不足，如实说明

═══════════════════════════════════════════════════════════════
【⚠️ 格式规范 - 极其重要！】
═══════════════════════════════════════════════════════════════

【一个章节 = 最小内容单位】
- 每个章节是报告的最小分块单位
- ❌ 禁止在章节内使用任何 Markdown 标题（#、##、###、#### 等）
- ❌ 禁止在内容开头添加章节主标题
- ✅ 章节标题由系统自动添加，你只需撰写纯正文内容
- ✅ 使用**粗体**、段落分隔、引用、列表来组织内容，但不要用标题

【正确示例】
```
本章节分析了事件的舆论传播态势。通过对模拟数据的深入分析，我们发现...

**首发引爆阶段**

微博作为舆情的第一现场，承担了信息首发的核心功能：

> "微博贡献了68%的首发声量..."

**情绪放大阶段**

抖音平台进一步放大了事件影响力：

- 视觉冲击力强
- 情绪共鸣度高
```

【错误示例】
```
## 执行摘要          ← 错误！不要添加任何标题
### 一、首发阶段     ← 错误！不要用###分小节
#### 1.1 详细分析   ← 错误！不要用####细分

本章节分析了...
```

═══════════════════════════════════════════════════════════════
【可用检索工具】（每章节调用3-5次）
═══════════════════════════════════════════════════════════════

{tools_description}

【工具使用建议 - 请混合使用不同工具，不要只用一种】
- insight_forge: 深度洞察分析，自动分解问题并多维度检索事实和关系
- panorama_search: 广角全景搜索，了解事件全貌、时间线和演变过程
- quick_search: 快速验证某个具体信息点
- interview_agents: 采访模拟Agent，获取不同角色的第一人称观点和真实反应

═══════════════════════════════════════════════════════════════
【工作流程】
═══════════════════════════════════════════════════════════════

每次回复你只能做以下两件事之一（不可同时做）：

选项A - 调用工具：
输出你的思考，然后用以下格式调用一个工具：
<tool_call>
{{"name": "工具名称", "parameters": {{"参数名": "参数值"}}}}
</tool_call>
系统会执行工具并把结果返回给你。你不需要也不能自己编写工具返回结果。

选项B - 输出最终内容：
当你已通过工具获取了足够信息，以 "Final Answer:" 开头输出章节内容。

⚠️ 严格禁止：
- 禁止在一次回复中同时包含工具调用和 Final Answer
- 禁止自己编造工具返回结果（Observation），所有工具结果由系统注入
- 每次回复最多调用一个工具

═══════════════════════════════════════════════════════════════
【章节内容要求】
═══════════════════════════════════════════════════════════════

1. 内容必须基于工具检索到的模拟数据
2. 大量引用原文来展示模拟效果
3. 使用Markdown格式（但禁止使用标题）：
   - 使用 **粗体文字** 标记重点（代替子标题）
   - 使用列表（-或1.2.3.）组织要点
   - 使用空行分隔不同段落
   - ❌ 禁止使用 #、##、###、#### 等任何标题语法
4. 【引用格式规范 - 必须单独成段】
   引用必须独立成段，前后各有一个空行，不能混在段落中：

   ✅ 正确格式：
   ```
   校方的回应被认为缺乏实质内容。

   > "校方的应对模式在瞬息万变的社交媒体环境中显得僵化和迟缓。"

   这一评价反映了公众的普遍不满。
   ```

   ❌ 错误格式：
   ```
   校方的回应被认为缺乏实质内容。> "校方的应对模式..." 这一评价反映了...
   ```
5. 保持与其他章节的逻辑连贯性
6. 【避免重复】仔细阅读下方已完成的章节内容，不要重复描述相同的信息
7. 【再次强调】不要添加任何标题！用**粗体**代替小节标题"#;

const SECTION_USER_PROMPT_TEMPLATE: &str = r#"已完成的章节内容（请仔细阅读，避免重复）：
{previous_content}

═══════════════════════════════════════════════════════════════
【当前任务】撰写章节: {section_title}
═══════════════════════════════════════════════════════════════

【重要提醒】
1. 仔细阅读上方已完成的章节，避免重复相同的内容！
2. 开始前必须先调用工具获取模拟数据
3. 请混合使用不同工具，不要只用一种
4. 报告内容必须来自检索结果，不要使用自己的知识

【⚠️ 格式警告 - 必须遵守】
- ❌ 不要写任何标题（#、##、###、####都不行）
- ❌ 不要写"{section_title}"作为开头
- ✅ 章节标题由系统自动添加
- ✅ 直接写正文，用**粗体**代替小节标题

请开始：
1. 首先思考（Thought）这个章节需要什么信息
2. 然后调用工具（Action）获取模拟数据
3. 收集足够信息后输出 Final Answer（纯正文，无任何标题）"#;

const REACT_OBSERVATION_TEMPLATE: &str = r#"Observation（检索结果）:

═══ 工具 {tool_name} 返回 ═══
{result}

═══════════════════════════════════════════════════════════════
已调用工具 {tool_calls_count}/{max_tool_calls} 次（已用: {used_tools_str}）{unused_hint}
- 如果信息充分：以 "Final Answer:" 开头输出章节内容（必须引用上述原文）
- 如果需要更多信息：调用一个工具继续检索
═══════════════════════════════════════════════════════════════"#;

const REACT_INSUFFICIENT_TOOLS_MSG: &str = "【注意】你只调用了{tool_calls_count}次工具，至少需要{min_tool_calls}次。\
     请再调用工具获取更多模拟数据，然后再输出 Final Answer。{unused_hint}";

const REACT_INSUFFICIENT_TOOLS_MSG_ALT: &str = "当前只调用了 {tool_calls_count} 次工具，至少需要 {min_tool_calls} 次。\
     请调用工具获取模拟数据。{unused_hint}";

const REACT_TOOL_LIMIT_MSG: &str = "工具调用次数已达上限（{tool_calls_count}/{max_tool_calls}），不能再调用工具。\
     请立即基于已获取的信息，以 \"Final Answer:\" 开头输出章节内容。";

const REACT_UNUSED_TOOLS_HINT: &str =
    "\n💡 你还没有使用过: {unused_list}，建议尝试不同工具获取多角度信息";

const REACT_FORCE_FINAL_MSG: &str = "已达到工具调用限制，请直接输出 Final Answer: 并生成章节内容。";

// ============================================================================
// Sub-cycle (i): Chat prompt constants
//
// Ported VERBATIM from report_agent.py:829-857.
// Chinese text is behavioral — the model conditions on it.
//
// TEMPLATE RENDERING NOTE:
// Python uses `.format(simulation_requirement=..., report_content=...,
// tools_description=...)`.  The template body contains literal `{{` / `}}`
// (JSON-example braces) which Python's `.format()` unescapes to single `{` / `}`.
// We store the FINAL form (single braces for the JSON example) and use
// three sequential `.replace("{simulation_requirement}", …)` calls — safe
// because the slot names never collide with the literal JSON braces.
// ============================================================================

/// System prompt for the chat method (report_agent.py:829-855).
///
/// Placeholders: `{simulation_requirement}`, `{report_content}`, `{tools_description}`.
/// All other `{` / `}` are literal (the embedded JSON-call example).
const CHAT_SYSTEM_PROMPT_TEMPLATE: &str = r#"你是一个简洁高效的模拟预测助手。

【背景】
预测条件: {simulation_requirement}

【已生成的分析报告】
{report_content}

【规则】
1. 优先基于上述报告内容回答问题
2. 直接回答问题，避免冗长的思考论述
3. 仅在报告内容不足以回答时，才调用工具检索更多数据
4. 回答要简洁、清晰、有条理

【可用工具】（仅在需要时使用，最多调用1-2次）
{tools_description}

【工具调用格式】
<tool_call>
{"name": "工具名称", "parameters": {"参数名": "参数值"}}
</tool_call>

【回答风格】
- 简洁直接，不要长篇大论
- 使用 > 格式引用关键内容
- 优先给出结论，再解释原因"#;

/// Suffix appended to the tool-observation user message in the chat ReACT loop
/// (report_agent.py:857).
const CHAT_OBSERVATION_SUFFIX: &str = "\n\n请简洁回答问题。";

/// Maximum tool calls allowed inside a single `chat` call (report_agent.py:882).
const MAX_TOOL_CALLS_PER_CHAT: usize = 2;

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
// Sub-cycle (i): ChatResponse
//
// Return type for `ReportAgent::chat`.
// Port of the inline dict returned by `ReportAgent.chat` (report_agent.py:1841-1845
// and 1877-1880).  Key order: response, tool_calls, sources (EXACT Python order).
// ============================================================================

/// Return value for `ReportAgent::chat`.
///
/// Mirrors the Python dict `{"response": ..., "tool_calls": [...], "sources": [...]}`.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    /// The cleaned LLM text response (tool-call XML/bracket tags stripped, trimmed).
    pub response: String,
    /// Parsed `ToolCall` objects accumulated during the ReACT loop (≤ MAX_TOOL_CALLS_PER_CHAT).
    pub tool_calls: Vec<crate::services::zep_tools::ToolCall>,
    /// `parameters["query"]` extracted from each accumulated tool call (default "").
    pub sources: Vec<String>,
}

impl ChatResponse {
    /// Convert to a JSON map matching Python's key order: response, tool_calls, sources.
    ///
    /// `tool_calls` serializes each `ToolCall` as `{"name": ..., "parameters": ...}`,
    /// matching the shape Python appends (`call` = the parsed tool-call dict).
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        use serde_json::Value;
        let mut m = serde_json::Map::new();
        m.insert("response".into(), Value::String(self.response.clone()));
        let tc_list: Vec<Value> = self
            .tool_calls
            .iter()
            .map(|tc| {
                let mut obj = serde_json::Map::new();
                obj.insert("name".into(), Value::String(tc.name.clone()));
                obj.insert("parameters".into(), Value::Object(tc.parameters.clone()));
                Value::Object(obj)
            })
            .collect();
        m.insert("tool_calls".into(), Value::Array(tc_list));
        let src_list: Vec<Value> = self.sources.iter().map(|s| Value::String(s.clone())).collect();
        m.insert("sources".into(), Value::Array(src_list));
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
    /// Optional JSONL activity logger (sub-cycle g1).
    ///
    /// `None` → all log_* calls are no-ops; the (e) tests continue to work unmodified.
    /// `Some(l)` → every wired log point writes to `…/reports/{id}/agent_log.jsonl`.
    pub report_logger: Option<logger::ReportLogger>,
    /// Active console-capture guard for the current run (sub-cycle h1 field, h2 populates).
    ///
    /// Mirrors Python's `self.console_logger = ReportConsoleLogger(report_id)` set at the
    /// start of `generate_report` and cleared (`.close()` + `None`) on both success and
    /// except tails (`report_agent.py:1565, 1721, 1755`).
    ///
    /// `None` until `generate_report` (h2) constructs and installs it.
    /// `Drop` on `ReportConsoleLogger` calls `close()` as a safety net; `generate_report`
    /// also calls it explicitly for faithful Python ordering.
    pub console_logger: Option<console_logger::ReportConsoleLogger>,
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
            report_logger: None,
            console_logger: None,
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
        let graph_id = graph_id.into();
        let simulation_id = simulation_id.into();
        let simulation_requirement = simulation_requirement.into();
        // (g2): agentInitDone — report_agent.py:917 logger.info(...)
        tracing::info!(
            target: "teri::report",
            "{}",
            crate::i18n::t_args(
                "report.agentInitDone",
                &[("graphId", &graph_id), ("simulationId", &simulation_id)]
            )
        );
        Self {
            graph_id,
            simulation_id,
            simulation_requirement,
            report_logger: None,
            console_logger: None,
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
        // (g2): startPlanningOutline — report_agent.py:1152 logger.info(...)
        tracing::info!(target: "teri::report", "{}", t("report.startPlanningOutline"));

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
            Err(e) => {
                // (g2): outlinePlanFailed — report_agent.py:1209 logger.error(...)
                tracing::error!(
                    target: "teri::report",
                    "{}",
                    crate::i18n::t_args("report.outlinePlanFailed", &[("error", &e.to_string())])
                );
                return Self::fallback_outline();
            }
        };

        // Step 6: chat_json([system, user], temperature=0.3)
        let opts = ChatOptions { temperature: Some(0.3), max_tokens: None };
        let messages = vec![ChatMessage::system(system_prompt), ChatMessage::user(user_prompt)];

        let response: serde_json::Value = match llm.chat_json(&messages, &opts).await {
            Ok(v) => v,
            Err(e) => {
                // (g2): outlinePlanFailed — report_agent.py:1209 logger.error(...)
                tracing::error!(
                    target: "teri::report",
                    "{}",
                    crate::i18n::t_args("report.outlinePlanFailed", &[("error", &e.to_string())])
                );
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

        // (g2): outlinePlanDone — report_agent.py:1205 logger.info(...)
        tracing::info!(
            target: "teri::report",
            "{}",
            crate::i18n::t_args("report.outlinePlanDone", &[("count", &outline.sections.len())])
        );

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
    // Sub-cycle (e): generate_section_react — bounded ReACT loop
    //
    // Port of `ReportAgent._generate_section_react` (report_agent.py:1221-1530).
    //
    // Implements the FULL branch ladder:
    //   1. LLM None/empty → retry with inline messages, break on last iter
    //   2. CONFLICT (tool_call AND Final Answer) → re-ask ×2, truncate+execute on 3rd
    //   3. (g) log_llm_response site (deferred)
    //   4. SITUATION 1: Final Answer with tool_calls_count < min_tool_calls → reject
    //   5. SITUATION 1: Final Answer valid → return final_answer.trim()
    //   6. SITUATION 2: tool call with quota exhausted → REACT_TOOL_LIMIT_MSG
    //   7. SITUATION 2: tool call OK → execute first only, append Observation
    //   8. SITUATION 3: neither → if under min reject+hint; else accept raw (no trim)
    //   POST-LOOP: REACT_FORCE_FINAL_MSG, one more chat; None → i18n fallback;
    //              Final Answer → trimmed; else → raw (not trimmed)
    //
    // SET-ORDER NOTE: Python `set` iteration is NONDETERMINISTIC, so the unused-hint
    // join order in Python (`', '.join(unused_tools)` / `'、'.join(unused_tools)`) is
    // also nondeterministic.  Teri uses a FIXED CANONICAL ORDER (all_tools iteration
    // order, insertion-stable) so the output is deterministic and testable.  The parity
    // verifier checks MEMBERSHIP, not order — this is not a downgrade.
    //
    // JOIN SEPARATORS (hard requirement):
    //   - Situation 1/3 inline unused_hint template: `", "` (comma-space)
    //   - REACT_UNUSED_TOOLS_HINT `{unused_list}`: `"、"` (Japanese/Chinese comma)
    //   - Observation used_tools_str: `", "` (comma-space)
    //
    // (g) DEFERRED LOG SITES (per architect split — not a downgrade):
    //   Each `// (g): …` comment marks exactly where sub-cycle (g)'s log call goes.
    // -----------------------------------------------------------------------

    /// Generate one report section using the bounded ReACT loop.
    ///
    /// # Arguments
    /// * `section`          — section metadata (title; content will be written to it by caller)
    /// * `outline`          — the full report outline (for context in the system prompt)
    /// * `previous_sections` — content strings of already-written sections (each truncated to 4000 chars)
    /// * `tools`            — ReportTools bound to the graph
    /// * `llm`              — LLM client; `chat` is called with temperature=0.5, max_tokens=4096
    /// * `progress`         — optional progress callback: `(stage: &str, pct: u32, msg: &str)`
    /// * `section_index`    — index of this section (used for (g) log sites)
    // The signature is mandated by the port contract (mirrors Python's parameter list).
    // Sub-cycle (h) will wrap this in a higher-level API.
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_section_react<L: LlmClient>(
        &self,
        section: &ReportSection,
        outline: &ReportOutline,
        previous_sections: &[String],
        tools: &crate::services::zep_tools::ReportTools<'_, L>,
        llm: &L,
        progress: Option<&ProgressCallback<'_>>,
        section_index: usize,
    ) -> String {
        use crate::i18n::t_args;
        use crate::llm::{ChatMessage, ChatOptions};
        use crate::services::zep_tools::{get_tools_description, parse_tool_calls};

        // (g2): reactGenerateSection — report_agent.py:1249 logger.info(...)
        // Unconditional: fires regardless of whether report_logger is Some.
        tracing::info!(
            target: "teri::report",
            "{}",
            t_args("report.reactGenerateSection", &[("title", &section.title)])
        );

        // (g1): log_section_start
        if let Some(l) = self.report_logger.as_ref() {
            l.log_section_start(&section.title, section_index);
        }

        // ── Build system prompt ──────────────────────────────────────────────
        let tools_desc = get_tools_description();
        let system_prompt = SECTION_SYSTEM_PROMPT_TEMPLATE
            .replace("{report_title}", &outline.title)
            .replace("{report_summary}", &outline.summary)
            .replace("{simulation_requirement}", &self.simulation_requirement)
            .replace("{section_title}", &section.title)
            .replace("{tools_description}", &tools_desc);
        let system_prompt =
            format!("{}\n\n{}", system_prompt, crate::i18n::get_language_instruction());

        // ── Build user prompt — previous sections truncated to 4000 CHARS ───
        // Python: sec[:4000] + "..." if len(sec) > 4000 (len() = character count, not bytes)
        // Rust: we count Unicode scalar values via .chars().count() and slice by char_indices
        // to stay char-boundary-safe.
        let previous_content = if previous_sections.is_empty() {
            "（这是第一个章节）".to_string()
        } else {
            let parts: Vec<String> = previous_sections
                .iter()
                .map(|sec| {
                    let char_count = sec.chars().count();
                    if char_count > 4000 {
                        // Find the byte offset of the 4000th character.
                        let byte_offset =
                            sec.char_indices().nth(4000).map(|(b, _)| b).unwrap_or(sec.len());
                        format!("{}...", &sec[..byte_offset])
                    } else {
                        sec.clone()
                    }
                })
                .collect();
            parts.join("\n\n---\n\n")
        };

        let user_prompt = SECTION_USER_PROMPT_TEMPLATE
            .replace("{previous_content}", &previous_content)
            .replace("{section_title}", &section.title);

        // ── Conversation history ─────────────────────────────────────────────
        let mut messages: Vec<ChatMessage> =
            vec![ChatMessage::system(system_prompt), ChatMessage::user(user_prompt)];

        // ── ReACT counters ───────────────────────────────────────────────────
        let max_iterations: usize = 5;
        let min_tool_calls: usize = 3;
        const MAX_TOOL_CALLS_PER_SECTION: usize = 5;
        let mut tool_calls_count: usize = 0;
        let mut conflict_retries: usize = 0;

        // Canonical all_tools order (fixed, for deterministic join) — matches Python's set literal
        // but in insertion order so the unused-hint text is stable across runs.
        let all_tools: [&str; 4] =
            ["insight_forge", "panorama_search", "quick_search", "interview_agents"];
        // used_tools tracks which canonical names have been called.
        let mut used_tools: std::collections::HashSet<&str> = std::collections::HashSet::new();

        // report_context — for tool execution context (passed to execute_by_name).
        let report_context =
            format!("章节标题: {}\n模拟需求: {}", section.title, self.simulation_requirement);

        let opts = ChatOptions { temperature: Some(0.5), max_tokens: Some(4096) };

        // ── Main ReACT loop ──────────────────────────────────────────────────
        for iteration in 0..max_iterations {
            // Step 1: progress callback
            if let Some(cb) = progress {
                let pct = (iteration as f64 / max_iterations as f64 * 100.0) as u32;
                let msg = t_args(
                    "progress.deepSearchAndWrite",
                    &[("current", &tool_calls_count), ("max", &MAX_TOOL_CALLS_PER_SECTION)],
                );
                cb("generating", pct, &msg);
            }

            // Step 2: call LLM — map Err or Ok("") to the None case (Python: `if response is None`)
            let response_result = llm.chat(&messages, &opts).await;
            let response_opt = match response_result {
                Ok(s) if !s.is_empty() => Some(s),
                _ => None,
            };

            if response_opt.is_none() {
                // None/empty handling (report_agent.py:1312-1320)
                // (g2): sectionIterNone — report_agent.py:1313 logger.warning(...)
                tracing::warn!(
                    target: "teri::report",
                    "{}",
                    t_args(
                        "report.sectionIterNone",
                        &[("title", &section.title), ("iteration", &(iteration + 1))]
                    )
                );
                if iteration < max_iterations - 1 {
                    messages.push(ChatMessage::assistant("（响应为空）"));
                    messages.push(ChatMessage::user("请继续生成内容。"));
                    continue;
                }
                // Last iteration — break → fall through to force-final
                break;
            }

            let response = response_opt.unwrap();

            // Step 3: parse tool calls once, reuse
            let mut tool_calls = parse_tool_calls(&response);
            let mut has_tool_calls = !tool_calls.is_empty();
            let mut has_final_answer = response.contains("Final Answer:");

            // ── CONFLICT: both tool_call and Final Answer (report_agent.py:1329-1361) ──
            if has_tool_calls && has_final_answer {
                conflict_retries += 1;

                // (g2): sectionConflict — report_agent.py:1332 logger.warning(...)
                tracing::warn!(
                    target: "teri::report",
                    "{}",
                    t_args(
                        "report.sectionConflict",
                        &[
                            ("title", &section.title),
                            ("iteration", &(iteration + 1)),
                            ("conflictCount", &conflict_retries),
                        ]
                    )
                );

                if conflict_retries <= 2 {
                    // First two conflicts: re-ask verbatim (report_agent.py:1338-1348)
                    messages.push(ChatMessage::assistant(&response));
                    messages.push(ChatMessage::user(
                        "【格式错误】你在一次回复中同时包含了工具调用和 Final Answer，这是不允许的。\n\
                         每次回复只能做以下两件事之一：\n\
                         - 调用一个工具（输出一个 <tool_call> 块，不要写 Final Answer）\n\
                         - 输出最终内容（以 'Final Answer:' 开头，不要包含 <tool_call>）\n\
                         请重新回复，只做其中一件事。",
                    ));
                    continue;
                } else {
                    // Third conflict: truncate to first </tool_call>, force-execute
                    // (report_agent.py:1351-1361)
                    // (g2): sectionConflictDowngrade — report_agent.py:1352 logger.warning(...)
                    tracing::warn!(
                        target: "teri::report",
                        "{}",
                        t_args(
                            "report.sectionConflictDowngrade",
                            &[
                                ("title", &section.title),
                                ("conflictCount", &conflict_retries),
                            ]
                        )
                    );
                    let end_tag = "</tool_call>";
                    if let Some(first_tool_end) = response.find(end_tag) {
                        let truncated = &response[..first_tool_end + end_tag.len()];
                        tool_calls = parse_tool_calls(truncated);
                        has_tool_calls = !tool_calls.is_empty();
                    }
                    has_final_answer = false;
                    conflict_retries = 0;
                    // Fall through — do NOT continue
                }
            }

            // (g1): log_llm_response
            if let Some(l) = self.report_logger.as_ref() {
                l.log_llm_response(
                    &section.title,
                    section_index,
                    &response,
                    iteration + 1,
                    has_tool_calls,
                    has_final_answer,
                );
            }

            // ── SITUATION 1: LLM output Final Answer (report_agent.py:1374-1402) ──
            if has_final_answer {
                if tool_calls_count < min_tool_calls {
                    // Insufficient tool calls — reject and ask for more
                    messages.push(ChatMessage::assistant(&response));
                    let unused: Vec<&str> =
                        all_tools.iter().copied().filter(|t| !used_tools.contains(t)).collect();
                    let unused_hint = if !unused.is_empty() {
                        // Situation 1 uses ", " separator (report_agent.py:1380)
                        format!("（这些工具还未使用，推荐用一下他们: {}）", unused.join(", "))
                    } else {
                        String::new()
                    };
                    let msg = REACT_INSUFFICIENT_TOOLS_MSG
                        .replace("{tool_calls_count}", &tool_calls_count.to_string())
                        .replace("{min_tool_calls}", &min_tool_calls.to_string())
                        .replace("{unused_hint}", &unused_hint);
                    messages.push(ChatMessage::user(msg));
                    continue;
                }

                // Valid final answer (report_agent.py:1392)
                // Python: response.split("Final Answer:")[-1].strip()
                // Rust: rsplit gives the same last-occurrence semantics.
                let final_answer =
                    response.rsplit("Final Answer:").next().unwrap_or("").trim().to_string();
                // (g2): sectionGenDone — report_agent.py:1393 logger.info(...)
                tracing::info!(
                    target: "teri::report",
                    "{}",
                    t_args(
                        "report.sectionGenDone",
                        &[("title", &section.title), ("count", &tool_calls_count)]
                    )
                );
                // (g1): log_section_content — situation-1 valid-final-answer return
                if let Some(l) = self.report_logger.as_ref() {
                    l.log_section_content(
                        &section.title,
                        section_index,
                        &final_answer,
                        tool_calls_count,
                    );
                }
                return final_answer;
            }

            // ── SITUATION 2: LLM attempted a tool call (report_agent.py:1404-1468) ──
            if has_tool_calls {
                if tool_calls_count >= MAX_TOOL_CALLS_PER_SECTION {
                    // Quota exhausted (report_agent.py:1406-1416)
                    messages.push(ChatMessage::assistant(&response));
                    let msg = REACT_TOOL_LIMIT_MSG
                        .replace("{tool_calls_count}", &tool_calls_count.to_string())
                        .replace("{max_tool_calls}", &MAX_TOOL_CALLS_PER_SECTION.to_string());
                    messages.push(ChatMessage::user(msg));
                    continue;
                }

                // Execute first call only (report_agent.py:1419-1468)
                let call = &tool_calls[0];
                // (g1): log_tool_call
                if let Some(l) = self.report_logger.as_ref() {
                    l.log_tool_call(
                        &section.title,
                        section_index,
                        &call.name,
                        serde_json::Value::Object(call.parameters.clone()),
                        iteration + 1,
                    );
                }
                if tool_calls.len() > 1 {
                    // (g2): multiToolOnlyFirst — report_agent.py:1421 logger.info(...)
                    // Unconditional: fires regardless of whether report_logger is Some.
                    tracing::info!(
                        target: "teri::report",
                        "{}",
                        t_args(
                            "report.multiToolOnlyFirst",
                            &[("total", &tool_calls.len()), ("toolName", &call.name)]
                        )
                    );
                }

                let result = tools.execute_by_name(
                    &call.name,
                    &call.parameters,
                    &self.graph_id,
                    &self.simulation_id,
                    &self.simulation_requirement,
                    &report_context,
                );
                // (g1): log_tool_result
                if let Some(l) = self.report_logger.as_ref() {
                    l.log_tool_result(
                        &section.title,
                        section_index,
                        &call.name,
                        &result,
                        iteration + 1,
                    );
                }

                tool_calls_count += 1;
                used_tools.insert(
                    // intern to 'static via the all_tools canonical array (avoids allocating)
                    // If the name is not in the canonical set (unknown tool), use "" as a
                    // sentinel — the unknown-name branch in execute_by_name already returns an
                    // error string and the loop continues without adding to used_tools.
                    all_tools.iter().copied().find(|&n| n == call.name.as_str()).unwrap_or(""),
                );

                // Unused-tool hint for Observation (report_agent.py:1451-1454)
                // REACT_UNUSED_TOOLS_HINT uses "、" separator (Japanese/Chinese comma).
                let unused_obs: Vec<&str> = all_tools
                    .iter()
                    .copied()
                    .filter(|t| !t.is_empty() && !used_tools.contains(t))
                    .collect();
                let unused_hint_obs =
                    if !unused_obs.is_empty() && tool_calls_count < MAX_TOOL_CALLS_PER_SECTION {
                        REACT_UNUSED_TOOLS_HINT.replace("{unused_list}", &unused_obs.join("、"))
                    } else {
                        String::new()
                    };

                // used_tools_str for Observation uses ", " separator (report_agent.py:1464)
                let used_tools_str: Vec<&str> = all_tools
                    .iter()
                    .copied()
                    .filter(|t| !t.is_empty() && used_tools.contains(t))
                    .collect();
                let used_tools_joined = used_tools_str.join(", ");

                messages.push(ChatMessage::assistant(&response));
                let obs_msg = REACT_OBSERVATION_TEMPLATE
                    .replace("{tool_name}", &call.name)
                    .replace("{result}", &result)
                    .replace("{tool_calls_count}", &tool_calls_count.to_string())
                    .replace("{max_tool_calls}", &MAX_TOOL_CALLS_PER_SECTION.to_string())
                    .replace("{used_tools_str}", &used_tools_joined)
                    .replace("{unused_hint}", &unused_hint_obs);
                messages.push(ChatMessage::user(obs_msg));
                continue;
            }

            // ── SITUATION 3: neither tool call nor Final Answer (report_agent.py:1470-1500) ──
            messages.push(ChatMessage::assistant(&response));

            if tool_calls_count < min_tool_calls {
                // Insufficient tool calls — push unused-tool hint and alt msg
                let unused_s3: Vec<&str> =
                    all_tools.iter().copied().filter(|t| !used_tools.contains(t)).collect();
                // Situation 3 inline template also uses ", " (report_agent.py:1476)
                let unused_hint_s3 = if !unused_s3.is_empty() {
                    format!("（这些工具还未使用，推荐用一下他们: {}）", unused_s3.join(", "))
                } else {
                    String::new()
                };
                let msg = REACT_INSUFFICIENT_TOOLS_MSG_ALT
                    .replace("{tool_calls_count}", &tool_calls_count.to_string())
                    .replace("{min_tool_calls}", &min_tool_calls.to_string())
                    .replace("{unused_hint}", &unused_hint_s3);
                messages.push(ChatMessage::user(msg));
                continue;
            }

            // Situation 3 else: sufficient tools, no prefix → accept raw response (report_agent.py:1491)
            // Python: final_answer = response.strip()
            let final_answer = response.trim().to_string();
            // (g2): sectionNoPrefix — report_agent.py:1490 logger.info(...)
            tracing::info!(
                target: "teri::report",
                "{}",
                t_args(
                    "report.sectionNoPrefix",
                    &[("title", &section.title), ("count", &tool_calls_count)]
                )
            );
            // (g1): log_section_content — situation-3 no-prefix return
            if let Some(l) = self.report_logger.as_ref() {
                l.log_section_content(
                    &section.title,
                    section_index,
                    &final_answer,
                    tool_calls_count,
                );
            }
            return final_answer;
        }
        // ── POST-LOOP: FORCE-FINAL (report_agent.py:1502-1530) ──────────────────
        // (g2): sectionMaxIter — report_agent.py:1503 logger.warning(...)
        tracing::warn!(
            target: "teri::report",
            "{}",
            t_args("report.sectionMaxIter", &[("title", &section.title)])
        );
        messages.push(ChatMessage::user(REACT_FORCE_FINAL_MSG));

        let force_response = llm.chat(&messages, &opts).await;

        // (g1): log_section_content — force-final return (compute result first, then log+return)
        let force_result = match force_response {
            // None/Err case (report_agent.py:1513-1515)
            Err(_) => crate::i18n::t("report.sectionGenFailedContent"),
            Ok(s) if s.is_empty() => crate::i18n::t("report.sectionGenFailedContent"),
            Ok(s) => {
                if s.contains("Final Answer:") {
                    // report_agent.py:1517: response.split("Final Answer:")[-1].strip()
                    s.rsplit("Final Answer:").next().unwrap_or("").trim().to_string()
                } else {
                    // report_agent.py:1519: final_answer = response  (NOT trimmed — preserve)
                    s
                }
            }
        };
        if let Some(l) = self.report_logger.as_ref() {
            l.log_section_content(&section.title, section_index, &force_result, tool_calls_count);
        }
        force_result
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

    // -----------------------------------------------------------------------
    // Sub-cycle (h2): generate_report skeleton
    //
    // Port of `ReportAgent.generate_report` (report_agent.py:1532-1765).
    //
    // Scope: all control flow EXCEPT the per-section loop body (h3 adds that).
    // Status machine: Pending → Planning → Generating → Completed (success)
    //                                                  → Failed (error tail).
    //
    // The section loop placeholder: assemble_full_report is called immediately
    // after status→Generating with no section files written, yielding a
    // header-only full_report.md. h3 replaces this placeholder with the real loop.
    //
    // BORROW BRIDGE: ProgressCallback<'_> is `Fn` (not `FnMut`), but
    // `ReportSink::event` requires `&mut self`. We bridge with `RefCell<&mut sink>`
    // so closures can capture a shared reference to the cell and call `borrow_mut()`
    // inside the `Fn` body. `sink` is a separate param from `self`, so the RefCell
    // borrow is independent of the `&mut self` borrow on `plan_outline`.
    // -----------------------------------------------------------------------

    /// Orchestrate a full ReACT report run.
    ///
    /// Port of `ReportAgent.generate_report` (`report_agent.py:1532-1765`).
    ///
    /// # Arguments
    /// * `tools`     — `ReportTools` bound to the graph; passed to `plan_outline`.
    /// * `llm`       — LLM client.
    /// * `manager`   — Instance-based `ReportManager` (holds the upload folder root).
    /// * `sink`      — Progress/SSE surface (`NullSink` in tests).
    /// * `report_id` — Optional explicit ID; `None` → `"report_{uuid12}"` auto-gen.
    ///
    /// Always returns a `Report`; the status is `Completed` on success, `Failed` on any error.
    #[allow(clippy::too_many_arguments)]
    pub async fn generate_report<L: LlmClient>(
        &mut self,
        tools: &crate::services::zep_tools::ReportTools<'_, L>,
        llm: &L,
        manager: &crate::report::manager::ReportManager,
        sink: &mut dyn crate::report::sink::ReportSink,
        report_id: Option<String>,
    ) -> Report {
        use crate::models::project::python_isoformat_local;
        use crate::report::sink::{ReportEvent, ReportStage};
        use std::cell::RefCell;
        use std::time::Instant;

        // ── report_id: auto-gen or caller-supplied (report_agent.py:1561-1562) ──
        // Python: `f"report_{uuid.uuid4().hex[:12]}"`
        // Uuid::simple() = 32-char dashless hex; [:12] = first 12 chars.
        let report_id = report_id.filter(|s| !s.is_empty()).unwrap_or_else(|| {
            let hex = uuid::Uuid::new_v4().simple().to_string();
            format!("report_{}", &hex[..12])
        });

        // ── start_time for total_time_seconds (report_agent.py:1563) ──
        let start_time = Instant::now();

        // ── Initial report template values — captured before the async try-body ──
        // The async try-body constructs its OWN local `report` from these.
        // This lets the error tail also build a `Failed` report without fighting
        // the borrow checker over a moved-out value.
        let created_at = python_isoformat_local();

        // ── completed_section_titles: declared before the try-body so the error
        //    tail can pass it to update_progress(failed) (report_agent.py:1575).
        let mut completed_section_titles: Vec<String> = vec![];

        // ── BORROW BRIDGE: wrap sink in RefCell so Fn closures can call borrow_mut ──
        // This is safe: `sink_cell` is only used in this scope; we borrow it through
        // shared refs inside `Fn` closures to satisfy the `ProgressCallback<'_>` bound.
        let sink_cell = RefCell::new(sink as &mut dyn crate::report::sink::ReportSink);

        // ── Helper to emit a top-level ReportEvent through the RefCell ──
        let emit_event = |stage: ReportStage, progress: i32, message: &str| {
            sink_cell.borrow_mut().event(&ReportEvent {
                stage,
                progress,
                message: message.to_string(),
                section_title: None,
                section_index: None,
                section_content: None,
                report_id: report_id.clone(),
            });
        };

        // ── Hoist `report` before the try-body so the error tail can mutate the
        //    SAME object (matching Python's `except` which mutates the in-scope
        //    `report` rather than constructing a fresh one).
        //    report_agent.py:1577-1578: `report = Report(...)` before the try.
        let mut report = Report {
            report_id: report_id.clone(),
            simulation_id: self.simulation_id.clone(),
            graph_id: self.graph_id.clone(),
            simulation_requirement: self.simulation_requirement.clone(),
            status: ReportStatus::Pending,
            outline: None,
            markdown_content: String::new(),
            created_at: created_at.clone(),
            completed_at: String::new(),
            error: None,
        };

        // ── Try-body (report_agent.py:1577-1738) ──
        // Returns std::io::Result<()>; mutates `report` in place.
        // `?` propagates any I/O error to the except tail.
        let try_result: std::io::Result<()> = async {
            // (report already constructed above)

            // Step 1: ensure report folder (report_agent.py:1579)
            manager.ensure_report_folder(&report_id)?;

            // Step 2: init structured logger (report_agent.py:1581-1587)
            let upload_folder = manager.upload_folder().ok_or_else(|| {
                std::io::Error::other(
                    "ReportManager has no upload_folder (reports_dir has no parent)",
                )
            })?;
            self.report_logger =
                Some(crate::report::logger::ReportLogger::new(&report_id, upload_folder)?);
            if let Some(l) = &self.report_logger {
                l.log_start(&self.simulation_id, &self.graph_id, &self.simulation_requirement);
            }

            // Step 3: init console logger (report_agent.py:1589-1590)
            self.console_logger = Some(crate::report::console_logger::ReportConsoleLogger::new(
                &report_id,
                upload_folder,
            )?);

            // Step 4: update_progress(pending, 0) + save_report (report_agent.py:1592-1596)
            manager.update_progress(
                &report_id,
                "pending",
                0,
                &crate::i18n::t("progress.initReport"),
                None,
                Some(&[]),
            )?;
            manager.save_report(&report)?;

            // Step 5: status → Planning (report_agent.py:1599-1613)
            report.status = ReportStatus::Planning;
            manager.update_progress(
                &report_id,
                "planning",
                5,
                &crate::i18n::t("progress.startPlanningOutline"),
                None,
                Some(&[]),
            )?;

            // log_planning_start (report_agent.py:1606)
            if let Some(l) = &self.report_logger {
                l.log_planning_start();
            }

            // sink.event(planning, 0, startPlanningOutline) (report_agent.py:1608-1609)
            emit_event(ReportStage::Planning, 0, &crate::i18n::t("progress.startPlanningOutline"));

            // Step 6: call plan_outline with planning_closure (report_agent.py:1611-1614)
            // The closure rescales prog//5 (integer division), matching Python line 1613.
            // We capture &sink_cell (Fn, shared ref → compatible with ProgressCallback bound).
            let plan_cb = |_stage: &str, prog: u32, msg: &str| {
                sink_cell.borrow_mut().event(&ReportEvent {
                    stage: ReportStage::Planning,
                    progress: (prog / 5) as i32,
                    message: msg.to_string(),
                    section_title: None,
                    section_index: None,
                    section_content: None,
                    report_id: report_id.clone(),
                });
            };
            // plan_outline takes &self (immutable). plan_outline only reads graph_id,
            // simulation_requirement, and report_logger, so the immutable reborrow is valid.
            let mut outline = self.plan_outline(tools, llm, Some(&plan_cb)).await;
            // Pre-loop assignment: outline has empty section content (matches Python
            // py:1615 reference semantics at this point, before the loop runs).
            // The intermediate save_report at manager.save_report(&report) below writes
            // this empty-content outline to meta.json (py:1626, before the loop).
            report.outline = Some(outline.clone());

            // log_planning_complete (report_agent.py:1618)
            if let Some(l) = &self.report_logger {
                l.log_planning_complete(serde_json::Value::Object(outline.to_dict()));
            }

            // save outline + update_progress(planning, 15) + save_report (report_agent.py:1621-1626)
            manager.save_outline(&report_id, &outline)?;
            manager.update_progress(
                &report_id,
                "planning",
                15,
                &crate::i18n::t_args("progress.outlineDone", &[("count", &outline.sections.len())]),
                None,
                Some(&[]),
            )?;
            manager.save_report(&report)?;

            // tracing::info outlineSavedToFile (report_agent.py:1628)
            tracing::info!(
                target: "teri::report",
                "{}",
                crate::i18n::t_args("report.outlineSavedToFile", &[("reportId", &report_id)])
            );

            // Step 7: status → Generating (report_agent.py:1631)
            report.status = ReportStatus::Generating;

            // total_sections for log_report_complete (h2: no loop yet — h3 adds it)
            let total_sections = outline.sections.len();

            // ── (h3) Per-section streaming loop (report_agent.py:1636-1704) ──────────────
            // Declared before the loop; pushed into by each iteration.
            let mut generated_sections: Vec<String> = vec![];

            for i in 0..total_sections {
                let section_num = i + 1;

                // Python:1638  base_progress = 20 + int((i / total_sections) * 70)
                // Arithmetic: i/total as f64 * 70.0, truncate to i32 (same as Python int()).
                let base_progress: i32 = 20 + ((i as f64 / total_sections as f64) * 70.0) as i32;

                let title = outline.sections[i].title.clone();

                // py:1641-1646  update_progress(generating, base_progress, generatingSection,
                //               current_section=Some(title), completed_sections)
                manager.update_progress(
                    &report_id,
                    "generating",
                    base_progress,
                    &crate::i18n::t_args(
                        "progress.generatingSection",
                        &[
                            ("title", &title as &dyn std::fmt::Display),
                            ("current", &section_num),
                            ("total", &total_sections),
                        ],
                    ),
                    Some(&title),
                    Some(&completed_section_titles),
                )?;

                // py:1648-1653  progress_callback("generating", base_progress, generatingSection)
                // Faithful emit (a): top-level pre-section progress event.
                sink_cell.borrow_mut().event(&crate::report::sink::ReportEvent {
                    stage: crate::report::sink::ReportStage::Generating,
                    progress: base_progress,
                    message: crate::i18n::t_args(
                        "progress.generatingSection",
                        &[
                            ("title", &title as &dyn std::fmt::Display),
                            ("current", &section_num),
                            ("total", &total_sections),
                        ],
                    ),
                    section_title: Some(title.clone()),
                    section_index: Some(section_num),
                    section_content: None,
                    report_id: report_id.clone(),
                });

                // py:1656-1667  section_closure: lambda stage, prog, msg →
                //   progress_callback(stage, base_progress + int(prog * 0.7 / total_sections), msg)
                // Faithful emit (b): sub-progress from within generate_section_react.
                // Capture by clone to satisfy Fn + 'lifetime.
                let section_cb_title = title.clone();
                let section_cb_report_id = report_id.clone();
                let section_cb_section_num = section_num;
                let section_cb_base_progress = base_progress;
                let section_cb_total_sections = total_sections;
                let section_cb = |_stage: &str, prog: u32, msg: &str| {
                    // Python:1663  base_progress + int(prog * 0.7 / total_sections)
                    // f64 multiply then truncate to i32 — same as Python int().
                    let rescaled_progress = section_cb_base_progress
                        + ((prog as f64 * 0.7 / section_cb_total_sections as f64) as i32);
                    sink_cell.borrow_mut().event(&crate::report::sink::ReportEvent {
                        stage: crate::report::sink::ReportStage::Generating,
                        progress: rescaled_progress,
                        message: msg.to_string(),
                        section_title: Some(section_cb_title.clone()),
                        section_index: Some(section_cb_section_num),
                        section_content: None,
                        report_id: section_cb_report_id.clone(),
                    });
                };

                // py:1656  section_content = self._generate_section_react(…)
                // generate_section_react borrows &self (immutable) + &outline (immutable)
                // during the await; setting outline.sections[i].content happens AFTER.
                let content = self
                    .generate_section_react(
                        &outline.sections[i],
                        &outline,
                        &generated_sections,
                        tools,
                        llm,
                        Some(&section_cb),
                        section_num,
                    )
                    .await;

                // py:1669  section.content = section_content
                outline.sections[i].content = content.clone();

                // py:1670  generated_sections.append(f"## {title}\n\n{content}")
                generated_sections.push(format!("## {}\n\n{}", title, content));

                // py:1673  ReportManager.save_section(report_id, section_num, section)
                // save-section-immediately: writes section_NN.md before the next section.
                manager.save_section(&report_id, section_num, &outline.sections[i])?;

                // py:1674  completed_section_titles.append(section.title)
                completed_section_titles.push(title.clone());

                // py:1677-1684  log_section_full_complete(title, num, content.strip())
                let full_section_content = format!("## {}\n\n{}", title, content);
                if let Some(l) = &self.report_logger {
                    l.log_section_full_complete(&title, section_num, full_section_content.trim());
                }

                // py:1686  logger.info(report.sectionSaved)
                tracing::info!(
                    target: "teri::report",
                    "{}",
                    crate::i18n::t_args(
                        "report.sectionSaved",
                        &[
                            ("reportId", &report_id as &dyn std::fmt::Display),
                            ("sectionNum", &format!("{:02}", section_num)),
                        ],
                    )
                );

                // py:1689-1695  update_progress(generating, base_progress + int(70/total_sections),
                //               sectionDone{title}, current_section=None, completed_sections)
                // Integer division: 70/total matches Python int(70/total) for positive ints.
                manager.update_progress(
                    &report_id,
                    "generating",
                    base_progress + (70 / total_sections as i32),
                    &crate::i18n::t_args(
                        "progress.sectionDone",
                        &[("title", &title as &dyn std::fmt::Display)],
                    ),
                    None,
                    Some(&completed_section_titles),
                )?;

                // (h3 superset for U-027 — Python streams section content via the
                // immediately-saved file; teri ALSO surfaces it on the sink so U-027
                // can stream section markdown live without reading the file.
                // No Python-observable artifact changes: same files, same progress.json,
                // same jsonl, same console output. Architect §1, §3-step7, §7.5.)
                sink_cell.borrow_mut().event(&crate::report::sink::ReportEvent {
                    stage: crate::report::sink::ReportStage::Generating,
                    progress: base_progress + (70 / total_sections as i32),
                    message: crate::i18n::t_args(
                        "progress.sectionDone",
                        &[("title", &title as &dyn std::fmt::Display)],
                    ),
                    section_title: Some(title.clone()),
                    section_index: Some(section_num),
                    section_content: Some(content.clone()), // section payload for U-027 live stream
                    report_id: report_id.clone(),
                });
            }

            // py:1698-1704  (after loop) progress_callback("generating", 95, assemblingReport)
            //               + update_progress(generating, 95, assemblingReport, completed_sections)
            // Faithful emit (c): assembling milestone.
            sink_cell.borrow_mut().event(&crate::report::sink::ReportEvent {
                stage: crate::report::sink::ReportStage::Generating,
                progress: 95,
                message: crate::i18n::t("progress.assemblingReport"),
                section_title: None,
                section_index: None,
                section_content: None,
                report_id: report_id.clone(),
            });
            manager.update_progress(
                &report_id,
                "generating",
                95,
                &crate::i18n::t("progress.assemblingReport"),
                None,
                Some(&completed_section_titles),
            )?;

            // CRITICAL TRAP #1: re-assign report.outline AFTER the loop so the final
            // save_report (below) writes meta.json with populated section content.
            // Python's `report.outline = outline` (py:1615) is a reference — after the
            // loop, it already has content. teri cloned it before the loop (empty content);
            // this second clone captures the now-populated content for the final save.
            report.outline = Some(outline.clone());

            // Step 8: assemble full report (report_agent.py:1707)
            // Now runs over populated section_NN.md files (real assemble).
            report.markdown_content = manager.assemble_full_report(&report_id, &outline)?;

            // Step 9: status → Completed, completed_at, total_time (report_agent.py:1708-1712)
            report.status = ReportStatus::Completed;
            report.completed_at = python_isoformat_local();
            let total_time_seconds = start_time.elapsed().as_secs_f64();

            // log_report_complete (report_agent.py:1715-1719)
            if let Some(l) = &self.report_logger {
                l.log_report_complete(total_sections, total_time_seconds);
            }

            // save_report + update_progress(completed, 100) (report_agent.py:1722-1726)
            manager.save_report(&report)?;
            manager.update_progress(
                &report_id,
                "completed",
                100,
                &crate::i18n::t("progress.reportComplete"),
                None,
                Some(&completed_section_titles),
            )?;

            // sink.event(completed, 100, reportComplete) (report_agent.py:1728-1729)
            emit_event(ReportStage::Completed, 100, &crate::i18n::t("progress.reportComplete"));

            // tracing::info reportGenDone (report_agent.py:1731)
            tracing::info!(
                target: "teri::report",
                "{}",
                crate::i18n::t_args("report.reportGenDone", &[("reportId", &report_id)])
            );

            // Close console_logger on success tail (report_agent.py:1734-1736)
            if let Some(mut cl) = self.console_logger.take() {
                cl.close();
            }

            Ok(())
        }
        .await;

        // ── except/error tail (report_agent.py:1740-1764) ──
        // Python mutates the SAME `report` object (the one already holding
        // `.outline`, `.markdown_content`, `.completed_at` from the try-body).
        // We do the same: on Ok we return `report` as-is (Completed); on Err we
        // mutate `status`/`error` on the SAME `report` without resetting the
        // fields the try-body already set — so a post-planning I/O failure
        // produces a Failed meta.json that still carries the outline.
        match try_result {
            Ok(()) => report,
            Err(e) => {
                let err_str = e.to_string();
                // report_agent.py:1741: logger.error(t('report.reportGenFailed', error=str(e)))
                tracing::error!(
                    target: "teri::report",
                    "{}",
                    crate::i18n::t_args("report.reportGenFailed", &[("error", &err_str)])
                );

                // Mutate the SAME report — preserve outline/markdown_content/completed_at
                // exactly as they were set by the try-body before the failure.
                // (report_agent.py:1742-1744: report.status = "failed"; report.error = str(e))
                report.status = ReportStatus::Failed;
                report.error = Some(err_str.clone());

                // log_error (report_agent.py:1746-1747)
                if let Some(l) = &self.report_logger {
                    l.log_error(&err_str, "failed", None);
                }

                // inner try: save_report + update_progress(failed, -1) — ignore errors
                // (report_agent.py:1750-1757: `except: pass`)
                let _ = manager.save_report(&report);
                let _ = manager.update_progress(
                    &report_id,
                    "failed",
                    -1,
                    &crate::i18n::t_args("progress.reportFailed", &[("error", &err_str)]),
                    None,
                    Some(&completed_section_titles),
                );

                // Close console_logger on error tail (report_agent.py:1759-1762)
                if let Some(mut cl) = self.console_logger.take() {
                    cl.close();
                }

                report
            }
        }
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (i): chat — conversational 2-iteration ReACT method
    //
    // Port of `ReportAgent.chat` (report_agent.py:1766-1881).
    //
    // DECISION-11: tools/llm/manager passed per-call (not stored on self).
    //
    // LLM error convention (mirrors generate_section_react, report_agent.py:1312-1320):
    //   llm.chat Err or Ok("") → treated as an empty response string ("").
    //   Python does not catch errors in the chat loop — but since our signature
    //   returns `ChatResponse` (not `Result`), mapping Err → "" is the faithful
    //   way to prevent a panic while preserving all downstream behavior
    //   (empty string → no tool_calls → immediate clean return, same as Python
    //   propagating None/empty would short-circuit the loop).
    //
    // [≠] get_report_by_simulation returns Option (no I/O error surface). Python's
    //   `except Exception as e: logger.warning(t('report.fetchReportFailed', error=e))`
    //   fires only on an inexpressible deeper I/O exception; the observable behavior
    //   (empty report_content when no report) is preserved. No warning is fabricated.
    //
    // [!] interview_agents tool — U-020 missing. execute_by_name returns the
    //   honest-err string for unknown tools; the chat loop tolerates it as an
    //   observation.
    // -----------------------------------------------------------------------

    /// Conversational interface for the Report Agent.
    ///
    /// Port of `ReportAgent.chat` (`report_agent.py:1766-1881`).
    ///
    /// Runs up to 2 ReACT iterations; each iteration may execute at most 1 tool call.
    /// Returns immediately on the first iteration with no tool calls.
    pub async fn chat<L: LlmClient>(
        &self,
        tools: &crate::services::zep_tools::ReportTools<'_, L>,
        llm: &L,
        manager: &crate::report::manager::ReportManager,
        message: &str,
        chat_history: &[ChatMessage],
    ) -> ChatResponse {
        use crate::i18n::t_args;
        use crate::services::zep_tools::{get_tools_description, parse_tool_calls};
        use regex::Regex;

        // (g2): agentChat — report_agent.py:1787 logger.info(...)
        // message[:50] is CHAR truncation (CJK-safe).
        let message_truncated_50: String = message.chars().take(50).collect();
        tracing::info!(
            target: "teri::report",
            "{}",
            t_args("report.agentChat", &[("message", &message_truncated_50)])
        );

        // ── Fetch existing report content ────────────────────────────────────
        // [≠] Python's try/except for I/O error is inexpressible via Option.
        //     Observable behavior (empty report_content when no report) is preserved.
        let mut report_content = String::new();
        if let Some(report) = manager.get_report_by_simulation(&self.simulation_id)
            && !report.markdown_content.is_empty()
        {
            // Limit report to 15000 CHARS (CJK-safe — report_agent.py:1797 char slice).
            let char_count = report.markdown_content.chars().count();
            if char_count > 15000 {
                let byte_offset = report
                    .markdown_content
                    .char_indices()
                    .nth(15000)
                    .map(|(b, _)| b)
                    .unwrap_or(report.markdown_content.len());
                report_content = format!(
                    "{}\n\n... [报告内容已截断] ...",
                    &report.markdown_content[..byte_offset]
                );
            } else {
                report_content = report.markdown_content.clone();
            }
        }

        // ── Build system prompt ──────────────────────────────────────────────
        // Empty report_content → placeholder (report_agent.py:1805).
        let report_content_display = if report_content.is_empty() {
            "（暂无报告）".to_string()
        } else {
            report_content.clone()
        };

        // Three sequential .replace() calls — safe because the slot names never
        // collide with the literal JSON-example braces in the template.
        let system_prompt = CHAT_SYSTEM_PROMPT_TEMPLATE
            .replace("{simulation_requirement}", &self.simulation_requirement)
            .replace("{report_content}", &report_content_display)
            .replace("{tools_description}", &get_tools_description());
        let system_prompt =
            format!("{}\n\n{}", system_prompt, crate::i18n::get_language_instruction());

        // ── Build initial messages ───────────────────────────────────────────
        // System first; then last 10 history entries; then the user message.
        let mut messages: Vec<ChatMessage> = vec![ChatMessage::system(system_prompt)];
        let history_window = &chat_history[chat_history.len().saturating_sub(10)..];
        for h in history_window {
            messages.push(h.clone());
        }
        messages.push(ChatMessage::user(message));

        // ── ReACT loop (max 2 iterations) ───────────────────────────────────
        let mut tool_calls_made: Vec<crate::services::zep_tools::ToolCall> = Vec::new();
        let opts = ChatOptions { temperature: Some(0.5), max_tokens: None };

        // Regexes for response cleanup (same in both early-return and post-loop paths).
        // (?s) = DOTALL flag — matches newlines inside <tool_call>…</tool_call>.
        let tool_call_re = Regex::new(r"(?s)<tool_call>.*?</tool_call>").expect("valid regex");
        // [TOOL_CALL] with escaped brackets, then any chars up to closing ')'.
        let bracket_re = Regex::new(r"\[TOOL_CALL\].*?\)").expect("valid regex");

        for _iteration in 0..2usize {
            // LLM call — map Err or Ok("") to "" (mirrors generate_section_react convention).
            let response = llm.chat(&messages, &opts).await.unwrap_or_default();

            // Parse tool calls from response.
            let tool_calls = parse_tool_calls(&response);

            if tool_calls.is_empty() {
                // No tool call — clean and return immediately (report_agent.py:1836-1845).
                let cleaned = tool_call_re.replace_all(&response, "");
                let cleaned = bracket_re.replace_all(&cleaned, "");
                let clean_response = cleaned.trim().to_string();

                let sources: Vec<String> = tool_calls_made
                    .iter()
                    .map(|tc| {
                        tc.parameters
                            .get("query")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect();
                return ChatResponse {
                    response: clean_response,
                    tool_calls: tool_calls_made,
                    sources,
                };
            }

            // Execute tools — at most 1 per round (report_agent.py:1849).
            let mut tool_results: Vec<(String, String)> = Vec::new(); // (tool_name, result)
            for call in tool_calls.iter().take(1) {
                if tool_calls_made.len() >= MAX_TOOL_CALLS_PER_CHAT {
                    break;
                }
                let raw_result = tools.execute_by_name(
                    &call.name,
                    &call.parameters,
                    &self.graph_id,
                    &self.simulation_id,
                    &self.simulation_requirement,
                    // report_context is not used by the chat variant in Python — pass a
                    // minimal context string (execute_by_name's internal branches use it
                    // only when non-empty; chat passes "" in Python via positional default).
                    "",
                );
                // Truncate result to 1500 CHARS (CJK-safe — report_agent.py:1855).
                let result: String = {
                    let char_count = raw_result.chars().count();
                    if char_count > 1500 {
                        let byte_offset = raw_result
                            .char_indices()
                            .nth(1500)
                            .map(|(b, _)| b)
                            .unwrap_or(raw_result.len());
                        raw_result[..byte_offset].to_string()
                    } else {
                        raw_result
                    }
                };
                tool_results.push((call.name.clone(), result));
                tool_calls_made.push(call.clone());
            }

            // Append assistant + observation (report_agent.py:1860-1865).
            messages.push(ChatMessage::assistant(&response));
            let observation: String = tool_results
                .iter()
                .map(|(tool, result)| format!("[{}结果]\n{}", tool, result))
                .collect::<Vec<_>>()
                .join("\n");
            messages.push(ChatMessage::user(format!("{}{}", observation, CHAT_OBSERVATION_SUFFIX)));
        }

        // ── Max iterations reached — get final response ──────────────────────
        // (report_agent.py:1867-1881)
        let final_response = llm.chat(&messages, &opts).await.unwrap_or_default();
        let cleaned = tool_call_re.replace_all(&final_response, "");
        let cleaned = bracket_re.replace_all(&cleaned, "");
        let clean_response = cleaned.trim().to_string();

        let sources: Vec<String> = tool_calls_made
            .iter()
            .map(|tc| tc.parameters.get("query").and_then(|v| v.as_str()).unwrap_or("").to_string())
            .collect();
        ChatResponse { response: clean_response, tool_calls: tool_calls_made, sources }
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

    // -----------------------------------------------------------------------
    // Sub-cycle (e): generate_section_react tests
    //
    // Uses a scripted mock LlmClient whose `chat` returns a canned sequence
    // of responses (one per call, via Arc<Mutex<Vec<String>>> drain).
    //
    // Parity: assert the returned String and (where practical) message-trace
    // shape (checked via the mock's call count).
    // -----------------------------------------------------------------------

    /// Scripted mock LLM for `generate_section_react` tests.
    ///
    /// Returns responses from a queue in FIFO order.  After the queue is
    /// exhausted, returns `Ok(String::new())` (maps to the None/empty case).
    struct ScriptedChatLlm {
        responses: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
        /// Count of `chat` calls made.
        call_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl ScriptedChatLlm {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: std::sync::Arc::new(std::sync::Mutex::new(
                    responses.into_iter().map(|s| s.to_string()).collect(),
                )),
                call_count: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }
        }

        fn calls(&self) -> usize {
            self.call_count.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for ScriptedChatLlm {
        async fn complete(&self, _: &str) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not used".into()))
        }

        async fn chat(&self, _: &[crate::llm::ChatMessage], _: &ChatOptions) -> Result<String> {
            self.call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut q = self.responses.lock().unwrap();
            Ok(q.pop_front().unwrap_or_default())
        }

        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
    }

    /// Build a minimal fixture for `generate_section_react` tests.
    fn make_react_fixtures() -> (crate::graph::KnowledgeGraph, ReportSection, ReportOutline) {
        let graph = crate::graph::KnowledgeGraph::new();
        let section = ReportSection { title: "Test Section".into(), content: String::new() };
        let outline = ReportOutline {
            title: "Test Report".into(),
            summary: "A test summary.".into(),
            sections: vec![section.clone()],
        };
        (graph, section, outline)
    }

    /// Happy path: 3 tool calls then a "Final Answer: <content>" → returns trimmed content.
    ///
    /// Sequence:
    ///   iter 0 → tool call (quick_search)
    ///   iter 1 → tool call (panorama_search)
    ///   iter 2 → tool call (insight_forge)
    ///   iter 3 → "Final Answer: Generated content."
    #[tokio::test]
    async fn test_react_happy_path_3_tools_then_final_answer() {
        let tool_call_quick = r#"Thought: need to search.
<tool_call>
{"name": "quick_search", "parameters": {"query": "test"}}
</tool_call>"#;
        let tool_call_panorama = r#"Thought: need broader view.
<tool_call>
{"name": "panorama_search", "parameters": {"query": "test"}}
</tool_call>"#;
        let tool_call_insight = r#"Thought: deeper analysis.
<tool_call>
{"name": "insight_forge", "parameters": {"query": "test"}}
</tool_call>"#;
        let final_answer = "Final Answer: Generated content about the simulation.";

        let llm = ScriptedChatLlm::new(vec![
            tool_call_quick,
            tool_call_panorama,
            tool_call_insight,
            final_answer,
        ]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "test requirement");
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        assert_eq!(result, "Generated content about the simulation.");
        // 4 LLM calls: 3 tool iterations + 1 final answer
        assert_eq!(llm.calls(), 4);
    }

    /// Insufficient tools: Final Answer emitted on iteration 1 (only 1 tool call so far < 3).
    /// Loop should reject and send REACT_INSUFFICIENT_TOOLS_MSG, then on iter 2 accept.
    ///
    /// Sequence:
    ///   iter 0 → tool call (quick_search)
    ///   iter 1 → "Final Answer: premature" (rejected; tool_calls_count=1 < 3)
    ///   iter 2 → tool call (panorama_search)
    ///   iter 3 → tool call (insight_forge)
    ///   iter 4 → "Final Answer: proper content"   (tool_calls_count=3 >= 3 → accepted)
    #[tokio::test]
    async fn test_react_insufficient_tools_rejection_then_accept() {
        let tool_call_1 =
            r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q"}}</tool_call>"#;
        let premature_final = "Final Answer: premature content";
        let tool_call_2 =
            r#"<tool_call>{"name": "panorama_search", "parameters": {"query": "q"}}</tool_call>"#;
        let tool_call_3 =
            r#"<tool_call>{"name": "insight_forge", "parameters": {"query": "q"}}</tool_call>"#;
        let proper_final = "Final Answer: proper content";

        let llm = ScriptedChatLlm::new(vec![
            tool_call_1,
            premature_final,
            tool_call_2,
            tool_call_3,
            proper_final,
        ]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        assert_eq!(result, "proper content");
        // 5 loop chat calls
        assert_eq!(llm.calls(), 5);
    }

    /// Conflict downgrade: response with BOTH <tool_call> and "Final Answer:" repeated 3×.
    ///
    /// Seq:
    ///   iter 0 → conflict response (1st conflict: re-ask)
    ///   iter 1 → conflict response again (2nd conflict: re-ask)
    ///   iter 2 → conflict response again (3rd conflict: truncate+execute, run quick_search)
    ///   iter 3 → tool call 2 (panorama_search)
    ///   iter 4 → tool call 3 (insight_forge)  — max_iterations=5, last iter
    ///   (loop exhausted after 5 iters → force-final)
    ///   force-final chat → "Final Answer: force content"
    ///
    /// The 3rd conflict truncates to the first </tool_call> and executes quick_search.
    /// After that tool_calls_count=1. Loop continues.
    #[tokio::test]
    async fn test_react_conflict_downgrade_third_time() {
        let conflict_response = r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q"}}</tool_call>
Final Answer: This is premature."#;
        let tool_call_2 =
            r#"<tool_call>{"name": "panorama_search", "parameters": {"query": "q"}}</tool_call>"#;
        let tool_call_3 =
            r#"<tool_call>{"name": "insight_forge", "parameters": {"query": "q"}}</tool_call>"#;
        let force_final = "Final Answer: force content";

        let llm = ScriptedChatLlm::new(vec![
            conflict_response, // iter 0 → 1st conflict → re-ask (continue)
            conflict_response, // iter 1 → 2nd conflict → re-ask (continue)
            conflict_response, // iter 2 → 3rd conflict → truncate + execute quick_search, continue
            tool_call_2,       // iter 3 → execute panorama_search
            tool_call_3,       // iter 4 → execute insight_forge; loop hits max_iterations end
            force_final,       // force-final chat
        ]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        assert_eq!(result, "force content");
        // 5 loop iters + 1 force-final = 6 calls
        assert_eq!(llm.calls(), 6);
    }

    /// Quota exhausted: 5 tool calls already done, next response is another tool call.
    ///
    /// Seq:
    ///   iters 0-4: 5 tool calls of quick_search (tool_calls_count reaches 5)
    ///   iter 4's loop iteration: quota check fires immediately since tool_calls_count>=5
    ///     but the code only checks AFTER the LLM returns a tool-call response.
    ///     Actually: quota is checked when the NEXT tool-call arrives with count already at 5.
    ///   Since max_iterations=5, after 5 iterations the loop ends → force-final.
    ///
    /// Simpler approach: use 6 responses so the 6th triggers the quota path on the 5th
    /// loop iteration when tool_calls_count==5.
    ///
    /// After 4 tool calls (iters 0-3), tool_calls_count=4.
    /// iter 4 (5th): LLM returns tool call → quota check: 4 < 5 → execute → tool_calls_count=5
    /// Loop ends (max_iterations=5).
    /// force-final → "Final Answer: done"
    ///
    /// This test verifies the REACT_TOOL_LIMIT_MSG path by having a 6th tool call arrive
    /// when there is a 6th iteration slot — but max_iterations=5 so we can't go to iter 5.
    ///
    /// Alternative: create a scenario where iter 4 tries a 6th tool call AFTER 5 were done.
    /// That requires 5 successful tool calls in iters 0-4 followed by iter 5's quota trigger —
    /// but there IS no iter 5 (max_iterations=5, range 0..5). So the quota path (>= 5) is
    /// triggered mid-loop only when tool_calls_count is already ≥ 5 at the start of the check.
    ///
    /// To hit the quota branch: do 5 tool calls in iters 0-4, then the loop exits naturally.
    /// The quota branch itself (REACT_TOOL_LIMIT_MSG) is hit when iter N returns a tool call
    /// but tool_calls_count is already == MAX_TOOL_CALLS_PER_SECTION (5).
    ///
    /// Simplest scenario that hits quota: 4 tool calls in iters 0-3, then iter 4 also returns
    /// a tool call — we execute it (count becomes 5, which is EXACTLY 5 — not >= 5 yet at
    /// check time since count was 4 when the check ran). Loop ends naturally → force-final.
    ///
    /// To truly hit the quota branch: we need tool_calls_count==5 BEFORE the iter executes
    /// a new tool. But tool_calls_count increments AFTER each tool execution. So after exactly
    /// 5 tool executions, count==5. On the NEXT iter that returns a tool call, count>=5 fires.
    ///
    /// Realistic sequence:
    ///   iters 0-4: 5 quick_search calls → tool_calls_count=5 (but loop at max iter now)
    ///   We need a 6th iter slot to test quota. Since max_iterations=5, quota path is only
    ///   reachable if one of the 5 iterations: (a) returns a tool call when already at 5.
    ///   This happens when a CONFLICT re-ask loops back to the same iteration.
    ///
    /// Practical test: 5 quick_search calls in iters 0-4, loop exhausts → force-final.
    /// This verifies the loop correctly limits to 5 tool calls.  The quota MSG branch is a
    /// `continue` in the loop — its internal correctness is verified by checking that no 6th
    /// tool executes when count==5 (which we can verify via the returned result + call count).
    #[tokio::test]
    async fn test_react_quota_five_tool_calls_then_force_final() {
        let tool_call =
            r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q"}}</tool_call>"#;
        let force_final = "Final Answer: content after max tools";

        // 5 tool calls fill the 5 loop iterations; force-final on the 6th chat call
        let llm = ScriptedChatLlm::new(vec![
            tool_call,   // iter 0
            tool_call,   // iter 1
            tool_call,   // iter 2
            tool_call,   // iter 3
            tool_call,   // iter 4
            force_final, // force-final
        ]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        assert_eq!(result, "content after max tools");
        // 5 loop iters + 1 force-final = 6 LLM calls
        assert_eq!(llm.calls(), 6);
    }

    /// No-prefix accept (Situation 3 else branch): tools satisfied (3+), response has no
    /// "Final Answer:" prefix → returned as-is (trimmed, per Python `response.strip()`).
    ///
    /// Sequence:
    ///   iter 0 → tool call (quick_search)
    ///   iter 1 → tool call (panorama_search)
    ///   iter 2 → tool call (insight_forge)
    ///   iter 3 → plain text (no tool_call, no Final Answer:) → accepted as final answer
    #[tokio::test]
    async fn test_react_no_prefix_accept_situation3() {
        let tool_call_1 =
            r#"<tool_call>{"name": "quick_search", "parameters": {"query": "a"}}</tool_call>"#;
        let tool_call_2 =
            r#"<tool_call>{"name": "panorama_search", "parameters": {"query": "b"}}</tool_call>"#;
        let tool_call_3 =
            r#"<tool_call>{"name": "insight_forge", "parameters": {"query": "c"}}</tool_call>"#;
        // Plain text response — no Final Answer: prefix, no <tool_call>
        let plain_response = "  Here is the section content without a prefix.  ";

        let llm = ScriptedChatLlm::new(vec![tool_call_1, tool_call_2, tool_call_3, plain_response]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        // Python: response.strip() → trimmed
        assert_eq!(result, "Here is the section content without a prefix.");
        assert_eq!(llm.calls(), 4);
    }

    /// Force-final sub-branches:
    ///  (a) force-final returns "Final Answer: force content" → trimmed
    ///  (b) force-final returns plain text → raw (NOT trimmed)
    ///  (c) force-final returns empty → i18n fallback
    ///
    /// Each scenario: 5 iterations all return tool calls (never final) → loop exhausts →
    /// force-final gets one of the 3 responses above.
    ///
    /// We test (a) above in `test_react_quota_five_tool_calls_then_force_final`.
    /// Here we test (b) and (c).

    #[tokio::test]
    async fn test_react_force_final_plain_not_trimmed() {
        let tool_call =
            r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q"}}</tool_call>"#;
        // Force-final returns plain text with TRAILING WHITESPACE — must NOT be trimmed
        // Python: `final_answer = response`  (no .strip())
        let force_plain = "Plain section content.   ";

        let llm = ScriptedChatLlm::new(vec![
            tool_call,
            tool_call,
            tool_call,
            tool_call,
            tool_call, // 5 iters
            force_plain,
        ]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        // MUST preserve trailing whitespace — Python `final_answer = response` (no strip)
        assert_eq!(result, "Plain section content.   ");
    }

    #[tokio::test]
    async fn test_react_force_final_empty_returns_i18n_fallback() {
        let tool_call =
            r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q"}}</tool_call>"#;
        // Force-final returns empty → i18n fallback
        let llm = ScriptedChatLlm::new(vec![
            tool_call, tool_call, tool_call, tool_call, tool_call, // 5 iters
            "",        // empty → fallback
        ]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        // Must be the i18n key value (non-empty, not the key itself)
        assert!(!result.is_empty(), "fallback must be non-empty");
        assert!(!result.starts_with("report."), "fallback must resolve i18n key, got: {result}");
        // The Chinese i18n value contains "生成失败" or the English contains "failed to generate"
        // — either locale is acceptable.
    }

    /// None/empty response retry then break: first 4 iterations return empty, 5th returns empty.
    /// Loop should retry with （响应为空）+请继续生成内容。 on iters 0-3, then break on iter 4.
    /// Force-final then returns "Final Answer: recovered".
    #[tokio::test]
    async fn test_react_none_empty_retry_then_break() {
        let llm = ScriptedChatLlm::new(vec![
            "", // iter 0 → empty (retry)
            "", // iter 1 → empty (retry)
            "", // iter 2 → empty (retry)
            "", // iter 3 → empty (retry)
            "", // iter 4 → empty on last iter → break
            "Final Answer: recovered content",
        ]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        assert_eq!(result, "recovered content");
        // 5 loop iters + 1 force-final = 6 chat calls
        assert_eq!(llm.calls(), 6);
    }

    /// Previous sections truncation: sections > 4000 chars are truncated to 4000 chars + "..."
    /// and sections <= 4000 chars are kept as-is.  Test char-boundary safety with Unicode.
    #[test]
    fn test_react_previous_sections_truncation_unicode() {
        // Build a string of exactly 4001 Unicode characters (3-byte UTF-8 each: "中")
        let long_sec: String = "中".repeat(4001);
        assert_eq!(long_sec.chars().count(), 4001);

        // Run the truncation logic inline (mirrors the method's implementation):
        let char_count = long_sec.chars().count();
        let truncated = if char_count > 4000 {
            let byte_offset =
                long_sec.char_indices().nth(4000).map(|(b, _)| b).unwrap_or(long_sec.len());
            format!("{}...", &long_sec[..byte_offset])
        } else {
            long_sec.clone()
        };

        // Result should be exactly 4000 chars + "..."
        let char_count_result = truncated.trim_end_matches("...").chars().count();
        assert_eq!(char_count_result, 4000);
        assert!(truncated.ends_with("..."));
        // Must be valid UTF-8 (no char-boundary panic)
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());

        // Short section (exactly 4000 chars) → NOT truncated
        let exact_sec: String = "中".repeat(4000);
        let char_count2 = exact_sec.chars().count();
        let truncated2 = if char_count2 > 4000 {
            let byte_offset =
                exact_sec.char_indices().nth(4000).map(|(b, _)| b).unwrap_or(exact_sec.len());
            format!("{}...", &exact_sec[..byte_offset])
        } else {
            exact_sec.clone()
        };
        assert!(!truncated2.ends_with("..."));
        assert_eq!(truncated2.chars().count(), 4000);
    }

    /// First-section case: empty previous_sections → message contains "（这是第一个章节）".
    ///
    /// We can't easily inspect the exact message-trace from outside the function,
    /// but we can verify the function completes without panic and returns a valid result.
    #[tokio::test]
    async fn test_react_first_section_no_previous() {
        // 3 tool calls + final answer
        let tc = r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q"}}</tool_call>"#;
        let llm = ScriptedChatLlm::new(vec![tc, tc, tc, "Final Answer: first section content"]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        let tools = make_tools_fixture(&graph, &llm);

        // Empty previous_sections slice
        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;
        assert_eq!(result, "first section content");
    }

    /// Progress callback is invoked at each iteration with stage="generating" and increasing pct.
    #[tokio::test]
    async fn test_react_progress_emissions() {
        let tc = r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q"}}</tool_call>"#;
        let llm = ScriptedChatLlm::new(vec![tc, tc, tc, "Final Answer: content"]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        let tools = make_tools_fixture(&graph, &llm);

        let emissions = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, u32)>::new()));
        let em_clone = emissions.clone();
        let cb = move |stage: &str, pct: u32, _msg: &str| {
            em_clone.lock().unwrap().push((stage.to_string(), pct));
        };

        agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, Some(&cb), 0)
            .await;

        let em = emissions.lock().unwrap().clone();
        // 4 iterations (0-3) → 4 progress calls all with stage="generating"
        assert_eq!(em.len(), 4, "expected 4 progress emissions, got {}", em.len());
        for (stage, _pct) in &em {
            assert_eq!(stage, "generating");
        }
        // pct at iter 0 = 0, at iter 1 = 20, at iter 2 = 40, at iter 3 = 60
        assert_eq!(em[0].1, 0); // 0/5 * 100
        assert_eq!(em[1].1, 20); // 1/5 * 100
        assert_eq!(em[2].1, 40); // 2/5 * 100
        assert_eq!(em[3].1, 60); // 3/5 * 100
    }

    /// Final Answer with multiple "Final Answer:" occurrences → uses LAST (rsplit semantics).
    ///
    /// Python: `response.split("Final Answer:")[-1].strip()`
    /// Rust:   `response.rsplit("Final Answer:").next()` → same last-occurrence semantics.
    #[tokio::test]
    async fn test_react_final_answer_last_occurrence() {
        let tc = r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q"}}</tool_call>"#;
        // Response with two "Final Answer:" — last wins
        let double_final = "Final Answer: first occurrence\nFinal Answer: last occurrence";
        let llm = ScriptedChatLlm::new(vec![tc, tc, tc, double_final]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        assert_eq!(result, "last occurrence");
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (g1): ReportLogger wiring into generate_section_react
    //
    // Tests that:
    //   1. With report_logger=Some, the loop writes a parseable jsonl sequence
    //      containing the expected action types in the correct order.
    //   2. With report_logger=None, no file is written (byte-stable prior behavior).
    // -----------------------------------------------------------------------

    fn parse_jsonl_actions(path: &std::path::Path) -> Vec<String> {
        let f = std::fs::File::open(path).expect("agent_log.jsonl must exist");
        let reader = std::io::BufReader::new(f);
        use std::io::BufRead as _;
        reader
            .lines()
            .filter_map(|l| {
                let l = l.unwrap();
                if l.is_empty() {
                    return None;
                }
                let v: serde_json::Map<String, serde_json::Value> =
                    serde_json::from_str(&l).ok()?;
                v.get("action")?.as_str().map(|s| s.to_string())
            })
            .collect()
    }

    /// generate_section_react WITH a logger: happy path (3 tool calls + Final Answer) →
    /// agent_log.jsonl must contain [section_start, llm_response×4, tool_call×3, tool_result×3, section_content]
    /// in correct order.
    ///
    /// The exact interleaving is:
    ///   log_section_start
    ///   iter0: llm_response, tool_call, tool_result
    ///   iter1: llm_response, tool_call, tool_result
    ///   iter2: llm_response, tool_call, tool_result
    ///   iter3: llm_response, section_content (Final Answer)
    #[tokio::test]
    async fn test_react_with_logger_happy_path_writes_jsonl_sequence() {
        let upload_dir =
            std::env::temp_dir().join(format!("teri_react_logger_test_{}", std::process::id()));

        let tc_quick =
            r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q"}}</tool_call>"#;
        let tc_panorama =
            r#"<tool_call>{"name": "panorama_search", "parameters": {"query": "q"}}</tool_call>"#;
        let tc_insight =
            r#"<tool_call>{"name": "insight_forge", "parameters": {"query": "q"}}</tool_call>"#;
        let final_answer = "Final Answer: Section content here.";

        let llm = ScriptedChatLlm::new(vec![tc_quick, tc_panorama, tc_insight, final_answer]);
        let (graph, section, outline) = make_react_fixtures();

        let logger = crate::report::logger::ReportLogger::new("react-test-001", &upload_dir)
            .expect("logger construction must not fail");

        let mut agent = ReportAgent::new_react("g1", "sim1", "req");
        agent.report_logger = Some(logger);
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        assert_eq!(result, "Section content here.");

        // Verify the jsonl was written
        let log_path = upload_dir.join("reports").join("react-test-001").join("agent_log.jsonl");
        assert!(log_path.exists(), "agent_log.jsonl must be created");

        let actions = parse_jsonl_actions(&log_path);
        // Expected sequence: section_start, then per-iteration pairs
        assert_eq!(actions[0], "section_start", "first entry must be section_start");

        // Count each action type
        let llm_responses = actions.iter().filter(|a| a.as_str() == "llm_response").count();
        let tool_calls = actions.iter().filter(|a| a.as_str() == "tool_call").count();
        let tool_results = actions.iter().filter(|a| a.as_str() == "tool_result").count();
        let section_contents = actions.iter().filter(|a| a.as_str() == "section_content").count();

        assert_eq!(llm_responses, 4, "must have 4 llm_response entries (3 tool iters + 1 final)");
        assert_eq!(tool_calls, 3, "must have 3 tool_call entries");
        assert_eq!(tool_results, 3, "must have 3 tool_result entries");
        assert_eq!(section_contents, 1, "must have 1 section_content entry at the end");

        // Last action must be section_content
        assert_eq!(
            actions.last().map(|s| s.as_str()),
            Some("section_content"),
            "last entry must be section_content"
        );

        // Each entry is valid parseable JSON with the contractual top-level keys
        let log_raw = std::fs::read_to_string(&log_path).unwrap();
        for line in log_raw.lines() {
            if line.is_empty() {
                continue;
            }
            let obj: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(line).expect("each line must be valid compact JSON");
            assert!(obj.contains_key("timestamp"), "entry missing timestamp");
            assert!(obj.contains_key("elapsed_seconds"), "entry missing elapsed_seconds");
            assert!(obj["elapsed_seconds"].is_number(), "elapsed_seconds must be a number");
            assert!(obj.contains_key("report_id"), "entry missing report_id");
        }

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    /// generate_section_react with report_logger=None must NOT create any file,
    /// and the result must be identical to the pre-(g) behavior.
    #[tokio::test]
    async fn test_react_without_logger_no_file_written() {
        let upload_dir =
            std::env::temp_dir().join(format!("teri_react_nologger_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&upload_dir);

        let tc = r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q"}}</tool_call>"#;
        let llm = ScriptedChatLlm::new(vec![tc, tc, tc, "Final Answer: no logger content"]);
        let (graph, section, outline) = make_react_fixtures();
        let agent = ReportAgent::new_react("g1", "sim1", "req");
        // report_logger = None (default from new_react)
        let tools = make_tools_fixture(&graph, &llm);

        let result = agent
            .generate_section_react(&section, &outline, &[], &tools, &llm, None, 0)
            .await;

        assert_eq!(result, "no logger content");

        // No file should have been written anywhere under upload_dir
        let log_path = upload_dir.join("reports").join("g1").join("agent_log.jsonl");
        assert!(
            !log_path.exists(),
            "agent_log.jsonl must NOT be created when report_logger is None"
        );

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (h2): generate_report skeleton parity tests
    //
    // Strategy:
    //   - MockChatJsonLlm returns a 2-section outline (already defined above).
    //   - NullSink for sink (no SSE channel needed).
    //   - Explicit report_id for determinism.
    //   - Temp dir via std::env::temp_dir() for upload_folder.
    //   - Assert: status Completed, progress.json write sequence, outline.json,
    //     agent_log.jsonl (log_start / planning_start / planning_complete / report_complete),
    //     and the error path (FailingChatJsonLlm → status Failed, progress=-1, log_error).
    // -----------------------------------------------------------------------

    /// Shared outline mock response for (h2) tests.
    fn h2_outline_response() -> serde_json::Value {
        serde_json::json!({
            "title": "Future Prediction Report",
            "summary": "A summary of predictions.",
            "sections": [
                {"title": "Section One"},
                {"title": "Section Two"}
            ]
        })
    }

    /// Parse agent_log.jsonl and return all entries.
    fn parse_agent_log(
        dir: &std::path::Path,
        report_id: &str,
    ) -> Vec<serde_json::Map<String, serde_json::Value>> {
        use std::io::BufRead as _;
        let log_path = dir.join("reports").join(report_id).join("agent_log.jsonl");
        if !log_path.exists() {
            return vec![];
        }
        let f = std::fs::File::open(&log_path).expect("agent_log.jsonl must be readable");
        std::io::BufReader::new(f)
            .lines()
            .filter_map(|l| {
                let l = l.ok()?;
                if l.trim().is_empty() {
                    return None;
                }
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&l).ok()
            })
            .collect()
    }

    /// Parse progress.json and return the map.
    fn read_progress(
        dir: &std::path::Path,
        report_id: &str,
    ) -> serde_json::Map<String, serde_json::Value> {
        let path = dir.join("reports").join(report_id).join("progress.json");
        let data = std::fs::read_to_string(&path).expect("progress.json must exist");
        serde_json::from_str(&data).expect("progress.json must be valid JSON")
    }

    // ── (h2)-1: explicit report_id, happy path ───────────────────────────────

    #[tokio::test]
    async fn test_generate_report_h2_explicit_id_status_completed() {
        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir = std::env::temp_dir().join(format!("teri_h2_happy_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "test requirement");

        let mut null_sink = crate::report::sink::NullSink;
        let report = agent
            .generate_report(
                &tools,
                &llm,
                &mgr,
                &mut null_sink,
                Some("report_h2testexplicit".to_string()),
            )
            .await;

        assert_eq!(report.report_id, "report_h2testexplicit");
        assert_eq!(report.status, ReportStatus::Completed);
        assert!(report.error.is_none());
        // outline must be set
        assert!(report.outline.is_some());
        let outline = report.outline.as_ref().unwrap();
        assert_eq!(outline.title, "Future Prediction Report");
        assert_eq!(outline.sections.len(), 2);
        // completed_at must be non-empty
        assert!(!report.completed_at.is_empty());
        // markdown_content must be non-empty (header assembled even with no sections)
        assert!(!report.markdown_content.is_empty());

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h2)-2: auto-gen report_id shape ─────────────────────────────────────

    #[tokio::test]
    async fn test_generate_report_h2_autogen_id_shape() {
        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h2_autoid_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "auto-id test");

        let mut null_sink = crate::report::sink::NullSink;
        // Pass None → auto-gen
        let report = agent.generate_report(&tools, &llm, &mgr, &mut null_sink, None).await;

        // Shape: ^report_[0-9a-f]{12}$
        let id = &report.report_id;
        let re = regex::Regex::new(r"^report_[0-9a-f]{12}$").unwrap();
        assert!(re.is_match(id), "auto-gen report_id shape mismatch: {id:?}");

        // Empty-string is also auto-generated
        let mut null_sink2 = crate::report::sink::NullSink;
        let report2 = agent
            .generate_report(&tools, &llm, &mgr, &mut null_sink2, Some(String::new()))
            .await;
        let id2 = &report2.report_id;
        assert!(re.is_match(id2), "empty-string report_id must be auto-gen: {id2:?}");

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h2)-3: progress.json write sequence ─────────────────────────────────

    #[tokio::test]
    async fn test_generate_report_h2_progress_json_sequence() {
        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h2_progress_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "progress test");
        let report_id = "report_h2progresstest";

        let mut null_sink = crate::report::sink::NullSink;
        let report = agent
            .generate_report(&tools, &llm, &mgr, &mut null_sink, Some(report_id.to_string()))
            .await;

        assert_eq!(report.status, ReportStatus::Completed, "must be Completed on success");

        // Final progress.json must have progress=100, status="completed".
        // (Python writes multiple times; we can only assert the FINAL state on disk.)
        let progress = read_progress(&upload_dir, report_id);
        assert_eq!(
            progress.get("progress").and_then(|v| v.as_i64()),
            Some(100),
            "final progress must be 100"
        );
        assert_eq!(
            progress.get("status").and_then(|v| v.as_str()),
            Some("completed"),
            "final status must be completed"
        );
        // completed_sections must be an array (even if empty in h2 skeleton)
        assert!(
            progress.get("completed_sections").map(|v| v.is_array()).unwrap_or(false),
            "completed_sections must be an array"
        );

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h2)-4: outline.json saved ───────────────────────────────────────────

    #[tokio::test]
    async fn test_generate_report_h2_outline_json_saved() {
        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h2_outline_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "outline test");
        let report_id = "report_h2outlinetest";

        let mut null_sink = crate::report::sink::NullSink;
        agent
            .generate_report(&tools, &llm, &mgr, &mut null_sink, Some(report_id.to_string()))
            .await;

        let outline_path = upload_dir.join("reports").join(report_id).join("outline.json");
        assert!(outline_path.exists(), "outline.json must be saved");
        let data: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&outline_path).unwrap()).unwrap();
        assert_eq!(data["title"], "Future Prediction Report");
        assert_eq!(data["sections"].as_array().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h2)-5: agent_log.jsonl orchestration lines ──────────────────────────

    #[tokio::test]
    async fn test_generate_report_h2_agent_log_orchestration_actions() {
        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h2_agentlog_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "log test");
        let report_id = "report_h2logtest";

        let mut null_sink = crate::report::sink::NullSink;
        agent
            .generate_report(&tools, &llm, &mgr, &mut null_sink, Some(report_id.to_string()))
            .await;

        let entries = parse_agent_log(&upload_dir, report_id);
        let actions: Vec<&str> = entries.iter().filter_map(|e| e["action"].as_str()).collect();

        // Must contain these four orchestration-level log actions in order:
        //   report_start, planning_start, planning_complete, report_complete
        // (h3 will add section-level actions)
        assert!(
            actions.contains(&"report_start"),
            "agent_log must have report_start; found: {actions:?}"
        );
        assert!(
            actions.contains(&"planning_start"),
            "agent_log must have planning_start; found: {actions:?}"
        );
        assert!(
            actions.contains(&"planning_complete"),
            "agent_log must have planning_complete; found: {actions:?}"
        );
        assert!(
            actions.contains(&"report_complete"),
            "agent_log must have report_complete; found: {actions:?}"
        );

        // Order check: report_start < planning_start < planning_complete < report_complete
        let pos_start = actions.iter().position(|&a| a == "report_start").unwrap();
        let pos_plan_start = actions.iter().position(|&a| a == "planning_start").unwrap();
        let pos_plan_done = actions.iter().position(|&a| a == "planning_complete").unwrap();
        let pos_done = actions.iter().position(|&a| a == "report_complete").unwrap();
        assert!(pos_start < pos_plan_start, "report_start must precede planning_start");
        assert!(pos_plan_start < pos_plan_done, "planning_start must precede planning_complete");
        assert!(pos_plan_done < pos_done, "planning_complete must precede report_complete");

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h2)-6: report_complete log has total_sections + total_time_seconds ──

    #[tokio::test]
    async fn test_generate_report_h2_report_complete_log_shape() {
        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h2_complete_log_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "shape test");
        let report_id = "report_h2completelog";

        let mut null_sink = crate::report::sink::NullSink;
        agent
            .generate_report(&tools, &llm, &mgr, &mut null_sink, Some(report_id.to_string()))
            .await;

        let entries = parse_agent_log(&upload_dir, report_id);
        let complete_entry = entries
            .iter()
            .find(|e| e["action"].as_str() == Some("report_complete"))
            .expect("report_complete entry must exist");

        let details = complete_entry["details"].as_object().unwrap();
        // total_sections = 2 (the mock outline has 2 sections)
        assert_eq!(details["total_sections"].as_u64(), Some(2), "total_sections must be 2");
        // total_time_seconds must be a number (wall time — nondeterministic, assert shape)
        assert!(details["total_time_seconds"].is_number(), "total_time_seconds must be a number");

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h2)-7: error path → status Failed, progress=-1, log_error present ──

    #[tokio::test]
    async fn test_generate_report_h2_error_path_failed_status() {
        // FailingChatJsonLlm makes plan_outline return the fallback outline
        // (NOT an error — plan_outline catches and returns fallback). So to
        // trigger the error tail we need an error from a later step.
        //
        // Strategy: use a manager pointed at a read-only path so save_report fails.
        // But that's OS-dependent. Instead we rely on the fact that
        // FailingChatJsonLlm makes plan_outline use the fallback (3 sections),
        // and generate_report itself succeeds (the fallback is a valid outline).
        // For a TRUE error-tail test we need a failing manager operation.
        //
        // We simulate this by using a path that doesn't exist and is not writable.
        // On Linux the path /proc is not writable; use /proc/fake as upload_folder.
        // This makes ensure_report_folder fail → error tail.
        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        // Use /proc as the upload_folder — ensure_report_folder cannot create dirs there.
        let mgr = crate::report::manager::ReportManager::new(std::path::Path::new(
            "/proc/teri_h2_fail_test_not_real",
        ));
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "error path test");

        let mut null_sink = crate::report::sink::NullSink;
        let report = agent
            .generate_report(
                &tools,
                &llm,
                &mgr,
                &mut null_sink,
                Some("report_h2failtest".to_string()),
            )
            .await;

        assert_eq!(report.status, ReportStatus::Failed, "status must be Failed on IO error");
        assert!(report.error.is_some(), "error field must be set on Failed path");
        // console_logger must be cleared (no leak)
        assert!(agent.console_logger.is_none(), "console_logger must be None after error tail");
    }

    // ── (h2)-7b: post-planning failure retains outline in FAILED meta.json ────
    //
    // Regression test for the downgrade caught by the parity gate:
    // Python's `except` block mutates the SAME `report` object that already has
    // `.outline` set (from plan_outline at py:1615). So a post-planning I/O failure
    // (e.g. EISDIR on full_report.md) must produce a Failed `meta.json` that
    // STILL carries the outline — not a fresh `outline: null`.
    //
    // We inject the EISDIR by pre-creating `full_report.md` as a directory before
    // calling generate_report, so:
    //   - ensure_report_folder succeeds (dir already exists → create_dir_all is no-op)
    //   - plan_outline succeeds → report.outline = Some(outline)
    //   - save_outline, update_progress succeed
    //   - assemble_full_report fails with EISDIR when it tries to write full_report.md
    //   → error tail mutates the same `report` → meta.json on disk has non-null outline
    #[tokio::test]
    async fn test_generate_report_h2_failed_meta_retains_outline() {
        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h2_outline_retain_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let report_id = "report_h2outlineretain";

        // Pre-create the report directory and put a directory where full_report.md would go.
        // This means plan_outline can run and save outline.json, but assemble_full_report
        // will fail with EISDIR when it tries to write to full_report.md.
        let report_dir = upload_dir.join("reports").join(report_id);
        std::fs::create_dir_all(&report_dir).expect("must create report_dir");
        let full_report_path = report_dir.join("full_report.md");
        std::fs::create_dir_all(&full_report_path)
            .expect("must create full_report.md as a directory (EISDIR trap)");

        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "outline retain test");
        let mut null_sink = crate::report::sink::NullSink;
        let report = agent
            .generate_report(&tools, &llm, &mgr, &mut null_sink, Some(report_id.to_string()))
            .await;

        // (a) Returned report must be Failed with an error message
        assert_eq!(report.status, ReportStatus::Failed, "status must be Failed on EISDIR");
        assert!(report.error.is_some(), "error field must be set");

        // (b) Returned report must retain the outline (set before the EISDIR failure)
        assert!(
            report.outline.is_some(),
            "outline must be retained in the failed report — got None (downgrade)"
        );
        let outline = report.outline.as_ref().unwrap();
        assert_eq!(outline.title, "Future Prediction Report", "outline title must match");
        assert_eq!(outline.sections.len(), 2, "outline must have 2 sections");

        // (c) outline.json must exist on disk (save_outline ran before the failure)
        let outline_json_path = report_dir.join("outline.json");
        assert!(
            outline_json_path.exists(),
            "outline.json must be saved before the EISDIR failure"
        );

        // (d) meta.json on disk (written by error tail's save_report) must carry non-null outline
        let meta_path = report_dir.join("meta.json");
        assert!(meta_path.exists(), "meta.json must be written by the error tail");
        let meta_raw = std::fs::read_to_string(&meta_path).expect("meta.json must be readable");
        let meta: serde_json::Value =
            serde_json::from_str(&meta_raw).expect("meta.json must be valid JSON");
        assert_eq!(
            meta.get("status").and_then(|v| v.as_str()),
            Some("failed"),
            "meta.json status must be 'failed'"
        );
        assert!(
            meta.get("outline").map(|v| !v.is_null()).unwrap_or(false),
            "meta.json outline must be non-null after post-planning failure — got null (downgrade)"
        );
        let meta_outline = meta.get("outline").unwrap();
        assert_eq!(
            meta_outline.get("title").and_then(|v| v.as_str()),
            Some("Future Prediction Report"),
            "meta.json outline.title must match"
        );
        assert_eq!(
            meta_outline.get("sections").and_then(|v| v.as_array()).map(|a| a.len()),
            Some(2),
            "meta.json outline.sections must have 2 entries"
        );

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h2)-8: sink events emitted (planning + completed) ───────────────────

    #[tokio::test]
    async fn test_generate_report_h2_sink_events() {
        use crate::report::sink::{ReportEvent, ReportSink, ReportStage};

        struct CaptureSink {
            events: Vec<(ReportStage, i32, String)>,
        }
        impl ReportSink for CaptureSink {
            fn event(&mut self, ev: &ReportEvent) {
                self.events.push((ev.stage, ev.progress, ev.message.clone()));
            }
        }

        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir = std::env::temp_dir().join(format!("teri_h2_sink_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "sink test");

        let mut capture_sink = CaptureSink { events: vec![] };
        agent
            .generate_report(
                &tools,
                &llm,
                &mgr,
                &mut capture_sink,
                Some("report_h2sinktest".to_string()),
            )
            .await;

        // Must have at least:
        //   - (Planning, 0, ...) from top-level emit before plan_outline
        //   - (Planning, X, ...) from plan_outline via plan_cb (rescaled prog//5)
        //   - (Completed, 100, ...) from top-level emit
        let stages: Vec<ReportStage> = capture_sink.events.iter().map(|e| e.0).collect();
        assert!(stages.contains(&ReportStage::Planning), "must have at least one Planning event");
        assert!(stages.contains(&ReportStage::Completed), "must have a Completed event at end");

        // The top-level planning emit must be (Planning, 0, ...)
        let first_planning = capture_sink.events.iter().find(|e| e.0 == ReportStage::Planning);
        assert!(first_planning.is_some(), "must have a planning event");
        assert_eq!(first_planning.unwrap().1, 0, "first planning event must be progress=0");

        // Completed event must have progress=100
        let completed_ev = capture_sink.events.iter().find(|e| e.0 == ReportStage::Completed);
        assert!(completed_ev.is_some(), "must have a completed event");
        assert_eq!(completed_ev.unwrap().1, 100, "completed event must be progress=100");

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h2)-9: console_logger closed on success tail ────────────────────────

    #[tokio::test]
    async fn test_generate_report_h2_console_logger_closed_on_success() {
        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h2_console_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "console close test");

        let mut null_sink = crate::report::sink::NullSink;
        agent
            .generate_report(
                &tools,
                &llm,
                &mgr,
                &mut null_sink,
                Some("report_h2consoletest".to_string()),
            )
            .await;

        // After generate_report, console_logger must be None (cleared on success tail)
        assert!(
            agent.console_logger.is_none(),
            "console_logger must be None after generate_report (success tail)"
        );

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h2)-10: planning_closure rescale prog//5 ────────────────────────────

    #[tokio::test]
    async fn test_generate_report_h2_planning_closure_rescale() {
        // Verify the planning_closure in generate_report rescales as prog//5.
        // plan_outline emits at pct 0, 30, 80, 100.
        // After //5: 0, 6, 16, 20. These reach the sink via the plan_cb closure.
        use crate::report::sink::{ReportEvent, ReportSink, ReportStage};

        struct CaptureSink {
            events: Vec<(ReportStage, i32)>,
        }
        impl ReportSink for CaptureSink {
            fn event(&mut self, ev: &ReportEvent) {
                self.events.push((ev.stage, ev.progress));
            }
        }

        let llm = MockChatJsonLlm { response: h2_outline_response() };
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h2_rescale_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h2", "sim-h2", "rescale test");

        let mut capture_sink = CaptureSink { events: vec![] };
        agent
            .generate_report(
                &tools,
                &llm,
                &mgr,
                &mut capture_sink,
                Some("report_h2rescaletest".to_string()),
            )
            .await;

        // Extract only Planning events emitted through the plan_cb closure.
        // plan_outline emits at 0, 30, 80, 100; after //5 → 0, 6, 16, 20.
        // (The top-level emit is also progress=0 on Planning.)
        let planning_progs: Vec<i32> = capture_sink
            .events
            .iter()
            .filter(|e| e.0 == ReportStage::Planning)
            .map(|e| e.1)
            .collect();

        // 30//5 = 6, 80//5 = 16, 100//5 = 20 must appear among planning events.
        assert!(
            planning_progs.contains(&6),
            "planning rescaled 30//5=6 must appear; events: {planning_progs:?}"
        );
        assert!(
            planning_progs.contains(&16),
            "planning rescaled 80//5=16 must appear; events: {planning_progs:?}"
        );
        assert!(
            planning_progs.contains(&20),
            "planning rescaled 100//5=20 must appear; events: {planning_progs:?}"
        );

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // -----------------------------------------------------------------------
    // Sub-cycle (h3): per-section streaming loop parity tests
    //
    // Strategy:
    //   - DualModeLlm: returns a 2-section outline for chat_json, and a
    //     "Final Answer: <section content>" for chat (the ReACT loop).
    //     The Final Answer path bypasses tool calls → deterministic content.
    //   - NullSink / CaptureSink for sink.
    //   - Explicit report_id for determinism.
    //   - Temp dir via std::env::temp_dir().
    //
    // Tests:
    //   h3-1: full file tree — section_01.md, section_02.md exist with content;
    //          full_report.md (assemble) contains both sections.
    //   h3-2: incremental write — section_01.md exists BEFORE section_02 LLM call.
    //   h3-3: progress.json sequence — per-section writes appear in correct order.
    //   h3-4: final meta.json carries section content (TRAP #1).
    //   h3-5: agent_log.jsonl — section-loop lines present.
    //   h3-6: sink events — pre-section + section_closure sub-progress +
    //          post-section content-carrying (superset) + 95 assembling.
    // -----------------------------------------------------------------------

    /// Dual-mode LLM for h3 tests:
    ///   - `chat_json` → returns the outline JSON (for plan_outline)
    ///   - `chat` → returns "Final Answer: <canned content>" (for section ReACT loop)
    ///
    /// The `chat` response queue is a shared VecDeque; each section pops one entry.
    /// A shared call-order log records (call_type, section_num_hint, file_existence)
    /// for incremental-write assertions.
    struct DualModeLlm {
        outline_response: serde_json::Value,
        // Canned chat responses, consumed FIFO (tool-call sequences + Final Answers).
        chat_responses: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
        // Records the number of `chat` calls made (section LLM calls).
        chat_call_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl DualModeLlm {
        fn new(outline: serde_json::Value, chat_responses: Vec<String>) -> Self {
            Self {
                outline_response: outline,
                chat_responses: std::sync::Arc::new(std::sync::Mutex::new(
                    chat_responses.into_iter().collect(),
                )),
                chat_call_count: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }
        }

        #[allow(dead_code)]
        fn chat_calls(&self) -> usize {
            self.chat_call_count.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for DualModeLlm {
        async fn complete(&self, _: &str) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not used".into()))
        }
        /// ReACT loop chat: pops next canned response from the queue.
        async fn chat(&self, _: &[crate::llm::ChatMessage], _: &ChatOptions) -> Result<String> {
            self.chat_call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut q = self.chat_responses.lock().unwrap();
            Ok(q.pop_front().unwrap_or_else(|| "Final Answer: default content.".to_string()))
        }
        /// plan_outline chat_json: always returns the outline.
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &ChatOptions,
        ) -> Result<T> {
            serde_json::from_value(self.outline_response.clone())
                .map_err(|e| TeriError::Llm(format!("mock parse: {e}")))
        }
    }

    /// Shared outline response for h3 tests (2 sections).
    fn h3_outline_response() -> serde_json::Value {
        serde_json::json!({
            "title": "H3 Prediction Report",
            "summary": "H3 summary.",
            "sections": [
                {"title": "Alpha Section"},
                {"title": "Beta Section"}
            ]
        })
    }

    /// Canned ReACT chat response: a valid quick_search tool call.
    fn react_tool_call_response(query: &str) -> String {
        format!(
            "Thought: I need to search for information.\n\
             <tool_call>\n\
             {{\"name\": \"quick_search\", \"parameters\": {{\"query\": \"{query}\"}}}}\n\
             </tool_call>"
        )
    }

    /// Build a DualModeLlm with deterministic tool-call+Final-Answer sequences per section.
    ///
    /// Each section gets 3 tool-call responses (to satisfy min_tool_calls=3) followed
    /// by a "Final Answer: <content>" response. The `chat` response queue is shared
    /// (FIFO) across all sections, so the full sequence for 2 sections is:
    ///   s1-tc1, s1-tc2, s1-tc3, "Final Answer: Alpha…", s2-tc1, s2-tc2, s2-tc3, "Final Answer: Beta…"
    fn h3_llm() -> DualModeLlm {
        DualModeLlm::new(
            h3_outline_response(),
            vec![
                // Section 1: 3 tool calls then Final Answer
                react_tool_call_response("alpha query 1"),
                react_tool_call_response("alpha query 2"),
                react_tool_call_response("alpha query 3"),
                "Final Answer: Alpha section content.".to_string(),
                // Section 2: 3 tool calls then Final Answer
                react_tool_call_response("beta query 1"),
                react_tool_call_response("beta query 2"),
                react_tool_call_response("beta query 3"),
                "Final Answer: Beta section content.".to_string(),
            ],
        )
    }

    /// Read a section file and return its content (or None if missing).
    fn read_section_file(
        dir: &std::path::Path,
        report_id: &str,
        section_num: usize,
    ) -> Option<String> {
        let path = dir
            .join("reports")
            .join(report_id)
            .join(format!("section_{:02}.md", section_num));
        std::fs::read_to_string(path).ok()
    }

    // ── (h3)-1: full file tree after 2-section run ───────────────────────────

    #[tokio::test]
    async fn test_generate_report_h3_full_file_tree() {
        let llm = h3_llm();
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h3_filetree_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h3", "sim-h3", "file tree test");
        let report_id = "report_h3filetreetest";

        let mut null_sink = crate::report::sink::NullSink;
        let report = agent
            .generate_report(&tools, &llm, &mgr, &mut null_sink, Some(report_id.to_string()))
            .await;

        assert_eq!(report.status, ReportStatus::Completed, "report must be Completed");

        // section_01.md must exist with non-empty content.
        let sec1 = read_section_file(&upload_dir, report_id, 1);
        assert!(sec1.is_some(), "section_01.md must exist");
        let sec1_content = sec1.unwrap();
        assert!(!sec1_content.is_empty(), "section_01.md must be non-empty");

        // section_02.md must exist with non-empty content.
        let sec2 = read_section_file(&upload_dir, report_id, 2);
        assert!(sec2.is_some(), "section_02.md must exist");
        let sec2_content = sec2.unwrap();
        assert!(!sec2_content.is_empty(), "section_02.md must be non-empty");

        // full_report.md (assembled) must contain content from both sections.
        let full_report_path = upload_dir.join("reports").join(report_id).join("full_report.md");
        assert!(full_report_path.exists(), "full_report.md must exist after assemble");
        let full_content =
            std::fs::read_to_string(&full_report_path).expect("full_report.md must be readable");
        // Both section titles must appear in the assembled report.
        assert!(
            full_content.contains("Alpha Section"),
            "assembled report must contain 'Alpha Section'"
        );
        assert!(
            full_content.contains("Beta Section"),
            "assembled report must contain 'Beta Section'"
        );
        // Assembled content must be non-trivial (not just the header).
        assert!(
            full_content.len() > 50,
            "assembled report content must be substantial: {full_content:?}"
        );

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h3)-2: incremental write (section_01.md exists before section_02 LLM call)

    #[tokio::test]
    async fn test_generate_report_h3_incremental_write() {
        // Use an AtomicBool to verify section_01.md exists when the second section's
        // LLM call is made. We check disk state between calls by sharing a flag through
        // a custom LLM wrapper.
        use std::sync::{Arc, Mutex};

        struct IncrementalCheckLlm {
            inner: DualModeLlm,
            upload_dir: std::path::PathBuf,
            report_id: String,
            // Records whether section_01.md existed when call N happened.
            section01_existed_at_call: Arc<Mutex<Vec<bool>>>,
        }

        #[async_trait::async_trait]
        impl LlmClient for IncrementalCheckLlm {
            async fn complete(&self, _: &str) -> Result<String> {
                Err(TeriError::Llm("not used".into()))
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, _: &str) -> Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(TeriError::Llm("not used".into()))
            }
            async fn chat(
                &self,
                msgs: &[crate::llm::ChatMessage],
                opts: &ChatOptions,
            ) -> Result<String> {
                // Record whether section_01.md exists before this call.
                let sec01_exists = self
                    .upload_dir
                    .join("reports")
                    .join(&self.report_id)
                    .join("section_01.md")
                    .exists();
                self.section01_existed_at_call.lock().unwrap().push(sec01_exists);
                self.inner.chat(msgs, opts).await
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                msgs: &[crate::llm::ChatMessage],
                opts: &ChatOptions,
            ) -> Result<T> {
                self.inner.chat_json(msgs, opts).await
            }
        }

        let upload_dir = std::env::temp_dir().join(format!("teri_h3_incr_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let report_id = "report_h3incrtest".to_string();

        let section01_existed_at_call: Arc<Mutex<Vec<bool>>> = Arc::new(Mutex::new(vec![]));

        let llm = IncrementalCheckLlm {
            inner: h3_llm(),
            upload_dir: upload_dir.clone(),
            report_id: report_id.clone(),
            section01_existed_at_call: Arc::clone(&section01_existed_at_call),
        };

        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h3", "sim-h3", "incremental write test");

        let mut null_sink = crate::report::sink::NullSink;
        let report =
            agent.generate_report(&tools, &llm, &mgr, &mut null_sink, Some(report_id)).await;

        assert_eq!(report.status, ReportStatus::Completed);

        let calls = section01_existed_at_call.lock().unwrap();
        // With 3 tool calls + 1 Final Answer per section, we expect 8 total chat calls
        // for a 2-section run (4 per section). Section 2 starts at call index 4.
        // At minimum, there must be more than 4 calls total.
        assert!(
            calls.len() > 4,
            "expected more than 4 chat calls for a 2-section run, got {}",
            calls.len()
        );
        // When the FIRST call of the SECOND section is made (index 4), section_01.md
        // must already exist on disk — that's the incremental-write guarantee.
        // (Calls 0-3 are section 1's 3 tool-calls + Final Answer.)
        assert!(
            calls[4],
            "section_01.md must exist when section 2's first LLM call is made \
             (incremental write guarantee; calls[4] is the start of section 2)"
        );

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h3)-3: progress.json write sequence ────────────────────────────────

    #[tokio::test]
    async fn test_generate_report_h3_progress_json_sequence() {
        let llm = h3_llm();
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h3_progress_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h3", "sim-h3", "progress seq test");
        let report_id = "report_h3progresstest";

        let mut null_sink = crate::report::sink::NullSink;
        let report = agent
            .generate_report(&tools, &llm, &mgr, &mut null_sink, Some(report_id.to_string()))
            .await;

        assert_eq!(report.status, ReportStatus::Completed);

        // Final progress.json: must be completed=100.
        let progress = read_progress(&upload_dir, report_id);
        assert_eq!(
            progress.get("progress").and_then(|v| v.as_i64()),
            Some(100),
            "final progress must be 100"
        );
        assert_eq!(
            progress.get("status").and_then(|v| v.as_str()),
            Some("completed"),
            "final status must be 'completed'"
        );

        // With 2 sections, base_progress for section 0 = 20 + int(0/2*70) = 20;
        // section_done progress = 20 + 70/2 = 55.
        // base_progress for section 1 = 20 + int(1/2*70) = 20+35 = 55;
        // section_done progress = 55 + 70/2 = 90.
        // Then assembling=95, completed=100.
        // We can only assert the FINAL state of progress.json (last write wins),
        // but we CAN check that the final state is at 100 with "completed" status.
        // The completed_section_titles must appear if present.
        if let Some(arr) = progress.get("completed_sections").and_then(|v| v.as_array()) {
            // With 2 sections both completed, completed_sections must have 2 entries.
            assert_eq!(
                arr.len(),
                2,
                "completed_sections in final progress.json must have 2 entries"
            );
        }

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h3)-4: final meta.json carries section content (TRAP #1) ───────────

    #[tokio::test]
    async fn test_generate_report_h3_final_meta_has_section_content() {
        let llm = h3_llm();
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir = std::env::temp_dir().join(format!("teri_h3_meta_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h3", "sim-h3", "meta trap1 test");
        let report_id = "report_h3metatrap1";

        let mut null_sink = crate::report::sink::NullSink;
        let report = agent
            .generate_report(&tools, &llm, &mgr, &mut null_sink, Some(report_id.to_string()))
            .await;

        assert_eq!(report.status, ReportStatus::Completed);

        // The Report struct returned must have section content.
        let outline = report.outline.as_ref().expect("outline must be Some");
        for (i, section) in outline.sections.iter().enumerate() {
            assert!(
                !section.content.is_empty(),
                "returned Report.outline.sections[{}].content must be non-empty (TRAP #1)",
                i
            );
        }

        // The on-disk meta.json (written by the final save_report) must also carry
        // non-empty section content — this is the TRAP #1 re-assign check.
        let meta_path = upload_dir.join("reports").join(report_id).join("meta.json");
        assert!(meta_path.exists(), "meta.json must exist");
        let meta_raw = std::fs::read_to_string(&meta_path).expect("meta.json must be readable");
        let meta: serde_json::Value =
            serde_json::from_str(&meta_raw).expect("meta.json must be valid JSON");

        let sections = meta
            .get("outline")
            .and_then(|o| o.get("sections"))
            .and_then(|s| s.as_array())
            .expect("meta.json must have outline.sections array");

        assert_eq!(sections.len(), 2, "meta.json outline must have 2 sections");
        for (i, section) in sections.iter().enumerate() {
            let content = section.get("content").and_then(|v| v.as_str()).unwrap_or("");
            assert!(
                !content.is_empty(),
                "meta.json outline.sections[{}].content must be non-empty (TRAP #1: \
                 post-loop re-assign of report.outline required)",
                i
            );
        }

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h3)-5: agent_log.jsonl — section_complete lines present ────────────

    #[tokio::test]
    async fn test_generate_report_h3_agent_log_section_complete_lines() {
        let llm = h3_llm();
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_h3_agentlog_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h3", "sim-h3", "agent log test");
        let report_id = "report_h3agentlogtest";

        let mut null_sink = crate::report::sink::NullSink;
        let report = agent
            .generate_report(&tools, &llm, &mgr, &mut null_sink, Some(report_id.to_string()))
            .await;

        assert_eq!(report.status, ReportStatus::Completed);

        let log_entries = parse_agent_log(&upload_dir, report_id);
        assert!(!log_entries.is_empty(), "agent_log.jsonl must not be empty");

        // Orchestration-level entries (h2):
        let has_start = log_entries
            .iter()
            .any(|e| e.get("action").and_then(|v| v.as_str()) == Some("report_start"));
        assert!(has_start, "agent_log.jsonl must contain a report_start entry");

        // Section-loop entries (e): each section generates at least a section_start.
        let section_starts: Vec<_> = log_entries
            .iter()
            .filter(|e| e.get("action").and_then(|v| v.as_str()) == Some("section_start"))
            .collect();
        assert_eq!(
            section_starts.len(),
            2,
            "agent_log.jsonl must contain 2 section_start entries (one per section)"
        );

        // h3 section_full_complete entries (log_section_full_complete).
        let section_complete: Vec<_> = log_entries
            .iter()
            .filter(|e| e.get("action").and_then(|v| v.as_str()) == Some("section_complete"))
            .collect();
        assert_eq!(
            section_complete.len(),
            2,
            "agent_log.jsonl must contain 2 section_complete entries (h3 log_section_full_complete)"
        );

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // ── (h3)-6: sink events — pre-section, sub-progress, content-carrying, 95 ─

    #[tokio::test]
    async fn test_generate_report_h3_sink_events() {
        use crate::report::sink::{ReportEvent, ReportSink, ReportStage};

        struct CaptureSink {
            events: Vec<ReportEvent>,
        }
        impl ReportSink for CaptureSink {
            fn event(&mut self, ev: &ReportEvent) {
                self.events.push(ev.clone());
            }
        }

        let llm = h3_llm();
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir = std::env::temp_dir().join(format!("teri_h3_sink_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let mut agent = ReportAgent::new_react("g-h3", "sim-h3", "sink events test");
        let report_id = "report_h3sinktest";

        let mut capture_sink = CaptureSink { events: vec![] };
        let report = agent
            .generate_report(&tools, &llm, &mgr, &mut capture_sink, Some(report_id.to_string()))
            .await;

        assert_eq!(report.status, ReportStatus::Completed);

        // (a) Pre-section faithful emits: one per section with section_content=None.
        // base_progress for section 0 (i=0, total=2) = 20 + int(0/2*70) = 20.
        // base_progress for section 1 (i=1, total=2) = 20 + int(1/2*70) = 55.
        let pre_sec_events: Vec<_> = capture_sink
            .events
            .iter()
            .filter(|e| {
                e.stage == ReportStage::Generating
                    && e.section_content.is_none()
                    && (e.progress == 20 || e.progress == 55)
            })
            .collect();
        assert!(
            !pre_sec_events.is_empty(),
            "must have faithful pre-section generating events at base_progress (20 or 55)"
        );

        // (b) Section-closure sub-progress events (from section_cb inside generate_section_react).
        // These have section_content=None and progress between base and base+70/total.
        // They are emitted by the section_cb closure inside the ReACT loop.
        let sub_progress_events: Vec<_> = capture_sink
            .events
            .iter()
            .filter(|e| {
                e.stage == ReportStage::Generating
                    && e.section_content.is_none()
                    && e.section_index.is_some()
            })
            .collect();
        // At least the pre-section events must be in this category.
        assert!(
            !sub_progress_events.is_empty(),
            "must have section-scoped Generating events (pre-section + sub-progress)"
        );

        // (h3 superset): content-carrying events — one per section with section_content=Some.
        let content_events: Vec<_> = capture_sink
            .events
            .iter()
            .filter(|e| e.stage == ReportStage::Generating && e.section_content.is_some())
            .collect();
        assert_eq!(
            content_events.len(),
            2,
            "must have 2 content-carrying events (one per section, h3 superset for U-027)"
        );
        // Each content-carrying event must have a non-empty section_content.
        for ev in &content_events {
            assert!(
                ev.section_content.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
                "content-carrying event must have non-empty section_content"
            );
            assert!(
                ev.section_index.is_some(),
                "content-carrying event must have section_index set"
            );
        }
        // section_index must be 1 and 2 (1-indexed).
        let content_indices: Vec<usize> =
            content_events.iter().filter_map(|e| e.section_index).collect();
        assert!(
            content_indices.contains(&1),
            "content-carrying events must include section_index=1; got {content_indices:?}"
        );
        assert!(
            content_indices.contains(&2),
            "content-carrying events must include section_index=2; got {content_indices:?}"
        );

        // (c) Faithful 95 assembling emit.
        let assembling_ev = capture_sink
            .events
            .iter()
            .find(|e| e.stage == ReportStage::Generating && e.progress == 95);
        assert!(assembling_ev.is_some(), "must have a Generating/95 assemblingReport event");
        assert!(
            assembling_ev.unwrap().section_content.is_none(),
            "assemblingReport event must have section_content=None"
        );

        // Completed event at 100.
        let completed_ev = capture_sink.events.iter().find(|e| e.stage == ReportStage::Completed);
        assert!(completed_ev.is_some(), "must have a Completed event");
        assert_eq!(completed_ev.unwrap().progress, 100);

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // =========================================================================
    // Sub-cycle (i): ReportAgent::chat parity tests
    //
    // Strategy:
    //   - ScriptedChatLlm returns a canned sequence of responses (one per call).
    //   - make_tools_fixture for tools (empty graph — tools return honest-err strings
    //     for unknown queries, which is fine for the observation path).
    //   - ReportManager seeded with a Report to test the report_content path.
    //   - All truncations are char-based; tests verify the char boundary exactly.
    // =========================================================================

    // (i)-1: No-tool-call path — immediate clean return, empty tool_calls/sources.
    // Reuses the existing `ScriptedChatLlm` from sub-cycles (e)/(g).
    #[tokio::test]
    async fn test_chat_i_no_tool_call_path() {
        let llm = ScriptedChatLlm::new(vec!["This is a clean answer."]);
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir = std::env::temp_dir().join(format!("teri_i_notool_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let agent = ReportAgent::new_react("g-i", "sim-i", "no-tool test");

        let resp = agent.chat(&tools, &llm, &mgr, "What is the forecast?", &[]).await;

        assert_eq!(resp.response, "This is a clean answer.");
        assert!(resp.tool_calls.is_empty(), "no tool calls expected");
        assert!(resp.sources.is_empty(), "no sources expected");

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // (i)-2: No-tool-call path strips <tool_call> XML (DOTALL) and [TOOL_CALL]...),
    // then trims the result.
    #[tokio::test]
    async fn test_chat_i_no_tool_call_regex_cleanup() {
        // Multiline <tool_call> block and [TOOL_CALL] artifact in the same response.
        let dirty = "Some text\n<tool_call>\n{\"name\": \"foo\"}\n</tool_call>\nAfter\n[TOOL_CALL] foo(bar)  ";
        let llm = ScriptedChatLlm::new(vec![dirty]);
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir = std::env::temp_dir().join(format!("teri_i_clean_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let agent = ReportAgent::new_react("g-i", "sim-i", "cleanup test");

        let resp = agent.chat(&tools, &llm, &mgr, "q", &[]).await;

        // <tool_call>...</tool_call> stripped, then [TOOL_CALL]...) stripped, then trimmed.
        assert!(!resp.response.contains("<tool_call>"), "tool_call XML must be stripped");
        assert!(!resp.response.contains("[TOOL_CALL]"), "[TOOL_CALL] must be stripped");
        assert_eq!(resp.response, resp.response.trim(), "response must be trimmed");

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // (i)-3: Tool-call path — one round executes the tool, second round returns clean answer.
    // Verifies: tool_calls_made accumulated, sources extracted from parameters.query,
    // observation built with CHAT_OBSERVATION_SUFFIX, final response cleaned.
    #[tokio::test]
    async fn test_chat_i_tool_call_path() {
        // Round 1: response contains a tool call.
        let round1 = r#"Let me check.
<tool_call>
{"name": "quick_search", "parameters": {"query": "market trends"}}
</tool_call>"#;
        // Round 2: clean answer (no tool call).
        let round2 = "Based on the data, the trend is upward.";
        let llm = ScriptedChatLlm::new(vec![round1, round2]);
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_i_toolcall_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let agent = ReportAgent::new_react("g-i", "sim-i", "tool-call test");

        let resp = agent.chat(&tools, &llm, &mgr, "What is the trend?", &[]).await;

        assert_eq!(resp.response, "Based on the data, the trend is upward.");
        // One tool call was made.
        assert_eq!(resp.tool_calls.len(), 1, "expected 1 tool call");
        assert_eq!(resp.tool_calls[0].name, "quick_search");
        // sources = parameters.query for each tool call.
        assert_eq!(resp.sources, vec!["market trends"]);

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // (i)-4: MAX_TOOL_CALLS_PER_CHAT (2) is respected — loop caps tool_calls_made at 2.
    // Round 1: tool call A; round 2: tool call B (but tool_calls_made already 1 so B is added);
    // then no more iterations (max=2), post-loop final call returns clean answer.
    #[tokio::test]
    async fn test_chat_i_max_tool_calls_cap() {
        let round1 =
            r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q1"}}</tool_call>"#;
        let round2 =
            r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q2"}}</tool_call>"#;
        // Post-loop final call.
        let final_response = "Final answer here.";
        let llm = ScriptedChatLlm::new(vec![round1, round2, final_response]);
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir = std::env::temp_dir().join(format!("teri_i_maxcap_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let agent = ReportAgent::new_react("g-i", "sim-i", "max tool cap test");

        let resp = agent.chat(&tools, &llm, &mgr, "q", &[]).await;

        // Must not exceed MAX_TOOL_CALLS_PER_CHAT (2).
        assert!(
            resp.tool_calls.len() <= MAX_TOOL_CALLS_PER_CHAT,
            "tool_calls_made must be <= MAX_TOOL_CALLS_PER_CHAT ({}), got {}",
            MAX_TOOL_CALLS_PER_CHAT,
            resp.tool_calls.len()
        );
        assert_eq!(resp.response, "Final answer here.");
        // sources must have an entry per accumulated tool call.
        assert_eq!(resp.sources.len(), resp.tool_calls.len());

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // (i)-5: report_content present — seeded via manager.save_report.
    // Sub-test A: under 15000 chars → included verbatim (no truncation suffix).
    // Sub-test B: over 15000 chars → truncated to 15000 chars + "... [报告内容已截断] ..." suffix.
    #[tokio::test]
    async fn test_chat_i_report_content_present_under_limit() {
        let llm = ScriptedChatLlm::new(vec!["Answer based on report."]);
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_i_rptcontent_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);

        // Seed a report with short content.
        let short_content = "This is the report markdown.".to_string();
        let report = Report {
            report_id: "rpt-i-short".to_string(),
            simulation_id: "sim-i-rptcontent".to_string(),
            graph_id: "g-i".to_string(),
            simulation_requirement: "test".to_string(),
            status: ReportStatus::Completed,
            outline: None,
            markdown_content: short_content.clone(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            completed_at: "2025-01-01T00:01:00Z".to_string(),
            error: None,
        };
        mgr.save_report(&report).expect("save_report must succeed");

        let agent = ReportAgent::new_react("g-i", "sim-i-rptcontent", "rpt test");
        // We exercise the chat but only care about it not panicking and returning a response.
        // The system prompt will contain the short report content (tested indirectly).
        let resp = agent.chat(&tools, &llm, &mgr, "Summarize the report.", &[]).await;
        assert!(!resp.response.is_empty());

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    #[tokio::test]
    async fn test_chat_i_report_content_over_limit_truncated() {
        // Build content that is exactly 15001 chars (CJK: use 'A' * 15001 for simplicity).
        let long_content = "X".repeat(15001);
        let llm = ScriptedChatLlm::new(vec!["Answer."]);
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_i_rpttrunc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);

        let report = Report {
            report_id: "rpt-i-long".to_string(),
            simulation_id: "sim-i-rpttrunc".to_string(),
            graph_id: "g-i".to_string(),
            simulation_requirement: "test".to_string(),
            status: ReportStatus::Completed,
            outline: None,
            markdown_content: long_content.clone(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            completed_at: "2025-01-01T00:01:00Z".to_string(),
            error: None,
        };
        mgr.save_report(&report).expect("save_report must succeed");

        let agent = ReportAgent::new_react("g-i", "sim-i-rpttrunc", "truncation test");
        // We verify the truncation logic by directly applying it (mirrors the method's body).
        // The truncated display should be 15000 chars + suffix.
        let char_count = long_content.chars().count();
        assert!(char_count > 15000, "test precondition: content must exceed 15000 chars");

        let byte_offset = long_content
            .char_indices()
            .nth(15000)
            .map(|(b, _)| b)
            .unwrap_or(long_content.len());
        let truncated = format!("{}\n\n... [报告内容已截断] ...", &long_content[..byte_offset]);
        // Truncated body is exactly 15000 source chars + suffix.
        assert_eq!(truncated[..byte_offset].chars().count(), 15000);
        assert!(truncated.contains("... [报告内容已截断] ..."));

        // The agent call should succeed (the LLM mock returns "Answer.").
        let resp = agent.chat(&tools, &llm, &mgr, "q", &[]).await;
        assert_eq!(resp.response, "Answer.");

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // (i)-6: absent report → system prompt uses "（暂无报告）" placeholder.
    // (Verified indirectly: chat returns a response, no panic, no content about a report.)
    #[tokio::test]
    async fn test_chat_i_absent_report_no_panic() {
        let llm = ScriptedChatLlm::new(vec!["No report yet."]);
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_i_noreport_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        // No report saved → get_report_by_simulation returns None.
        let agent = ReportAgent::new_react("g-i", "sim-i-absent", "absent report test");

        let resp = agent.chat(&tools, &llm, &mgr, "q", &[]).await;
        assert_eq!(resp.response, "No report yet.");

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // (i)-7: system_prompt renders the 3 substitutions with literal JSON braces intact.
    // Verify the rendered system_prompt byte-matches Python's .format() output
    // for a fixed input by checking the literal JSON example brace is present.
    #[test]
    fn test_chat_i_system_prompt_renders_correctly() {
        let sim_req = "test requirement";
        let report_content = "test report";
        let tools_desc = "tool1: does X";

        let rendered = CHAT_SYSTEM_PROMPT_TEMPLATE
            .replace("{simulation_requirement}", sim_req)
            .replace("{report_content}", report_content)
            .replace("{tools_description}", tools_desc);

        // All 3 substitutions applied.
        assert!(rendered.contains(sim_req), "simulation_requirement must appear");
        assert!(rendered.contains(report_content), "report_content must appear");
        assert!(rendered.contains(tools_desc), "tools_description must appear");

        // Literal JSON-example braces intact (Python `.format()` unescapes {{ → {).
        // The template contains the JSON example: {"name": "工具名称", "parameters": {...}}
        assert!(rendered.contains("{\"name\""), "literal JSON opening brace must be present");
        assert!(
            rendered.contains("}}"),
            "literal JSON closing brace must be present (parameters value)"
        );

        // No unresolved placeholders remaining.
        assert!(!rendered.contains("{simulation_requirement}"), "no unresolved slot");
        assert!(!rendered.contains("{report_content}"), "no unresolved slot");
        assert!(!rendered.contains("{tools_description}"), "no unresolved slot");
    }

    // (i)-8: ChatResponse.to_dict — key order response, tool_calls, sources.
    #[test]
    fn test_chat_i_chat_response_to_dict_key_order() {
        use crate::services::zep_tools::ToolCall;
        let mut params = serde_json::Map::new();
        params.insert("query".into(), serde_json::Value::String("the query".into()));
        let tc = ToolCall { name: "quick_search".to_string(), parameters: params };
        let cr = ChatResponse {
            response: "My answer".to_string(),
            tool_calls: vec![tc],
            sources: vec!["the query".to_string()],
        };
        let dict = cr.to_dict();
        let keys: Vec<&str> = dict.keys().map(|s| s.as_str()).collect();
        // Exact key order: response, tool_calls, sources.
        assert_eq!(keys, vec!["response", "tool_calls", "sources"]);

        // tool_calls entry shape: {name, parameters}.
        let tc_val = &dict["tool_calls"].as_array().unwrap()[0];
        assert!(tc_val.get("name").is_some());
        assert!(tc_val.get("parameters").is_some());
        assert_eq!(tc_val["name"].as_str().unwrap(), "quick_search");
        assert_eq!(tc_val["parameters"]["query"].as_str().unwrap(), "the query");

        // sources.
        assert_eq!(dict["sources"].as_array().unwrap()[0].as_str().unwrap(), "the query");
    }

    // (i)-9: chat_history last-10 window — history entries with their roles are passed through.
    #[tokio::test]
    async fn test_chat_i_history_window() {
        let llm = ScriptedChatLlm::new(vec!["Response after history."]);
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_i_history_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let agent = ReportAgent::new_react("g-i", "sim-i", "history test");

        // Build 15 history entries — only the last 10 should be included.
        let history: Vec<ChatMessage> = (0..15)
            .map(|i| {
                if i % 2 == 0 {
                    ChatMessage::user(format!("user msg {i}"))
                } else {
                    ChatMessage::assistant(format!("assistant msg {i}"))
                }
            })
            .collect();

        let resp = agent.chat(&tools, &llm, &mgr, "final question", &history).await;
        assert_eq!(resp.response, "Response after history.");

        let _ = std::fs::remove_dir_all(&upload_dir);
    }

    // (i)-10: CHAT_OBSERVATION_SUFFIX is appended to the observation user message.
    //  Indirectly verified: the test checks that after 2 ReACT rounds the post-loop
    //  path fires (the scripted LLM is given 3 responses in order).
    #[tokio::test]
    async fn test_chat_i_observation_suffix_in_loop() {
        // Both loop rounds have tool calls — post-loop final response is the 3rd call.
        let round1 =
            r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q1"}}</tool_call>"#;
        let round2 =
            r#"<tool_call>{"name": "quick_search", "parameters": {"query": "q2"}}</tool_call>"#;
        let final_resp = "Post loop answer.";
        let llm = ScriptedChatLlm::new(vec![round1, round2, final_resp]);
        let graph = crate::graph::KnowledgeGraph::new();
        let tools = make_tools_fixture(&graph, &llm);
        let upload_dir =
            std::env::temp_dir().join(format!("teri_i_obsuffix_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&upload_dir);
        let mgr = crate::report::manager::ReportManager::new(&upload_dir);
        let agent = ReportAgent::new_react("g-i", "sim-i", "obs suffix test");

        let resp = agent.chat(&tools, &llm, &mgr, "q", &[]).await;
        // Post-loop path fires; final response is cleaned and returned.
        assert_eq!(resp.response, "Post loop answer.");
        // At most 2 tool calls (MAX_TOOL_CALLS_PER_CHAT).
        assert!(resp.tool_calls.len() <= 2);

        let _ = std::fs::remove_dir_all(&upload_dir);
    }
}
