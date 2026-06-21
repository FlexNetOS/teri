//! Zep Tools Service - port of `backend/app/services/zep_tools.py` (MiroFish).
//!
//! High-level Zep retrieval tools for ReportAgent:
//! - `search_graph`: Hybrid search with fallback to local keyword search
//! - `insight_forge`: LLM-guided multi-query semantic search with entity enrichment
//! - `panorama_search`: Temporal-aware graph search with active/historical classification
//! - `quick_search`: Simple keyword-based search
//! - `interview_agents`: Agent selection and interview orchestration
//!
//! # Sub-cycle (b) additions — `ReportTools<'g, L>` facade
//!
//! `ReportTools` binds a borrowed `&KnowledgeGraph` and `&L` (LLM client) to give
//! the ReportAgent real graph reads.  Per DECISION-9 the graph is borrowed (not `Arc`);
//! per DECISION-11 handles are passed per-call, not stored at construction.
//!
//! ## Method status
//! - REAL graph reads (no more `TeriError::Unknown` stubs):
//!   `quick_search`, `panorama_search`, `get_entities_by_type`, `get_entity_summary`,
//!   `get_graph_statistics`, `get_simulation_context`, `get_all_nodes`, `get_all_edges`,
//!   `get_node_detail`, `get_node_edges`, `local_search`, `search_graph`.
//!
//! - `insight_forge` — DEFERRED to sub-cycle (b2); semantic vec ranking needs OQ-3
//!   (`query_vec_similarity` on the graph). Ships as keyword fallback with `[!]` note;
//!   the multi-sub-query structure is preserved (not dropped).
//!
//! - `interview_agents` — DEFERRED to sub-cycle (e); requires U-020 simulation IPC.
//!   Returns honest error string the ReACT loop tolerates (mirrors Python `_execute_tool`
//!   try/except → error text). Method present, marked pending.
//!
//! # Sub-cycle (c) additions — ReACT tool dispatch + parser
//!
//! Adds the pure ReACT plumbing (no loop — that is sub-cycle (e)):
//! - `ReportTool` enum with back-compat redirect arms
//! - `ToolCall` parsed struct
//! - `parse_tool_calls` free fn — 3-tier priority parse (xml / bare-json / trailing-json)
//!   with `{"tool"/"params"}`→`{"name"/"parameters"}` normalization
//! - `VALID_TOOL_NAMES` constant gate for tiers 2–3
//! - `get_tools_description` free fn — generates the tool descriptions the model sees
//! - `ReportTools::execute` — dispatch table over `ReportTool` with all param coercions
//! - Tool description constants (`TOOL_DESC_*`) verbatim from `report_agent.py`

use crate::error::{Result, TeriError};
use crate::graph::{Entity, KnowledgeGraph};
use crate::i18n::{t, t_args};
use crate::llm::{ChatMessage, ChatOptions, LlmClient};
use crate::services::entity_reader::KnowledgeGraphEntityReader;
use crate::services::simulation_runner::SimulationRunner;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

// ---------------------------------------------------------------------------
// DTOs — port of Python dataclasses in zep_tools.py
// These are kept VERBATIM (U-017 baseline; shapes are contractual).
// ---------------------------------------------------------------------------

/// Search result from graph search operations.
///
/// Port of `SearchResult` dataclass (`zep_tools.py:28-54`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchResult {
    pub facts: Vec<String>,
    pub edges: Vec<serde_json::Map<String, serde_json::Value>>,
    pub nodes: Vec<serde_json::Map<String, serde_json::Value>>,
    pub query: String,
    pub total_count: i64,
}

impl SearchResult {
    /// Convert to dict matching Python `to_dict()`.
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("facts".into(), serde_json::to_value(&self.facts).unwrap_or_default());
        m.insert("edges".into(), serde_json::to_value(&self.edges).unwrap_or_default());
        m.insert("nodes".into(), serde_json::to_value(&self.nodes).unwrap_or_default());
        m.insert("query".into(), self.query.clone().into());
        m.insert("total_count".into(), self.total_count.into());
        m
    }

    /// Convert to text matching Python `to_text()`.
    pub fn to_text(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("搜索查询: {}", self.query));
        lines.push(format!("找到 {} 条相关信息", self.total_count));

        if !self.facts.is_empty() {
            lines.push("\n### 相关事实:".to_string());
            for (i, fact) in self.facts.iter().enumerate() {
                lines.push(format!("{}. {}", i + 1, fact));
            }
        }

        lines.join("\n")
    }
}

/// Information about a graph node.
///
/// Port of `NodeInfo` dataclass (`zep_tools.py:57-79`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeInfo {
    pub uuid: String,
    pub name: String,
    pub labels: Vec<String>,
    pub summary: String,
    pub attributes: serde_json::Map<String, serde_json::Value>,
}

impl NodeInfo {
    /// Convert to dict matching Python `to_dict()`.
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("uuid".into(), self.uuid.clone().into());
        m.insert("name".into(), self.name.clone().into());
        m.insert("labels".into(), serde_json::to_value(&self.labels).unwrap_or_default());
        m.insert("attributes".into(), serde_json::to_value(&self.attributes).unwrap_or_default());
        m
    }

    /// Convert to text matching Python `to_text()`.
    pub fn to_text(&self) -> String {
        let entity_type = self
            .labels
            .iter()
            .find(|l| *l != "Entity" && *l != "Node")
            .map(|s| s.as_str())
            .unwrap_or("未知类型");
        format!("实体: {} (类型: {})\n摘要: {}", self.name, entity_type, self.summary)
    }
}

/// Information about a graph edge.
///
/// Port of `EdgeInfo` dataclass (`zep_tools.py:82-136`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EdgeInfo {
    pub uuid: String,
    pub name: String,
    pub fact: String,
    pub source_node_uuid: String,
    pub target_node_uuid: String,
    pub created_at: Option<String>,
    pub valid_at: Option<String>,
    pub invalid_at: Option<String>,
    pub expired_at: Option<String>,
}

impl EdgeInfo {
    /// Convert to dict matching Python `to_dict()`.
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("uuid".into(), self.uuid.clone().into());
        m.insert("name".into(), self.name.clone().into());
        m.insert("fact".into(), self.fact.clone().into());
        m.insert("source_node_uuid".into(), self.source_node_uuid.clone().into());
        m.insert("target_node_uuid".into(), self.target_node_uuid.clone().into());

        if let Some(v) = &self.created_at {
            m.insert("created_at".into(), v.clone().into());
        }
        if let Some(v) = &self.valid_at {
            m.insert("valid_at".into(), v.clone().into());
        }
        if let Some(v) = &self.invalid_at {
            m.insert("invalid_at".into(), v.clone().into());
        }
        if let Some(v) = &self.expired_at {
            m.insert("expired_at".into(), v.clone().into());
        }

        m
    }

    /// Convert to text matching Python `to_text()`.
    pub fn to_text(&self, include_temporal: bool) -> String {
        let source = if self.source_node_uuid.len() > 8 {
            &self.source_node_uuid[..8]
        } else {
            &self.source_node_uuid
        };
        let target = if self.target_node_uuid.len() > 8 {
            &self.target_node_uuid[..8]
        } else {
            &self.target_node_uuid
        };

        let mut base =
            format!("关系: {} --[{}]--> {}\n事实: {}", source, self.name, target, self.fact);

        if include_temporal {
            let valid_at = self.valid_at.as_deref().unwrap_or("未知");
            let invalid_at = self.invalid_at.as_deref().unwrap_or("至今");
            base.push_str(&format!("\n时效: {} - {}", valid_at, invalid_at));
            if let Some(expired) = &self.expired_at {
                base.push_str(&format!(" (已过期: {})", expired));
            }
        }

        base
    }

    /// Check if this edge is expired based on temporal attributes.
    ///
    /// Port of `EdgeInfo.is_expired` property (`zep_tools.py:129`).
    /// Python: `return self.expired_at is not None`
    pub fn is_expired(&self) -> bool {
        self.expired_at.is_some()
    }

    /// Check if this edge is invalid.
    ///
    /// Port of `EdgeInfo.is_invalid` property (`zep_tools.py:133`).
    /// Python: `return self.invalid_at is not None`
    /// NOTE: Python checks `invalid_at is not None`, NOT `source_node_uuid.is_empty()`.
    /// The original Rust had a divergence here — corrected to match Python.
    pub fn is_invalid(&self) -> bool {
        self.invalid_at.is_some()
    }
}

/// Result from insight_forge operation.
///
/// Port of `InsightForgeResult` dataclass (`zep_tools.py:138-211`).
///
/// Python fields (verbatim):
///   query, simulation_requirement, sub_queries,
///   semantic_facts, entity_insights, relationship_chains,
///   total_facts, total_entities, total_relationships
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InsightForgeResult {
    pub query: String,
    pub simulation_requirement: String,
    pub sub_queries: Vec<String>,
    /// Semantic search facts — `[!]` sub-cycle (b2): populated by keyword search until
    /// OQ-3 `query_vec_similarity` on graph is available.
    pub semantic_facts: Vec<String>,
    /// Entity insights with name/type/summary/related_facts.
    pub entity_insights: Vec<serde_json::Map<String, serde_json::Value>>,
    /// Relationship chains as human-readable strings.
    pub relationship_chains: Vec<String>,
    pub total_facts: i64,
    pub total_entities: i64,
    pub total_relationships: i64,
}

impl InsightForgeResult {
    /// Convert to dict matching Python `to_dict()`.
    ///
    /// Key order matches `InsightForgeResult.to_dict()` (`zep_tools.py:158-169`).
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("query".into(), self.query.clone().into());
        m.insert("simulation_requirement".into(), self.simulation_requirement.clone().into());
        m.insert(
            "sub_queries".into(),
            serde_json::to_value(&self.sub_queries).unwrap_or_default(),
        );
        m.insert(
            "semantic_facts".into(),
            serde_json::to_value(&self.semantic_facts).unwrap_or_default(),
        );
        m.insert(
            "entity_insights".into(),
            serde_json::to_value(&self.entity_insights).unwrap_or_default(),
        );
        m.insert(
            "relationship_chains".into(),
            serde_json::to_value(&self.relationship_chains).unwrap_or_default(),
        );
        m.insert("total_facts".into(), self.total_facts.into());
        m.insert("total_entities".into(), self.total_entities.into());
        m.insert("total_relationships".into(), self.total_relationships.into());
        m
    }

    /// Convert to text matching Python `to_text()` (`zep_tools.py:171-211`).
    pub fn to_text(&self) -> String {
        let mut text_parts = vec![
            "## 未来预测深度分析".to_string(),
            format!("分析问题: {}", self.query),
            format!("预测场景: {}", self.simulation_requirement),
            "\n### 预测数据统计".to_string(),
            format!("- 相关预测事实: {}条", self.total_facts),
            format!("- 涉及实体: {}个", self.total_entities),
            format!("- 关系链: {}条", self.total_relationships),
        ];

        if !self.sub_queries.is_empty() {
            text_parts.push("\n### 分析的子问题".to_string());
            for (i, sq) in self.sub_queries.iter().enumerate() {
                text_parts.push(format!("{}. {}", i + 1, sq));
            }
        }

        if !self.semantic_facts.is_empty() {
            text_parts.push("\n### 【关键事实】(请在报告中引用这些原文)".to_string());
            for (i, fact) in self.semantic_facts.iter().enumerate() {
                text_parts.push(format!("{}. \"{}\"", i + 1, fact));
            }
        }

        if !self.entity_insights.is_empty() {
            text_parts.push("\n### 【核心实体】".to_string());
            for entity in &self.entity_insights {
                let name = entity.get("name").and_then(|v| v.as_str()).unwrap_or("未知");
                let kind = entity.get("type").and_then(|v| v.as_str()).unwrap_or("实体");
                text_parts.push(format!("- **{}** ({})", name, kind));
                if let Some(summary) =
                    entity.get("summary").and_then(|v| v.as_str()).filter(|s| !s.is_empty())
                {
                    text_parts.push(format!("  摘要: \"{}\"", summary));
                }
                if let Some(rf) = entity.get("related_facts").and_then(|v| v.as_array()) {
                    text_parts.push(format!("  相关事实: {}条", rf.len()));
                }
            }
        }

        if !self.relationship_chains.is_empty() {
            text_parts.push("\n### 【关系链】".to_string());
            for chain in &self.relationship_chains {
                text_parts.push(format!("- {}", chain));
            }
        }

        text_parts.join("\n")
    }
}

/// Result from panorama_search operation.
///
/// Port of `PanoramaResult` dataclass (`zep_tools.py:214-281`).
///
/// Python fields (verbatim):
///   query, all_nodes, all_edges, active_facts, historical_facts,
///   total_nodes, total_edges, active_count, historical_count
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PanoramaResult {
    pub query: String,
    pub all_nodes: Vec<NodeInfo>,
    pub all_edges: Vec<EdgeInfo>,
    pub active_facts: Vec<String>,
    pub historical_facts: Vec<String>,
    pub total_nodes: i64,
    pub total_edges: i64,
    pub active_count: i64,
    pub historical_count: i64,
}

impl PanoramaResult {
    /// Convert to dict matching Python `to_dict()` (`zep_tools.py:237-248`).
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("query".into(), self.query.clone().into());
        let all_nodes: Vec<_> = self
            .all_nodes
            .iter()
            .map(|n| serde_json::to_value(n.to_dict()).unwrap_or_default())
            .collect();
        m.insert("all_nodes".into(), serde_json::Value::Array(all_nodes));
        let all_edges: Vec<_> = self
            .all_edges
            .iter()
            .map(|e| serde_json::to_value(e.to_dict()).unwrap_or_default())
            .collect();
        m.insert("all_edges".into(), serde_json::Value::Array(all_edges));
        m.insert(
            "active_facts".into(),
            serde_json::to_value(&self.active_facts).unwrap_or_default(),
        );
        m.insert(
            "historical_facts".into(),
            serde_json::to_value(&self.historical_facts).unwrap_or_default(),
        );
        m.insert("total_nodes".into(), self.total_nodes.into());
        m.insert("total_edges".into(), self.total_edges.into());
        m.insert("active_count".into(), self.active_count.into());
        m.insert("historical_count".into(), self.historical_count.into());
        m
    }

    /// Convert to text matching Python `to_text()` (`zep_tools.py:250-281`).
    pub fn to_text(&self) -> String {
        let mut text_parts = vec![
            "## 广度搜索结果（未来全景视图）".to_string(),
            format!("查询: {}", self.query),
            "\n### 统计信息".to_string(),
            format!("- 总节点数: {}", self.total_nodes),
            format!("- 总边数: {}", self.total_edges),
            format!("- 当前有效事实: {}条", self.active_count),
            format!("- 历史/过期事实: {}条", self.historical_count),
        ];

        if !self.active_facts.is_empty() {
            text_parts.push("\n### 【当前有效事实】(模拟结果原文)".to_string());
            for (i, fact) in self.active_facts.iter().enumerate() {
                text_parts.push(format!("{}. \"{}\"", i + 1, fact));
            }
        }

        if !self.historical_facts.is_empty() {
            text_parts.push("\n### 【历史/过期事实】(演变过程记录)".to_string());
            for (i, fact) in self.historical_facts.iter().enumerate() {
                text_parts.push(format!("{}. \"{}\"", i + 1, fact));
            }
        }

        if !self.all_nodes.is_empty() {
            text_parts.push("\n### 【涉及实体】".to_string());
            for node in &self.all_nodes {
                let entity_type = node
                    .labels
                    .iter()
                    .find(|l| *l != "Entity" && *l != "Node")
                    .map(|s| s.as_str())
                    .unwrap_or("实体");
                text_parts.push(format!("- **{}** ({})", node.name, entity_type));
            }
        }

        text_parts.join("\n")
    }
}

/// A single agent's interview record.
///
/// Port of `AgentInterview` dataclass (`zep_tools.py:285-340`). The full field
/// set (U-024 DTO-widening; the prior narrowed `{agent_id, platform, profile}`
/// shape was a hidden `[≠]` downgrade now corrected).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentInterview {
    pub agent_name: String,
    pub agent_role: String,
    pub agent_bio: String,
    pub question: String,
    pub response: String,
    pub key_quotes: Vec<String>,
}

impl AgentInterview {
    /// Convert to dict matching Python `to_dict()` (`zep_tools.py:294-300`).
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("agent_name".into(), self.agent_name.clone().into());
        m.insert("agent_role".into(), self.agent_role.clone().into());
        m.insert("agent_bio".into(), self.agent_bio.clone().into());
        m.insert("question".into(), self.question.clone().into());
        m.insert("response".into(), self.response.clone().into());
        m.insert("key_quotes".into(), serde_json::to_value(&self.key_quotes).unwrap_or_default());
        m
    }

    /// Convert to text matching Python `to_text()` (`zep_tools.py:301-340`).
    ///
    /// Emits bold name/role, full bio, Q/A, then a `关键引言` block with the
    /// elaborate per-quote cleaning (strip quote chars + leading punctuation,
    /// skip `问题{1-9}`, truncate >150 chars at first `。` after pos 80, drop
    /// quotes shorter than 10 chars). Byte-identical output required.
    pub fn to_text(&self) -> String {
        let mut text = format!("**{}** ({})\n", self.agent_name, self.agent_role);
        text += &format!("_简介: {}_\n\n", self.agent_bio);
        text += &format!("**Q:** {}\n\n", self.question);
        text += &format!("**A:** {}\n", self.response);
        if !self.key_quotes.is_empty() {
            text += "\n**关键引言:**\n";
            for quote in &self.key_quotes {
                // Clean quotes: remove unicode quote chars (curly “”, straight ",
                // CJK corner 「」) — equivalent to Python's chained `.replace(...)`.
                let mut clean =
                    quote.replace(['\u{201c}', '\u{201d}', '"', '\u{300c}', '\u{300d}'], "");
                clean = clean.trim().to_string();
                // Strip leading punctuation.
                while !clean.is_empty() {
                    let first = clean.chars().next().unwrap();
                    if "，,；;：:、。！？\n\r\t ".contains(first) {
                        let byte_len = first.len_utf8();
                        clean = clean[byte_len..].to_string();
                    } else {
                        break;
                    }
                }
                // Skip quotes containing 问题1-9.
                let skip = ('1'..='9').any(|d| clean.contains(&format!("问题{}", d)));
                if skip {
                    continue;
                }
                // Truncate >150 chars.
                let char_count = clean.chars().count();
                if char_count > 150 {
                    // Find first 。 after position 80.
                    let chars: Vec<char> = clean.chars().collect();
                    let dot_pos = chars[80..].iter().position(|&c| c == '。').map(|p| p + 80);
                    if let Some(pos) = dot_pos {
                        clean = chars[..=pos].iter().collect();
                    } else {
                        clean = chars[..147].iter().collect::<String>() + "...";
                    }
                }
                if clean.chars().count() >= 10 {
                    text += &format!("> \"{}\"\n", clean);
                }
            }
        }
        text
    }
}

/// Result from `interview_agents` operation.
///
/// Port of `InterviewResult` dataclass (`zep_tools.py:341-398`). The full field
/// set (U-024 DTO-widening; the prior narrowed `{agent_interviews, questions,
/// responses}` shape was a hidden `[≠]` downgrade now corrected).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InterviewResult {
    pub interview_topic: String,
    pub interview_questions: Vec<String>,
    pub selected_agents: Vec<serde_json::Map<String, serde_json::Value>>,
    pub interviews: Vec<AgentInterview>,
    pub selection_reasoning: String,
    pub summary: String,
    pub total_agents: i64,
    pub interviewed_count: i64,
}

impl InterviewResult {
    /// Construct with the two eagerly-set fields (Python `InterviewResult(
    /// interview_topic=…, interview_questions=…)`, `zep_tools.py:1310`).
    pub fn new(interview_topic: String, interview_questions: Vec<String>) -> Self {
        Self { interview_topic, interview_questions, ..Default::default() }
    }

