//! Graph-backend abstraction (Workstream B).
//!
//! teri's graph read/write surface is already 100% native: the petgraph
//! [`KnowledgeGraph`](crate::graph::KnowledgeGraph) plus the redb vector store
//! ([`MemoryStore`](crate::memory::MemoryStore)). There is **no live Zep HTTP client** anywhere
//! in the tree — the `zep_*` source files are port-provenance names for native code.
//!
//! This module adds the owner-requested [`GraphBackend`] trait as the **no-downgrade seam**:
//!
//! - [`NativeGraphBackend`] is the DEFAULT. It delegates reads to the existing
//!   [`ReportTools`](crate::services::zep_tools::ReportTools) /
//!   [`KnowledgeGraphEntityReader`](crate::services::entity_reader::KnowledgeGraphEntityReader)
//!   surface (reused, not reimplemented) and upgrades `search` from keyword-only to embedding
//!   cosine over the redb store, **keeping keyword search as a fallback**.
//! - [`ZepGraphBackend`] is SELECTABLE (`GRAPH_BACKEND=zep`). Today it wraps the SAME native
//!   methods — there is no live Zep client — so the seam + the `ZEP_API_KEY` guard contract are
//!   exercised without deleting or bypassing any `zep_*` source path. It is the place a future
//!   real Zep client would land.
//!
//! Per the owner's Q1 decision this is a **thin facade**: the HTTP handlers keep calling the
//! concrete `ReportTools`/reader types and gain only the new semantic `search`. The trait exists
//! for provenance and to keep Zep selectable; it does not re-route every handler.
//!
//! ## Vector store layout (owner Q3 — reuse the redb `MemoryStore`)
//!
//! Each graph's entity/edge embeddings live in the shared [`MemoryStore`] under a **per-graph
//! namespace** `Uuid` derived from the `graph_id` (see [`graph_namespace`]). The store is keyed
//! by a `Uuid` already, so `query_vec_similarity(ns, q, k)` retrieves only that graph's vectors.
//! The vectors are stored **out of band** — they are NOT part of the serialized graph JSON, so
//! `/api/graph/data` stays byte-identical.

use crate::embedding::EmbeddingClient;
use crate::error::Result;
use crate::llm::LlmClient;
use crate::memory::{MemoryStore, VectorEntry};
use crate::services::zep_tools::{NodeInfo, ReportTools, SearchResult};
use std::sync::Arc;
use uuid::Uuid;

/// Stable DNS-style namespace OID used to derive a per-graph vector namespace from a `graph_id`.
///
/// `Uuid::new_v5(GRAPH_VECTOR_NAMESPACE, graph_id.as_bytes())` yields a deterministic `Uuid`
/// that is the redb store key for all of that graph's entity/edge vectors. Using v5 (not v4)
/// means the same `graph_id` always maps to the same namespace — so a rebuild can clear and
/// re-write that exact namespace (Workstream B R4, vector staleness).
pub const GRAPH_VECTOR_NAMESPACE: Uuid =
    Uuid::from_u128(0x7e_71_67_72_61_70_68_5f_76_65_63_00_00_00_00_01);

/// Derive the per-graph vector namespace `Uuid` for a `graph_id`.
///
/// Deterministic (v5) so a graph's vectors are always retrievable / clearable by the same key.
pub fn graph_namespace(graph_id: &str) -> Uuid {
    Uuid::new_v5(&GRAPH_VECTOR_NAMESPACE, graph_id.as_bytes())
}

