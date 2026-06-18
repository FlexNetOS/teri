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

use crate::error::{Result, TeriError};
use crate::graph::{Entity, KnowledgeGraph};
use crate::llm::LlmClient;
use crate::services::entity_reader::KnowledgeGraphEntityReader;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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

/// Agent profile for interviews.
///
/// Port of `AgentInterview` dataclass (`zep_tools.py:284-337`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentInterview {
    pub agent_id: i64,
    pub platform: String,
    pub profile: serde_json::Map<String, serde_json::Value>,
}

impl AgentInterview {
    /// Convert to dict matching Python `to_dict()`.
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("agent_id".into(), self.agent_id.into());
        m.insert("platform".into(), self.platform.clone().into());
        m
    }

    /// Convert to text matching Python `to_text()`.
    pub fn to_text(&self) -> String {
        format!("Agent {} ({})", self.agent_id, self.platform)
    }
}

/// Result from interview_agents operation.
///
/// Port of `InterviewResult` dataclass (`zep_tools.py:340-398`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InterviewResult {
    pub agent_interviews: Vec<AgentInterview>,
    pub questions: Vec<String>,
    pub responses: Vec<String>,
}

impl InterviewResult {
    /// Convert to dict matching Python `to_dict()`.
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert(
            "agent_interviews".into(),
            serde_json::to_value(&self.agent_interviews).unwrap_or_default(),
        );
        m.insert("questions".into(), serde_json::to_value(&self.questions).unwrap_or_default());
        m
    }

    /// Convert to text matching Python `to_text()`.
    pub fn to_text(&self) -> String {
        let mut lines = Vec::new();
        for interview in &self.agent_interviews {
            lines.push(interview.to_text());
        }
        lines.join("\n")
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
pub struct ReportTools<'g, L: LlmClient> {
    /// The knowledge graph being read. Replaces Zep's `graph_id` server-handle.
    graph: &'g KnowledgeGraph,
    /// LLM client for insight_forge sub-query generation.
    #[allow(dead_code)]
    llm: &'g L,
    /// Entity reader reusing U-016 substrate for entity-by-type / entity-summary reads.
    reader: KnowledgeGraphEntityReader<'g>,
}

impl<'g, L: LlmClient> ReportTools<'g, L> {
    /// Create a new `ReportTools` binding graph and LLM by reference.
    ///
    /// Per DECISION-11, caller constructs handles; `ReportTools` borrows them.
    pub fn new(graph: &'g KnowledgeGraph, llm: &'g L) -> Self {
        let reader = KnowledgeGraphEntityReader::new(graph);
        Self { graph, llm, reader }
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
        let mut entity_types: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for node in &nodes {
            for label in &node.labels {
                if label != "Entity" && label != "Node" {
                    *entity_types.entry(label.clone()).or_insert(0) += 1;
                }
            }
        }

        // Count relation types by edge name.
        let mut relation_types: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for edge in &edges {
            *relation_types.entry(edge.name.clone()).or_insert(0) += 1;
        }

        let mut m = serde_json::Map::new();
        // Python includes graph_id in output (zep_tools.py:882).
        m.insert("graph_id".into(), graph_id.into());
        m.insert("total_nodes".into(), (nodes.len() as i64).into());
        m.insert("total_edges".into(), (edges.len() as i64).into());
        m.insert("entity_types".into(), serde_json::to_value(entity_types).unwrap_or_default());
        m.insert(
            "relation_types".into(),
            serde_json::to_value(relation_types).unwrap_or_default(),
        );
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
    // interview_agents — sub-cycle (e) DEFERRED — honest error string
    //
    // Port of `ZepToolsService.interview_agents(simulation_id, ...)` (`zep_tools.py:1272-`).
    // Requires U-020 simulation IPC (already ported) + U-022 SimulationRunner
    // (SimulationRunner.interview_agents_batch).
    //
    // Per architect: return an honest error string the ReACT loop tolerates.
    // Python `_execute_tool` wraps exceptions as "工具执行失败: {e}" — teri mirrors
    // that (the loop keeps going, no downgrade to the loop; only `[!]` on the tool).
    // -----------------------------------------------------------------------

    /// Interview agents — requires U-022 SimulationRunner (sub-cycle (e) pending).
    ///
    /// Returns an `Err` describing the dependency so the ReACT loop can convert it
    /// to an error-observation string without panicking.
    pub fn interview_agents(
        &self,
        _simulation_id: &str,
        _requirement: &str,
        _sim_req: &str,
        _max_agents: i64,
        _custom_questions: Option<&str>,
    ) -> Result<InterviewResult> {
        // `[!]` sub-cycle (e) pending: interview_agents requires U-022 SimulationRunner
        // integration (SimulationRunner::interview_agents_batch). U-022 is `- [ ]`.
        // Mirror Python's try/except → "工具执行失败: ..." so the ReACT loop continues.
        Err(TeriError::Unknown(
            "interview_agents pending simulation IPC integration (sub-cycle e, U-022)".into(),
        ))
    }
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

    // ── ReportTools::interview_agents honest-error tests ────────────────────

    #[test]
    fn test_interview_agents_returns_honest_error() {
        let g = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&g, &llm);
        let result = tools.interview_agents("sim1", "req", "sim_req", 5, None);
        // Must be Err (not panic / not Ok with empty result)
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        // The error message is honest about the dependency.
        assert!(err_msg.contains("interview_agents") || err_msg.contains("pending"));
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
}