    /// Convert to dict matching Python `to_dict()` (`zep_tools.py:362-379`).
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("interview_topic".into(), self.interview_topic.clone().into());
        m.insert(
            "interview_questions".into(),
            serde_json::to_value(&self.interview_questions).unwrap_or_default(),
        );
        m.insert(
            "selected_agents".into(),
            serde_json::to_value(&self.selected_agents).unwrap_or_default(),
        );
        m.insert(
            "interviews".into(),
            serde_json::Value::Array(
                self.interviews.iter().map(|i| serde_json::Value::Object(i.to_dict())).collect(),
            ),
        );
        m.insert("selection_reasoning".into(), self.selection_reasoning.clone().into());
        m.insert("summary".into(), self.summary.clone().into());
        m.insert("total_agents".into(), self.total_agents.into());
        m.insert("interviewed_count".into(), self.interviewed_count.into());
        m
    }

    /// Convert to text matching Python `to_text()` (`zep_tools.py:380-398`).
    pub fn to_text(&self) -> String {
        let mut text_parts = vec![
            "## 深度采访报告".to_string(),
            format!("**采访主题:** {}", self.interview_topic),
            format!("**采访人数:** {} / {} 位模拟Agent", self.interviewed_count, self.total_agents),
            "\n### 采访对象选择理由".to_string(),
            if self.selection_reasoning.is_empty() {
                "（自动选择）".to_string()
            } else {
                self.selection_reasoning.clone()
            },
            "\n---".to_string(),
            "\n### 采访实录".to_string(),
        ];

        if !self.interviews.is_empty() {
            for (i, interview) in self.interviews.iter().enumerate() {
                text_parts.push(format!("\n#### 采访 #{}: {}", i + 1, interview.agent_name));
                text_parts.push(interview.to_text());
                text_parts.push("\n---".to_string());
            }
        } else {
            text_parts.push("（无采访记录）\n\n---".to_string());
        }

        text_parts.push("\n### 采访摘要与核心观点".to_string());
        text_parts.push(if self.summary.is_empty() {
            "（无摘要）".to_string()
        } else {
            self.summary.clone()
        });

        text_parts.join("\n")
    }
}

// ---------------------------------------------------------------------------
// ReportTools<'g, L> — sub-cycle (b) BLOCKER — real graph reads
//
// Binds a borrowed &KnowledgeGraph + &L (LLM) + a KnowledgeGraphEntityReader<'g>
// per DECISION-9 (borrow, not Arc) and DECISION-11 (caller constructs handles).
// ---------------------------------------------------------------------------

/// Graph-backed tool facade for the ReportAgent's ReACT loop.
///
/// Port of `ZepToolsService` methods that require graph access, re-homed here
/// per the sub-cycle (b) architecture decision (DECISION-9/11).
///
/// `graph_id` parameters are kept in signatures where Python has them
/// (observable contract / `[≠]` label: the bound `&KnowledgeGraph` IS the
/// selector; `graph_id` is ignored for selection but may appear in output
/// fields such as `get_graph_statistics["graph_id"]`).
///
/// # U-024 `runner` seam
/// `L` carries the `Send + Sync + 'static` bound required by `SimulationRunner<L>`
/// so the optional `runner` borrow can be held. All real callers (`OpenAiAdapter`)
/// and tests (`StubLlm`) already satisfy this; `new(graph, llm)` stays byte-compatible.
pub struct ReportTools<'g, L: LlmClient + Send + Sync + 'static> {
    /// The knowledge graph being read. Replaces Zep's `graph_id` server-handle.
    graph: &'g KnowledgeGraph,
    /// LLM client for insight_forge sub-query generation.
    #[allow(dead_code)]
    llm: &'g L,
    /// Entity reader reusing U-016 substrate for entity-by-type / entity-summary reads.
    reader: KnowledgeGraphEntityReader<'g>,
    /// Optional live simulation runner for `interview_agents` IPC dispatch (U-024).
    /// `None` for the graph-only construction sites (debug routes, tests) — those
    /// keep `new(graph, llm)`. `Some(...)` only on the report-generation routes that
    /// can reach a live sim. `&'g` runner: caller-owned (`Arc` held by `ApiState`).
    runner: Option<&'g SimulationRunner<L>>,
}