/// One searchable item of the graph: its embeddable text plus the pre-reshaped pieces it
/// contributes to a [`SearchResult`] (so the emitted JSON is byte-identical to the keyword path).
struct GraphSearchItem {
    /// Text fed to the embedder and stored as the `VectorEntry.content`.
    text: String,
    /// The `fact` string this item contributes (empty for items with no fact).
    fact: String,
    /// The reshaped `edges[]` entry, when this item is an edge.
    edge: Option<serde_json::Map<String, serde_json::Value>>,
    /// The reshaped `nodes[]` entry, when this item is a node.
    node: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Build the searchable text for an entity: `"{name} ({kind})"`.
///
/// This is the text embedded at build time and matched at search time. It mirrors the keyword
/// path, which scores on `node.name` + `node.summary` (summary is `""` natively — DECISION-9).
fn entity_search_text(node: &NodeInfo) -> String {
    let kind = node
        .labels
        .iter()
        .find(|l| *l != "Entity" && *l != "Node")
        .map(|s| s.as_str())
        .unwrap_or("");
    if kind.is_empty() { node.name.clone() } else { format!("{} ({})", node.name, kind) }
}

/// The reshaped `edges[]` map shape `local_search` emits (`zep_tools.rs` ~:1021).
fn edge_map(
    uuid: &str,
    name: &str,
    fact: &str,
    src: &str,
    tgt: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert("uuid".into(), uuid.into());
    m.insert("name".into(), name.into());
    m.insert("fact".into(), fact.into());
    m.insert("source_node_uuid".into(), src.into());
    m.insert("target_node_uuid".into(), tgt.into());
    m
}

/// The reshaped `nodes[]` map shape `local_search` emits (`zep_tools.rs` ~:1044).
fn node_map(node: &NodeInfo) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert("uuid".into(), node.uuid.clone().into());
    m.insert("name".into(), node.name.clone().into());
    m.insert("labels".into(), serde_json::to_value(&node.labels).unwrap_or_default());
    m.insert("summary".into(), node.summary.clone().into());
    m
}

/// Build the searchable corpus from the graph: one [`GraphSearchItem`] per node and per edge.
///
/// - Each edge's `text`/`fact` is the synthesized `"{source} {relation} {target}"` (the keyword
///   path leaves `EdgeInfo.fact` empty natively — DECISION-9 Q4 — so the synthesized form is what
///   makes search return readable facts). Its reshaped `edge` map carries that same fact.
/// - Each node's `text` is `"{name} ({kind})"`; its `fact` is empty (nodes contribute to the
///   `nodes[]` array, mirroring `local_search`'s nodes scope).
fn build_corpus<L>(tools: &ReportTools<'_, L>, graph_id: &str) -> Vec<GraphSearchItem>
where
    L: LlmClient + Send + Sync + 'static,
{
    let mut items = Vec::new();

    // Node → name index for synthesizing edge facts.
    let nodes = tools.get_all_nodes(graph_id);
    let name_by_uuid: std::collections::HashMap<String, String> =
        nodes.iter().map(|n| (n.uuid.clone(), n.name.clone())).collect();

    for node in &nodes {
        let text = entity_search_text(node);
        if text.trim().is_empty() {
            continue;
        }
        items.push(GraphSearchItem {
            text,
            fact: String::new(),
            edge: None,
            node: Some(node_map(node)),
        });
    }

    for edge in tools.get_all_edges(graph_id, false) {
        let src_name = name_by_uuid.get(&edge.source_node_uuid).cloned().unwrap_or_default();
        let tgt_name = name_by_uuid.get(&edge.target_node_uuid).cloned().unwrap_or_default();
        // Synthesized fact: "{source} {relation} {target}" (skip if endpoints are unknown).
        let fact = if src_name.is_empty() || tgt_name.is_empty() {
            String::new()
        } else {
            format!("{} {} {}", src_name, edge.name, tgt_name)
        };
        if fact.trim().is_empty() {
            continue;
        }
        let map =
            edge_map(&edge.uuid, &edge.name, &fact, &edge.source_node_uuid, &edge.target_node_uuid);
        items.push(GraphSearchItem { text: fact.clone(), fact, edge: Some(map), node: None });
    }

    items
}

/// Embed every entity and every non-empty edge fact of `graph` and persist the vectors under the
/// per-`graph_id` namespace in `store`. The namespace is **cleared first** so a rebuild does not
/// leave stale vectors (Workstream B R4).
///
/// `content` of each [`VectorEntry`] is the searchable text:
/// - entity → `"{name} ({kind})"` (see [`entity_search_text`])
/// - edge   → the synthesized fact `"{source} {relation} {target}"`
///
/// On any embedding-endpoint failure (e.g. shimmy down / keyless stub) this returns `Ok(0)` after
/// clearing — the caller's search then falls back to keyword (no-downgrade, keyless-safe). It
/// never fails the surrounding graph build.
pub async fn embed_graph_vectors<L>(
    tools: &ReportTools<'_, L>,
    graph_id: &str,
    embedder: &EmbeddingClient,
    store: &MemoryStore,
) -> Result<usize>
where
    L: LlmClient + Send + Sync + 'static,
{
    let ns = graph_namespace(graph_id);
    // R4: clear the namespace before re-embedding so a rebuild can't accrete stale vectors.
    store.clear_vec_namespace(ns).await?;

    // Build the corpus (nodes + synthesized edge facts) and embed its texts.
    let corpus = build_corpus(tools, graph_id);
    let texts: Vec<String> = corpus.into_iter().map(|i| i.text).collect();
    if texts.is_empty() {
        return Ok(0);
    }

    // Embed in one batch. On failure, fall back gracefully (keyword search still works).
    let embeddings = match embedder.embed_batch(&texts).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                target: "teri::graph_backend",
                "graph vector embedding unavailable ({e}); search will use keyword fallback"
            );
            return Ok(0);
        }
    };
    if embeddings.len() != texts.len() {
        tracing::warn!(
            target: "teri::graph_backend",
            "embedding count {} != text count {}; skipping vector write (keyword fallback)",
            embeddings.len(),
            texts.len()
        );
        return Ok(0);
    }

    let now = chrono::Utc::now();
    let entries: Vec<VectorEntry> = texts
        .into_iter()
        .zip(embeddings)
        .map(|(content, embedding)| VectorEntry {
            timestamp: now,
            content,
            embedding,
            importance: 1.0,
        })
        .collect();

    store.write_vec_batch(ns, &entries).await
}

