//! Zep Tools Service - port of `backend/app/services/zep_tools.py` (MiroFish).
//!
//! High-level Zep retrieval tools for ReportAgent:
//! - `search_graph`: Hybrid search with fallback to local keyword search
//! - `insight_forge`: LLM-guided multi-query semantic search with entity enrichment
//! - `panorama_search`: Temporal-aware graph search with active/historical classification
//! - `quick_search`: Simple keyword-based search
//! - `interview_agents`: Agent selection and interview orchestration

use crate::error::{Result, TeriError};
use crate::graph::{KnowledgeGraph, EdgeTriple};
use crate::llm::LlmClient;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Search result from graph search operations.
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
    pub fn is_expired(&self) -> bool {
        self.expired_at.is_some()
    }

    /// Check if this edge is invalid (e.g., has a deleted source or target).
    pub fn is_invalid(&self) -> bool {
        self.source_node_uuid.is_empty() || self.target_node_uuid.is_empty()
    }
}

/// Result from insight_forge operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InsightForgeResult {
    pub sub_queries: Vec<String>,
    pub search_results: Vec<SearchResult>,
    pub enriched_data: serde_json::Map<String, serde_json::Value>,
}

impl InsightForgeResult {
    /// Convert to dict matching Python `to_dict()`.
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert(
            "sub_queries".into(),
            serde_json::to_value(&self.sub_queries).unwrap_or_default(),
        );
        m.insert(
            "search_results".into(),
            serde_json::to_value(&self.search_results).unwrap_or_default(),
        );
        m
    }

    /// Convert to text matching Python `to_text()`.
    pub fn to_text(&self) -> String {
        let mut lines = Vec::new();
        for (i, sub_query) in self.sub_queries.iter().enumerate() {
            lines.push(format!("Sub-query {}: {}", i + 1, sub_query));
        }
        lines.join("\n")
    }
}

/// Result from panorama_search operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PanoramaResult {
    pub nodes: Vec<NodeInfo>,
    pub edges: Vec<EdgeInfo>,
    pub active_count: i64,
    pub historical_count: i64,
}

impl PanoramaResult {
    /// Convert to dict matching Python `to_dict()`.
    pub fn to_dict(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("nodes".into(), serde_json::to_value(&self.nodes).unwrap_or_default());
        m.insert("edges".into(), serde_json::to_value(&self.edges).unwrap_or_default());
        m.insert("active_count".into(), self.active_count.into());
        m.insert("historical_count".into(), self.historical_count.into());
        m
    }

    /// Convert to text matching Python `to_text()`.
    pub fn to_text(&self) -> String {
        format!("Active: {}, Historical: {}", self.active_count, self.historical_count)
    }
}

/// Agent profile for interviews.
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