impl<'g, L: LlmClient + Send + Sync + 'static> ReportTools<'g, L> {
    /// Create a new `ReportTools` binding graph and LLM by reference.
    ///
    /// Per DECISION-11, caller constructs handles; `ReportTools` borrows them.
    /// Graph-only facade (`runner: None`) — back-compat with all existing callers.
    pub fn new(graph: &'g KnowledgeGraph, llm: &'g L) -> Self {
        Self::with_runner(graph, llm, None)
    }

    /// Create a `ReportTools` binding graph + LLM + an optional live simulation
    /// runner (U-024). The report-generation routes pass `Some(&state.sim_runner)`
    /// so `interview_agents` can reach the live IPC seam.
    pub fn with_runner(
        graph: &'g KnowledgeGraph,
        llm: &'g L,
        runner: Option<&'g SimulationRunner<L>>,
    ) -> Self {
        let reader = KnowledgeGraphEntityReader::new(graph);
        Self { graph, llm, reader, runner }
    }

    // -----------------------------------------------------------------------
    // get_all_nodes — real graph read
    //
    // Port of `ZepToolsService.get_all_nodes(graph_id)` (`zep_tools.py:650-676`).
    // Returns all entities mapped to `NodeInfo`.
    // `graph_id` param kept for signature parity ([≠] inexpressible, ignored for selection).
    // -----------------------------------------------------------------------

    /// Get all nodes from the graph.
    pub fn get_all_nodes(&self, _graph_id: &str) -> Vec<NodeInfo> {
        self.graph.get_all_entities().into_iter().map(entity_to_node_info).collect()
    }

    // -----------------------------------------------------------------------
    // get_all_edges — real graph read
    //
    // Port of `ZepToolsService.get_all_edges(graph_id, include_temporal=True)`
    // (`zep_tools.py:678-714`).
    // Returns all edges mapped to `EdgeInfo`.
    // Temporal fields: valid_at + invalid_at derived from `Relation.valid_at` (GAP-1 landed).
    // -----------------------------------------------------------------------

    /// Get all edges from the graph.
    ///
    /// `include_temporal`: when true, fills in `valid_at`/`invalid_at` from
    /// `Relation.valid_at` window (GAP-1 already landed).
    pub fn get_all_edges(&self, _graph_id: &str, include_temporal: bool) -> Vec<EdgeInfo> {
        self.graph
            .get_all_edges()
            .into_iter()
            .map(|(from_id, to_id, relation)| {
                edge_triple_to_edge_info(from_id, to_id, &relation, include_temporal)
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // get_node_detail — real graph read
    //
    // Port of `ZepToolsService.get_node_detail(node_uuid)` (`zep_tools.py:716-746`).
    // Python: looks up by uuid; returns None on missing/error.
    // -----------------------------------------------------------------------

    /// Get detailed information about a node by UUID.
    ///
    /// Returns `None` if `node_uuid` cannot be parsed or is not in the graph.
    pub fn get_node_detail(&self, node_uuid: &str) -> Option<NodeInfo> {
        let id = node_uuid.parse::<uuid::Uuid>().ok()?;
        let entity = self.graph.get_entity_by_id(id)?;
        Some(entity_to_node_info(entity))
    }

    // -----------------------------------------------------------------------
    // get_node_edges — real graph read
    //
    // Port of `ZepToolsService.get_node_edges(graph_id, node_uuid)`
    // (`zep_tools.py:748-778`).
    // Python: gets all edges then filters by source/target matching node_uuid.
    // Fallback: returns [] on any error.
    // -----------------------------------------------------------------------

    /// Get edges connected to a node.
    ///
    /// Returns empty `Vec` if `node_uuid` is invalid or not found.
    pub fn get_node_edges(&self, _graph_id: &str, node_uuid: &str) -> Vec<EdgeInfo> {
        let all_edges = self.get_all_edges(_graph_id, true);
        all_edges
            .into_iter()
            .filter(|e| e.source_node_uuid == node_uuid || e.target_node_uuid == node_uuid)
            .collect()
    }

    // -----------------------------------------------------------------------
    // get_entities_by_type — real graph read via U-016 reader
    //
    // Port of `ZepToolsService.get_entities_by_type(graph_id, entity_type)`
    // (`zep_tools.py:780-806`).
    // Python: gets all nodes then filters where entity_type is in node.labels.
    // -----------------------------------------------------------------------

    /// Get entities by type.
    pub fn get_entities_by_type(&self, _graph_id: &str, entity_type: &str) -> Vec<NodeInfo> {
        // Use U-016 reader which provides the same filter semantics.
        let filtered = self.reader.get_entities_by_type(entity_type, false);
        filtered
            .into_iter()
            .map(|en| NodeInfo {
                uuid: en.uuid,
                name: en.name,
                labels: en.labels,
                summary: en.summary,
                attributes: en.attributes,
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // get_entity_summary — real graph read
    //
    // Port of `ZepToolsService.get_entity_summary(graph_id, entity_name)`
    // (`zep_tools.py:808-853`).
    // Python: searches by name (case-insensitive) across all nodes,
    //         runs search_graph for related facts, gets node_edges.
    //         Returns dict with entity_name, entity_info, related_facts, related_edges, total_relations.
    // -----------------------------------------------------------------------

    /// Get a summary for an entity by name.
    ///
    /// Returns a JSON dict matching Python `get_entity_summary` output:
    /// `{entity_name, entity_info, related_facts, related_edges, total_relations}`.
    pub fn get_entity_summary(
        &self,
        graph_id: &str,
        entity_name: &str,
    ) -> serde_json::Map<String, serde_json::Value> {
        // Search for related facts via local keyword search.
        let search_result = self.local_search(graph_id, entity_name, 20, Some("edges"));

        // Find the entity node by case-insensitive name match.
        let all_nodes = self.get_all_nodes(graph_id);
        let entity_node = all_nodes
            .iter()
            .find(|n| n.name.to_lowercase() == entity_name.to_lowercase())
            .cloned();

        let related_edges = if let Some(ref node) = entity_node {
            self.get_node_edges(graph_id, &node.uuid)
        } else {
            Vec::new()
        };

        let total_relations = related_edges.len() as i64;

        let mut m = serde_json::Map::new();
        m.insert("entity_name".into(), entity_name.into());
        m.insert(
            "entity_info".into(),
            entity_node
                .as_ref()
                .map(|n| serde_json::to_value(n.to_dict()).unwrap_or(serde_json::Value::Null))
                .unwrap_or(serde_json::Value::Null),
        );
        m.insert(
            "related_facts".into(),
            serde_json::to_value(&search_result.facts).unwrap_or_default(),
        );
        let edges_dicts: Vec<_> = related_edges
            .iter()
            .map(|e| serde_json::to_value(e.to_dict()).unwrap_or_default())
            .collect();
        m.insert("related_edges".into(), serde_json::Value::Array(edges_dicts));
        m.insert("total_relations".into(), total_relations.into());
        m
    }

    // -----------------------------------------------------------------------
    // get_graph_statistics — real graph read
    //
    // Port of `ZepToolsService.get_graph_statistics(graph_id)`
    // (`zep_tools.py:855-888`).
    // Returns: {graph_id, total_nodes, total_edges, entity_types, relation_types}
    // -----------------------------------------------------------------------

    /// Get graph statistics.
    ///
    /// Returns dict matching Python `get_graph_statistics` shape:
    /// `{graph_id, total_nodes, total_edges, entity_types, relation_types}`.
    pub fn get_graph_statistics(
        &self,
        graph_id: &str,
    ) -> serde_json::Map<String, serde_json::Value> {
        let nodes = self.get_all_nodes(graph_id);
        let edges = self.get_all_edges(graph_id, false);

        // Count entity types (exclude "Entity"/"Node" base labels — mirrors Python L874).
        // Python uses a plain dict, so the key order is FIRST-SEEN insertion order
        // (zep_tools.py:870–874). Count into a `serde_json::Map` (the crate is built with
        // `preserve_order` → IndexMap-backed) so the `{entity_types}` order is DETERMINISTIC
        // and matches Python's insertion order — NOT a randomized `HashMap`.
        let mut entity_types = serde_json::Map::new();
        for node in &nodes {
            for label in &node.labels {
                if label != "Entity" && label != "Node" {
                    let e = entity_types
                        .entry(label.clone())
                        .or_insert_with(|| serde_json::Value::from(0i64));
                    let next = e.as_i64().unwrap_or(0) + 1;
                    *e = serde_json::Value::from(next);
                }
            }
        }

        // Count relation types by edge name (same first-seen insertion-order contract).
        let mut relation_types = serde_json::Map::new();
        for edge in &edges {
            let e = relation_types
                .entry(edge.name.clone())
                .or_insert_with(|| serde_json::Value::from(0i64));
            let next = e.as_i64().unwrap_or(0) + 1;
            *e = serde_json::Value::from(next);
        }

        let mut m = serde_json::Map::new();
        // Python includes graph_id in output (zep_tools.py:882).
        m.insert("graph_id".into(), graph_id.into());
        m.insert("total_nodes".into(), (nodes.len() as i64).into());
        m.insert("total_edges".into(), (edges.len() as i64).into());
        m.insert("entity_types".into(), serde_json::Value::Object(entity_types));
        m.insert("relation_types".into(), serde_json::Value::Object(relation_types));
        m
    }

    // -----------------------------------------------------------------------
    // get_simulation_context — real graph read
    //
    // Port of `ZepToolsService.get_simulation_context(graph_id, simulation_requirement, limit=30)`
    // (`zep_tools.py:890-941`).
    // Returns: {simulation_requirement, related_facts, graph_statistics, entities, total_entities}
    // -----------------------------------------------------------------------

    /// Get simulation context for ReportAgent.
    ///
    /// Returns dict matching Python `get_simulation_context` shape.
    pub fn get_simulation_context(
        &self,
        graph_id: &str,
        simulation_requirement: &str,
        limit: usize,
    ) -> serde_json::Map<String, serde_json::Value> {
        let limit = if limit == 0 { 30 } else { limit };

        // Search for related facts via local search.
        let search_result =
            self.local_search(graph_id, simulation_requirement, limit as i64, Some("edges"));

        // Get graph statistics.
        let stats = self.get_graph_statistics(graph_id);

        // Get all nodes and filter to those with a custom (non-base) type.
        let all_nodes = self.get_all_nodes(graph_id);
        let entities: Vec<serde_json::Value> = all_nodes
            .iter()
            .filter_map(|node| {
                // custom_labels = [l for l in node.labels if l not in ["Entity", "Node"]]
                let custom_labels: Vec<&str> = node
                    .labels
                    .iter()
                    .map(|l| l.as_str())
                    .filter(|l| *l != "Entity" && *l != "Node")
                    .collect();
                if custom_labels.is_empty() {
                    return None;
                }
                let mut e = serde_json::Map::new();
                e.insert("name".into(), node.name.clone().into());
                e.insert("type".into(), custom_labels[0].into());
                // summary is [≠] (no per-entity summary in teri); emit ""
                e.insert("summary".into(), "".into());
                Some(serde_json::Value::Object(e))
            })
            .take(limit)
            .collect();

        let total_entities = all_nodes
            .iter()
            .filter(|n| n.labels.iter().any(|l| l != "Entity" && l != "Node"))
            .count();

        let mut m = serde_json::Map::new();
        m.insert("simulation_requirement".into(), simulation_requirement.into());
        m.insert(
            "related_facts".into(),
            serde_json::to_value(&search_result.facts).unwrap_or_default(),
        );
        m.insert("graph_statistics".into(), serde_json::Value::Object(stats));
        m.insert("entities".into(), serde_json::Value::Array(entities));
        m.insert("total_entities".into(), (total_entities as i64).into());
        m
    }

    // -----------------------------------------------------------------------
    // local_search — real graph read (keyword scan)
    //
    // Port of `ZepToolsService._local_search(graph_id, query, limit=10, scope="edges")`
    // (`zep_tools.py:546-648`).
    // Keyword scoring: exact query match → 100, per-keyword match → +10.
    // Supports scope: "edges", "nodes", "both".
    // -----------------------------------------------------------------------

    /// Local keyword-based search over the graph.
    ///
    /// Replaces the stub `local_search` that returned an empty result; now performs
    /// a real keyword scan over `get_all_entities` + `get_all_edges`.
    pub fn local_search(
        &self,
        _graph_id: &str,
        query: &str,
        limit: i64,
        scope: Option<&str>,
    ) -> SearchResult {
        let limit = if limit <= 0 { 10 } else { limit as usize };
        let scope = scope.unwrap_or("edges");

        let query_lower = query.to_lowercase();
        // Simple tokenisation matching Python: split on space + comma (+ full-width comma),
        // keep tokens longer than 1 char.
        let keywords: Vec<String> = query_lower
            .replace([',', '，'], " ")
            .split_whitespace()
            .filter(|w| w.len() > 1)
            .map(|w| w.to_string())
            .collect();

        let match_score = |text: &str| -> i64 {
            if text.is_empty() {
                return 0;
            }
            let text_lower = text.to_lowercase();
            if text_lower.contains(&query_lower) {
                return 100;
            }
            let mut score = 0i64;
            for kw in &keywords {
                if text_lower.contains(kw.as_str()) {
                    score += 10;
                }
            }
            score
        };

        let mut facts: Vec<String> = Vec::new();
        let mut edges_result: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
        let mut nodes_result: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();

        if scope == "edges" || scope == "both" {
            // Get all edges and score by fact + name.
            let all_edges = self.get_all_edges(_graph_id, false);
            let mut scored_edges: Vec<(i64, EdgeInfo)> = all_edges
                .into_iter()
                .filter_map(|edge| {
                    let score = match_score(&edge.fact) + match_score(&edge.name);
                    if score > 0 { Some((score, edge)) } else { None }
                })
                .collect();
            // Sort by score descending (matches Python `sorted(key=x[0], reverse=True)`).
            scored_edges.sort_by(|a, b| b.0.cmp(&a.0));

            for (_, edge) in scored_edges.into_iter().take(limit) {
                if !edge.fact.is_empty() {
                    facts.push(edge.fact.clone());
                }
                let mut m = serde_json::Map::new();
                m.insert("uuid".into(), edge.uuid.clone().into());
                m.insert("name".into(), edge.name.clone().into());
                m.insert("fact".into(), edge.fact.clone().into());
                m.insert("source_node_uuid".into(), edge.source_node_uuid.clone().into());
                m.insert("target_node_uuid".into(), edge.target_node_uuid.clone().into());
                edges_result.push(m);
            }
        }

        if scope == "nodes" || scope == "both" {
            // Get all nodes and score by name + summary.
            let all_nodes = self.get_all_nodes(_graph_id);
            let mut scored_nodes: Vec<(i64, NodeInfo)> = all_nodes
                .into_iter()
                .filter_map(|node| {
                    let score = match_score(&node.name) + match_score(&node.summary);
                    if score > 0 { Some((score, node)) } else { None }
                })
                .collect();
            scored_nodes.sort_by(|a, b| b.0.cmp(&a.0));

            for (_, node) in scored_nodes.into_iter().take(limit) {
                let mut m = serde_json::Map::new();
                m.insert("uuid".into(), node.uuid.clone().into());
                m.insert("name".into(), node.name.clone().into());
                m.insert("labels".into(), serde_json::to_value(&node.labels).unwrap_or_default());
                m.insert("summary".into(), node.summary.clone().into());
                nodes_result.push(m);
                // Node summary counts as a fact (Python L634-635).
                if !node.summary.is_empty() {
                    facts.push(format!("[{}]: {}", node.name, node.summary));
                }
            }
        }

        let count = facts.len() as i64;
        SearchResult {
            facts,
            edges: edges_result,
            nodes: nodes_result,
            query: query.to_string(),
            total_count: count,
        }
    }

    // -----------------------------------------------------------------------
    // search_graph — real graph read (delegates to local_search)
    //
    // Port of `ZepToolsService.search_graph(graph_id, query, limit=10, scope="edges")`
    // (`zep_tools.py:464-544`).
    // Python: attempts Zep Cloud hybrid search + cross-encoder; falls back to
    // `_local_search` on failure. In teri there is no Zep server, so we go
    // directly to `local_search` (the fallback IS the implementation).
    // `[≠]`: Zep cross-encoder reranking / hybrid semantic+BM25 (server-side).
    // -----------------------------------------------------------------------

    /// Search the graph — delegates to local keyword search.
    ///
    /// Mirrors Python's fallback path (`_local_search`); the Zep Cloud hybrid
    /// search is `[≠]` (inexpressible: requires Zep server).
    pub fn search_graph(
        &self,
        graph_id: &str,
        query: &str,
        limit: i64,
        scope: Option<&str>,
    ) -> SearchResult {
        self.local_search(graph_id, query, limit, scope)
    }

    // -----------------------------------------------------------------------
    // quick_search — real graph read
    //
    // Port of `ZepToolsService.quick_search(graph_id, query, limit=10)`
    // (`zep_tools.py:1237-1270`).
    // Python: calls search_graph with scope="edges".
    // -----------------------------------------------------------------------

    /// Quick search — simple keyword-based search.
    ///
    /// Port of Python `quick_search` which calls `search_graph(scope="edges")`.
    pub fn quick_search(&self, graph_id: &str, query: &str, limit: i64) -> SearchResult {
        self.search_graph(graph_id, query, limit, Some("edges"))
    }

    // -----------------------------------------------------------------------
    // panorama_search — real graph read using partition_edges_at
    //
    // Port of `ZepToolsService.panorama_search(graph_id, query, include_expired=True, limit=50)`
    // (`zep_tools.py:1145-1235`).
    // Uses `KnowledgeGraph::partition_edges_at(t)` (GAP-1 landed) to classify
    // edges as active vs historical at the current unix timestamp.
    //
    // Python's `is_historical = edge.is_expired or edge.is_invalid` maps to:
    //   teri: active = `partition_edges_at(now).0`, historical = `.1`
    //   (Relation::is_active_at uses the valid_at window which is the Rust equivalent
    //    of Zep's expired_at / invalid_at fields).
    // -----------------------------------------------------------------------

    /// Panorama search — temporal-aware full-graph scan.
    ///
    /// Classifies all edges as active or historical at `timestamp_secs`.
    /// Pass `None` to use the current system time.
    pub fn panorama_search(
        &self,
        graph_id: &str,
        query: &str,
        include_expired: bool,
        limit: i64,
        timestamp_secs: Option<u64>,
    ) -> PanoramaResult {
        let limit = if limit <= 0 { 50 } else { limit as usize };
        let t = timestamp_secs.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        });

        // Get all nodes.
        let all_nodes = self.get_all_nodes(graph_id);
        let total_nodes = all_nodes.len() as i64;

        // Build a node UUID → NodeInfo map for fact labeling (mirrors Python's `node_map`).
        let node_map: std::collections::HashMap<String, &NodeInfo> =
            all_nodes.iter().map(|n| (n.uuid.clone(), n)).collect();

        // Partition edges into active vs historical at timestamp t.
        let (active_triples, historical_triples) = self.graph.partition_edges_at(t);
        let total_edges = (active_triples.len() + historical_triples.len()) as i64;

        // Build all_edges (both active + historical combined, with temporal info).
        let mut all_edges: Vec<EdgeInfo> = Vec::new();
        for (from_id, to_id, rel) in active_triples.iter().chain(historical_triples.iter()) {
            all_edges.push(edge_triple_to_edge_info(*from_id, *to_id, rel, true));
        }

        // Build keyword relevance scorer (mirrors Python `relevance_score`).
        let query_lower = query.to_lowercase();
        let keywords: Vec<String> = query_lower
            .replace([',', '，'], " ")
            .split_whitespace()
            .filter(|w| w.len() > 1)
            .map(|w| w.to_string())
            .collect();
        let relevance_score = |fact: &str| -> i64 {
            let fact_lower = fact.to_lowercase();
            if fact_lower.contains(&query_lower) {
                return 100;
            }
            let mut score = 0i64;
            for kw in &keywords {
                if fact_lower.contains(kw.as_str()) {
                    score += 10;
                }
            }
            score
        };

        // Build active_facts from active triples (Python: `if not edge.is_expired and not edge.is_invalid`).
        let mut active_facts: Vec<(i64, String)> = active_triples
            .iter()
            .filter_map(|(from_id, to_id, rel)| {
                // In teri, Relation has no "fact" string ([≠] DECISION-9 Q4).
                // We synthesize a fact from the relation kind and entity names,
                // mirroring what MiroFish stored as `edge.fact`.
                let fact = synthesize_fact_from_triple(*from_id, *to_id, rel, &node_map);
                if fact.is_empty() { None } else { Some((relevance_score(&fact), fact)) }
            })
            .collect();
        active_facts.sort_by(|a, b| b.0.cmp(&a.0));
        let active_facts: Vec<String> =
            active_facts.into_iter().take(limit).map(|(_, f)| f).collect();
        let active_count = active_facts.len() as i64;

        // Build historical_facts from historical triples.
        let mut historical_facts: Vec<(i64, String)> = historical_triples
            .iter()
            .filter_map(|(from_id, to_id, rel)| {
                let fact = synthesize_fact_from_triple(*from_id, *to_id, rel, &node_map);
                if fact.is_empty() {
                    return None;
                }
                // Add temporal tag matching Python: "[{valid_at} - {invalid_at}] {fact}".
                let valid_at_str =
                    rel.valid_at.map(|(s, _)| s.to_string()).unwrap_or_else(|| "未知".to_string());
                let invalid_at_str = rel
                    .valid_at
                    .and_then(|(_, e)| e)
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "未知".to_string());
                let tagged = format!("[{} - {}] {}", valid_at_str, invalid_at_str, fact);
                Some((relevance_score(&tagged), tagged))
            })
            .collect();
        historical_facts.sort_by(|a, b| b.0.cmp(&a.0));
        let historical_facts_full: Vec<String> =
            historical_facts.into_iter().take(limit).map(|(_, f)| f).collect();
        let historical_count = historical_facts_full.len() as i64;
        let historical_facts_out = if include_expired { historical_facts_full } else { vec![] };

        PanoramaResult {
            query: query.to_string(),
            all_nodes,
            all_edges,
            active_facts,
            historical_facts: historical_facts_out,
            total_nodes,
            total_edges,
            active_count,
            historical_count,
        }
    }

    // -----------------------------------------------------------------------
    // insight_forge — sub-cycle (b2) DEFERRED — keyword fallback
    //
    // Port of `ZepToolsService.insight_forge(graph_id, query, sim_req, context, max_sub_queries=5)`
    // (`zep_tools.py:945-1090`).
    //
    // Full implementation: LLM chat_json → sub-queries → per-sub-query semantic search
    // (needs OQ-3 `query_vec_similarity` on graph) → entity enrichment.
    //
    // Sub-cycle (b2) blocker: semantic ranking needs GAP-2 (OQ-3 shimmy embeddings).
    // Current: preserves the multi-sub-query STRUCTURE (sub_queries populated from
    // keyword-based decomposition), uses keyword search as backend.
    //
    // `[!]` ledger note: semantic ranking quality is pending OQ-3. The multi-query
    // structure IS preserved — this is not a silent drop.
    // -----------------------------------------------------------------------

    /// Insight forge — multi-query deep analysis.
    ///
    /// **Sub-cycle (b2) pending**: semantic ranking via `query_vec_similarity` (OQ-3/GAP-2).
    /// Ships with keyword-search backend preserving the full multi-sub-query structure.
    pub fn insight_forge(
        &self,
        graph_id: &str,
        query: &str,
        simulation_requirement: &str,
        _report_context: &str,
        max_sub_queries: i64,
    ) -> InsightForgeResult {
        let max_sub_queries = if max_sub_queries <= 0 { 5 } else { max_sub_queries as usize };

        // `[!]` (b2-pending): LLM sub-query decomposition via chat_json needs OQ-3.
        // Fallback: use keyword-based sub-query variants (matches Python's own exception
        // fallback in `_generate_sub_queries`, zep_tools.py:1135-1143).
        let sub_queries: Vec<String> = vec![
            query.to_string(),
            format!("{} 的主要参与者", query),
            format!("{} 的原因和影响", query),
            format!("{} 的发展过程", query),
        ]
        .into_iter()
        .take(max_sub_queries)
        .collect();

        // Step 2: per-sub-query keyword search (mirrors Python's search_graph per sub-query).
        let mut all_facts: Vec<String> = Vec::new();
        let mut all_edges: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
        let mut seen_facts: HashSet<String> = HashSet::new();

        for sub_query in &sub_queries {
            let result = self.search_graph(graph_id, sub_query, 15, Some("edges"));
            for fact in result.facts {
                if !seen_facts.contains(&fact) {
                    seen_facts.insert(fact.clone());
                    all_facts.push(fact);
                }
            }
            all_edges.extend(result.edges);
        }

        // Also search the original query (Python L1011-1021).
        let main_search = self.search_graph(graph_id, query, 20, Some("edges"));
        for fact in main_search.facts {
            if !seen_facts.contains(&fact) {
                seen_facts.insert(fact.clone());
                all_facts.push(fact);
            }
        }

        let total_facts = all_facts.len() as i64;

        // Step 3: extract entity UUIDs from edges and build entity insights.
        // `[!]` (b2-pending): in Python this calls `get_node_detail(uuid)` which returns
        // a full NodeInfo with summary. In teri, summary is `[≠]` "". We still
        // populate the entity_insights structure with available data (name/type).
        let mut entity_uuids: HashSet<String> = HashSet::new();
        for edge_data in &all_edges {
            if let Some(src) = edge_data
                .get("source_node_uuid")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                entity_uuids.insert(src.to_string());
            }
            if let Some(tgt) = edge_data
                .get("target_node_uuid")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                entity_uuids.insert(tgt.to_string());
            }
        }

        let mut entity_insights: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
        // Build a name→uuid lookup so we can populate related_facts by name.
        let all_nodes = self.get_all_nodes(graph_id);
        let uuid_to_name: std::collections::HashMap<String, String> =
            all_nodes.iter().map(|n| (n.uuid.clone(), n.name.clone())).collect();

        for uuid in &entity_uuids {
            if let Some(node) = self.get_node_detail(uuid) {
                let entity_type = node
                    .labels
                    .iter()
                    .find(|l| *l != "Entity" && *l != "Node")
                    .cloned()
                    .unwrap_or_else(|| "实体".to_string());
                let related_facts: Vec<&String> = all_facts
                    .iter()
                    .filter(|f| f.to_lowercase().contains(&node.name.to_lowercase()))
                    .collect();
                let mut insight = serde_json::Map::new();
                insight.insert("uuid".into(), uuid.clone().into());
                insight.insert("name".into(), node.name.clone().into());
                insight.insert("type".into(), entity_type.into());
                // summary is [≠] in teri; emit "" (DECISION-9 Q2).
                insight.insert("summary".into(), "".into());
                insight.insert(
                    "related_facts".into(),
                    serde_json::to_value(related_facts).unwrap_or_default(),
                );
                entity_insights.push(insight);
            }
        }
        let total_entities = entity_insights.len() as i64;

        // Step 4: build relationship chains (Python L1071-1087).
        let mut relationship_chains: Vec<String> = Vec::new();
        for edge_data in &all_edges {
            let src_uuid = edge_data.get("source_node_uuid").and_then(|v| v.as_str()).unwrap_or("");
            let tgt_uuid = edge_data.get("target_node_uuid").and_then(|v| v.as_str()).unwrap_or("");
            let rel_name = edge_data.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let src_name = uuid_to_name
                .get(src_uuid)
                .map(|s| s.as_str())
                .unwrap_or_else(|| &src_uuid[..src_uuid.len().min(8)]);
            let tgt_name = uuid_to_name
                .get(tgt_uuid)
                .map(|s| s.as_str())
                .unwrap_or_else(|| &tgt_uuid[..tgt_uuid.len().min(8)]);
            let chain = format!("{} --[{}]--> {}", src_name, rel_name, tgt_name);
            if !relationship_chains.contains(&chain) {
                relationship_chains.push(chain);
            }
        }
        let total_relationships = relationship_chains.len() as i64;

        InsightForgeResult {
            query: query.to_string(),
            simulation_requirement: simulation_requirement.to_string(),
            sub_queries,
            semantic_facts: all_facts,
            entity_insights,
            relationship_chains,
            total_facts,
            total_entities,
            total_relationships,
        }
    }

    // -----------------------------------------------------------------------
    // interview_agents (+ 5 private helpers) — U-024 full port
    //
    // Port of `ZepToolsService.interview_agents(simulation_id, ...)` and its 5
    // private helpers (`zep_tools.py:1272-1763`). Requires a live `SimulationRunner`
    // (threaded as `self.runner`) for the batch-interview IPC seam.
    //
    // [!] U-024-PROD-PENDING: the terminal IPC call returns env-not-running until
    // the live IPC producer lands (U-026-k). The full logic is ported; only the
    // live-data flip is deferred — NOT a stub.
    // -----------------------------------------------------------------------

    /// Load agent-profile dicts for a simulation.
    ///
    /// Port of `_load_agent_profiles(simulation_id)` (`zep_tools.py:1505-1549`).
    /// Prefers `reddit_profiles.json` (a JSON array, returned as-is); else falls
    /// back to `twitter_profiles.csv` mapped row-by-row. Read failures log a
    /// warning and fall through (returning an empty list).
    fn load_agent_profiles(
        sim_data_dir: &Path,
        simulation_id: &str,
    ) -> Vec<serde_json::Map<String, serde_json::Value>> {
        let sim_dir = sim_data_dir.join(simulation_id);

        // Prefer reddit_profiles.json (JSON array).
        let reddit_path = sim_dir.join("reddit_profiles.json");
        if reddit_path.exists() {
            match std::fs::read_to_string(&reddit_path).map_err(|e| e.to_string()).and_then(|c| {
                serde_json::from_str::<serde_json::Value>(&c).map_err(|e| e.to_string())
            }) {
                Ok(serde_json::Value::Array(arr)) => {
                    let profiles: Vec<serde_json::Map<String, serde_json::Value>> = arr
                        .into_iter()
                        .filter_map(|v| match v {
                            serde_json::Value::Object(m) => Some(m),
                            _ => None,
                        })
                        .collect();
                    tracing::info!(
                        target: "teri::report",
                        "{}",
                        t_args(
                            "console.loadedRedditProfiles",
                            &[("count", &profiles.len())]
                        )
                    );
                    return profiles;
                }
                Ok(_) => { /* not an array — fall through to twitter */ }
                Err(e) => {
                    tracing::warn!(
                        target: "teri::report",
                        "{}",
                        t_args("console.readRedditProfilesFailed", &[("error", &e)])
                    );
                }
            }
        }

        // Fallback: twitter_profiles.csv via csv DictReader-equivalent.
        let twitter_path = sim_dir.join("twitter_profiles.csv");
        if twitter_path.exists() {
            match csv::Reader::from_path(&twitter_path) {
                Ok(mut rdr) => {
                    let mut profiles: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
                    for record in rdr.deserialize::<std::collections::HashMap<String, String>>() {
                        match record {
                            Ok(row) => {
                                let get = |k: &str| row.get(k).cloned().unwrap_or_default();
                                let mut m = serde_json::Map::new();
                                m.insert("realname".into(), get("name").into());
                                m.insert("username".into(), get("username").into());
                                m.insert("bio".into(), get("description").into());
                                m.insert("persona".into(), get("user_char").into());
                                m.insert("profession".into(), "未知".into());
                                profiles.push(m);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    target: "teri::report",
                                    "{}",
                                    t_args(
                                        "console.readTwitterProfilesFailed",
                                        &[("error", &e.to_string())]
                                    )
                                );
                            }
                        }
                    }
                    tracing::info!(
                        target: "teri::report",
                        "{}",
                        t_args(
                            "console.loadedTwitterProfiles",
                            &[("count", &profiles.len())]
                        )
                    );
                    return profiles;
                }
                Err(e) => {
                    tracing::warn!(
                        target: "teri::report",
                        "{}",
                        t_args(
                            "console.readTwitterProfilesFailed",
                            &[("error", &e.to_string())]
                        )
                    );
                }
            }
        }

        Vec::new()
    }

    /// Clean a tool-call-wrapped response.
    ///
    /// Port of `_clean_tool_call_response(response)` (`zep_tools.py:1484-1504`,
    /// static). If the response is empty or doesn't look like JSON, return as-is.
    /// Otherwise unwrap `arguments.{content,text,body,message,reply}` (first hit);
    /// on JSON/key error, regex-extract `"content": "..."`.
    fn clean_tool_call_response(response: String) -> String {
        let trimmed = response.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            return response;
        }
        // `'tool_name' not in text[:80]` → return as-is.
        let head: String = trimmed.chars().take(80).collect();
        if !head.contains("tool_name") {
            return response;
        }
        // Python `try: data = json.loads(text)` — the regex fallback runs ONLY in
        // the `except (JSONDecodeError, KeyError, TypeError)` branch. A successful
        // parse with no matching key falls through to `return response` (no regex).
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(serde_json::Value::Object(data)) => {
                if let Some(serde_json::Value::Object(args)) = data.get("arguments") {
                    for key in ["content", "text", "body", "message", "reply"] {
                        if let Some(v) = args.get(key) {
                            // Python `str(data['arguments'][key])`.
                            return match v {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                        }
                    }
                }
                response
            }
            Ok(_) => response, // parsed but not a dict — Python's `isinstance(data, dict)` false
            Err(_) => {
                // JSONDecodeError → regex fallback: "content"\s*:\s*"((?:[^"\\]|\\.)*)"
                let re = Regex::new(r#""content"\s*:\s*"((?:[^"\\]|\\.)*)""#).unwrap();
                if let Some(g1) = re.captures(trimmed).and_then(|caps| caps.get(1)) {
                    return g1.as_str().replace("\\n", "\n").replace("\\\"", "\"");
                }
                response
            }
        }
    }

    /// Select which agents to interview via the LLM.
    ///
    /// Port of `_select_agents_for_interview(...)` (`zep_tools.py:1551-1632`).
    /// Returns `(selected_agents, selected_indices, reasoning)`. On any LLM
    /// failure, falls back to the first `max_agents` profiles.
    async fn select_agents_for_interview(
        &self,
        profiles: &[serde_json::Map<String, serde_json::Value>],
        interview_requirement: &str,
        simulation_requirement: &str,
        max_agents: i64,
    ) -> (Vec<serde_json::Map<String, serde_json::Value>>, Vec<usize>, String) {
        let max_agents_usize = max_agents.max(0) as usize;

        // Build per-profile summary dicts.
        let mut agents_summary = Vec::new();
        for (i, profile) in profiles.iter().enumerate() {
            let name = resolve_agent_name(profile, i);
            // Python `profile.get("profession", "未知")` — default ONLY when key absent.
            let profession = py_get_str(profile, "profession", "未知");
            let bio_full = py_get_str(profile, "bio", "");
            let bio: String = bio_full.chars().take(200).collect();
            let interested_topics = profile
                .get("interested_topics")
                .cloned()
                .unwrap_or(serde_json::Value::Array(Vec::new()));

            let mut m = serde_json::Map::new();
            m.insert("index".into(), serde_json::Value::from(i));
            m.insert("name".into(), name.into());
            m.insert("profession".into(), profession.into());
            m.insert("bio".into(), bio.into());
            m.insert("interested_topics".into(), interested_topics);
            agents_summary.push(serde_json::Value::Object(m));
        }

        // Python `json.dumps(agent_summaries, ensure_ascii=False, indent=2)`.
        let agents_json =
            serde_json::to_string_pretty(&agents_summary).unwrap_or_else(|_| "[]".to_string());

        let system_prompt = "你是一个专业的采访策划专家。你的任务是根据采访需求，从模拟Agent列表中选择最适合采访的对象。\n\n选择标准：\n1. Agent的身份/职业与采访主题相关\n2. Agent可能持有独特或有价值的观点\n3. 选择多样化的视角（如：支持方、反对方、中立方、专业人士等）\n4. 优先选择与事件直接相关的角色\n\n返回JSON格式：\n{\n    \"selected_indices\": [选中Agent的索引列表],\n    \"reasoning\": \"选择理由说明\"\n}";
        let sim_bg =
            if simulation_requirement.is_empty() { "未提供" } else { simulation_requirement };
        let user_prompt = format!(
            "采访需求：\n{}\n\n模拟背景：\n{}\n\n可选择的Agent列表（共{}个）：\n{}\n\n请选择最多{}个最适合采访的Agent，并说明选择理由。",
            interview_requirement,
            sim_bg,
            agents_summary.len(),
            agents_json,
            max_agents
        );

        let messages = [ChatMessage::system(system_prompt), ChatMessage::user(user_prompt)];
        let opts = ChatOptions { temperature: Some(0.3), max_tokens: None };

        let fallback = |profiles: &[serde_json::Map<String, serde_json::Value>]| {
            let n = max_agents_usize.min(profiles.len());
            let indices: Vec<usize> = (0..n).collect();
            let selected: Vec<serde_json::Map<String, serde_json::Value>> = profiles[..n].to_vec();
            (selected, indices, "使用默认选择策略".to_string())
        };

        match self.llm.chat_json::<serde_json::Value>(&messages, &opts).await {
            Ok(resp) => {
                let selected_indices_raw = resp
                    .get("selected_indices")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let reasoning = resp
                    .get("reasoning")
                    .and_then(|v| v.as_str())
                    .unwrap_or("基于相关性自动选择")
                    .to_string();

                // Take first max_agents indices, filter to valid in-range.
                let mut selected_agents = Vec::new();
                let mut valid_indices = Vec::new();
                for idx_val in selected_indices_raw.iter().take(max_agents_usize) {
                    // Python `if 0 <= idx < len(profiles)`.
                    let valid =
                        idx_val.as_i64().filter(|&idx| idx >= 0 && (idx as usize) < profiles.len());
                    if let Some(idx) = valid {
                        let u = idx as usize;
                        selected_agents.push(profiles[u].clone());
                        valid_indices.push(u);
                    }
                }
                (selected_agents, valid_indices, reasoning)
            }
            Err(e) => {
                tracing::warn!(
                    target: "teri::report",
                    "{}",
                    t_args("console.llmSelectAgentFailed", &[("error", &e.to_string())])
                );
                fallback(profiles)
            }
        }
    }

    /// Generate interview questions via the LLM.
    ///
    /// Port of `_generate_interview_questions(...)` (`zep_tools.py:1634-1681`).
    async fn generate_interview_questions(
        &self,
        interview_requirement: &str,
        simulation_requirement: &str,
        selected_agents: &[serde_json::Map<String, serde_json::Value>],
    ) -> Vec<String> {
        // Python `[a.get("profession", "未知") for a in selected_agents]`.
        let agent_roles: Vec<String> =
            selected_agents.iter().map(|a| py_get_str(a, "profession", "未知")).collect();

        let system_prompt = "你是一个专业的记者/采访者。根据采访需求，生成3-5个深度采访问题。\n\n问题要求：\n1. 开放性问题，鼓励详细回答\n2. 针对不同角色可能有不同答案\n3. 涵盖事实、观点、感受等多个维度\n4. 语言自然，像真实采访一样\n5. 每个问题控制在50字以内，简洁明了\n6. 直接提问，不要包含背景说明或前缀\n\n返回JSON格式：{\"questions\": [\"问题1\", \"问题2\", ...]}";
        let sim_bg =
            if simulation_requirement.is_empty() { "未提供" } else { simulation_requirement };
        let user_prompt = format!(
            "采访需求：{}\n\n模拟背景：{}\n\n采访对象角色：{}\n\n请生成3-5个采访问题。",
            interview_requirement,
            sim_bg,
            agent_roles.join(", ")
        );

        let messages = [ChatMessage::system(system_prompt), ChatMessage::user(user_prompt)];
        let opts = ChatOptions { temperature: Some(0.5), max_tokens: None };

        match self.llm.chat_json::<serde_json::Value>(&messages, &opts).await {
            Ok(resp) => {
                // Python `response.get("questions", [<1-item default>])` — default
                // applies ONLY when the key is absent; present-but-empty returns [].
                match resp.get("questions").and_then(|v| v.as_array()) {
                    Some(arr) => {
                        arr.iter().filter_map(|q| q.as_str().map(|s| s.to_string())).collect()
                    }
                    None => vec![format!("关于{}，您有什么看法？", interview_requirement)],
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "teri::report",
                    "{}",
                    t_args(
                        "console.generateInterviewQuestionsFailed",
                        &[("error", &e.to_string())]
                    )
                );
                vec![
                    format!("关于{}，您的观点是什么？", interview_requirement),
                    "这件事对您或您所代表的群体有什么影响？".to_string(),
                    "您认为应该如何解决或改进这个问题？".to_string(),
                ]
            }
        }
    }

    /// Generate a summary across the interviews via the LLM.
    ///
    /// Port of `_generate_interview_summary(...)` (`zep_tools.py:1683-1763`).
    async fn generate_interview_summary(
        &self,
        interviews: &[AgentInterview],
        interview_requirement: &str,
    ) -> String {
        if interviews.is_empty() {
            return "未完成任何采访".to_string();
        }

        let interview_texts: Vec<String> = interviews
            .iter()
            .map(|iv| {
                let resp: String = iv.response.chars().take(500).collect();
                format!("【{}（{}）】\n{}", iv.agent_name, iv.agent_role, resp)
            })
            .collect();

        let quote_instruction = if crate::i18n::get_locale() == "zh" {
            "引用受访者原话时使用中文引号「」"
        } else {
            "Use quotation marks \"\" when quoting interviewees"
        };

        let system_prompt = format!(
            "你是一个专业的新闻编辑。请根据多位受访者的回答，生成一份采访摘要。\n\n摘要要求：\n1. 提炼各方主要观点\n2. 指出观点的共识和分歧\n3. 突出有价值的引言\n4. 客观中立，不偏袒任何一方\n5. 控制在1000字内\n\n格式约束（必须遵守）：\n- 使用纯文本段落，用空行分隔不同部分\n- 不要使用Markdown标题（如#、##、###）\n- 不要使用分割线（如---、***）\n- {}\n- 可以使用**加粗**标记关键词，但不要使用其他Markdown语法",
            quote_instruction
        );
        // Python `"".join(interview_texts)` — NO separator between blocks.
        let user_prompt = format!(
            "采访主题：{}\n\n采访内容：\n{}\n\n请生成采访摘要。",
            interview_requirement,
            interview_texts.join("")
        );

        let messages = [ChatMessage::system(system_prompt), ChatMessage::user(user_prompt)];
        let opts = ChatOptions { temperature: Some(0.3), max_tokens: Some(800) };

        match self.llm.chat(&messages, &opts).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    target: "teri::report",
                    "{}",
                    t_args(
                        "console.generateInterviewSummaryFailed",
                        &[("error", &e.to_string())]
                    )
                );
                let agent_names: Vec<String> =
                    interviews.iter().map(|iv| iv.agent_name.clone()).collect();
                format!("共采访了{}位受访者，包括：{}", interviews.len(), agent_names.join("、"))
            }
        }
    }

    /// Interview agents — the full U-024 port (`zep_tools.py:1272-1482`).
    ///
    /// Loads profiles, LLM-selects agents, LLM-generates questions, dispatches a
    /// batch interview over the live `SimulationRunner` IPC seam, parses dual-
    /// platform responses (cleaning tool-call wrappers + extracting key quotes),
    /// and LLM-summarizes. `async` because it awaits IPC + 3 LLM calls.
    pub async fn interview_agents(
        &self,
        simulation_id: &str,
        interview_requirement: &str,
        simulation_requirement: &str,
        max_agents: i64,
        custom_questions: Option<Vec<String>>,
    ) -> Result<InterviewResult> {
        // Step 0: start log (requirement char-sliced [:50]).
        let req_head: String = interview_requirement.chars().take(50).collect();
        tracing::info!(
            target: "teri::report",
            "{}",
            t_args("console.interviewAgentsStart", &[("requirement", &req_head)])
        );

        let mut result = InterviewResult::new(
            interview_requirement.to_string(),
            custom_questions.unwrap_or_default(),
        );

        // The runner is the live IPC seam. Without it (graph-only facade), the
        // sync `execute_inner` arm already returns the honest tolerated error;
        // the async path only runs when a runner is threaded.
        let runner = match self.runner {
            Some(r) => r,
            None => {
                return Err(TeriError::Unknown(
                    "interview_agents requires a live SimulationRunner (no runner threaded)".into(),
                ));
            }
        };

        // Step 1: load profiles.
        let profiles = Self::load_agent_profiles(runner.sim_data_dir(), simulation_id);

        // Empty-profiles guard → early return.
        if profiles.is_empty() {
            tracing::info!(
                target: "teri::report",
                "{}",
                t_args("console.profilesNotFound", &[("simId", &simulation_id)])
            );
            result.summary = "未找到可采访的Agent人设文件".to_string();
            return Ok(result);
        }

        result.total_agents = profiles.len() as i64;
        tracing::info!(
            target: "teri::report",
            "{}",
            t_args("console.loadedProfiles", &[("count", &profiles.len())])
        );

        // Step 2: select agents.
        let (selected_agents, selected_indices, reasoning) = self
            .select_agents_for_interview(
                &profiles,
                interview_requirement,
                simulation_requirement,
                max_agents,
            )
            .await;
        result.selected_agents = selected_agents.clone();
        result.selection_reasoning = reasoning;
        let indices_repr = format!(
            "[{}]",
            selected_indices.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
        );
        tracing::info!(
            target: "teri::report",
            "{}",
            t_args(
                "console.selectedAgentsForInterview",
                &[("count", &selected_agents.len()), ("indices", &indices_repr)]
            )
        );

        // Step 3: questions (generate only if none supplied; log inside the branch
        // to match Python `zep_tools.py:1340-1346`).
        if result.interview_questions.is_empty() {
            result.interview_questions = self
                .generate_interview_questions(
                    interview_requirement,
                    simulation_requirement,
                    &selected_agents,
                )
                .await;
            tracing::info!(
                target: "teri::report",
                "{}",
                t_args(
                    "console.generatedInterviewQuestions",
                    &[("count", &result.interview_questions.len())]
                )
            );
        }

        // Build combined prompt.
        let combined_prompt = result
            .interview_questions
            .iter()
            .enumerate()
            .map(|(i, q)| format!("{}. {}", i + 1, q))
            .collect::<Vec<_>>()
            .join("\n");
        let optimized_prompt = format!("{}{}", INTERVIEW_AGENTS_PROMPT_PREFIX, combined_prompt);

        // Step 4: batch interview.
        let interviews_request: Vec<serde_json::Value> = selected_indices
            .iter()
            .map(|agent_idx| {
                let mut m = serde_json::Map::new();
                m.insert("agent_id".into(), serde_json::Value::from(*agent_idx));
                m.insert("prompt".into(), optimized_prompt.clone().into());
                serde_json::Value::Object(m)
            })
            .collect();

        tracing::info!(
            target: "teri::report",
            "{}",
            t_args(
                "console.callingBatchInterviewApi",
                &[("count", &interviews_request.len())]
            )
        );

        // 180s timeout (NOT the 120s method default).
        let api_result = match runner
            .interview_agents_batch(
                simulation_id,
                interviews_request,
                None,
                Duration::from_secs_f64(180.0),
            )
            .await
        {
            Ok(resp) => resp,
            Err(TeriError::Sim(msg)) => {
                // env-not-running / not-found → ValueError branch.
                tracing::warn!(
                    target: "teri::report",
                    "{}",
                    t_args("console.interviewApiCallFailed", &[("error", &msg)])
                );
                result.summary =
                    format!("采访失败：{}。模拟环境可能已关闭，请确保OASIS环境正在运行。", msg);
                return Ok(result);
            }
            Err(e) => {
                // Any other error → generic Exception branch.
                tracing::error!(
                    target: "teri::report",
                    "{}",
                    t_args("console.interviewApiCallException", &[("error", &e.to_string())])
                );
                result.summary = format!("采访过程发生错误：{}", e);
                return Ok(result);
            }
        };

        // Success check: status == Completed AND error.is_none().
        let success = api_result.status
            == crate::services::simulation_ipc::CommandStatus::Completed
            && api_result.error.is_none();
        let api_data = api_result.result.clone().unwrap_or_default();
        let results_dict =
            api_data.get("results").and_then(|v| v.as_object()).cloned().unwrap_or_default();
        tracing::info!(
            target: "teri::report",
            "{}",
            t_args(
                "console.interviewApiReturned",
                &[("count", &results_dict.len()), ("success", &success)]
            )
        );

        // API failure guard → early return.
        if !success {
            let error_msg = api_result.error.clone().unwrap_or_else(|| "未知错误".to_string());
            tracing::warn!(
                target: "teri::report",
                "{}",
                t_args("console.interviewApiReturnedFailure", &[("error", &error_msg)])
            );
            result.summary = format!("采访API调用失败：{}。请检查OASIS模拟环境状态。", error_msg);
            return Ok(result);
        }

        // Step 5: parse results.
        for (i, agent_idx) in selected_indices.iter().enumerate() {
            let agent = &selected_agents[i];
            // Python `agent.get("realname", agent.get("username", f"Agent_{agent_idx}"))`
            // — `.get` defaults apply ONLY when the key is ABSENT (empty string wins).
            let agent_name = resolve_agent_name(agent, *agent_idx);
            let agent_role = py_get_str(agent, "profession", "未知");
            let agent_bio = py_get_str(agent, "bio", "");

            let twitter_result = results_dict
                .get(&format!("twitter_{}", agent_idx))
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let reddit_result = results_dict
                .get(&format!("reddit_{}", agent_idx))
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();

            let twitter_response_raw = twitter_result
                .get("response")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let reddit_response_raw =
                reddit_result.get("response").and_then(|v| v.as_str()).unwrap_or("").to_string();

            let twitter_response = Self::clean_tool_call_response(twitter_response_raw);
            let reddit_response = Self::clean_tool_call_response(reddit_response_raw);

            let twitter_text = if twitter_response.is_empty() {
                "（该平台未获得回复）".to_string()
            } else {
                twitter_response.clone()
            };
            let reddit_text = if reddit_response.is_empty() {
                "（该平台未获得回复）".to_string()
            } else {
                reddit_response.clone()
            };
            let response_text = format!(
                "【Twitter平台回答】\n{}\n\n【Reddit平台回答】\n{}",
                twitter_text, reddit_text
            );

            // Key-quote extraction.
            let combined_responses = format!("{} {}", twitter_response, reddit_response);
            let key_quotes = extract_key_quotes(&combined_responses);

            let agent_bio_truncated: String = agent_bio.chars().take(1000).collect();
            let key_quotes_capped: Vec<String> = key_quotes.into_iter().take(5).collect();

            result.interviews.push(AgentInterview {
                agent_name,
                agent_role,
                agent_bio: agent_bio_truncated,
                question: combined_prompt.clone(),
                response: response_text,
                key_quotes: key_quotes_capped,
            });
        }
        result.interviewed_count = result.interviews.len() as i64;

        // Step 6: summary.
        if !result.interviews.is_empty() {
            result.summary =
                self.generate_interview_summary(&result.interviews, interview_requirement).await;
        }

        tracing::info!(
            target: "teri::report",
            "{}",
            t_args(
                "console.interviewAgentsComplete",
                &[("count", &result.interviewed_count)]
            )
        );

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// interview_agents free helpers + verbatim literals (U-024)
// ---------------------------------------------------------------------------

/// Interview prompt prefix prepended to the combined questions
/// (`zep_tools.py:1352-1362`). The 6-rule multi-line CJK block is OBSERVABLE in
/// the prompt sent to the agent — preserve it byte-for-byte. NOTE: this is a
/// DIFFERENT literal from the single-line `optimize_interview_prompt` prefix in
/// `api::simulation` (which ports `simulation.py:23`).
const INTERVIEW_AGENTS_PROMPT_PREFIX: &str = "你正在接受一次采访。请结合你的人设、所有的过往记忆与行动，以纯文本方式直接回答以下问题。\n回复要求：\n1. 直接用自然语言回答，不要调用任何工具\n2. 不要返回JSON格式或工具调用格式\n3. 不要使用Markdown标题（如#、##、###）\n4. 按问题编号逐一回答，每个回答以「问题X：」开头（X为问题编号）\n5. 每个问题的回答之间用空行分隔\n6. 回答要有实质内容，每个问题至少回答2-3句话\n\n";

/// Python `dict.get(key, default)` for a string value: returns the stored string
/// when the key is PRESENT (even if empty), else `default`. (Distinct from
/// `.unwrap_or("")` which would also fire when the value is a non-string.)
fn py_get_str(m: &serde_json::Map<String, serde_json::Value>, key: &str, default: &str) -> String {
    match m.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => default.to_string(),
    }
}