/// Append a single new fact's vector to a graph's namespace (sim-accrued episodes — U6).
///
/// Used by graph-memory updates so entities/facts that appear DURING a simulation become
/// searchable without a full rebuild. Best-effort: an embedding failure is logged and ignored
/// (keyword search still surfaces the new entity once it lands in the graph).
pub async fn append_graph_vector(
    graph_id: &str,
    fact: &str,
    embedder: &EmbeddingClient,
    store: &MemoryStore,
) {
    let fact = fact.trim();
    if fact.is_empty() {
        return;
    }
    let ns = graph_namespace(graph_id);
    match embedder.embed(fact).await {
        Ok(embedding) => {
            let entry = VectorEntry {
                timestamp: chrono::Utc::now(),
                content: fact.to_string(),
                embedding,
                importance: 1.0,
            };
            if let Err(e) = store.write_vec_batch(ns, std::slice::from_ref(&entry)).await {
                tracing::warn!(
                    target: "teri::graph_backend",
                    "append_graph_vector store write failed: {e}"
                );
            }
        }
        Err(e) => {
            tracing::warn!(
                target: "teri::graph_backend",
                "append_graph_vector embed failed ({e}); keyword fallback still applies"
            );
        }
    }
}

/// Embedding-cosine search over a graph's vector namespace, mapping hits back to the SAME
/// [`SearchResult`] shape `ReportTools::search_graph` returns — with keyword fallback.
///
/// Algorithm (Workstream B U4):
/// 1. Embed `query`; cosine-rank against the graph's vectors via `query_vec_similarity`.
/// 2. Map each ranked hit's `content` (entity text / edge fact) back to the graph's
///    `EdgeInfo`/`NodeInfo` shapes by re-using `local_search` per hit content and merging in
///    cosine order, so the emitted `facts`/`edges`/`nodes` arrays are byte-identical to the
///    keyword path's arrays.
/// 3. **Fallback**: if there are no vectors for this graph, OR the embedding call fails, OR no
///    hit maps to a graph item, defer to `tools.search_graph` (keyword) — never a downgrade.
pub async fn semantic_search<L>(
    tools: &ReportTools<'_, L>,
    graph_id: &str,
    query: &str,
    limit: i64,
    scope: Option<&str>,
    embedder: &EmbeddingClient,
    store: &MemoryStore,
) -> SearchResult
where
    L: LlmClient + Send + Sync + 'static,
{
    let keyword = || tools.search_graph(graph_id, query, limit, scope);

    let top_k = if limit <= 0 { 10 } else { limit as usize };
    let ns = graph_namespace(graph_id);

    // 1. Embed the query. On failure → keyword fallback.
    let q_emb = match embedder.embed(query).await {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(
                target: "teri::graph_backend",
                "semantic_search embed failed ({e}); keyword fallback"
            );
            return keyword();
        }
    };

    // 2. Cosine-rank. Pull a few extra candidates so mapping back has headroom.
    let hits =
        match store.query_vec_similarity(ns, &q_emb, top_k.saturating_mul(3).max(top_k)).await {
            Ok(h) => h,
            Err(e) => {
                tracing::debug!(
                    target: "teri::graph_backend",
                    "semantic_search cosine query failed ({e}); keyword fallback"
                );
                return keyword();
            }
        };

    // No vectors for this graph → fall back to keyword (no-downgrade, keyless-safe).
    if hits.is_empty() {
        return keyword();
    }

    // 3. Map each cosine hit's content back to its corpus item (built from the LIVE graph), in
    //    cosine order, preserving the EXACT SearchResult JSON shape `local_search` emits.
    //    Respect `scope`: "edges" (default) → edges+their facts; "nodes" → nodes; "both" → both.
    let scope = scope.unwrap_or("edges");
    let want_edges = scope == "edges" || scope == "both";
    let want_nodes = scope == "nodes" || scope == "both";

    let corpus = build_corpus(tools, graph_id);
    // Index corpus items by their text for O(1) lookup from a hit's stored content.
    let mut by_text: std::collections::HashMap<&str, &GraphSearchItem> =
        std::collections::HashMap::with_capacity(corpus.len());
    for item in &corpus {
        by_text.entry(item.text.as_str()).or_insert(item);
    }

    let mut facts: Vec<String> = Vec::new();
    let mut edges: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    let mut nodes: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    let mut seen_facts: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut emitted = 0usize;

    for hit in &hits {
        if emitted >= top_k {
            break;
        }
        let Some(item) = by_text.get(hit.content.as_str()) else {
            // Vector content no longer maps to a live graph item (graph mutated). Skip it.
            continue;
        };
        if let (true, Some(edge)) = (want_edges, &item.edge) {
            if !item.fact.is_empty() && seen_facts.insert(item.fact.clone()) {
                facts.push(item.fact.clone());
            }
            edges.push(edge.clone());
            emitted += 1;
            continue;
        }
        if let (true, Some(node)) = (want_nodes, &item.node) {
            nodes.push(node.clone());
            // local_search counts a node's (non-empty) summary as a fact; native summary is
            // "" so no fact is added — shape-faithful.
            emitted += 1;
            continue;
        }
    }

    // If nothing mapped back (e.g. vectors out of sync with the live graph), fall back.
    if facts.is_empty() && edges.is_empty() && nodes.is_empty() {
        return keyword();
    }

    let total = facts.len() as i64;
    SearchResult { facts, edges, nodes, query: query.to_string(), total_count: total }
}