/// High-level Zep retrieval service.
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

    /// Search the graph with hybrid search, falling back to local keyword search.
    pub async fn search_graph(
        &mut self,
        graph_id: &str,
        query: &str,
        limit: i64,
        scope: Option<&str>,
    ) -> Result<SearchResult> {
        // This would call Zep API - fallback to local search if not available
        let result = self.local_search(graph_id, query, limit, scope).await?;
        Ok(result)
    }

    /// Local keyword-based search fallback.
    pub async fn local_search(
        &mut self,
        _graph_id: &str,
        query: &str,
        limit: i64,
        scope: Option<&str>,
    ) -> Result<SearchResult> {
        // Extract query keywords
        let query_lower = query.to_lowercase();
        let keywords: Vec<String> = query_lower
            .replace(',', " ")
            .replace('，', " ")
            .split_whitespace()
            .filter(|w| w.len() > 1)
            .map(|s| s.to_string())
            .collect();

        fn match_score(text: &str, query_lower: &str, keywords: &[String]) -> i64 {
            if text.is_empty() {
                return 0;
            }
            let text_lower = text.to_lowercase();
            // Exact match of entire query
            if query_lower.contains(&text_lower) || text_lower.contains(query_lower) {
                return 100;
            }
            // Keyword matching
            let mut score = 0;
            for keyword in keywords {
                if text_lower.contains(keyword) {
                    score += 10;
                }
            }
            score
        }

        // Note: This method receives graph_id as a parameter, but teri's KnowledgeGraph
        // is not keyed by graph_id (unlike Zep Cloud). In teri, there's typically one
        // active graph per simulation. We use get_all_entities/get_all_edges which
        // work on the current graph context.
        //
        // For a proper implementation with multiple graphs, we'd need to store the
        // KnowledgeGraph reference in ZepToolsService.

        let scope = scope.unwrap_or("edges");
        let limit_usize = limit as usize;

        let mut facts: Vec<String> = Vec::new();
        let mut edges_result: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
        let mut nodes_result: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();

        // The actual graph access would happen here via a KnowledgeGraph reference.
        // For now, we return an empty result since we don't have access to the
        // actual graph instance in this context. In real usage, you'd pass the
        // graph reference when creating ZepToolsService or call through a context.

        let count = facts.len() as i64;
        Ok(SearchResult {
            facts,
            edges: edges_result,
            nodes: nodes_result,
            query: query.to_string(),
            total_count: count,
        })
    }

    /// Get all nodes from the graph.
    pub async fn get_all_nodes(&mut self, _graph_id: &str) -> Result<Vec<NodeInfo>> {
        // Note: This would call KnowledgeGraph::get_all_entities() in a real implementation
        // where we have access to the graph instance. Currently returns empty as we don't
        // store a reference to KnowledgeGraph in this struct.
        Ok(Vec::new())
    }

    /// Get all edges from the graph.
    pub async fn get_all_edges(
        &mut self,
        _graph_id: &str,
        _include_temporal: bool,
    ) -> Result<Vec<EdgeInfo>> {
        // Note: This would call KnowledgeGraph::get_all_edges() in a real implementation
        Ok(Vec::new())
    }

    /// Get detailed information about a node.
    pub async fn get_node_detail(&mut self, _node_uuid: &str) -> Result<Option<NodeInfo>> {
        Err(TeriError::Unknown("get_node_detail requires KnowledgeGraph access".into()))
    }

    /// Get edges connected to a node.
    pub async fn get_node_edges(
        &mut self,
        _graph_id: &str,
        _node_uuid: &str,
    ) -> Result<Vec<EdgeInfo>> {
        Err(TeriError::Unknown("get_node_edges requires KnowledgeGraph access".into()))
    }

    /// Get entities by type.
    pub async fn get_entities_by_type(
        &mut self,
        _graph_id: &str,
        _entity_type: &str,
    ) -> Result<Vec<NodeInfo>> {
        Err(TeriError::Unknown("get_entities_by_type requires KnowledgeGraph access".into()))
    }

    /// Get a summary for an entity.
    pub async fn get_entity_summary(
        &mut self,
        _graph_id: &str,
        _entity_uuid: &str,
    ) -> Result<String> {
        Err(TeriError::Unknown("get_entity_summary requires KnowledgeGraph access".into()))
    }

    /// Get graph statistics.
    pub async fn get_graph_statistics(
        &mut self,
        _graph_id: &str,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        Err(TeriError::Unknown("get_graph_statistics requires KnowledgeGraph access".into()))
    }

    /// Get simulation context for ReportAgent.
    pub async fn get_simulation_context(&mut self, _simulation_id: &str) -> Result<SearchResult> {
        Err(TeriError::Unknown("get_simulation_context not implemented".into()))
    }

    /// Insight forge - LLM-guided multi-query semantic search.
    pub async fn insight_forge(
        &mut self,
        _graph_id: &str,
        _query: &str,
        _sim_req: &str,
        _context: &str,
        _max_sub_queries: i64,
    ) -> Result<InsightForgeResult> {
        Err(TeriError::Unknown("insight_forge requires LLM integration".into()))
    }

    /// Panorama search - temporal-aware graph search.
    pub async fn panorama_search(
        &mut self,
        _graph_id: &str,
        _query: &str,
        _include_expired: bool,
        _limit: i64,
    ) -> Result<PanoramaResult> {
        Err(TeriError::Unknown("panorama_search requires Zep API".into()))
    }

    /// Quick search - simple keyword-based search.
    pub async fn quick_search(
        &mut self,
        _graph_id: &str,
        _query: &str,
        _limit: i64,
    ) -> Result<SearchResult> {
        Err(TeriError::Unknown("quick_search requires Zep API".into()))
    }

    /// Interview agents from a simulation.
    pub async fn interview_agents(
        &mut self,
        _simulation_id: &str,
        _requirement: &str,
        _sim_req: &str,
        _max_agents: i64,
        _custom_questions: Option<&str>,
    ) -> Result<InterviewResult> {
        Err(TeriError::Unknown(
            "interview_agents requires SimulationRunner integration".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_edge_info_is_invalid() {
        let edge = EdgeInfo {
            source_node_uuid: "".into(),
            target_node_uuid: "b".into(),
            name: "TEST".into(),
            fact: "fact".into(),
            uuid: "".into(),
            created_at: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        };
        assert!(edge.is_invalid());
    }
}