/// Python `profile.get("realname", profile.get("username", f"Agent_{idx}"))`:
/// realname if its key is present, else username if present, else `Agent_{idx}`.
fn resolve_agent_name(m: &serde_json::Map<String, serde_json::Value>, idx: usize) -> String {
    if let Some(v) = m.get("realname") {
        return match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
    }
    if let Some(v) = m.get("username") {
        return match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
    }
    format!("Agent_{}", idx)
}

/// Extract key quotes from a combined dual-platform response.
///
/// Port of the inline key-quote extraction in `interview_agents`
/// (`zep_tools.py:1421-1448`). The regexes + ordering are CONTRACTUAL; `len`
/// throughout is a CHARACTER count.
fn extract_key_quotes(combined_responses: &str) -> Vec<String> {
    // Sequential cleaning (exact order from Python).
    let re_heading = Regex::new(r"#{1,6}\s+").unwrap();
    let re_toolname = Regex::new(r"\{[^}]*tool_name[^}]*\}").unwrap();
    let re_markdown = Regex::new(r"[*_`|>~\-]{2,}").unwrap();
    let re_question = Regex::new(r"问题\d+[：:]\s*").unwrap();
    let re_bracket = Regex::new(r"【[^】]+】").unwrap();

    let mut clean_text = re_heading.replace_all(combined_responses, "").into_owned();
    clean_text = re_toolname.replace_all(&clean_text, "").into_owned();
    clean_text = re_markdown.replace_all(&clean_text, "").into_owned();
    clean_text = re_question.replace_all(&clean_text, "").into_owned();
    clean_text = re_bracket.replace_all(&clean_text, "").into_owned();

    // Strategy 1: full meaningful sentences.
    // Python `re.split(r'[。！？]', clean_text)`.
    let re_split = Regex::new(r"[。！？]").unwrap();
    let re_leading = Regex::new(r"^[\s\W，,；;：:、]+").unwrap();

    let mut meaningful: Vec<String> = re_split
        .split(&clean_text)
        .map(|s| s.trim().to_string())
        .filter(|s| {
            let len = s.chars().count();
            (20..=150).contains(&len)
                && !re_leading.is_match(s)
                && !s.starts_with('{')
                && !s.starts_with("问题")
        })
        .collect();

    // `meaningful.sort(key=len, reverse=True)` — Python `len` is char count; ties
    // keep input (insertion) order under a stable sort.
    meaningful.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
    let key_quotes: Vec<String> =
        meaningful.into_iter().take(3).map(|s| format!("{}。", s)).collect();

    if !key_quotes.is_empty() {
        return key_quotes;
    }

    // Strategy 2: correctly-paired CJK/curly-quote long text.
    let re_curly = Regex::new("\u{201c}([^\u{201c}\u{201d}]{15,100})\u{201d}").unwrap();
    let re_corner = Regex::new("\u{300c}([^\u{300c}\u{300d}]{15,100})\u{300d}").unwrap();
    let re_q_leading = Regex::new(r"^[，,；;：:、]").unwrap();

    let mut paired: Vec<String> = Vec::new();
    for caps in re_curly.captures_iter(&clean_text) {
        paired.push(caps[1].to_string());
    }
    for caps in re_corner.captures_iter(&clean_text) {
        paired.push(caps[1].to_string());
    }
    paired.into_iter().filter(|q| !re_q_leading.is_match(q)).take(3).collect()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Convert an `Entity` reference to a `NodeInfo` DTO.
///
/// Fields summary="" and attributes={} are [≠] DECISION-9 Q2 (no per-entity
/// summary or attribute bag in teri; consumers have graceful fallbacks).
fn entity_to_node_info(entity: &Entity) -> NodeInfo {
    NodeInfo {
        uuid: entity.id.to_string(),
        name: entity.name.clone(),
        labels: vec![entity.kind.to_string()],
        summary: String::new(),             // [≠] DECISION-9 Q2
        attributes: serde_json::Map::new(), // [≠] DECISION-9 Q2
    }
}

/// Convert an `EdgeTriple` to an `EdgeInfo` DTO.
///
/// Fields uuid="" and fact="" are [≠] DECISION-9 Q4 (no UUID or LLM-generated
/// fact string on `Relation`; consumers use edge_name+direction fallback).
fn edge_triple_to_edge_info(
    from_id: uuid::Uuid,
    to_id: uuid::Uuid,
    relation: &crate::graph::Relation,
    include_temporal: bool,
) -> EdgeInfo {
    let (valid_at_str, invalid_at_str, expired_at_str) = if include_temporal {
        match relation.valid_at {
            None => (None, None, None),
            Some((start, None)) => (Some(start.to_string()), None, None),
            Some((start, Some(end))) => {
                // Window has closed → this is a historical/expired edge.
                (Some(start.to_string()), Some(end.to_string()), Some(end.to_string()))
            }
        }
    } else {
        (None, None, None)
    };

    EdgeInfo {
        uuid: String::new(), // [≠] DECISION-9 Q4
        name: relation.kind.to_string(),
        fact: String::new(), // [≠] DECISION-9 Q4
        source_node_uuid: from_id.to_string(),
        target_node_uuid: to_id.to_string(),
        created_at: None, // [≠] DECISION-9 Q4: Relation has no created_at
        valid_at: valid_at_str,
        invalid_at: invalid_at_str,
        expired_at: expired_at_str,
    }
}

/// Synthesize a human-readable fact string from an edge triple.
///
/// In MiroFish, `edge.fact` is an LLM-generated natural language sentence stored
/// by Zep ([≠] DECISION-9 Q4). In teri we have no such sentence, so we synthesize
/// one from the entity names and relation kind. This preserves the fact-classification
/// behavior of `panorama_search` (active_facts / historical_facts are non-empty).
///
/// The synthesized form is `"{source_name} {relation_kind} {target_name}"`.
fn synthesize_fact_from_triple(
    from_id: uuid::Uuid,
    to_id: uuid::Uuid,
    relation: &crate::graph::Relation,
    node_map: &std::collections::HashMap<String, &NodeInfo>,
) -> String {
    let from_str = from_id.to_string();
    let to_str = to_id.to_string();
    let src_name = node_map.get(&from_str).map(|n| n.name.as_str()).unwrap_or("");
    let tgt_name = node_map.get(&to_str).map(|n| n.name.as_str()).unwrap_or("");
    if src_name.is_empty() || tgt_name.is_empty() {
        return String::new();
    }
    format!("{} {} {}", src_name, relation.kind, tgt_name)
}

// ---------------------------------------------------------------------------
// Sub-cycle (c): ReACT tool dispatch + parser
//
// Sources: `report_agent.py` —
//   `_define_tools`, `_execute_tool`, `_parse_tool_calls`, `_is_valid_tool_call`,
//   `_get_tools_description`, `VALID_TOOL_NAMES`, `TOOL_DESC_*` constants.
// ---------------------------------------------------------------------------

// ── Tool description constants (verbatim from report_agent.py:476–548) ──────

/// Tool description: insight_forge (report_agent.py:476–492).
pub const TOOL_DESC_INSIGHT_FORGE: &str = "\
【深度洞察检索 - 强大的检索工具】
这是我们强大的检索函数，专为深度分析设计。它会：
1. 自动将你的问题分解为多个子问题
2. 从多个维度检索模拟图谱中的信息
3. 整合语义搜索、实体分析、关系链追踪的结果
4. 返回最全面、最深度的检索内容

【使用场景】
- 需要深入分析某个话题
- 需要了解事件的多个方面
- 需要获取支撑报告章节的丰富素材

【返回内容】
- 相关事实原文（可直接引用）
- 核心实体洞察
- 关系链分析";

/// Tool description: panorama_search (report_agent.py:494–509).
pub const TOOL_DESC_PANORAMA_SEARCH: &str = "\
【广度搜索 - 获取全貌视图】
这个工具用于获取模拟结果的完整全貌，特别适合了解事件演变过程。它会：
1. 获取所有相关节点和关系
2. 区分当前有效的事实和历史/过期的事实
3. 帮助你了解舆情是如何演变的

【使用场景】
- 需要了解事件的完整发展脉络
- 需要对比不同阶段的舆情变化
- 需要获取全面的实体和关系信息

【返回内容】
- 当前有效事实（模拟最新结果）
- 历史/过期事实（演变记录）
- 所有涉及的实体";

/// Tool description: quick_search (report_agent.py:511–521).
pub const TOOL_DESC_QUICK_SEARCH: &str = "\
【简单搜索 - 快速检索】
轻量级的快速检索工具，适合简单、直接的信息查询。

【使用场景】
- 需要快速查找某个具体信息
- 需要验证某个事实
- 简单的信息检索

【返回内容】
- 与查询最相关的事实列表";

/// Tool description: interview_agents (report_agent.py:523–548).
pub const TOOL_DESC_INTERVIEW_AGENTS: &str = "\
【深度采访 - 真实Agent采访（双平台）】
调用OASIS模拟环境的采访API，对正在运行的模拟Agent进行真实采访！
这不是LLM模拟，而是调用真实的采访接口获取模拟Agent的原始回答。
默认在Twitter和Reddit两个平台同时采访，获取更全面的观点。

功能流程：
1. 自动读取人设文件，了解所有模拟Agent
2. 智能选择与采访主题最相关的Agent（如学生、媒体、官方等）
3. 自动生成采访问题
4. 调用 /api/simulation/interview/batch 接口在双平台进行真实采访
5. 整合所有采访结果，提供多视角分析

【使用场景】
- 需要从不同角色视角了解事件看法（学生怎么看？媒体怎么看？官方怎么说？）
- 需要收集多方意见和立场
- 需要获取模拟Agent的真实回答（来自OASIS模拟环境）
- 想让报告更生动，包含\"采访实录\"

【返回内容】
- 被采访Agent的身份信息
- 各Agent在Twitter和Reddit两个平台的采访回答
- 关键引言（可直接引用）
- 采访摘要和观点对比

【重要】需要OASIS模拟环境正在运行才能使用此功能！";

// ── Tool enum (report_agent.py:919–954 `_define_tools` + _execute_tool dispatch) ─

/// Closed set of tools the ReACT loop can dispatch.
///
/// Back-compat redirect names are additional arms so an LLM emitting an old
/// tool name still dispatches (observable behavior — PORTED per architect §3a).
///
/// Port of the `if/elif` dispatch in `ReportAgent._execute_tool`
/// (`report_agent.py:956–1062`) and the implicit names in `_define_tools`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReportTool {
    /// Deep multi-query analysis.  Back-compat: "get_simulation_context" → this.
    InsightForge,
    /// Temporal-aware full-graph scan.
    PanoramaSearch,
    /// Simple keyword search.  Back-compat: "search_graph" → this.
    QuickSearch,
    /// Interview simulation agents.
    InterviewAgents,
    /// Back-compat redirect → `QuickSearch` (`report_agent.py:1025–1028`).
    SearchGraph,
    /// Back-compat legacy tool → calls `get_graph_statistics` directly
    /// (`report_agent.py:1030–1032`).
    GetGraphStatistics,
    /// Back-compat legacy tool → calls `get_entity_summary` directly
    /// (`report_agent.py:1034–1040`).
    GetEntitySummary,
    /// Back-compat redirect → `InsightForge` (`report_agent.py:1042–1046`).
    GetSimulationContext,
    /// Back-compat legacy tool → calls `get_entities_by_type` directly
    /// (`report_agent.py:1048–1055`).
    GetEntitiesByType,
}