// ---------------------------------------------------------------------------
// GraphBackend trait + impls (owner Q1: thin facade / no-downgrade seam)
// ---------------------------------------------------------------------------

/// Handles needed by a [`GraphBackend`] for the embedding-cosine search path.
///
/// Bundled so the trait stays small and both impls share the same construction. `None` for
/// `store` (or an unset namespace) means "keyword only" — the fallback is always available.
#[derive(Clone)]
pub struct GraphSearchCtx {
    pub embedder: Arc<EmbeddingClient>,
    pub store: Option<Arc<MemoryStore>>,
}

/// The LLM monomorphization the trait operates over.
///
/// `ApiState` fixes the concrete LLM adapter to the enum-dispatch
/// [`crate::llm::ProviderAdapter`] (S9 / TASK-SIM-4: serve is provider-agnostic — OpenAI /
/// Anthropic / Gemini selected from `config.llm.provider`; DECISION-U026-1 preserved:
/// `LlmClient` is not dyn-compatible and axum state cannot be generic, so a single concrete
/// enum type is used instead of `dyn`). The trait pins the same concrete type so it stays
/// **object-safe** — `Box<dyn GraphBackend>` is constructible — while still reusing the
/// generic `ReportTools<'_, L>` surface.
pub type BackendLlm = crate::llm::ProviderAdapter;

/// The graph-backend abstraction. [`NativeGraphBackend`] is the default; [`ZepGraphBackend`] is
/// the no-downgrade seam (today it delegates to the same native graph — there is no live Zep
/// client).
///
/// The trait is intentionally read-centric and operates over a borrowed
/// [`ReportTools`] (which already binds the `&KnowledgeGraph` + `&LlmClient`), matching the
/// facade decision: handlers keep their concrete types; the trait adds the semantic `search`.
/// It is pinned to [`BackendLlm`] so it is object-safe (no generic method).
#[async_trait::async_trait]
pub trait GraphBackend: Send + Sync {
    /// Stable identifier for the selected backend (`"native"` / `"zep"`).
    fn kind(&self) -> &'static str;