impl ReportTool {
    /// Parse a tool name string to a `ReportTool` variant.
    ///
    /// Returns `None` for unknown names (caller emits the unknown-tool string).
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "insight_forge" => Some(Self::InsightForge),
            "panorama_search" => Some(Self::PanoramaSearch),
            "quick_search" => Some(Self::QuickSearch),
            "interview_agents" => Some(Self::InterviewAgents),
            // back-compat redirects
            "search_graph" => Some(Self::SearchGraph),
            "get_graph_statistics" => Some(Self::GetGraphStatistics),
            "get_entity_summary" => Some(Self::GetEntitySummary),
            "get_simulation_context" => Some(Self::GetSimulationContext),
            "get_entities_by_type" => Some(Self::GetEntitiesByType),
            _ => None,
        }
    }

    /// The canonical tool name string for this variant (used in log messages).
    pub fn name(&self) -> &'static str {
        match self {
            Self::InsightForge => "insight_forge",
            Self::PanoramaSearch => "panorama_search",
            Self::QuickSearch => "quick_search",
            Self::InterviewAgents => "interview_agents",
            Self::SearchGraph => "search_graph",
            Self::GetGraphStatistics => "get_graph_statistics",
            Self::GetEntitySummary => "get_entity_summary",
            Self::GetSimulationContext => "get_simulation_context",
            Self::GetEntitiesByType => "get_entities_by_type",
        }
    }
}

// ── ToolCall parsed struct ───────────────────────────────────────────────────

/// A parsed tool call from the LLM response.
///
/// Mirrors the dict shape `{"name": "...", "parameters": {...}}` produced by
/// `_parse_tool_calls` after key-normalization (`_is_valid_tool_call`).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    /// Tool name (after normalization: "tool" key is renamed to "name").
    pub name: String,
    /// Tool parameters (after normalization: "params" key is renamed to "parameters").
    pub parameters: serde_json::Map<String, serde_json::Value>,
}

// ── VALID_TOOL_NAMES (report_agent.py:1065) ─────────────────────────────────

/// Canonical tool names that gate tier-2 and tier-3 bare-JSON parse.
///
/// Port of `VALID_TOOL_NAMES = {"insight_forge", ...}` (`report_agent.py:1065`).
pub const VALID_TOOL_NAMES: [&str; 4] =
    ["insight_forge", "panorama_search", "quick_search", "interview_agents"];

// ── parse_tool_calls (report_agent.py:1067–1112) ────────────────────────────

/// Parse tool calls from an LLM response string.
///
/// 3-tier priority (verbatim from `_parse_tool_calls` / `_is_valid_tool_call`):
/// 1. `<tool_call>…</tool_call>` XML tags (DOTALL, multiple allowed).
/// 2. Bare whole-response JSON if it starts with `{` and ends with `}`.
/// 3. Trailing `{"name"|"tool": …}` regex at end of response.
///
/// Tiers 2–3 are gated by `VALID_TOOL_NAMES`.
/// `{"tool"/"params"}` keys are normalised to `{"name"/"parameters"}` in-place
/// (mirrors `_is_valid_tool_call` mutation, `report_agent.py:1120–1123`).
pub fn parse_tool_calls(response: &str) -> Vec<ToolCall> {
    // Tier 1: <tool_call>…</tool_call> (report_agent.py:1077–1087)
    // Python: r'<tool_call>\s*(\{.*?\})\s*</tool_call>'  re.DOTALL
    let xml_re = Regex::new(r"(?s)<tool_call>\s*(\{.*?\})\s*</tool_call>").unwrap();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for cap in xml_re.captures_iter(response) {
        let json_str = &cap[1];
        // Tier-1 XML format: Python appends `json.loads(...)` RAW — no VALID_TOOL_NAMES
        // gate AND no key-normalization (normalization lives only in `_is_valid_tool_call`,
        // which tier-1 never calls; report_agent.py:1079–1082). So we read the RAW "name"
        // and RAW "parameters" keys here: a `{"name":..,"params":..}` tier-1 call yields
        // EMPTY parameters, matching Python's downstream `call.get("parameters", {})` == {}.
        // [≠] Python pushes name-less/aliased objects raw too, then KeyError-crashes at the
        // downstream `call["name"]` access (report_agent.py:1419/1852) — a Python defect we
        // do not preserve: a tier-1 object lacking a "name" key is skipped gracefully and the
        // parser falls through to tiers 2/3 (which DO normalize + gate). Observable only on
        // malformed LLM output; the well-formed `{"name":..}` path is byte-faithful.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str)
            && let Some(obj) = val.as_object()
            && let Some(name) = obj.get("name").and_then(|v| v.as_str())
        {
            let name = name.to_string();
            let parameters =
                obj.get("parameters").and_then(|v| v.as_object()).cloned().unwrap_or_default();
            tool_calls.push(ToolCall { name, parameters });
        }
    }

    if !tool_calls.is_empty() {
        return tool_calls;
    }

    // Tier 2: bare whole-response JSON (report_agent.py:1089–1099)
    let stripped = response.trim();
    if stripped.starts_with('{')
        && stripped.ends_with('}')
        && let Ok(mut val) = serde_json::from_str::<serde_json::Value>(stripped)
        && let Some(obj) = val.as_object_mut()
        && is_valid_tool_call(obj)
    {
        let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parameters =
            obj.get("parameters").and_then(|v| v.as_object()).cloned().unwrap_or_default();
        tool_calls.push(ToolCall { name, parameters });
        return tool_calls;
    }

    // Tier 3: trailing {"name"|"tool": …} at end of response (report_agent.py:1101–1110)
    // Python: r'(\{"(?:name|tool)"\s*:.*?\})\s*$'  re.DOTALL
    let trailing_re = Regex::new(r#"(?s)(\{"(?:name|tool)"\s*:.*?\})\s*$"#).unwrap();
    if let Some(cap) = trailing_re.captures(stripped)
        && let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&cap[1])
        && let Some(obj) = val.as_object_mut()
        && is_valid_tool_call(obj)
    {
        let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parameters =
            obj.get("parameters").and_then(|v| v.as_object()).cloned().unwrap_or_default();
        tool_calls.push(ToolCall { name, parameters });
    }

    tool_calls
}