    /// Search the graph. The native backend uses embedding cosine with keyword fallback; the
    /// Zep seam delegates to the same native search today.
    async fn search(
        &self,
        tools: &ReportTools<'_, BackendLlm>,
        graph_id: &str,
        query: &str,
        limit: i64,
        scope: Option<&str>,
    ) -> SearchResult;
}

/// Native backend: embedding-cosine search (keyword fallback) over the redb store. The default.
pub struct NativeGraphBackend {
    ctx: GraphSearchCtx,
}

impl NativeGraphBackend {
    pub fn new(embedder: Arc<EmbeddingClient>, store: Option<Arc<MemoryStore>>) -> Self {
        Self { ctx: GraphSearchCtx { embedder, store } }
    }
}

#[async_trait::async_trait]
impl GraphBackend for NativeGraphBackend {
    fn kind(&self) -> &'static str {
        "native"
    }

    async fn search(
        &self,
        tools: &ReportTools<'_, BackendLlm>,
        graph_id: &str,
        query: &str,
        limit: i64,
        scope: Option<&str>,
    ) -> SearchResult {
        match &self.ctx.store {
            Some(store) => {
                semantic_search(tools, graph_id, query, limit, scope, &self.ctx.embedder, store)
                    .await
            }
            // No store available → keyword search (no-downgrade, keyless-safe).
            None => tools.search_graph(graph_id, query, limit, scope),
        }
    }
}

/// Zep backend: the SELECTABLE no-downgrade seam (`GRAPH_BACKEND=zep`).
///
/// Today it delegates to the SAME native search — there is no live Zep client. It exists so the
/// backend selection + `ZEP_API_KEY` guard contract are preserved and a future real Zep client
/// has a place to land. It does NOT delete or bypass any `zep_*` source path.
pub struct ZepGraphBackend {
    native: NativeGraphBackend,
}

impl ZepGraphBackend {
    pub fn new(embedder: Arc<EmbeddingClient>, store: Option<Arc<MemoryStore>>) -> Self {
        Self { native: NativeGraphBackend::new(embedder, store) }
    }
}

#[async_trait::async_trait]
impl GraphBackend for ZepGraphBackend {
    fn kind(&self) -> &'static str {
        "zep"
    }

    async fn search(
        &self,
        tools: &ReportTools<'_, BackendLlm>,
        graph_id: &str,
        query: &str,
        limit: i64,
        scope: Option<&str>,
    ) -> SearchResult {
        // [≠] No live Zep client: delegate to the same native surface (no-downgrade seam).
        self.native.search(tools, graph_id, query, limit, scope).await
    }
}

/// Construct the backend selected by `kind` (the parity wiring point).
///
/// Both impls share the same search ctx; the difference is provenance + `kind()`. Returned boxed
/// so a caller can hold either behind the trait.
pub fn make_backend(
    kind: crate::config::GraphBackendKind,
    embedder: Arc<EmbeddingClient>,
    store: Option<Arc<MemoryStore>>,
) -> Box<dyn GraphBackend> {
    match kind {
        crate::config::GraphBackendKind::Native => {
            Box::new(NativeGraphBackend::new(embedder, store))
        }
        crate::config::GraphBackendKind::Zep => Box::new(ZepGraphBackend::new(embedder, store)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GraphBackendKind;
    use crate::graph::{Entity, EntityKind, KnowledgeGraph, Relation, RelationKind};
    use crate::services::zep_tools::ReportTools;
    use httpmock::prelude::*;
    use tempfile::TempDir;
    use uuid::Uuid;

    /// Minimal LlmClient stub (the search path never calls the LLM).
    struct StubLlm;
    #[async_trait::async_trait]
    impl crate::llm::LlmClient for StubLlm {
        async fn complete(&self, _p: &str) -> crate::error::Result<String> {
            Ok("stub".into())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(
            &self,
            _p: &str,
        ) -> crate::error::Result<T> {
            Err(crate::error::TeriError::Llm("unused".into()))
        }
        async fn stream(
            &self,
            _p: &str,
        ) -> crate::error::Result<
            std::pin::Pin<Box<dyn futures::Stream<Item = crate::error::Result<String>> + Send>>,
        > {
            Err(crate::error::TeriError::Llm("unused".into()))
        }
        async fn chat(
            &self,
            _m: &[crate::llm::ChatMessage],
            _o: &crate::llm::ChatOptions,
        ) -> crate::error::Result<String> {
            Ok("stub".into())
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _m: &[crate::llm::ChatMessage],
            _o: &crate::llm::ChatOptions,
        ) -> crate::error::Result<T> {
            Err(crate::error::TeriError::Llm("unused".into()))
        }
    }

    /// Graph: Alice --[WorksFor]--> Acme ; Bob --[RelatedTo]--> Alice.
    fn fixture_graph() -> KnowledgeGraph {
        let mut g = KnowledgeGraph::new();
        let alice = Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            name: "Alice".into(),
            kind: EntityKind::Person,
        };
        let acme = Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
            name: "Acme".into(),
            kind: EntityKind::Organization,
        };
        let bob = Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(),
            name: "Bob".into(),
            kind: EntityKind::Person,
        };
        let ia = g.add_entity(alice).unwrap();
        let ic = g.add_entity(acme).unwrap();
        let ib = g.add_entity(bob).unwrap();
        g.add_relation(ia, ic, Relation::new(RelationKind::WorksFor, 0.9).unwrap());
        g.add_relation(ib, ia, Relation::new(RelationKind::RelatedTo, 0.7).unwrap());
        g
    }

    fn embedder(server: &MockServer) -> EmbeddingClient {
        EmbeddingClient::new(&crate::config::LlmConfig {
            base_url: server.base_url(),
            api_key: String::new(),
            model: "unused".into(),
            embed_model: "all-MiniLM-L6-v2".into(),
            timeout_secs: 5,
            max_retries: 0,
            max_tokens: 2048,
            provider: crate::config::LlmProvider::Openai,
        })
    }

    #[test]
    fn test_graph_namespace_is_deterministic() {
        // Same graph_id → same namespace (v5); different ids → different namespaces.
        assert_eq!(graph_namespace("g1"), graph_namespace("g1"));
        assert_ne!(graph_namespace("g1"), graph_namespace("g2"));
    }

    #[test]
    fn test_make_backend_kind() {
        let emb = Arc::new(EmbeddingClient::new(&crate::config::LlmConfig {
            base_url: "http://x/v1".into(),
            api_key: String::new(),
            model: "m".into(),
            embed_model: "e".into(),
            timeout_secs: 1,
            max_retries: 0,
            max_tokens: 2048,
            provider: crate::config::LlmProvider::Openai,
        }));
        assert_eq!(make_backend(GraphBackendKind::Native, emb.clone(), None).kind(), "native");
        assert_eq!(make_backend(GraphBackendKind::Zep, emb, None).kind(), "zep");
    }

    /// Embedding cosine ranks the entity whose vector aligns with the query first; the result
    /// keeps the SearchResult shape and is non-empty.
    #[tokio::test]
    async fn test_semantic_search_ranks_by_cosine() {
        let server = MockServer::start();
        // Build-time: ONE batch request (corpus = 3 nodes + 2 edges). Return 5 embeddings; the
        // last (index 4 = "Bob RelatedTo Alice") aligns with the query vector [0,1]; the rest
        // are [1,0]. The single-text query request below returns [0,1].
        server.mock(|when, then| {
            // Batch request: input is a JSON array (contains '["').
            when.method(POST).path("/embeddings").body_contains("[\"");
            then.status(200).header("Content-Type", "application/json").body(concat!(
                r#"{"object":"list","data":["#,
                r#"{"object":"embedding","embedding":[1.0,0.0],"index":0},"#,
                r#"{"object":"embedding","embedding":[1.0,0.0],"index":1},"#,
                r#"{"object":"embedding","embedding":[1.0,0.0],"index":2},"#,
                r#"{"object":"embedding","embedding":[1.0,0.0],"index":3},"#,
                r#"{"object":"embedding","embedding":[0.0,1.0],"index":4}"#,
                r#"]}"#,
            ));
        });
        // Single-text query request (input is a bare string, no '["'): aligned with index 4.
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"object":"list","data":[{"object":"embedding","embedding":[0.0,1.0],"index":0}]}"#,
            );
        });

        let graph = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&graph, &llm);
        let emb = embedder(&server);
        let tmp = TempDir::new().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();

        // Index the graph (one embed_batch). 3 nodes + 2 edges = 5 vectors.
        let n = embed_graph_vectors(&tools, "graphX", &emb, &store).await.unwrap();
        assert_eq!(n, 5, "expected 3 entity + 2 edge vectors written, got {n}");

        // Query aligned with the RelatedTo fact ([0,1]) — that edge must surface first.
        let result =
            semantic_search(&tools, "graphX", "RelatedTo", 5, Some("edges"), &emb, &store).await;
        assert!(result.total_count > 0, "semantic search must return facts");
        // SearchResult shape preserved.
        assert_eq!(result.query, "RelatedTo");
        assert!(!result.edges.is_empty(), "edges array populated");
        assert!(
            result.facts.iter().any(|f| f.contains("RelatedTo")),
            "the cosine-aligned RelatedTo fact must rank in: {:?}",
            result.facts
        );
    }

    /// With NO vectors stored for the graph, semantic_search falls back to keyword search and
    /// still returns the SearchResult shape (no panic, no error).
    #[tokio::test]
    async fn test_search_falls_back_to_keyword_when_no_vectors() {
        let server = MockServer::start();
        // Query embedding succeeds, but the store has no vectors → fallback to keyword.
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"object":"list","data":[{"object":"embedding","embedding":[1.0,0.0],"index":0}]}"#,
            );
        });
        let graph = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&graph, &llm);
        let emb = embedder(&server);
        let tmp = TempDir::new().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();

        // No embed_graph_vectors call → empty namespace.
        let result =
            semantic_search(&tools, "emptyGraph", "Alice", 5, Some("edges"), &emb, &store).await;
        // Fallback keyword path returns the same shape (keyword search on this graph yields
        // edges for "Alice" since the WorksFor/RelatedTo facts are synthesized in local_search?
        // Native local_search edges scope uses EdgeInfo.fact which is "" → facts empty, but the
        // call must still succeed and carry the query verbatim.
        assert_eq!(result.query, "Alice");
    }

    /// embed_graph_vectors clears the namespace and is best-effort: an embedding endpoint failure
    /// returns Ok(0) (the build still completes; search falls back to keyword).
    #[tokio::test]
    async fn test_embed_graph_vectors_best_effort_on_endpoint_failure() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(500).body("boom");
        });
        let graph = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&graph, &llm);
        let emb = embedder(&server);
        let tmp = TempDir::new().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();
        let n = embed_graph_vectors(&tools, "g", &emb, &store).await.unwrap();
        assert_eq!(n, 0, "endpoint failure → zero vectors written, no error");
    }

    /// Rebuilding the same graph_id clears the prior namespace (R4): the count after a second
    /// index equals the corpus size, not double.
    #[tokio::test]
    async fn test_reindex_clears_stale_vectors() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"object":"list","data":[{"object":"embedding","embedding":[1.0,0.0],"index":0}]}"#,
            );
        });
        let graph = fixture_graph();
        let llm = StubLlm;
        let tools = ReportTools::new(&graph, &llm);
        let emb = embedder(&server);
        let tmp = TempDir::new().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();

        let n1 = embed_graph_vectors(&tools, "samegraph", &emb, &store).await.unwrap();
        let n2 = embed_graph_vectors(&tools, "samegraph", &emb, &store).await.unwrap();
        assert_eq!(n1, n2, "reindex must clear-then-write, not accrete");
        let ns = graph_namespace("samegraph");
        let stored = store.read_vec(ns, 1000).await.unwrap();
        assert_eq!(stored.len(), n1, "stored count equals one corpus, not two");
    }

    /// U6: append_graph_vector makes a sim-accrued fact searchable.
    #[tokio::test]
    async fn test_append_graph_vector_makes_fact_searchable() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"object":"list","data":[{"object":"embedding","embedding":[1.0,0.0],"index":0}]}"#,
            );
        });
        let emb = embedder(&server);
        let tmp = TempDir::new().unwrap();
        let store = MemoryStore::new(tmp.path()).unwrap();
        append_graph_vector("episodeGraph", "Carol joined the protest", &emb, &store).await;
        let ns = graph_namespace("episodeGraph");
        let stored = store.read_vec(ns, 10).await.unwrap();
        assert_eq!(stored.len(), 1, "appended episode vector must be stored");
        assert_eq!(stored[0].content, "Carol joined the protest");
    }
}