/// Validate a parsed JSON object as a tool call AND normalise its keys in-place.
///
/// Port of `_is_valid_tool_call` (`report_agent.py:1114–1125`).
/// Accepts `{"name": …}` or `{"tool": …}` as the tool-name key,
/// and `{"parameters": …}` or `{"params": …}` as the params key.
/// Mutates the map: renames `"tool"→"name"` and `"params"→"parameters"`.
/// Gates on `VALID_TOOL_NAMES`.
fn is_valid_tool_call(obj: &mut serde_json::Map<String, serde_json::Value>) -> bool {
    // Extract tool name from either "name" or "tool" key.
    let tool_name = obj
        .get("name")
        .or_else(|| obj.get("tool"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if let Some(ref name) = tool_name
        && VALID_TOOL_NAMES.contains(&name.as_str())
    {
        // Normalise keys in-place (report_agent.py:1120–1123).
        normalize_tool_call_keys(obj);
        return true;
    }
    false
}

/// Normalise `"tool"→"name"` and `"params"→"parameters"` in a JSON object.
///
/// Port of the key-rename mutation in `_is_valid_tool_call`
/// (`report_agent.py:1120–1123`):
///   if "tool" in data: data["name"] = data.pop("tool")
///   if "params" in data and "parameters" not in data: data["parameters"] = data.pop("params")
fn normalize_tool_call_keys(obj: &mut serde_json::Map<String, serde_json::Value>) {
    if let Some(tool_val) = obj.remove("tool") {
        obj.insert("name".to_string(), tool_val);
    }
    if obj.contains_key("params")
        && !obj.contains_key("parameters")
        && let Some(params_val) = obj.remove("params")
    {
        obj.insert("parameters".to_string(), params_val);
    }
}

// ── get_tools_description (report_agent.py:1127–1135) ───────────────────────

/// Generate the tool descriptions text the model sees.
///
/// Port of `_get_tools_description` (`report_agent.py:1127–1135`).
///
/// Output format (verbatim):
/// ```text
/// 可用工具：
/// - insight_forge: <TOOL_DESC_INSIGHT_FORGE>
///   参数: query: ..., report_context: ...
/// - panorama_search: ...
/// ...
/// ```
pub fn get_tools_description() -> String {
    // Mirror the Python dict insertion order in `_define_tools` (report_agent.py:919–954).
    #[allow(clippy::type_complexity)]
    let tools: &[(&str, &str, &[(&str, &str)])] = &[
        (
            "insight_forge",
            TOOL_DESC_INSIGHT_FORGE,
            &[
                ("query", "你想深入分析的问题或话题"),
                ("report_context", "当前报告章节的上下文（可选，有助于生成更精准的子问题）"),
            ],
        ),
        (
            "panorama_search",
            TOOL_DESC_PANORAMA_SEARCH,
            &[
                ("query", "搜索查询，用于相关性排序"),
                ("include_expired", "是否包含过期/历史内容（默认True）"),
            ],
        ),
        (
            "quick_search",
            TOOL_DESC_QUICK_SEARCH,
            &[("query", "搜索查询字符串"), ("limit", "返回结果数量（可选，默认10）")],
        ),
        (
            "interview_agents",
            TOOL_DESC_INTERVIEW_AGENTS,
            &[
                ("interview_topic", "采访主题或需求描述（如：'了解学生对宿舍甲醛事件的看法'）"),
                ("max_agents", "最多采访的Agent数量（可选，默认5，最大10）"),
            ],
        ),
    ];

    let mut parts = vec!["可用工具：".to_string()];
    for (name, desc, params) in tools {
        parts.push(format!("- {}: {}", name, desc));
        if !params.is_empty() {
            let params_desc = params
                .iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("  参数: {}", params_desc));
        }
    }
    parts.join("\n")
}

// ── ReportTools::execute (report_agent.py:956–1062 `_execute_tool`) ─────────

impl<'g, L: LlmClient + Send + Sync + 'static> ReportTools<'g, L> {
    /// Dispatch a tool call by enum variant, parsing params and applying coercions.
    ///
    /// Port of `ReportAgent._execute_tool` (`report_agent.py:956–1062`).
    ///
    /// Always returns a `String` (the Observation text).  Errors are swallowed
    /// and returned as the `"工具执行失败: {e}"` text — matching Python's
    /// `try/except Exception as e: return f"工具执行失败: {str(e)}"` —
    /// so the ReACT loop keeps going after a tool failure.
    ///
    /// `simulation_id` and `simulation_requirement` are needed for `InterviewAgents`.
    /// `graph_id` is the `[≠]`-label string passed through to legacy tool outputs
    /// (`get_graph_statistics` includes it in its return dict).
    pub fn execute(
        &self,
        tool: ReportTool,
        params: &serde_json::Map<String, serde_json::Value>,
        graph_id: &str,
        simulation_id: &str,
        simulation_requirement: &str,
        report_context: &str,
    ) -> String {
        let tool_name = tool.name();
        // (g2): executingTool — report_agent.py:968 logger.info(...)
        // Params are formatted as debug repr (Python: params=parameters dict repr).
        let params_repr = serde_json::to_string(params).unwrap_or_else(|_| "{}".to_string());
        tracing::info!(
            target: "teri::report",
            "{}",
            t_args(
                "report.executingTool",
                &[("toolName", &tool_name), ("params", &params_repr)]
            )
        );
        match self.execute_inner(
            tool,
            params,
            graph_id,
            simulation_id,
            simulation_requirement,
            report_context,
        ) {
            Ok(s) => s,
            Err(e) => {
                let err_str = e.to_string();
                // (g2): toolExecFailed — report_agent.py:1061 logger.error(...)
                tracing::error!(
                    target: "teri::report",
                    "{}",
                    t_args(
                        "report.toolExecFailed",
                        &[("toolName", &tool_name), ("error", &err_str)]
                    )
                );
                format!("工具执行失败: {}", e)
            }
        }
    }

    // `simulation_id` is now only threaded through the recursive redirect arms
    // (SearchGraph/GetSimulationContext) after U-024 moved the live interview path
    // to `execute_by_name_async`; the sync `InterviewAgents` arm no longer consumes
    // it. The param stays in the signature for contract parity with `execute`.
    #[allow(clippy::only_used_in_recursion)]
    fn execute_inner(
        &self,
        tool: ReportTool,
        params: &serde_json::Map<String, serde_json::Value>,
        graph_id: &str,
        simulation_id: &str,
        simulation_requirement: &str,
        report_context: &str,
    ) -> Result<String> {
        match tool {
            // ── insight_forge (report_agent.py:971–980) ──────────────────────
            ReportTool::InsightForge => {
                let query = str_param(params, "query");
                let ctx = if !str_param(params, "report_context").is_empty() {
                    str_param(params, "report_context")
                } else {
                    report_context.to_string()
                };
                let result = self.insight_forge(graph_id, &query, simulation_requirement, &ctx, 5);
                Ok(result.to_text())
            }

            // ── panorama_search (report_agent.py:982–993) ────────────────────
            ReportTool::PanoramaSearch => {
                let query = str_param(params, "query");
                // include_expired: str→bool coercion (report_agent.py:985–987)
                // Python: include_expired = True (default); str check: in ['true','1','yes']
                let include_expired = coerce_include_expired(params, true);
                let result = self.panorama_search(graph_id, &query, include_expired, 50, None);
                Ok(result.to_text())
            }

            // ── quick_search (report_agent.py:995–1006) ──────────────────────
            ReportTool::QuickSearch => {
                let query = str_param(params, "query");
                // limit: str→int coercion (report_agent.py:998–1000)
                let limit = coerce_int_param(params, "limit", 10)?;
                let result = self.quick_search(graph_id, &query, limit);
                Ok(result.to_text())
            }

            // ── interview_agents (report_agent.py:1008–1021) ─────────────────
            //
            // U-024 sync arm (architecture §4 option ii): `interview_agents` is now an
            // `async fn` requiring a live runner; the async dispatcher
            // (`execute_by_name_async`) intercepts this tool name and runs the real
            // body. The SYNC path (graph-only facades: debug routes / tests with
            // `runner: None`) keeps returning the honest tolerated error so the ReACT
            // loop continues (Python `_execute_tool` try/except → "工具执行失败: ...").
            ReportTool::InterviewAgents => {
                // Preserve the param coercions for parity (a bad `max_agents` still
                // surfaces as the same failure text; topic falls back to `query`).
                let _interview_topic = {
                    let t = str_param(params, "interview_topic");
                    if t.is_empty() { str_param(params, "query") } else { t }
                };
                // max_agents: str→int, then min(n, 10) (report_agent.py:1011–1014).
                let max_agents = coerce_int_param(params, "max_agents", 5)?;
                let _max_agents = max_agents.min(10);
                Err(TeriError::Unknown(
                    "interview_agents requires the async dispatch path (execute_by_name_async) \
                     with a live SimulationRunner"
                        .into(),
                ))
            }

            // ── back-compat: search_graph → quick_search (report_agent.py:1025–1028) ──
            ReportTool::SearchGraph => {
                // (g2): redirectToQuickSearch — report_agent.py:1027 logger.info(...)
                tracing::info!(
                    target: "teri::report",
                    "{}",
                    t("report.redirectToQuickSearch")
                );
                self.execute_inner(
                    ReportTool::QuickSearch,
                    params,
                    graph_id,
                    simulation_id,
                    simulation_requirement,
                    report_context,
                )
            }

            // ── back-compat: get_graph_statistics (report_agent.py:1030–1032) ─
            ReportTool::GetGraphStatistics => {
                let result = self.get_graph_statistics(graph_id);
                Ok(serde_json::to_string_pretty(&result).unwrap_or_default())
            }

            // ── back-compat: get_entity_summary (report_agent.py:1034–1040) ──
            ReportTool::GetEntitySummary => {
                let entity_name = str_param(params, "entity_name");
                let result = self.get_entity_summary(graph_id, &entity_name);
                Ok(serde_json::to_string_pretty(&result).unwrap_or_default())
            }

            // ── back-compat: get_simulation_context → insight_forge
            //    (report_agent.py:1042–1046) ───────────────────────────────────
            ReportTool::GetSimulationContext => {
                // (g2): redirectToInsightForge — report_agent.py:1044 logger.info(...)
                tracing::info!(
                    target: "teri::report",
                    "{}",
                    t("report.redirectToInsightForge")
                );
                // Redirect: use "query" param or fall back to simulation_requirement.
                let query_raw = str_param(params, "query");
                let query = if query_raw.is_empty() {
                    simulation_requirement.to_string()
                } else {
                    query_raw
                };
                let mut redirected = params.clone();
                redirected.insert("query".to_string(), serde_json::Value::String(query));
                self.execute_inner(
                    ReportTool::InsightForge,
                    &redirected,
                    graph_id,
                    simulation_id,
                    simulation_requirement,
                    report_context,
                )
            }

            // ── back-compat: get_entities_by_type (report_agent.py:1048–1055) ─
            ReportTool::GetEntitiesByType => {
                let entity_type = str_param(params, "entity_type");
                let nodes = self.get_entities_by_type(graph_id, &entity_type);
                let dicts: Vec<serde_json::Value> = nodes
                    .iter()
                    .map(|n| serde_json::to_value(n.to_dict()).unwrap_or_default())
                    .collect();
                Ok(serde_json::to_string_pretty(&dicts).unwrap_or_default())
            }
        }
    }
}

// ── execute() entry-point: accepts a raw tool name string ───────────────────

impl<'g, L: LlmClient + Send + Sync + 'static> ReportTools<'g, L> {
    /// Execute a tool by raw name string (as the LLM emits it).
    ///
    /// Unknown tool names return `"未知工具: {name}。请使用以下工具之一: ..."`.
    /// This is the variant called by the ReACT loop.
    pub fn execute_by_name(
        &self,
        tool_name: &str,
        params: &serde_json::Map<String, serde_json::Value>,
        graph_id: &str,
        simulation_id: &str,
        simulation_requirement: &str,
        report_context: &str,
    ) -> String {
        match ReportTool::from_name(tool_name) {
            Some(tool) => self.execute(
                tool,
                params,
                graph_id,
                simulation_id,
                simulation_requirement,
                report_context,
            ),
            // Unknown tool string (report_agent.py:1057–1058) — byte-identical.
            None => format!(
                "未知工具: {}。请使用以下工具之一: insight_forge, panorama_search, quick_search",
                tool_name
            ),
        }
    }

    /// Async tool dispatch — the variant the ReACT loop calls when a live runner
    /// may be available (U-024, architecture §4).
    ///
    /// Intercepts ONLY `interview_agents` (the single tool that needs to `.await`
    /// the batch-interview IPC + 3 LLM calls) and delegates every other tool name
    /// to the existing sync [`execute_by_name`]. The interview branch mirrors
    /// `execute`'s logging + `"工具执行失败: {e}"` error-wrapping so the loop keeps
    /// going after a tool failure.
    pub async fn execute_by_name_async(
        &self,
        tool_name: &str,
        params: &serde_json::Map<String, serde_json::Value>,
        graph_id: &str,
        simulation_id: &str,
        simulation_requirement: &str,
        report_context: &str,
    ) -> String {
        if ReportTool::from_name(tool_name) == Some(ReportTool::InterviewAgents) {
            // (g2): executingTool — report_agent.py:968.
            let params_repr = serde_json::to_string(params).unwrap_or_else(|_| "{}".to_string());
            tracing::info!(
                target: "teri::report",
                "{}",
                t_args(
                    "report.executingTool",
                    &[("toolName", &tool_name), ("params", &params_repr)]
                )
            );

            // interview_topic falls back to "query" param (report_agent.py:1010).
            let interview_topic = {
                let t = str_param(params, "interview_topic");
                if t.is_empty() { str_param(params, "query") } else { t }
            };
            // max_agents: str→int, then min(n, 10) (report_agent.py:1011–1014).
            let max_agents = match coerce_int_param(params, "max_agents", 5) {
                Ok(n) => n.min(10),
                Err(e) => return format!("工具执行失败: {}", e),
            };

            return match self
                .interview_agents(
                    simulation_id,
                    &interview_topic,
                    simulation_requirement,
                    max_agents,
                    None,
                )
                .await
            {
                Ok(result) => result.to_text(),
                Err(e) => {
                    // (g2): toolExecFailed — report_agent.py:1061.
                    tracing::error!(
                        target: "teri::report",
                        "{}",
                        t_args(
                            "report.toolExecFailed",
                            &[("toolName", &tool_name), ("error", &e.to_string())]
                        )
                    );
                    format!("工具执行失败: {}", e)
                }
            };
        }

        // Every other tool name → existing sync dispatch (no .await needed).
        self.execute_by_name(
            tool_name,
            params,
            graph_id,
            simulation_id,
            simulation_requirement,
            report_context,
        )
    }
}

// ── Param coercion helpers ───────────────────────────────────────────────────

/// Extract a string parameter, returning "" on missing / non-string.
fn str_param(params: &serde_json::Map<String, serde_json::Value>, key: &str) -> String {
    params.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

/// `include_expired` str→bool coercion (report_agent.py:985–987).
///
/// Python: `if isinstance(include_expired, str): include_expired = include_expired.lower() in ['true', '1', 'yes']`
/// Default is `true` (Python `include_expired = parameters.get("include_expired", True)`).
fn coerce_include_expired(
    params: &serde_json::Map<String, serde_json::Value>,
    default: bool,
) -> bool {
    match params.get("include_expired") {
        None => default,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => {
            matches!(s.to_lowercase().as_str(), "true" | "1" | "yes")
        }
        // Any other JSON type: treat as truthy if non-zero number, else default.
        Some(serde_json::Value::Number(n)) => n.as_i64().map(|i| i != 0).unwrap_or(default),
        Some(_) => default,
    }
}

/// Integer parameter with str→int coercion (report_agent.py:998–1000, 1011–1013).
///
/// Python: `limit = parameters.get("limit", 10); if isinstance(limit, str): limit = int(limit)`.
/// The `default` applies ONLY when the key is MISSING (`.get(..., default)`). When the key is
/// present but a non-numeric string, Python `int(...)` raises `ValueError`, which propagates
/// through `_execute_tool`'s try/except (report_agent.py:1060–1062) to the `"工具执行失败: {e}"`
/// wrapper. We mirror that: an unparseable string returns `Err`, so `execute_inner` surfaces the
/// failure text instead of silently running the tool with a wrong/default limit.
/// [≠] The inner Python message (`invalid literal for int() with base 10: '…'`) is a
/// Python-runtime artifact; teri emits its own parse-error text under the identical
/// `"工具执行失败: "` prefix — the observable CONTRACT (tool fails, no results) is preserved.
fn coerce_int_param(
    params: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    default: i64,
) -> Result<i64> {
    match params.get(key) {
        None => Ok(default),
        Some(serde_json::Value::Number(n)) => Ok(n.as_i64().unwrap_or(default)),
        Some(serde_json::Value::String(s)) => s.parse::<i64>().map_err(|_| {
            TeriError::Unknown(format!("invalid integer for parameter '{}': '{}'", key, s))
        }),
        // A non-string, non-number JSON value: Python would pass it through unchanged and the
        // downstream slice/`min` would raise — mirror as a failure rather than a silent default.
        Some(other) => Err(TeriError::Unknown(format!(
            "invalid integer for parameter '{}': {}",
            key, other
        ))),
    }
}

// ---------------------------------------------------------------------------
// ZepToolsService — legacy DTO + retry namespace (U-017 baseline, KEPT VERBATIM)
// ---------------------------------------------------------------------------

/// High-level Zep retrieval service.
///
/// Keeps the DTO/`call_with_retry` namespace from U-017. The graph-touching
/// methods have migrated to `ReportTools<'g, L>` (DECISION-11). Remaining
/// methods on this struct are kept for backward-compatibility of the retry
/// infrastructure.
pub struct ZepToolsService<L: LlmClient + Send + Sync + 'static> {
    #[allow(dead_code)]
    api_key: Option<String>,
    llm_client: Option<L>,
}

impl<L: LlmClient + Send + Sync + 'static> ZepToolsService<L> {
    /// Maximum number of retries for API calls
    pub const MAX_RETRIES: i32 = 3;
    /// Delay between retries in seconds
    pub const RETRY_DELAY: f64 = 2.0;

    /// Create a new ZepToolsService instance.
    pub fn new(api_key: Option<String>, llm_client: Option<L>) -> Self {
        Self { api_key, llm_client }
    }

    /// Get the LLM client.
    pub fn llm(&self) -> Option<&L> {
        self.llm_client.as_ref()
    }

    /// Set the LLM client.
    pub fn set_llm(&mut self, llm: L) {
        self.llm_client = Some(llm);
    }

    /// Call a function with retry logic.
    pub async fn call_with_retry<F, T>(&mut self, mut func: F, _operation_name: &str) -> Result<T>
    where
        F: FnMut() -> Result<T>,
    {
        let mut last_error = None;

        for attempt in 0..Self::MAX_RETRIES {
            match func() {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if attempt < Self::MAX_RETRIES - 1 {
                        tokio::time::sleep(std::time::Duration::from_secs(
                            ((attempt as f64 * Self::RETRY_DELAY) as i64).max(2) as u64,
                        ))
                        .await;
                    }
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| TeriError::Unknown("Unknown error".into())))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{EntityKind, KnowledgeGraph, Relation, RelationKind};
    use uuid::Uuid;

    // ── Fixture helpers ────────────────────────────────────────────────────

    /// Build a fixture KnowledgeGraph with known entities and edges.
    ///
    /// Graph layout:
    ///   Alice (Person) --[WorksFor]--> Acme (Organization)
    ///   Bob   (Person) --[RelatedTo]--> Alice (Person)
    ///   Alice (Person) --[LocatedIn]--> Beijing (Location)   [expired: valid_at Some(100, Some(200))]
    fn fixture_graph() -> KnowledgeGraph {
        let mut g = KnowledgeGraph::new();

        let e_alice = crate::graph::Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            name: "Alice".to_string(),
            kind: EntityKind::Person,
        };
        let e_acme = crate::graph::Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
            name: "Acme".to_string(),
            kind: EntityKind::Organization,
        };
        let e_bob = crate::graph::Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(),
            name: "Bob".to_string(),
            kind: EntityKind::Person,
        };
        let e_beijing = crate::graph::Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000004").unwrap(),
            name: "Beijing".to_string(),
            kind: EntityKind::Location,
        };

        let idx_alice = g.add_entity(e_alice).unwrap();
        let idx_acme = g.add_entity(e_acme).unwrap();
        let idx_bob = g.add_entity(e_bob).unwrap();
        let idx_beijing = g.add_entity(e_beijing).unwrap();

        // Active edge: Alice --[WorksFor]--> Acme (no time constraint → always active)
        g.add_relation(idx_alice, idx_acme, Relation::new(RelationKind::WorksFor, 0.9).unwrap());

        // Active edge: Bob --[RelatedTo]--> Alice
        g.add_relation(idx_bob, idx_alice, Relation::new(RelationKind::RelatedTo, 0.7).unwrap());

        // Expired edge: Alice --[LocatedIn]--> Beijing [100, 200) — expired at t=300
        g.add_relation(
            idx_alice,
            idx_beijing,
            Relation::with_validity(RelationKind::LocatedIn, 0.5, Some((100, Some(200)))).unwrap(),
        );

        g
    }

    /// A minimal LLM stub that satisfies the LlmClient bound.
    struct StubLlm;

    #[async_trait::async_trait]
    impl crate::llm::LlmClient for StubLlm {
        async fn complete(&self, _prompt: &str) -> crate::error::Result<String> {
            Ok("stub".to_string())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(
            &self,
            p: &str,
        ) -> crate::error::Result<T> {
            let s = self.complete(p).await?;
            serde_json::from_str(&s).map_err(|e| crate::error::TeriError::Llm(e.to_string()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> crate::error::Result<
            std::pin::Pin<Box<dyn futures::Stream<Item = crate::error::Result<String>> + Send>>,
        > {
            Err(crate::error::TeriError::Llm("not used".into()))
        }
        async fn chat(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> crate::error::Result<String> {
            Ok("stub".to_string())
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[crate::llm::ChatMessage],
            _: &crate::llm::ChatOptions,
        ) -> crate::error::Result<T> {
            Err(crate::error::TeriError::Llm("not used".into()))
        }
    }

    // ── DTO tests (U-017 baseline — kept from original file) ────────────────

    #[test]
    fn test_search_result_to_dict() {
        let result = SearchResult {
            facts: vec!["fact1".to_string()],
            edges: vec![],
            nodes: vec![],
            query: "test".to_string(),
            total_count: 1,
        };
        let dict = result.to_dict();
        assert_eq!(dict.get("facts").and_then(|v| v.as_array()), Some(&vec!["fact1".into()]));
    }

    #[test]
    fn test_node_info_to_text() {
        let node = NodeInfo {
            uuid: "test-uuid".into(),
            name: "Test Node".into(),
            labels: vec!["Person".into()],
            summary: "".into(),
            attributes: serde_json::Map::new(),
        };
        let text = node.to_text();
        assert!(text.contains("实体: Test Node"));
    }

    #[test]
    fn test_edge_info_to_text() {
        let edge = EdgeInfo {
            source_node_uuid: "abc123456789".into(),
            target_node_uuid: "def987654321".into(),
            name: "FRIENDS".into(),
            fact: "They are friends".into(),
            uuid: "".into(),
            created_at: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        };
        let text = edge.to_text(false);
        assert!(text.contains("abc12345"));
        assert!(text.contains("def98765"));
    }

    #[test]
    fn test_edge_info_expired() {
        let edge = EdgeInfo {
            source_node_uuid: "a".into(),
            target_node_uuid: "b".into(),
            name: "TEST".into(),
            fact: "fact".into(),
            uuid: "".into(),
            created_at: None,
            valid_at: None,
            invalid_at: None,
            expired_at: Some("2024-01-01".to_string()),
        };
        assert!(edge.is_expired());
    }

    #[test]
    fn test_edge_info_is_invalid_uses_invalid_at() {
        // Python: is_invalid = self.invalid_at is not None (NOT source_node_uuid.is_empty())
        let edge = EdgeInfo {
            source_node_uuid: "".into(), // empty source
            target_node_uuid: "b".into(),
            name: "TEST".into(),
            fact: "fact".into(),
            uuid: "".into(),
            created_at: None,
            valid_at: None,
            invalid_at: None, // no invalid_at → NOT invalid per Python semantics
            expired_at: None,
        };
        // With the corrected Python-faithful is_invalid: invalid_at=None → false
        assert!(!edge.is_invalid());

        let edge2 = EdgeInfo {
            source_node_uuid: "a".into(),
            target_node_uuid: "b".into(),
            name: "TEST".into(),
            fact: "fact".into(),
            uuid: "".into(),
            created_at: None,
            valid_at: None,
            invalid_at: Some("2024-01-01".to_string()), // invalid_at set → true
            expired_at: None,
        };
        assert!(edge2.is_invalid());
    }

    // ── ReportTools::quick_search tests ─────────────────────────────────────

    #[test]
    fn test_quick_search_returns_search_result_type() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let result = tools.quick_search("graph1", "Alice", 10);
        // quick_search delegates to search_graph(scope="edges")
        // "Alice" matches entity names via synthesized edge names.
        // At minimum we get a SearchResult with the query.
        assert_eq!(result.query, "Alice");
    }

    #[test]
    fn test_quick_search_empty_graph() {
        let g = KnowledgeGraph::new();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let result = tools.quick_search("graph1", "anything", 10);
        assert_eq!(result.total_count, 0);
        assert!(result.facts.is_empty());
    }

    // ── ReportTools::panorama_search tests ──────────────────────────────────

    #[test]
    fn test_panorama_search_active_vs_historical() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);

        // At t=300: Alice--WorksFor-->Acme (None→active), Bob--RelatedTo-->Alice (None→active),
        //           Alice--LocatedIn-->Beijing [100,200) → is_active_at(300) = 300>=100 && 300<200 = false → historical
        let result = tools.panorama_search("graph1", "Alice", true, 50, Some(300));

        // 3 nodes total (Alice, Acme, Bob, Beijing = 4)
        assert_eq!(result.total_nodes, 4);
        assert_eq!(result.total_edges, 3);

        // 2 active edges (WorksFor + RelatedTo), 1 historical (LocatedIn expired)
        assert_eq!(result.active_count, 2, "Expected 2 active facts");
        assert_eq!(result.historical_count, 1, "Expected 1 historical fact");
    }

    #[test]
    fn test_panorama_search_exclude_expired() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);

        let result = tools.panorama_search("graph1", "Alice", false, 50, Some(300));
        // include_expired=false → historical_facts should be empty
        assert!(result.historical_facts.is_empty());
        // but historical_count still reflects the underlying count
        assert_eq!(result.historical_count, 1);
    }

    #[test]
    fn test_panorama_search_to_text_contains_headers() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let result = tools.panorama_search("graph1", "test", true, 50, Some(300));
        let text = result.to_text();
        assert!(text.contains("广度搜索结果（未来全景视图）"));
        assert!(text.contains("统计信息"));
    }

    #[test]
    fn test_panorama_result_to_dict_key_order() {
        let r = PanoramaResult {
            query: "q".to_string(),
            all_nodes: vec![],
            all_edges: vec![],
            active_facts: vec!["fact1".to_string()],
            historical_facts: vec![],
            total_nodes: 0,
            total_edges: 0,
            active_count: 1,
            historical_count: 0,
        };
        let d = r.to_dict();
        let keys: Vec<&str> = d.keys().map(|s| s.as_str()).collect();
        // Python field order: query, all_nodes, all_edges, active_facts, historical_facts,
        //                      total_nodes, total_edges, active_count, historical_count
        assert_eq!(
            keys,
            &[
                "query",
                "all_nodes",
                "all_edges",
                "active_facts",
                "historical_facts",
                "total_nodes",
                "total_edges",
                "active_count",
                "historical_count",
            ]
        );
    }

    // ── ReportTools::get_entities_by_type tests ──────────────────────────────

    #[test]
    fn test_get_entities_by_type_person() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let persons = tools.get_entities_by_type("graph1", "person");
        // Alice and Bob are Person
        assert_eq!(persons.len(), 2);
        let names: Vec<&str> = persons.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"Alice"));
        assert!(names.contains(&"Bob"));
    }

    #[test]
    fn test_get_entities_by_type_unknown() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let result = tools.get_entities_by_type("graph1", "Robot");
        assert!(result.is_empty());
    }

    // ── ReportTools::get_graph_statistics tests ──────────────────────────────

    #[test]
    fn test_get_graph_statistics() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let stats = tools.get_graph_statistics("mygraph");

        assert_eq!(stats.get("graph_id").and_then(|v| v.as_str()), Some("mygraph"));
        assert_eq!(stats.get("total_nodes").and_then(|v| v.as_i64()), Some(4));
        assert_eq!(stats.get("total_edges").and_then(|v| v.as_i64()), Some(3));

        let entity_types = stats
            .get("entity_types")
            .and_then(|v| v.as_object())
            .expect("entity_types is object");
        // Alice + Bob = 2 persons, Acme = 1 organization, Beijing = 1 location
        assert_eq!(entity_types.get("person").and_then(|v| v.as_i64()), Some(2));
        assert_eq!(entity_types.get("organization").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(entity_types.get("location").and_then(|v| v.as_i64()), Some(1));

        let relation_types =
            stats.get("relation_types").and_then(|v| v.as_object()).expect("relation_types");
        assert_eq!(relation_types.get("WorksFor").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(relation_types.get("RelatedTo").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(relation_types.get("LocatedIn").and_then(|v| v.as_i64()), Some(1));
    }

    // ── ReportTools::get_simulation_context tests ────────────────────────────

    #[test]
    fn test_get_simulation_context_structure() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let ctx = tools.get_simulation_context("graph1", "workforce prediction", 30);

        assert_eq!(
            ctx.get("simulation_requirement").and_then(|v| v.as_str()),
            Some("workforce prediction")
        );
        assert!(ctx.contains_key("related_facts"));
        assert!(ctx.contains_key("graph_statistics"));
        assert!(ctx.contains_key("entities"));
        assert!(ctx.contains_key("total_entities"));
    }

    // ── InsightForgeResult DTO tests ─────────────────────────────────────────

    #[test]
    fn test_insight_forge_result_to_dict_key_order() {
        let r = InsightForgeResult {
            query: "q".into(),
            simulation_requirement: "sr".into(),
            sub_queries: vec!["sq1".into()],
            semantic_facts: vec!["f1".into()],
            entity_insights: vec![],
            relationship_chains: vec![],
            total_facts: 1,
            total_entities: 0,
            total_relationships: 0,
        };
        let d = r.to_dict();
        let keys: Vec<&str> = d.keys().map(|s| s.as_str()).collect();
        assert_eq!(
            keys,
            &[
                "query",
                "simulation_requirement",
                "sub_queries",
                "semantic_facts",
                "entity_insights",
                "relationship_chains",
                "total_facts",
                "total_entities",
                "total_relationships",
            ]
        );
    }

    #[test]
    fn test_insight_forge_result_to_text_contains_headers() {
        let r = InsightForgeResult {
            query: "future".into(),
            simulation_requirement: "predict X".into(),
            sub_queries: vec!["sub1".into()],
            semantic_facts: vec!["fact A".into()],
            entity_insights: vec![],
            relationship_chains: vec!["A --[B]--> C".into()],
            total_facts: 1,
            total_entities: 0,
            total_relationships: 1,
        };
        let text = r.to_text();
        assert!(text.contains("## 未来预测深度分析"));
        assert!(text.contains("分析问题: future"));
        assert!(text.contains("预测场景: predict X"));
        assert!(text.contains("关键事实"));
        assert!(text.contains("关系链"));
    }

    // ── ReportTools::insight_forge keyword-fallback tests ───────────────────

    #[test]
    fn test_insight_forge_keyword_fallback_structure() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let result = tools.insight_forge("graph1", "Alice workforce", "predict", "", 5);

        // Multi-sub-query structure preserved (not dropped).
        assert!(!result.sub_queries.is_empty(), "sub_queries must not be empty");
        assert!(result.sub_queries.len() <= 5);
        assert_eq!(result.query, "Alice workforce");
        assert_eq!(result.simulation_requirement, "predict");
    }

    // ── ReportTools::interview_agents (U-024) ───────────────────────────────

    // U-024: the sync `execute_by_name` path with `runner: None` (graph-only
    // facade) keeps returning the honest tolerated error text — the async
    // `interview_agents` body is only reachable via `execute_by_name_async`
    // with a live runner. (The old direct sync `.interview_agents(...)` call is
    // gone: the method is now `async fn` and requires a `SimulationRunner`.)
    #[test]
    fn test_interview_agents_sync_path_returns_honest_error() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm); // runner: None
        let params = params_with("interview_topic", "What do students think?");
        let result = tools.execute_by_name("interview_agents", &params, "g", "s1", "req", "");
        // The honest tolerated error text (Python `_execute_tool` try/except).
        assert!(result.contains("工具执行失败"), "expected error text, got: {result}");
    }

    // The async dispatch with NO runner still yields the honest error text (the
    // async `interview_agents` early-returns `Err` when `self.runner` is `None`).
    #[tokio::test]
    async fn test_interview_agents_async_no_runner_returns_error_text() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm); // runner: None
        let params = params_with("interview_topic", "topic");
        let result = tools
            .execute_by_name_async("interview_agents", &params, "g", "s1", "req", "")
            .await;
        assert!(result.contains("工具执行失败"), "expected error text, got: {result}");
    }

    // The async dispatcher delegates every NON-interview tool to the sync path.
    #[tokio::test]
    async fn test_execute_by_name_async_delegates_other_tools() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let params = params_with("query", "Bob");
        let sync = tools.execute_by_name("quick_search", &params, "g", "s1", "req", "");
        let async_r =
            tools.execute_by_name_async("quick_search", &params, "g", "s1", "req", "").await;
        assert_eq!(sync, async_r);
    }

    // ── U-024 DTO parity tests (AgentInterview / InterviewResult) ────────────

    #[test]
    fn test_agent_interview_to_dict_field_set() {
        let iv = AgentInterview {
            agent_name: "Alice".to_string(),
            agent_role: "学生".to_string(),
            agent_bio: "bio text".to_string(),
            question: "Q?".to_string(),
            response: "A.".to_string(),
            key_quotes: vec!["quote one".to_string()],
        };
        let d = iv.to_dict();
        assert_eq!(d.get("agent_name").unwrap(), "Alice");
        assert_eq!(d.get("agent_role").unwrap(), "学生");
        assert_eq!(d.get("agent_bio").unwrap(), "bio text");
        assert_eq!(d.get("question").unwrap(), "Q?");
        assert_eq!(d.get("response").unwrap(), "A.");
        assert_eq!(d.get("key_quotes").unwrap(), &serde_json::json!(["quote one"]));
        // Exactly 6 keys, no extras.
        assert_eq!(d.len(), 6);
    }

    #[test]
    fn test_agent_interview_to_text_basic_format() {
        let iv = AgentInterview {
            agent_name: "Bob".to_string(),
            agent_role: "工程师".to_string(),
            agent_bio: "background".to_string(),
            question: "你怎么看？".to_string(),
            response: "我觉得不错。".to_string(),
            key_quotes: vec![],
        };
        let txt = iv.to_text();
        assert!(txt.contains("**Bob** (工程师)"));
        assert!(txt.contains("_简介: background_"));
        assert!(txt.contains("**Q:** 你怎么看？"));
        assert!(txt.contains("**A:** 我觉得不错。"));
        // No key-quote block when key_quotes is empty.
        assert!(!txt.contains("**关键引言:**"));
    }

    #[test]
    fn test_agent_interview_to_text_key_quote_cleaning() {
        let iv = AgentInterview {
            agent_name: "C".to_string(),
            agent_role: "r".to_string(),
            agent_bio: "b".to_string(),
            question: "q".to_string(),
            response: "resp".to_string(),
            key_quotes: vec![
                // Leading punctuation + curly quotes stripped, len >= 10 → kept.
                "\u{201c}，这是一个足够长的引言内容。\u{201d}".to_string(),
                // Contains 问题3 → skipped.
                "问题3：这是一个被跳过的引言内容啊。".to_string(),
                // Too short (< 10 chars after cleaning) → dropped.
                "短句".to_string(),
            ],
        };
        let txt = iv.to_text();
        assert!(txt.contains("**关键引言:**"));
        assert!(txt.contains("这是一个足够长的引言内容"));
        // 问题3 quote skipped.
        assert!(!txt.contains("被跳过"));
        // Short quote dropped.
        assert!(!txt.contains("> \"短句\""));
    }

    #[test]
    fn test_agent_interview_to_text_truncation_over_150() {
        // > 150 chars, with a 。 after position 80 → truncated at that period.
        let head: String = "甲".repeat(85);
        let tail: String = "乙".repeat(80);
        let quote = format!("{}。{}", head, tail); // 85 + 1 + 80 = 166 chars
        let iv = AgentInterview {
            agent_name: "n".to_string(),
            agent_role: "r".to_string(),
            agent_bio: "b".to_string(),
            question: "q".to_string(),
            response: "x".to_string(),
            key_quotes: vec![quote],
        };
        let txt = iv.to_text();
        // Truncated to the first 。 after pos 80 → keeps head + 。, drops the 乙 tail.
        assert!(txt.contains(&format!("> \"{}。\"", head)));
        assert!(!txt.contains("乙乙"));
    }

    #[test]
    fn test_interview_result_to_dict_field_set() {
        let mut r = InterviewResult::new("主题".to_string(), vec!["q1".to_string()]);
        r.total_agents = 5;
        r.interviewed_count = 2;
        r.summary = "摘要".to_string();
        r.selection_reasoning = "理由".to_string();
        let d = r.to_dict();
        for k in [
            "interview_topic",
            "interview_questions",
            "selected_agents",
            "interviews",
            "selection_reasoning",
            "summary",
            "total_agents",
            "interviewed_count",
        ] {
            assert!(d.contains_key(k), "missing key {k}");
        }
        assert_eq!(d.get("interview_topic").unwrap(), "主题");
        assert_eq!(d.get("total_agents").unwrap(), &serde_json::json!(5));
        assert_eq!(d.get("interviewed_count").unwrap(), &serde_json::json!(2));
        assert_eq!(d.len(), 8);
    }

    #[test]
    fn test_interview_result_to_text_empty_interviews() {
        let r = InterviewResult::new("我的主题".to_string(), vec![]);
        let txt = r.to_text();
        assert!(txt.contains("## 深度采访报告"));
        assert!(txt.contains("**采访主题:** 我的主题"));
        assert!(txt.contains("**采访人数:** 0 / 0 位模拟Agent"));
        assert!(txt.contains("### 采访对象选择理由"));
        // Empty reasoning → 自动选择 placeholder.
        assert!(txt.contains("（自动选择）"));
        // Empty interviews → 无采访记录.
        assert!(txt.contains("（无采访记录）"));
        // Empty summary → 无摘要.
        assert!(txt.contains("### 采访摘要与核心观点"));
        assert!(txt.contains("（无摘要）"));
    }

    #[test]
    fn test_interview_result_to_text_non_empty() {
        let mut r = InterviewResult::new("话题".to_string(), vec![]);
        r.total_agents = 3;
        r.interviewed_count = 1;
        r.selection_reasoning = "因为相关".to_string();
        r.summary = "总结内容".to_string();
        r.interviews.push(AgentInterview {
            agent_name: "受访者甲".to_string(),
            agent_role: "教师".to_string(),
            agent_bio: "bio".to_string(),
            question: "1. q".to_string(),
            response: "答复".to_string(),
            key_quotes: vec![],
        });
        let txt = r.to_text();
        assert!(txt.contains("**采访人数:** 1 / 3 位模拟Agent"));
        assert!(txt.contains("因为相关"));
        assert!(txt.contains("### 采访实录"));
        assert!(txt.contains("#### 采访 #1: 受访者甲"));
        assert!(txt.contains("**受访者甲** (教师)"));
        assert!(txt.contains("### 采访摘要与核心观点"));
        assert!(txt.contains("总结内容"));
        assert!(!txt.contains("（无采访记录）"));
    }

    #[test]
    fn test_clean_tool_call_response_passthrough_when_not_json() {
        let plain = "这是一段普通的回答，没有任何工具调用。".to_string();
        assert_eq!(ReportTools::<StubLlm>::clean_tool_call_response(plain.clone()), plain);
        // Starts with { but no tool_name in first 80 chars → passthrough.
        let no_tool = "{\"key\": \"value without the marker\"}".to_string();
        assert_eq!(ReportTools::<StubLlm>::clean_tool_call_response(no_tool.clone()), no_tool);
    }

    #[test]
    fn test_clean_tool_call_response_unwraps_arguments_content() {
        let wrapped = "{\"tool_name\": \"reply\", \"arguments\": {\"content\": \"真实回答内容\"}}"
            .to_string();
        assert_eq!(ReportTools::<StubLlm>::clean_tool_call_response(wrapped), "真实回答内容");
    }

    // ── get_all_nodes / get_all_edges real reads ─────────────────────────────

    #[test]
    fn test_get_all_nodes_count() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let nodes = tools.get_all_nodes("g");
        assert_eq!(nodes.len(), 4);
    }

    #[test]
    fn test_get_all_edges_count() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let edges = tools.get_all_edges("g", false);
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn test_get_all_edges_temporal() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let edges = tools.get_all_edges("g", true);
        // The LocatedIn edge [100, 200) should have valid_at and invalid_at set.
        let located_in = edges.iter().find(|e| e.name == "LocatedIn");
        assert!(located_in.is_some());
        let e = located_in.unwrap();
        assert_eq!(e.valid_at.as_deref(), Some("100"));
        assert_eq!(e.invalid_at.as_deref(), Some("200"));
        assert_eq!(e.expired_at.as_deref(), Some("200")); // closed window → expired
    }

    #[test]
    fn test_get_node_detail_found() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let alice_uuid = "00000000-0000-0000-0000-000000000001";
        let node = tools.get_node_detail(alice_uuid);
        assert!(node.is_some());
        assert_eq!(node.unwrap().name, "Alice");
    }

    #[test]
    fn test_get_node_detail_missing() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let node = tools.get_node_detail("00000000-0000-0000-0000-000000000099");
        assert!(node.is_none());
    }

    #[test]
    fn test_get_node_detail_bad_uuid() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let node = tools.get_node_detail("not-a-uuid");
        assert!(node.is_none());
    }

    #[test]
    fn test_get_node_edges() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let alice_uuid = "00000000-0000-0000-0000-000000000001";
        let edges = tools.get_node_edges("g", alice_uuid);
        // Alice has 3 edges: WorksFor Acme (out), RelatedTo Alice incoming from Bob, LocatedIn Beijing
        assert!(!edges.is_empty());
    }

    // ── get_entity_summary ───────────────────────────────────────────────────

    #[test]
    fn test_get_entity_summary_found() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let summary = tools.get_entity_summary("g", "Alice");
        assert_eq!(summary.get("entity_name").and_then(|v| v.as_str()), Some("Alice"));
        assert!(summary.contains_key("entity_info"));
        assert!(summary.get("entity_info").map(|v| !v.is_null()).unwrap_or(false));
        assert!(summary.contains_key("total_relations"));
    }

    #[test]
    fn test_get_entity_summary_not_found() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let summary = tools.get_entity_summary("g", "Nonexistent");
        assert_eq!(summary.get("entity_name").and_then(|v| v.as_str()), Some("Nonexistent"));
        assert!(summary.get("entity_info").map(|v| v.is_null()).unwrap_or(false));
        assert_eq!(summary.get("total_relations").and_then(|v| v.as_i64()), Some(0));
    }

    // ── Sub-cycle (c): parse_tool_calls — 3-tier tests ──────────────────────

    // Tier 1: <tool_call>…</tool_call>
    #[test]
    fn test_parse_tool_calls_tier1_xml_single() {
        let resp = r#"Some thought here.
<tool_call>
{"name": "quick_search", "parameters": {"query": "hello"}}
</tool_call>"#;
        let calls = parse_tool_calls(resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "quick_search");
        assert_eq!(calls[0].parameters.get("query").and_then(|v| v.as_str()), Some("hello"));
    }

    #[test]
    fn test_parse_tool_calls_tier1_xml_multiple() {
        // Multiple <tool_call> tags → return all (Python iterates finditer)
        let resp = r#"<tool_call>{"name": "quick_search", "parameters": {"query": "a"}}</tool_call>
<tool_call>{"name": "panorama_search", "parameters": {"query": "b"}}</tool_call>"#;
        let calls = parse_tool_calls(resp);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "quick_search");
        assert_eq!(calls[1].name, "panorama_search");
    }

    #[test]
    fn test_parse_tool_calls_tier1_dotall() {
        // Multiline JSON inside <tool_call>
        let resp = "<tool_call>\n{\n  \"name\": \"insight_forge\",\n  \"parameters\": {\n    \"query\": \"test\"\n  }\n}\n</tool_call>";
        let calls = parse_tool_calls(resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "insight_forge");
    }

    #[test]
    fn test_parse_tool_calls_tier1_bad_json_skipped() {
        let resp = "<tool_call>NOT_JSON</tool_call>";
        let calls = parse_tool_calls(resp);
        assert!(calls.is_empty());
    }

    // Tier 2: bare whole-response JSON
    #[test]
    fn test_parse_tool_calls_tier2_bare_json() {
        let resp = r#"{"name": "quick_search", "parameters": {"query": "foo"}}"#;
        let calls = parse_tool_calls(resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "quick_search");
    }

    #[test]
    fn test_parse_tool_calls_tier2_bare_json_invalid_tool_rejected() {
        // A valid JSON object but unknown tool name → empty (VALID_TOOL_NAMES gate)
        let resp = r#"{"name": "unknown_tool", "parameters": {}}"#;
        let calls = parse_tool_calls(resp);
        // Tier 2 rejects it (unknown tool); tier 3 might pick it up via trailing
        // regex — but "unknown_tool" still fails VALID_TOOL_NAMES gate there too.
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_tool_calls_tier2_not_bare_if_text_before() {
        // Tier 2 only triggers if the WHOLE stripped response is a JSON object.
        // If there's text before/after, tier 2 is skipped and tier 3 may catch it.
        let resp = "Thinking...\n{\"name\": \"quick_search\", \"parameters\": {\"query\": \"q\"}}";
        let calls = parse_tool_calls(resp);
        // Tier 3 (trailing regex) should catch it
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "quick_search");
    }

    // Tier 3: trailing {"name"|"tool": …} regex
    #[test]
    fn test_parse_tool_calls_tier3_trailing_name_key() {
        let resp = "Some thinking text\n{\"name\": \"panorama_search\", \"parameters\": {\"query\": \"x\"}}";
        let calls = parse_tool_calls(resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "panorama_search");
    }

    #[test]
    fn test_parse_tool_calls_tier3_trailing_tool_key() {
        // "tool" key is accepted by tier-3 regex and normalised to "name"
        let resp = "Some text\n{\"tool\": \"quick_search\", \"params\": {\"query\": \"z\"}}";
        let calls = parse_tool_calls(resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "quick_search");
        assert_eq!(calls[0].parameters.get("query").and_then(|v| v.as_str()), Some("z"));
    }

    #[test]
    fn test_parse_tool_calls_tier3_trailing_unknown_rejected() {
        let resp = "Some text\n{\"tool\": \"bad_tool\", \"params\": {}}";
        let calls = parse_tool_calls(resp);
        assert!(calls.is_empty());
    }

    // ── Key normalization is TIER-2/3 ONLY (Python `_is_valid_tool_call`); tier-1 is RAW ──
    // Python tier-1 appends `json.loads(...)` directly with NO normalization
    // (report_agent.py:1079–1082). These goldens are derived from the real Python behavior.

    #[test]
    fn test_parse_tool_calls_tier1_params_not_normalized() {
        // Python tier-1: `{"name":"quick_search","params":{...}}` is appended raw; the
        // downstream `call.get("parameters", {})` then sees the RAW dict → "params" is NOT
        // promoted → parameters is EMPTY. (Differs from tiers 2/3 which DO normalize.)
        let resp = r#"<tool_call>{"name": "quick_search", "params": {"query": "p"}}</tool_call>"#;
        let calls = parse_tool_calls(resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "quick_search");
        // RAW "parameters" key is absent → empty (Python parity, NOT Some("p")).
        assert!(calls[0].parameters.is_empty());
    }

    #[test]
    fn test_parse_tool_calls_tier1_tool_key_without_name_skipped() {
        // [≠] Python tier-1 pushes `{"tool":..}` raw (no "name" key), then KeyError-crashes at
        // the downstream `call["name"]` access (report_agent.py:1419/1852) — a Python defect.
        // teri does not preserve the crash: the name-less tier-1 object is skipped and the
        // parser falls through (tier-2 needs a bare `{...}` start; tier-3 needs a trailing
        // `}` not `</tool_call>`), so the result is empty.
        let resp =
            r#"<tool_call>{"tool": "quick_search", "parameters": {"query": "n"}}</tool_call>"#;
        let calls = parse_tool_calls(resp);
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_tool_calls_tier1_reads_raw_parameters_key() {
        // Tier-1 reads the RAW "parameters" key directly (no promotion of "params"); when
        // "parameters" IS present, its value is used verbatim — matching Python's
        // `call.get("parameters", {})` over the raw dict.
        let resp = r#"<tool_call>{"name": "quick_search", "params": {"query": "OLD"}, "parameters": {"query": "NEW"}}</tool_call>"#;
        let calls = parse_tool_calls(resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].parameters.get("query").and_then(|v| v.as_str()), Some("NEW"));
    }

    #[test]
    fn test_parse_tool_calls_tier2_normalizes_tool_to_name() {
        // Bare JSON (tier-2) DOES go through `_is_valid_tool_call` → "tool"→"name" rename.
        let resp = r#"{"tool": "quick_search", "parameters": {"query": "n"}}"#;
        let calls = parse_tool_calls(resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "quick_search");
    }

    #[test]
    fn test_parse_tool_calls_tier2_normalizes_params_to_parameters() {
        // Bare JSON (tier-2) DOES promote "params"→"parameters".
        let resp = r#"{"name": "quick_search", "params": {"query": "p"}}"#;
        let calls = parse_tool_calls(resp);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].parameters.get("query").and_then(|v| v.as_str()), Some("p"));
    }

    // Empty / malformed input
    #[test]
    fn test_parse_tool_calls_empty_string() {
        let calls = parse_tool_calls("");
        assert!(calls.is_empty());
    }

    #[test]
    fn test_parse_tool_calls_plain_text_no_json() {
        let calls = parse_tool_calls("Final Answer: here is the answer");
        assert!(calls.is_empty());
    }

    // ── Sub-cycle (c): ReportTool::from_name ────────────────────────────────

    #[test]
    fn test_report_tool_from_name_canonical() {
        assert_eq!(ReportTool::from_name("insight_forge"), Some(ReportTool::InsightForge));
        assert_eq!(ReportTool::from_name("panorama_search"), Some(ReportTool::PanoramaSearch));
        assert_eq!(ReportTool::from_name("quick_search"), Some(ReportTool::QuickSearch));
        assert_eq!(ReportTool::from_name("interview_agents"), Some(ReportTool::InterviewAgents));
    }

    #[test]
    fn test_report_tool_from_name_back_compat() {
        assert_eq!(ReportTool::from_name("search_graph"), Some(ReportTool::SearchGraph));
        assert_eq!(
            ReportTool::from_name("get_graph_statistics"),
            Some(ReportTool::GetGraphStatistics)
        );
        assert_eq!(ReportTool::from_name("get_entity_summary"), Some(ReportTool::GetEntitySummary));
        assert_eq!(
            ReportTool::from_name("get_simulation_context"),
            Some(ReportTool::GetSimulationContext)
        );
        assert_eq!(
            ReportTool::from_name("get_entities_by_type"),
            Some(ReportTool::GetEntitiesByType)
        );
    }

    #[test]
    fn test_report_tool_from_name_unknown() {
        assert_eq!(ReportTool::from_name("nonexistent"), None);
        assert_eq!(ReportTool::from_name(""), None);
    }

    // ── Sub-cycle (c): execute_by_name — dispatch + unknown-tool string ──────

    fn empty_params() -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::new()
    }

    fn params_with(key: &str, val: &str) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert(key.to_string(), serde_json::Value::String(val.to_string()));
        m
    }

    #[test]
    fn test_execute_by_name_unknown_tool() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let result = tools.execute_by_name("does_not_exist", &empty_params(), "g", "s1", "req", "");
        // Byte-identical to Python: "未知工具: {name}。请使用以下工具之一: ..."
        assert!(result.starts_with("未知工具: does_not_exist"));
        assert!(result.contains("insight_forge"));
    }

    #[test]
    fn test_execute_by_name_quick_search() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let params = params_with("query", "Alice");
        let result = tools.execute_by_name("quick_search", &params, "g", "s1", "req", "");
        // Returns text (the SearchResult.to_text())
        assert!(!result.is_empty());
        assert!(result.contains("搜索查询"));
    }

    #[test]
    fn test_execute_by_name_panorama_search() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let params = params_with("query", "works");
        let result = tools.execute_by_name("panorama_search", &params, "g", "s1", "req", "");
        assert!(!result.is_empty());
        // panorama_search to_text() opens with "广度搜索结果" header
        assert!(result.contains("广度搜索结果") || result.contains("查询:"));
    }

    #[test]
    fn test_execute_by_name_insight_forge() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let params = params_with("query", "Alice relationships");
        let result = tools.execute_by_name("insight_forge", &params, "g", "s1", "test sim req", "");
        assert!(!result.is_empty());
        assert!(result.contains("未来预测深度分析") || result.contains("分析问题"));
    }

    #[test]
    fn test_execute_by_name_interview_agents_returns_error_text() {
        // interview_agents is [!] pending (sub-cycle e); must return error text, not panic.
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let params = params_with("interview_topic", "What do students think?");
        let result = tools.execute_by_name("interview_agents", &params, "g", "s1", "req", "");
        // Must be the "工具执行失败: ..." text (not a panic)
        assert!(result.contains("工具执行失败"), "expected error text, got: {}", result);
    }

    // ── Sub-cycle (c): back-compat redirects ─────────────────────────────────

    #[test]
    fn test_execute_by_name_search_graph_redirects_to_quick_search() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let params = params_with("query", "Bob");
        let r1 = tools.execute_by_name("search_graph", &params, "g", "s1", "req", "");
        let r2 = tools.execute_by_name("quick_search", &params, "g", "s1", "req", "");
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_execute_by_name_get_graph_statistics() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let result =
            tools.execute_by_name("get_graph_statistics", &empty_params(), "g", "s1", "req", "");
        // Returns JSON string of graph stats
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        assert!(parsed.get("total_nodes").is_some());
        assert!(parsed.get("total_edges").is_some());
    }

    #[test]
    fn test_execute_by_name_get_entity_summary() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let params = params_with("entity_name", "Alice");
        let result = tools.execute_by_name("get_entity_summary", &params, "g", "s1", "req", "");
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        assert_eq!(parsed.get("entity_name").and_then(|v| v.as_str()), Some("Alice"));
    }

    #[test]
    fn test_execute_by_name_get_simulation_context_redirects_to_insight_forge() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let params = params_with("query", "Alice");
        let result =
            tools.execute_by_name("get_simulation_context", &params, "g", "s1", "sim req", "");
        // Should return InsightForge text
        assert!(result.contains("未来预测深度分析") || result.contains("分析问题"));
    }

    #[test]
    fn test_execute_by_name_get_simulation_context_fallback_to_sim_req() {
        // If no "query" param, fallback to simulation_requirement
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let result = tools.execute_by_name(
            "get_simulation_context",
            &empty_params(),
            "g",
            "s1",
            "sim req fallback",
            "",
        );
        assert!(!result.is_empty());
    }

    #[test]
    fn test_execute_by_name_get_entities_by_type() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let params = params_with("entity_type", "person");
        let result = tools.execute_by_name("get_entities_by_type", &params, "g", "s1", "req", "");
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        let arr = parsed.as_array().expect("should be array");
        assert_eq!(arr.len(), 2); // Alice + Bob
    }

    // ── Sub-cycle (c): param coercions ───────────────────────────────────────

    #[test]
    fn test_coerce_include_expired_true_variants() {
        // "true", "1", "yes" → true
        for s in &["true", "1", "yes", "True", "YES", "1"] {
            let mut m = serde_json::Map::new();
            m.insert("include_expired".to_string(), serde_json::Value::String(s.to_string()));
            assert!(coerce_include_expired(&m, false), "failed for '{}'", s);
        }
    }

    #[test]
    fn test_coerce_include_expired_false_variants() {
        for s in &["false", "0", "no", "nope", ""] {
            let mut m = serde_json::Map::new();
            m.insert("include_expired".to_string(), serde_json::Value::String(s.to_string()));
            assert!(!coerce_include_expired(&m, true), "failed for '{}'", s);
        }
    }

    #[test]
    fn test_coerce_include_expired_bool_value() {
        let mut m = serde_json::Map::new();
        m.insert("include_expired".to_string(), serde_json::Value::Bool(false));
        assert!(!coerce_include_expired(&m, true));

        let mut m2 = serde_json::Map::new();
        m2.insert("include_expired".to_string(), serde_json::Value::Bool(true));
        assert!(coerce_include_expired(&m2, false));
    }

    #[test]
    fn test_coerce_include_expired_missing_uses_default() {
        let m = serde_json::Map::new();
        assert!(coerce_include_expired(&m, true));
        assert!(!coerce_include_expired(&m, false));
    }

    #[test]
    fn test_coerce_int_param_from_number() {
        let mut m = serde_json::Map::new();
        m.insert("limit".to_string(), serde_json::json!(7));
        assert_eq!(coerce_int_param(&m, "limit", 10).unwrap(), 7);
    }

    #[test]
    fn test_coerce_int_param_from_string() {
        let mut m = serde_json::Map::new();
        m.insert("limit".to_string(), serde_json::Value::String("15".to_string()));
        assert_eq!(coerce_int_param(&m, "limit", 10).unwrap(), 15);
    }

    #[test]
    fn test_coerce_int_param_bad_string_is_err() {
        // Python parity: `int("not_a_number")` raises ValueError → tool fails with
        // "工具执行失败: …" (report_agent.py:1060–1062). The default applies ONLY on a
        // MISSING key, never on an unparseable present value.
        let mut m = serde_json::Map::new();
        m.insert("limit".to_string(), serde_json::Value::String("not_a_number".to_string()));
        assert!(coerce_int_param(&m, "limit", 10).is_err());
    }

    #[test]
    fn test_coerce_int_param_missing_uses_default() {
        let m = serde_json::Map::new();
        assert_eq!(coerce_int_param(&m, "limit", 42).unwrap(), 42);
    }

    #[test]
    fn test_execute_quick_search_bad_limit_returns_failure_text() {
        // End-to-end: a non-numeric `limit` makes quick_search fail with the Python-parity
        // "工具执行失败: " prefix instead of silently running with the default.
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let mut p = serde_json::Map::new();
        p.insert("query".to_string(), serde_json::Value::String("x".to_string()));
        p.insert("limit".to_string(), serde_json::Value::String("abc".to_string()));
        let out = tools.execute_by_name("quick_search", &p, "g", "s", "req", "");
        assert!(out.starts_with("工具执行失败: "), "got: {out}");
    }

    // max_agents cap at 10 (report_agent.py:1014: max_agents = min(max_agents, 10))
    #[test]
    fn test_execute_max_agents_capped_at_10() {
        // The cap happens in execute_inner before calling interview_agents.
        // interview_agents returns Err (pending), which becomes error text — that's fine.
        // We verify the cap by checking the error text still comes back (not a panic from
        // an out-of-range value).
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let mut params = serde_json::Map::new();
        params.insert("interview_topic".to_string(), serde_json::json!("topic"));
        params.insert("max_agents".to_string(), serde_json::json!("999")); // should be capped to 10
        let result = tools.execute_by_name("interview_agents", &params, "g", "s1", "req", "");
        // Still returns error text (not a panic)
        assert!(result.contains("工具执行失败"));
    }

    // interview_topic falls back to "query" when not present (report_agent.py:1010)
    #[test]
    fn test_execute_interview_agents_topic_fallback_to_query() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        // Only "query" param, no "interview_topic"
        let params = params_with("query", "student opinions");
        let result = tools.execute_by_name("interview_agents", &params, "g", "s1", "req", "");
        // Must return error-text (pending), not panic
        assert!(result.contains("工具执行失败"));
    }

    // ── Sub-cycle (c): get_tools_description ─────────────────────────────────

    #[test]
    fn test_get_tools_description_contains_all_tools() {
        let desc = get_tools_description();
        assert!(desc.contains("可用工具："));
        assert!(desc.contains("insight_forge"));
        assert!(desc.contains("panorama_search"));
        assert!(desc.contains("quick_search"));
        assert!(desc.contains("interview_agents"));
    }

    #[test]
    fn test_get_tools_description_contains_params() {
        let desc = get_tools_description();
        assert!(desc.contains("参数:"));
        // insight_forge params
        assert!(desc.contains("report_context"));
        // panorama_search params
        assert!(desc.contains("include_expired"));
        // quick_search params
        assert!(desc.contains("limit"));
        // interview_agents params
        assert!(desc.contains("max_agents"));
    }

    #[test]
    fn test_get_tools_description_order() {
        let desc = get_tools_description();
        let pos_insight = desc.find("insight_forge").unwrap();
        let pos_panorama = desc.find("panorama_search").unwrap();
        let pos_quick = desc.find("quick_search").unwrap();
        let pos_interview = desc.find("interview_agents").unwrap();
        // Order must match Python _define_tools insertion order
        assert!(pos_insight < pos_panorama);
        assert!(pos_panorama < pos_quick);
        assert!(pos_quick < pos_interview);
    }

    // ── Sub-cycle (c): tool description constants verbatim spot-checks ────────

    #[test]
    fn test_tool_desc_insight_forge_verbatim() {
        assert!(TOOL_DESC_INSIGHT_FORGE.contains("深度洞察检索"));
        assert!(TOOL_DESC_INSIGHT_FORGE.contains("自动将你的问题分解为多个子问题"));
        assert!(TOOL_DESC_INSIGHT_FORGE.contains("关系链分析"));
    }

    #[test]
    fn test_tool_desc_panorama_search_verbatim() {
        assert!(TOOL_DESC_PANORAMA_SEARCH.contains("广度搜索"));
        assert!(TOOL_DESC_PANORAMA_SEARCH.contains("区分当前有效的事实和历史/过期的事实"));
    }

    #[test]
    fn test_tool_desc_quick_search_verbatim() {
        assert!(TOOL_DESC_QUICK_SEARCH.contains("简单搜索"));
        assert!(TOOL_DESC_QUICK_SEARCH.contains("与查询最相关的事实列表"));
    }

    #[test]
    fn test_tool_desc_interview_agents_verbatim() {
        assert!(TOOL_DESC_INTERVIEW_AGENTS.contains("深度采访"));
        assert!(TOOL_DESC_INTERVIEW_AGENTS.contains("OASIS模拟环境正在运行"));
    }
}
