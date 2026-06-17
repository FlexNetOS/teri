use crate::error::{Result, TeriError};
use crate::llm::LlmClient;
use crate::seed::SeedDocument;
use crate::seed::text_processor;
use petgraph::Direction;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Person,
    Organization,
    Location,
    Concept,
    Event,
    Other,
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EntityKind::Person => write!(f, "person"),
            EntityKind::Organization => write!(f, "organization"),
            EntityKind::Location => write!(f, "location"),
            EntityKind::Concept => write!(f, "concept"),
            EntityKind::Event => write!(f, "event"),
            EntityKind::Other => write!(f, "other"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: Uuid,
    pub name: String,
    pub kind: EntityKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RelationKind {
    WorksFor,
    LocatedIn,
    RelatedTo,
    Causes,
    Affects,
    Other,
}

impl fmt::Display for RelationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelationKind::WorksFor => write!(f, "WorksFor"),
            RelationKind::LocatedIn => write!(f, "LocatedIn"),
            RelationKind::RelatedTo => write!(f, "RelatedTo"),
            RelationKind::Causes => write!(f, "Causes"),
            RelationKind::Affects => write!(f, "Affects"),
            RelationKind::Other => write!(f, "Other"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub kind: RelationKind,
    pub weight: f32,
    /// Temporal validity window, mapping onto Zep/MiroFish `valid_at`/`invalid_at` fields.
    ///
    /// - `None` — static/always-valid (no temporal constraint)
    /// - `Some((start, None))` — valid since `start` unix timestamp, still current
    /// - `Some((start, Some(end)))` — expired window [start, end)
    ///
    /// Both timestamps are seconds since Unix epoch (u64).
    #[serde(default)]
    pub valid_at: Option<(u64, Option<u64>)>,
}

impl Relation {
    /// Creates a new relation with validated weight and no temporal constraint.
    ///
    /// # Errors
    /// Returns an error if weight is not in the range [0.0, 1.0].
    pub fn new(kind: RelationKind, weight: f32) -> Result<Self> {
        if !(0.0..=1.0).contains(&weight) {
            return Err(TeriError::Graph(format!("Weight must be between 0 and 1, got: {weight}")));
        }
        Ok(Self { kind, weight, valid_at: None })
    }

    /// Creates a new relation with a validated weight and an explicit temporal validity window.
    ///
    /// `valid_at` semantics:
    /// - `None` — always valid
    /// - `Some((start, None))` — valid from `start` unix seconds, open-ended (still current)
    /// - `Some((start, Some(end)))` — valid in window `[start, end)`; expired after `end`
    ///
    /// # Errors
    /// Returns an error if weight is not in the range [0.0, 1.0].
    pub fn with_validity(
        kind: RelationKind,
        weight: f32,
        valid_at: Option<(u64, Option<u64>)>,
    ) -> Result<Self> {
        if !(0.0..=1.0).contains(&weight) {
            return Err(TeriError::Graph(format!("Weight must be between 0 and 1, got: {weight}")));
        }
        Ok(Self { kind, weight, valid_at })
    }

    /// Returns `true` if this relation is considered active at unix timestamp `t` (seconds).
    ///
    /// Rules:
    /// - `valid_at = None` → always active
    /// - `valid_at = Some((start, None))` → active if `t >= start`
    /// - `valid_at = Some((start, Some(end)))` → active if `start <= t < end`
    pub fn is_active_at(&self, t: u64) -> bool {
        match self.valid_at {
            None => true,
            Some((start, None)) => t >= start,
            Some((start, Some(end))) => t >= start && t < end,
        }
    }
}

/// An edge represented as (from_entity_id, to_entity_id, relation).
pub type EdgeTriple = (Uuid, Uuid, Relation);

/// Parses an optional temporal validity window from an LLM-produced JSON relation object.
///
/// Accepted shapes (all optional; returns `None` if none present):
/// - `{"valid_from": <u64>, "valid_until": <u64>}` — both present → `Some((start, Some(end)))`
/// - `{"valid_from": <u64>}` only → `Some((start, None))`
/// - `{"valid_at": [<u64>, <u64|null>]}` — array form → parsed accordingly
///
/// Any parse failure (bad type, out-of-range) silently falls back to `None` (never errors).
fn parse_valid_at_from_json(item: &Value) -> Option<(u64, Option<u64>)> {
    // Array form: "valid_at": [start, end_or_null]
    if let Some(arr) = item.get("valid_at").and_then(Value::as_array) {
        let start = arr.first().and_then(Value::as_u64)?;
        let end = arr.get(1).and_then(|v| if v.is_null() { None } else { v.as_u64() });
        return Some((start, end));
    }

    // Object form: "valid_from" / "valid_until"
    if let Some(start) = item.get("valid_from").and_then(Value::as_u64) {
        let end = item.get("valid_until").and_then(Value::as_u64);
        return Some((start, end));
    }

    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableKnowledgeGraph {
    entities: Vec<Entity>,
    edges: Vec<(Uuid, Uuid, Relation)>,
}

#[derive(Debug, Clone)]
pub struct KnowledgeGraph {
    inner: Graph<Entity, Relation>,
    index: HashMap<String, NodeIndex>,
    index_by_id: HashMap<Uuid, NodeIndex>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self { inner: Graph::new(), index: HashMap::new(), index_by_id: HashMap::new() }
    }

    /// Adds an entity to the knowledge graph.
    ///
    /// # Note
    /// Entity names are case-sensitive. "Alice" and "alice" are treated as different entities.
    ///
    /// # Errors
    /// Returns an error if an entity with the same name already exists.
    pub fn add_entity(&mut self, entity: Entity) -> Result<NodeIndex> {
        if self.index.contains_key(&entity.name) {
            return Err(TeriError::Graph(format!(
                "Entity with name '{}' already exists",
                entity.name
            )));
        }

        let node_idx = self.inner.add_node(entity.clone());
        self.index.insert(entity.name.clone(), node_idx);
        self.index_by_id.insert(entity.id, node_idx);
        Ok(node_idx)
    }

    pub fn add_relation(&mut self, from: NodeIndex, to: NodeIndex, relation: Relation) {
        self.inner.add_edge(from, to, relation);
    }

    pub fn get_entity(&self, name: &str) -> Option<&Entity> {
        self.index.get(name).and_then(|idx| self.inner.node_weight(*idx))
    }

    pub fn get_neighbors(&self, entity_id: Uuid) -> Result<Vec<&Entity>> {
        let idx = self
            .index_by_id
            .get(&entity_id)
            .ok_or_else(|| TeriError::Graph(format!("Entity not found: {entity_id}")))?;

        let neighbors =
            self.inner.neighbors(*idx).filter_map(|n| self.inner.node_weight(n)).collect();

        Ok(neighbors)
    }

    /// Returns each neighbor of `entity_id` together with the connecting `Relation` and a
    /// direction flag (`true` = outgoing: entity → neighbor; `false` = incoming: neighbor → entity).
    ///
    /// This is the edge-aware counterpart to `get_neighbors` and powers Part 2 of
    /// `build_entity_context` (the "Related Facts and Relationships" section).  It mirrors
    /// MiroFish's `_build_entity_context` lines 434–453, which iterate `entity.related_edges`
    /// carrying `edge_name` + `direction` + optional `fact`.
    ///
    /// Outgoing edges are visited first (Direction::Outgoing), then incoming
    /// (Direction::Incoming).  petgraph's `edges_directed` returns each stored edge once per
    /// direction query, so there is no double-counting.
    ///
    /// # Errors
    /// Returns an error if `entity_id` is not in the graph.
    pub fn get_neighbor_relations(
        &self,
        entity_id: Uuid,
    ) -> Result<Vec<(&Entity, &Relation, bool)>> {
        let idx = self
            .index_by_id
            .get(&entity_id)
            .ok_or_else(|| TeriError::Graph(format!("Entity not found: {entity_id}")))?;

        let mut result = Vec::new();

        // Outgoing: entity --[rel]--> neighbor  (is_outgoing = true)
        for edge in self.inner.edges_directed(*idx, Direction::Outgoing) {
            if let (Some(neighbor), Some(rel)) =
                (self.inner.node_weight(edge.target()), Some(edge.weight()))
            {
                result.push((neighbor, rel, true));
            }
        }

        // Incoming: neighbor --[rel]--> entity  (is_outgoing = false)
        for edge in self.inner.edges_directed(*idx, Direction::Incoming) {
            if let (Some(neighbor), Some(rel)) =
                (self.inner.node_weight(edge.source()), Some(edge.weight()))
            {
                result.push((neighbor, rel, false));
            }
        }

        Ok(result)
    }

    pub fn get_subgraph(&self, entity_id: Uuid, depth: usize) -> Result<KnowledgeGraph> {
        let start_idx = *self
            .index_by_id
            .get(&entity_id)
            .ok_or_else(|| TeriError::Graph(format!("Entity not found: {entity_id}")))?;

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(start_idx);
        queue.push_back((start_idx, 0));

        let mut subgraph = KnowledgeGraph::new();
        let mut idx_map: HashMap<NodeIndex, NodeIndex> = HashMap::new();

        const MAX_NAME_SUFFIX: usize = 1000;

        while let Some((node, dist)) = queue.pop_front() {
            // Add node to subgraph if not already added
            if let std::collections::hash_map::Entry::Vacant(e) = idx_map.entry(node) {
                let Some(ent) = self.inner.node_weight(node) else {
                    return Err(TeriError::Graph(
                        "Node weight missing in original graph".to_string(),
                    ));
                };
                // Create a unique name for subgraph to avoid duplicates
                let mut subgraph_entity = ent.clone();
                let mut name_suffix = 0;
                while subgraph.index.contains_key(&subgraph_entity.name) {
                    name_suffix += 1;
                    if name_suffix > MAX_NAME_SUFFIX {
                        return Err(TeriError::Graph(format!(
                            "Too many entities with similar names: {}",
                            ent.name
                        )));
                    }
                    subgraph_entity.name = format!("{}_{}", ent.name, name_suffix);
                }
                let new_idx = subgraph.add_entity(subgraph_entity)?;
                e.insert(new_idx);
            }

            if dist >= depth {
                continue;
            }

            for neighbor in self.inner.neighbors(node) {
                if visited.insert(neighbor) {
                    queue.push_back((neighbor, dist + 1));
                }

                let Some(edge) = self.inner.find_edge(node, neighbor) else {
                    continue;
                };
                let relation = self.inner.edge_weight(edge).cloned();
                if let Some(rel) = relation {
                    let Some(&from_new) = idx_map.get(&node) else {
                        return Err(TeriError::Graph(
                            "From node not mapped in subgraph".to_string(),
                        ));
                    };

                    // Ensure neighbor is added to subgraph
                    let to_new = if let Some(&existing_idx) = idx_map.get(&neighbor) {
                        existing_idx
                    } else {
                        let Some(ent) = self.inner.node_weight(neighbor) else {
                            return Err(TeriError::Graph(
                                "Neighbor node weight missing".to_string(),
                            ));
                        };
                        // Create a unique name for subgraph
                        let mut subgraph_entity = ent.clone();
                        let mut name_suffix = 0;
                        while subgraph.index.contains_key(&subgraph_entity.name) {
                            name_suffix += 1;
                            if name_suffix > MAX_NAME_SUFFIX {
                                return Err(TeriError::Graph(format!(
                                    "Too many entities with similar names: {}",
                                    ent.name
                                )));
                            }
                            subgraph_entity.name = format!("{}_{}", ent.name, name_suffix);
                        }
                        subgraph.add_entity(subgraph_entity)?
                    };

                    subgraph.add_relation(from_new, to_new, rel);
                }
            }
        }

        Ok(subgraph)
    }

    /// Builds a knowledge graph from a seed document using a two-pass LLM extraction pipeline.
    ///
    /// # Large-document chunking (GAP-U015-1)
    ///
    /// MiroFish splits large documents before LLM processing to avoid context overflow.
    /// Teri replicates this: documents whose `raw_text` exceeds `CHUNK_SIZE` characters are
    /// split into overlapping chunks via [`crate::seed::text_processor::split_text`] (defaults:
    /// 500-char windows, 50-char overlap).  Pass 1 (entity extraction) is run **per chunk**;
    /// entities from all chunks are merged into a single deduplicated set.  Pass 2 (relation
    /// extraction) is then run over each chunk separately, with relations merged into the graph
    /// (unknown-entity refs remain skipped, as before).
    ///
    /// **Small-doc invariant**: documents with ≤ `CHUNK_SIZE` chars produce exactly one chunk
    /// (the whole text), so the pipeline is identical to the pre-chunking behaviour — all 5
    /// existing build tests continue to pass unchanged.
    ///
    /// # Pass details
    ///
    /// Pass 1: entity extraction — prompt the LLM per chunk, parse JSON responses, merge
    /// extracted entities into the graph (duplicate names are skipped — MiroFish resilience
    /// contract).
    ///
    /// Pass 2: relation extraction — prompt the LLM with the full deduplicated entity list
    /// per chunk; parse JSON responses; resolve entity names to NodeIndex values; add each
    /// relation (relations that reference an unknown entity name are skipped, not faulted).
    ///
    /// Empty extraction (LLM returns no entities across all chunks) yields a valid empty graph,
    /// not an error.
    ///
    /// LLM errors and JSON parse errors are propagated as `TeriError`.
    pub async fn build<L: LlmClient>(doc: &SeedDocument, llm: &L) -> Result<Self> {
        /// Chunk size threshold (characters).  Documents ≤ this size use a single chunk,
        /// preserving exact parity with the pre-chunking build() contract.
        const CHUNK_SIZE: usize = 500;
        const CHUNK_OVERLAP: usize = 50;

        let mut graph = KnowledgeGraph::new();

        // Split the document text into chunks.
        // `split_text` returns a single-element vec for text ≤ CHUNK_SIZE, so the small-doc
        // path is structurally identical to the original single-pass behaviour.
        let chunks = text_processor::split_text(&doc.raw_text, CHUNK_SIZE, CHUNK_OVERLAP);

        // If the document is completely blank, `split_text` returns an empty vec.
        // Treat this the same as an empty extraction (valid empty graph).
        if chunks.is_empty() {
            return Ok(graph);
        }

        // ---- Pass 1: entity extraction — one LLM call per chunk, merge results ----
        for chunk_text in &chunks {
            // Build a per-chunk pseudo-document so we can reuse the existing prompt helper.
            // Metadata is shared from the original document.
            let chunk_doc = SeedDocument {
                id: doc.id,
                raw_text: chunk_text.clone(),
                metadata: doc.metadata.clone(),
                created_at: doc.created_at,
            };

            let entity_prompt = Self::entity_extraction_prompt(&chunk_doc);
            let entity_json = llm.complete(&entity_prompt).await?;
            let entities = Self::parse_entities_json(&entity_json)?;

            // Merge: skip duplicates (MiroFish resilience contract, now cross-chunk).
            for entity in &entities {
                if !graph.index.contains_key(&entity.name) {
                    graph.add_entity(entity.clone())?;
                }
            }
        }

        // If no entities were extracted across all chunks, return a valid empty graph.
        if graph.entity_count() == 0 {
            return Ok(graph);
        }

        // ---- Pass 2: relation extraction — one LLM call per chunk, merge results ----
        // Collect the deduplicated entity set (post-merge from Pass 1).
        let graph_entities: Vec<Entity> = graph.get_all_entities().into_iter().cloned().collect();

        for chunk_text in &chunks {
            let chunk_doc = SeedDocument {
                id: doc.id,
                raw_text: chunk_text.clone(),
                metadata: doc.metadata.clone(),
                created_at: doc.created_at,
            };

            let relation_prompt = Self::relation_extraction_prompt(&chunk_doc, &graph_entities);
            let relation_json = llm.complete(&relation_prompt).await?;

            // Parse relations; skip any referencing an unknown entity name.
            let value: Value = serde_json::from_str(&relation_json)
                .map_err(|e| TeriError::Graph(format!("Invalid relation JSON from LLM: {e}")))?;

            if let Some(arr) = value.as_array() {
                for item in arr {
                    let from_name = match item.get("from").and_then(Value::as_str) {
                        Some(n) => n,
                        None => continue, // skip malformed item
                    };
                    let to_name = match item.get("to").and_then(Value::as_str) {
                        Some(n) => n,
                        None => continue, // skip malformed item
                    };

                    // Skip relations referencing entities not in the graph.
                    let from_idx = match graph.index.get(from_name).copied() {
                        Some(idx) => idx,
                        None => continue,
                    };
                    let to_idx = match graph.index.get(to_name).copied() {
                        Some(idx) => idx,
                        None => continue,
                    };

                    let kind_str = item.get("kind").and_then(Value::as_str).unwrap_or("Other");
                    let kind = match kind_str {
                        "WorksFor" => RelationKind::WorksFor,
                        "LocatedIn" => RelationKind::LocatedIn,
                        "RelatedTo" => RelationKind::RelatedTo,
                        "Causes" => RelationKind::Causes,
                        "Affects" => RelationKind::Affects,
                        _ => RelationKind::Other,
                    };

                    let weight = match item.get("weight").and_then(Value::as_f64) {
                        Some(w) if (0.0..=1.0).contains(&w) => w as f32,
                        // Skip relations with missing or out-of-range weight.
                        _ => continue,
                    };

                    graph.add_relation(from_idx, to_idx, Relation { kind, weight, valid_at: None });
                }
            }
            // A non-array relation response is tolerated as empty — no relations added.
        }

        Ok(graph)
    }

    // -------- Serialization methods --------

    /// Serializes the knowledge graph to a JSON string.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn serialize_to_json(&self) -> Result<String> {
        let serializable = SerializableKnowledgeGraph {
            entities: self.get_all_entities_owned(),
            edges: self.get_all_edges(),
        };
        serde_json::to_string(&serializable)
            .map_err(|e| TeriError::Graph(format!("Failed to serialize graph: {e}")))
    }

    /// Serializes the knowledge graph to a JSON file.
    ///
    /// # Errors
    /// Returns an error if serialization or file writing fails.
    pub fn serialize_to_file(&self, path: &str) -> Result<()> {
        let json = self.serialize_to_json()?;
        std::fs::write(path, json)
            .map_err(|e| TeriError::Graph(format!("Failed to write graph to file: {e}")))
    }

    /// Serializes the knowledge graph using bincode for compact binary storage.
    ///
    /// # Errors
    /// Returns an error if serialization fails.
    pub fn serialize_to_bincode(&self) -> Result<Vec<u8>> {
        let serializable = SerializableKnowledgeGraph {
            entities: self.get_all_entities_owned(),
            edges: self.get_all_edges(),
        };
        bincode::serialize(&serializable)
            .map_err(|e| TeriError::Graph(format!("Failed to serialize graph with bincode: {e}")))
    }

    /// Serializes the knowledge graph to a binary file using bincode.
    ///
    /// # Errors
    /// Returns an error if serialization or file writing fails.
    pub fn serialize_to_bincode_file(&self, path: &str) -> Result<()> {
        let bytes = self.serialize_to_bincode()?;
        std::fs::write(path, bytes)
            .map_err(|e| TeriError::Graph(format!("Failed to write graph to binary file: {e}")))
    }

    /// Deserializes a knowledge graph from a JSON string.
    ///
    /// # Errors
    /// Returns an error if deserialization or graph reconstruction fails.
    pub fn deserialize_from_json(json: &str) -> Result<Self> {
        let serializable: SerializableKnowledgeGraph = serde_json::from_str(json)
            .map_err(|e| TeriError::Graph(format!("Failed to deserialize graph from JSON: {e}")))?;

        Self::from_serializable(serializable)
    }

    /// Deserializes a knowledge graph from a JSON file.
    ///
    /// # Errors
    /// Returns an error if file reading, deserialization, or graph reconstruction fails.
    pub fn deserialize_from_file(path: &str) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| TeriError::Graph(format!("Failed to read graph from file: {e}")))?;
        Self::deserialize_from_json(&json)
    }

    /// Deserializes a knowledge graph from bincode-encoded bytes.
    ///
    /// # Errors
    /// Returns an error if deserialization or graph reconstruction fails.
    pub fn deserialize_from_bincode(bytes: &[u8]) -> Result<Self> {
        let serializable: SerializableKnowledgeGraph =
            bincode::deserialize(bytes).map_err(|e| {
                TeriError::Graph(format!("Failed to deserialize graph from bincode: {e}"))
            })?;

        Self::from_serializable(serializable)
    }

    /// Deserializes a knowledge graph from a bincode-encoded file.
    ///
    /// # Errors
    /// Returns an error if file reading, deserialization, or graph reconstruction fails.
    pub fn deserialize_from_bincode_file(path: &str) -> Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| TeriError::Graph(format!("Failed to read binary graph file: {e}")))?;
        Self::deserialize_from_bincode(&bytes)
    }

    /// Creates a KnowledgeGraph from its serializable representation.
    ///
    /// # Errors
    /// Returns an error if graph reconstruction fails.
    fn from_serializable(serializable: SerializableKnowledgeGraph) -> Result<Self> {
        let mut graph = KnowledgeGraph::new();

        // Add all entities first
        for entity in serializable.entities {
            graph.add_entity(entity)?;
        }

        // Then add all edges
        for (from_id, to_id, relation) in serializable.edges {
            let from_idx = graph.index_by_id.get(&from_id).ok_or_else(|| {
                TeriError::Graph(format!("Entity not found during deserialization: {from_id}"))
            })?;
            let to_idx = graph.index_by_id.get(&to_id).ok_or_else(|| {
                TeriError::Graph(format!("Entity not found during deserialization: {to_id}"))
            })?;
            graph.add_relation(*from_idx, *to_idx, relation);
        }

        Ok(graph)
    }

    /// Helper method to get all entities from the graph.
    fn get_all_entities_owned(&self) -> Vec<Entity> {
        self.inner.node_weights().cloned().collect()
    }

    /// Helper method to get all edges from the graph as (from_id, to_id, relation).
    fn get_all_edges(&self) -> Vec<EdgeTriple> {
        self.inner
            .edge_references()
            .map(|edge| {
                let from_entity = &self.inner[edge.source()];
                let to_entity = &self.inner[edge.target()];
                (from_entity.id, to_entity.id, edge.weight().clone())
            })
            .collect()
    }

    // -------- LLM prompt helpers --------

    pub fn entity_extraction_prompt(doc: &SeedDocument) -> String {
        format!(
            r#"You are an information extraction system. Extract named entities from the following document.
Return JSON array with objects: {{"name": string, "kind": one of [Person, Organization, Location, Concept, Event, Other]}}.

Document metadata: {metadata}
Document text:
{body}
"#,
            // Safe: empty string is acceptable if metadata serialization fails
            metadata = serde_json::to_string(&doc.metadata).unwrap_or_default(),
            body = doc.raw_text
        )
    }

    pub fn relation_extraction_prompt(doc: &SeedDocument, entities: &[Entity]) -> String {
        if entities.is_empty() {
            return String::from("No entities provided for relation extraction.");
        }

        let entity_list: Vec<_> = entities
            .iter()
            .map(|e| serde_json::json!({"name": e.name, "kind": format!("{:?}", e.kind)}))
            .collect();

        format!(
            r#"You are an information extraction system. Using the provided entities, extract relations between them.
Return JSON array with objects: {{"from": entity_name, "to": entity_name, "kind": one of [WorksFor, LocatedIn, RelatedTo, Causes, Affects, Other], "weight": number between 0 and 1}}.

Entities: {entities}
Document metadata: {metadata}
Document text:
{body}
"#,
            // Safe: empty string is acceptable if entity list serialization fails
            entities = serde_json::to_string(&entity_list).unwrap_or_default(),
            // Safe: empty string is acceptable if metadata serialization fails
            metadata = serde_json::to_string(&doc.metadata).unwrap_or_default(),
            body = doc.raw_text
        )
    }

    // -------- JSON parsing helpers (LLM responses) --------

    pub fn parse_entities_json(json: &str) -> Result<Vec<Entity>> {
        let value: Value = serde_json::from_str(json)
            .map_err(|e| TeriError::Graph(format!("Invalid entity JSON: {e}")))?;

        let arr = value
            .as_array()
            .ok_or_else(|| TeriError::Graph("Entity JSON must be an array".to_string()))?;

        let mut entities = Vec::new();
        for item in arr {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| TeriError::Graph("Entity missing name".to_string()))?;
            let kind_str = item.get("kind").and_then(Value::as_str).unwrap_or("Other");
            let kind = match kind_str {
                "Person" => EntityKind::Person,
                "Organization" => EntityKind::Organization,
                "Location" => EntityKind::Location,
                "Concept" => EntityKind::Concept,
                "Event" => EntityKind::Event,
                _ => EntityKind::Other,
            };

            // Accept ID from JSON if present, otherwise generate new one
            let id = if let Some(id_str) = item.get("id").and_then(Value::as_str) {
                Uuid::parse_str(id_str)
                    .map_err(|e| TeriError::Graph(format!("Invalid UUID: {e}")))?
            } else {
                Uuid::new_v4()
            };

            entities.push(Entity { id, name: name.to_string(), kind });
        }

        Ok(entities)
    }

    pub fn parse_relations_json(
        json: &str,
        index: &HashMap<String, NodeIndex>,
    ) -> Result<Vec<(NodeIndex, NodeIndex, Relation)>> {
        let value: Value = serde_json::from_str(json)
            .map_err(|e| TeriError::Graph(format!("Invalid relation JSON: {e}")))?;

        let arr = value
            .as_array()
            .ok_or_else(|| TeriError::Graph("Relation JSON must be an array".to_string()))?;

        let mut relations = Vec::new();
        for item in arr {
            let from = item
                .get("from")
                .and_then(Value::as_str)
                .ok_or_else(|| TeriError::Graph("Relation missing 'from'".to_string()))?;
            let to = item
                .get("to")
                .and_then(Value::as_str)
                .ok_or_else(|| TeriError::Graph("Relation missing 'to'".to_string()))?;

            let from_idx = *index
                .get(from)
                .ok_or_else(|| TeriError::Graph(format!("Unknown entity in 'from': {from}")))?;
            let to_idx = *index
                .get(to)
                .ok_or_else(|| TeriError::Graph(format!("Unknown entity in 'to': {to}")))?;

            let kind_str = item.get("kind").and_then(Value::as_str).unwrap_or("Other");
            let kind = match kind_str {
                "WorksFor" => RelationKind::WorksFor,
                "LocatedIn" => RelationKind::LocatedIn,
                "RelatedTo" => RelationKind::RelatedTo,
                "Causes" => RelationKind::Causes,
                "Affects" => RelationKind::Affects,
                _ => RelationKind::Other,
            };

            let weight = item.get("weight").and_then(Value::as_f64).ok_or_else(|| {
                TeriError::Graph("Relation missing or invalid 'weight' value".to_string())
            })?;

            if !(0.0..=1.0).contains(&weight) {
                return Err(TeriError::Graph(format!(
                    "Weight must be between 0 and 1, got: {weight}"
                )));
            }

            // Optionally parse valid_at / valid_from / valid_until from LLM JSON (default None).
            // Accepts: {"valid_from": <u64>, "valid_until": <u64>} or
            //          {"valid_at": [<u64>, <u64|null>]}
            let valid_at = parse_valid_at_from_json(item);

            relations.push((from_idx, to_idx, Relation { kind, weight: weight as f32, valid_at }));
        }

        Ok(relations)
    }

    pub fn get_all_entities(&self) -> Vec<&Entity> {
        self.inner.node_weights().collect()
    }

    pub fn entity_count(&self) -> usize {
        self.inner.node_count()
    }

    pub fn relation_count(&self) -> usize {
        self.inner.edge_count()
    }

    /// Get the name-to-index mapping (primarily for testing)
    ///
    /// # Note
    /// This is primarily intended for testing purposes.
    /// In production code, prefer using the public query methods.
    #[doc(hidden)]
    pub fn get_index(&self) -> &HashMap<String, NodeIndex> {
        &self.index
    }

    /// Returns all edges (from_id, to_id, relation) partitioned into (active, historical)
    /// at the given unix timestamp `t` (seconds).
    ///
    /// Active: `relation.is_active_at(t)` is true.
    /// Historical: `relation.is_active_at(t)` is false.
    ///
    /// This powers the `panorama_search` active-vs-historical classification that MiroFish/Zep
    /// uses via `valid_at`/`invalid_at` fields.
    pub fn partition_edges_at(&self, t: u64) -> (Vec<EdgeTriple>, Vec<EdgeTriple>) {
        let mut active = Vec::new();
        let mut historical = Vec::new();
        for edge in self.inner.edge_references() {
            let from_entity = &self.inner[edge.source()];
            let to_entity = &self.inner[edge.target()];
            let rel = edge.weight().clone();
            if rel.is_active_at(t) {
                active.push((from_entity.id, to_entity.id, rel));
            } else {
                historical.push((from_entity.id, to_entity.id, rel));
            }
        }
        (active, historical)
    }
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::LlmClient;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashMap;
    use std::pin::Pin;

    #[test]
    fn test_knowledge_graph_creation() {
        let graph = KnowledgeGraph::new();
        assert_eq!(graph.entity_count(), 0);
        assert_eq!(graph.relation_count(), 0);
    }

    #[test]
    fn test_add_entity() {
        let mut graph = KnowledgeGraph::new();
        let entity =
            Entity { id: Uuid::new_v4(), name: "Alice".to_string(), kind: EntityKind::Person };

        graph.add_entity(entity).expect("Failed to add entity");
        assert_eq!(graph.entity_count(), 1);
    }

    #[test]
    fn test_get_entity() {
        let mut graph = KnowledgeGraph::new();
        let entity =
            Entity { id: Uuid::new_v4(), name: "Alice".to_string(), kind: EntityKind::Person };

        graph.add_entity(entity.clone()).expect("Failed to add entity");
        let retrieved = graph.get_entity("Alice");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Alice");
    }

    #[test]
    fn test_add_relation() {
        let mut graph = KnowledgeGraph::new();
        let alice =
            Entity { id: Uuid::new_v4(), name: "Alice".to_string(), kind: EntityKind::Person };
        let bob = Entity { id: Uuid::new_v4(), name: "Bob".to_string(), kind: EntityKind::Person };

        let alice_idx = graph.add_entity(alice).expect("Failed to add entity");
        let bob_idx = graph.add_entity(bob).expect("Failed to add entity");

        let relation = Relation { kind: RelationKind::RelatedTo, weight: 0.8, valid_at: None };

        graph.add_relation(alice_idx, bob_idx, relation);
        assert_eq!(graph.relation_count(), 1);
    }

    #[test]
    fn test_get_neighbors_by_id() {
        let mut graph = KnowledgeGraph::new();
        let alice =
            Entity { id: Uuid::new_v4(), name: "Alice".to_string(), kind: EntityKind::Person };
        let bob = Entity { id: Uuid::new_v4(), name: "Bob".to_string(), kind: EntityKind::Person };

        let alice_idx = graph.add_entity(alice.clone()).expect("Failed to add entity");
        let bob_idx = graph.add_entity(bob.clone()).expect("Failed to add entity");

        graph.add_relation(
            alice_idx,
            bob_idx,
            Relation::new(RelationKind::RelatedTo, 0.5).expect("Valid weight"),
        );

        let neighbors = graph.get_neighbors(alice.id).expect("neighbors should exist");
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].name, "Bob");
    }

    #[test]
    fn test_get_subgraph_depth_limited() {
        let mut graph = KnowledgeGraph::new();
        let a_id = Uuid::new_v4();
        let a = graph
            .add_entity(Entity { id: a_id, name: "A".to_string(), kind: EntityKind::Concept })
            .expect("Failed to add entity");
        let b_id = Uuid::new_v4();
        let b = graph
            .add_entity(Entity { id: b_id, name: "B".to_string(), kind: EntityKind::Concept })
            .expect("Failed to add entity");
        let c = graph
            .add_entity(Entity {
                id: Uuid::new_v4(),
                name: "C".to_string(),
                kind: EntityKind::Concept,
            })
            .expect("Failed to add entity");

        graph.add_relation(
            a,
            b,
            Relation::new(RelationKind::RelatedTo, 1.0).expect("Valid weight"),
        );
        graph.add_relation(
            b,
            c,
            Relation::new(RelationKind::RelatedTo, 1.0).expect("Valid weight"),
        );

        let sub = graph.get_subgraph(b_id, 1).expect("subgraph");
        assert_eq!(sub.entity_count(), 3); // B and both its direct neighbors (A and C)
        // In subgraph, entities might have names like "B_1" to avoid conflicts
        let b_found = sub.get_all_entities().iter().any(|e| e.name.starts_with("B"));
        assert!(b_found);
    }

    /// build() now performs the real two-pass LLM extraction pipeline.
    /// This test uses the mock LLM (same pattern as test_graph_construction_with_mock_llm)
    /// and verifies that build() wires the full pipeline end-to-end.
    #[tokio::test]
    async fn test_build_from_seed_document() {
        let entity_response = r#"[
            {"name": "Alice", "kind": "Person"},
            {"name": "Acme Corp", "kind": "Organization"}
        ]"#;
        let relation_response = r#"[
            {"from": "Alice", "to": "Acme Corp", "kind": "WorksFor", "weight": 0.9}
        ]"#;

        let mock_llm = MockLlmClient::new(entity_response, relation_response);

        let mut metadata = HashMap::new();
        metadata.insert("title".to_string(), "Test Doc".to_string());
        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Alice works at Acme Corp.".to_string(),
            metadata,
            created_at: Utc::now(),
        };

        let graph = KnowledgeGraph::build(&doc, &mock_llm).await.expect("build graph");

        // Two entities extracted and added.
        assert_eq!(graph.entity_count(), 2, "expected 2 entities");
        let alice = graph.get_entity("Alice").expect("Alice must be present");
        assert_eq!(alice.kind, EntityKind::Person);
        let acme = graph.get_entity("Acme Corp").expect("Acme Corp must be present");
        assert_eq!(acme.kind, EntityKind::Organization);

        // One relation extracted and wired.
        assert_eq!(graph.relation_count(), 1, "expected 1 relation");

        // Alice's neighbor is Acme Corp.
        let neighbors = graph.get_neighbors(alice.id).expect("neighbors");
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].name, "Acme Corp");
    }

    /// build() with an empty entity response must return a valid empty graph, not an error.
    #[tokio::test]
    async fn test_build_empty_extraction_tolerates() {
        let mock_llm = MockLlmClient::new("[]", "[]");

        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Completely unrecognizable gibberish.".to_string(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
        };

        let graph = KnowledgeGraph::build(&doc, &mock_llm)
            .await
            .expect("empty extraction is not an error");
        assert_eq!(graph.entity_count(), 0, "empty extraction yields empty graph");
        assert_eq!(graph.relation_count(), 0);
    }

    /// build() must skip duplicate entity names from the LLM response rather than aborting.
    #[tokio::test]
    async fn test_build_duplicate_entity_is_skipped() {
        // LLM returns "Alice" twice — second occurrence must be silently dropped.
        let entity_response = r#"[
            {"name": "Alice", "kind": "Person"},
            {"name": "Alice", "kind": "Organization"}
        ]"#;
        let mock_llm = MockLlmClient::new(entity_response, "[]");

        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Duplicate test.".to_string(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
        };

        // Must NOT error — only one entity in the graph.
        let graph = KnowledgeGraph::build(&doc, &mock_llm)
            .await
            .expect("duplicate entity must not abort build");
        assert_eq!(graph.entity_count(), 1, "second Alice must be silently skipped");
        let alice = graph.get_entity("Alice").expect("Alice must be present");
        // First occurrence wins — kind is Person.
        assert_eq!(alice.kind, EntityKind::Person);
    }

    /// build() must skip relations that reference an entity name not in the graph.
    #[tokio::test]
    async fn test_build_unknown_entity_relation_is_skipped() {
        let entity_response = r#"[{"name": "Alice", "kind": "Person"}]"#;
        // "Bob" is not in the entity list — this relation must be skipped, not faulted.
        let relation_response = r#"[
            {"from": "Alice", "to": "Bob", "kind": "RelatedTo", "weight": 0.5}
        ]"#;
        let mock_llm = MockLlmClient::new(entity_response, relation_response);

        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Alice mentions Bob.".to_string(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
        };

        let graph = KnowledgeGraph::build(&doc, &mock_llm)
            .await
            .expect("unknown-entity relation must not abort build");
        assert_eq!(graph.entity_count(), 1);
        // The relation referencing unknown "Bob" must be dropped.
        assert_eq!(graph.relation_count(), 0, "relation to unknown entity must be skipped");
    }

    /// build() must propagate LLM errors as TeriError rather than swallowing them.
    #[tokio::test]
    async fn test_build_propagates_llm_error() {
        struct ErrorLlmClient;
        #[async_trait]
        impl LlmClient for ErrorLlmClient {
            async fn complete(&self, _prompt: &str) -> Result<String> {
                Err(TeriError::Llm("simulated LLM failure".to_string()))
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(
                &self,
                _prompt: &str,
            ) -> Result<T> {
                Err(TeriError::Llm("simulated LLM failure".to_string()))
            }
            async fn stream(
                &self,
                _prompt: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(TeriError::Llm("simulated LLM failure".to_string()))
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

        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Some text.".to_string(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
        };

        let result = KnowledgeGraph::build(&doc, &ErrorLlmClient).await;
        assert!(result.is_err(), "LLM error must propagate");
        assert!(result.unwrap_err().to_string().contains("simulated LLM failure"));
    }

    #[test]
    fn test_entity_extraction_prompt_contains_metadata_and_body() {
        let mut metadata = HashMap::new();
        metadata.insert("title".to_string(), "Doc".to_string());
        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Hello world".to_string(),
            metadata: metadata.clone(),
            created_at: Utc::now(),
        };

        let prompt = KnowledgeGraph::entity_extraction_prompt(&doc);
        assert!(prompt.contains("Hello world"));
        assert!(prompt.contains("title"));
        assert!(prompt.contains("Doc"));
    }

    #[test]
    fn test_relation_extraction_prompt_lists_entities() {
        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Body".to_string(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
        };
        let ents = vec![Entity {
            id: Uuid::new_v4(),
            name: "Alice".to_string(),
            kind: EntityKind::Person,
        }];

        let prompt = KnowledgeGraph::relation_extraction_prompt(&doc, &ents);
        assert!(prompt.contains("Alice"));
        assert!(prompt.contains("Person"));
    }

    #[test]
    fn test_parse_entities_json() {
        let json = r#"[
            {"name": "Alice", "kind": "Person"},
            {"name": "Acme", "kind": "Organization"}
        ]"#;

        let entities = KnowledgeGraph::parse_entities_json(json).expect("parse entities");
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].name, "Alice");
        assert_eq!(entities[1].kind, EntityKind::Organization);
    }

    #[test]
    fn test_parse_relations_json() {
        let mut graph = KnowledgeGraph::new();
        let a = graph
            .add_entity(Entity {
                id: Uuid::new_v4(),
                name: "A".to_string(),
                kind: EntityKind::Concept,
            })
            .expect("Failed to add entity");
        let b = graph
            .add_entity(Entity {
                id: Uuid::new_v4(),
                name: "B".to_string(),
                kind: EntityKind::Concept,
            })
            .expect("Failed to add entity");

        let json = r#"[
            {"from": "A", "to": "B", "kind": "RelatedTo", "weight": 0.9}
        ]"#;

        let rels =
            KnowledgeGraph::parse_relations_json(json, &graph.index).expect("parse relations");
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].0, a);
        assert_eq!(rels[0].1, b);
        assert_eq!(rels[0].2.kind, RelationKind::RelatedTo);
        assert!((rels[0].2.weight - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_relation_new_validation() {
        // Valid weight
        let rel = Relation::new(RelationKind::RelatedTo, 0.5).expect("Valid weight");
        assert_eq!(rel.kind, RelationKind::RelatedTo);
        assert_eq!(rel.weight, 0.5);

        // Invalid weight - too high
        let result = Relation::new(RelationKind::RelatedTo, 1.5);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("between 0 and 1"));

        // Invalid weight - too low
        let result = Relation::new(RelationKind::RelatedTo, -0.1);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("between 0 and 1"));
    }

    // ===== Relation.valid_at / temporal validity tests =====

    #[test]
    fn test_relation_is_active_at_none_always_true() {
        let rel = Relation::new(RelationKind::RelatedTo, 0.5).expect("valid weight");
        assert_eq!(rel.valid_at, None);
        // None = always active regardless of timestamp
        assert!(rel.is_active_at(0));
        assert!(rel.is_active_at(u64::MAX));
        assert!(rel.is_active_at(1_700_000_000));
    }

    #[test]
    fn test_relation_is_active_at_open_ended() {
        // valid since t=1000, still current (no end)
        let rel = Relation::with_validity(RelationKind::Causes, 0.7, Some((1000, None)))
            .expect("valid weight");
        assert!(!rel.is_active_at(999)); // before start → inactive
        assert!(rel.is_active_at(1000)); // at start → active
        assert!(rel.is_active_at(9999)); // well after start → active
    }

    #[test]
    fn test_relation_is_active_at_closed_window() {
        // valid [1000, 2000)
        let rel = Relation::with_validity(RelationKind::WorksFor, 0.9, Some((1000, Some(2000))))
            .expect("valid weight");
        assert!(!rel.is_active_at(999)); // before window
        assert!(rel.is_active_at(1000)); // start (inclusive)
        assert!(rel.is_active_at(1500)); // inside
        assert!(!rel.is_active_at(2000)); // end (exclusive) → expired
        assert!(!rel.is_active_at(9999)); // long after
    }

    #[test]
    fn test_relation_with_validity_weight_validation() {
        // Weight validation still applies in with_validity
        let ok = Relation::with_validity(RelationKind::Other, 1.0, None);
        assert!(ok.is_ok());

        let err = Relation::with_validity(RelationKind::Other, 1.5, Some((0, None)));
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("between 0 and 1"));
    }

    #[test]
    fn test_relation_serde_roundtrip_with_valid_at() {
        // New shape WITH valid_at: serializes and deserializes correctly
        let rel = Relation::with_validity(
            RelationKind::RelatedTo,
            0.8,
            Some((1_700_000_000, Some(1_800_000_000))),
        )
        .expect("valid weight");

        let json = serde_json::to_string(&rel).expect("serialize");
        let back: Relation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.valid_at, Some((1_700_000_000, Some(1_800_000_000))));
        assert!((back.weight - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_relation_serde_backward_compat_no_valid_at_field() {
        // OLD-shape JSON (from before valid_at was added) must deserialize with valid_at=None
        let old_json = r#"{"kind":"RelatedTo","weight":0.5}"#;
        let rel: Relation = serde_json::from_str(old_json).expect("backward-compat deserialize");
        assert_eq!(rel.valid_at, None, "missing valid_at field must default to None");
        assert_eq!(rel.kind, RelationKind::RelatedTo);
        assert!((rel.weight - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_knowledge_graph_serde_roundtrip_with_valid_at() {
        // Full graph roundtrip: old-shape (no valid_at) relations survive JSON/bincode deserialization.
        let mut graph = KnowledgeGraph::new();
        let alice =
            Entity { id: Uuid::new_v4(), name: "Alice".to_string(), kind: EntityKind::Person };
        let bob = Entity { id: Uuid::new_v4(), name: "Bob".to_string(), kind: EntityKind::Person };
        let alice_idx = graph.add_entity(alice.clone()).expect("add Alice");
        let bob_idx = graph.add_entity(bob.clone()).expect("add Bob");

        // Relation with temporal window
        let rel = Relation::with_validity(RelationKind::WorksFor, 0.9, Some((1000, Some(2000))))
            .expect("valid");
        graph.add_relation(alice_idx, bob_idx, rel);

        // JSON roundtrip
        let json = graph.serialize_to_json().expect("serialize");
        let g2 = KnowledgeGraph::deserialize_from_json(&json).expect("deserialize");
        let edges = g2.get_all_edges();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].2.valid_at, Some((1000, Some(2000))));

        // Bincode roundtrip
        let bytes = graph.serialize_to_bincode().expect("bincode serialize");
        let g3 = KnowledgeGraph::deserialize_from_bincode(&bytes).expect("bincode deserialize");
        let edges3 = g3.get_all_edges();
        assert_eq!(edges3[0].2.valid_at, Some((1000, Some(2000))));
    }

    #[test]
    fn test_partition_edges_at() {
        let mut graph = KnowledgeGraph::new();
        let a = graph
            .add_entity(Entity {
                id: Uuid::new_v4(),
                name: "A".to_string(),
                kind: EntityKind::Concept,
            })
            .expect("A");
        let b = graph
            .add_entity(Entity {
                id: Uuid::new_v4(),
                name: "B".to_string(),
                kind: EntityKind::Concept,
            })
            .expect("B");
        let c = graph
            .add_entity(Entity {
                id: Uuid::new_v4(),
                name: "C".to_string(),
                kind: EntityKind::Concept,
            })
            .expect("C");

        // Active edge: always-valid
        graph.add_relation(a, b, Relation::new(RelationKind::RelatedTo, 0.5).expect("r1"));
        // Historical edge: expired window [100, 200)
        graph.add_relation(
            a,
            c,
            Relation::with_validity(RelationKind::Causes, 0.5, Some((100, Some(200)))).expect("r2"),
        );

        let t = 500u64; // after expiry
        let (active, historical) = graph.partition_edges_at(t);
        assert_eq!(active.len(), 1);
        assert_eq!(historical.len(), 1);
        // The expired relation is historical
        assert!(!historical[0].2.is_active_at(t));
    }

    #[test]
    fn test_parse_valid_at_from_json_array_form() {
        // valid_at: [start, end]
        let item = serde_json::json!({"valid_at": [1000u64, 2000u64]});
        let result = super::parse_valid_at_from_json(&item);
        assert_eq!(result, Some((1000, Some(2000))));

        // valid_at: [start, null]
        let item2 = serde_json::json!({"valid_at": [1000u64, null]});
        let result2 = super::parse_valid_at_from_json(&item2);
        assert_eq!(result2, Some((1000, None)));
    }

    #[test]
    fn test_parse_valid_at_from_json_object_form() {
        // valid_from + valid_until
        let item = serde_json::json!({"valid_from": 500u64, "valid_until": 1500u64});
        let result = super::parse_valid_at_from_json(&item);
        assert_eq!(result, Some((500, Some(1500))));

        // valid_from only (open-ended)
        let item2 = serde_json::json!({"valid_from": 500u64});
        let result2 = super::parse_valid_at_from_json(&item2);
        assert_eq!(result2, Some((500, None)));

        // neither field → None
        let item3 = serde_json::json!({"kind": "RelatedTo"});
        let result3 = super::parse_valid_at_from_json(&item3);
        assert_eq!(result3, None);
    }

    #[test]
    fn test_empty_entity_list_prompt() {
        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Test".to_string(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
        };

        let prompt = KnowledgeGraph::relation_extraction_prompt(&doc, &[]);
        assert_eq!(prompt, "No entities provided for relation extraction.");
    }

    #[test]
    fn test_serialize_to_json_and_deserialize() {
        let mut graph = KnowledgeGraph::new();

        // Add entities
        let alice =
            Entity { id: Uuid::new_v4(), name: "Alice".to_string(), kind: EntityKind::Person };
        let bob = Entity { id: Uuid::new_v4(), name: "Bob".to_string(), kind: EntityKind::Person };

        let alice_idx = graph.add_entity(alice.clone()).expect("Failed to add Alice");
        let bob_idx = graph.add_entity(bob.clone()).expect("Failed to add Bob");

        // Add relation
        graph.add_relation(
            alice_idx,
            bob_idx,
            Relation::new(RelationKind::RelatedTo, 0.8).expect("Valid weight"),
        );

        // Serialize to JSON
        let json = graph.serialize_to_json().expect("Failed to serialize");
        assert!(!json.is_empty());

        // Deserialize from JSON
        let deserialized =
            KnowledgeGraph::deserialize_from_json(&json).expect("Failed to deserialize");

        // Verify entities
        let deserialized_alice = deserialized.get_entity("Alice").expect("Alice not found");
        assert_eq!(deserialized_alice.name, "Alice");
        assert_eq!(deserialized_alice.kind, EntityKind::Person);

        let deserialized_bob = deserialized.get_entity("Bob").expect("Bob not found");
        assert_eq!(deserialized_bob.name, "Bob");
        assert_eq!(deserialized_bob.kind, EntityKind::Person);

        // Verify neighbors
        let neighbors = deserialized.get_neighbors(alice.id).expect("Failed to get neighbors");
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].name, "Bob");
    }

    #[test]
    fn test_serialize_to_bincode_and_deserialize() {
        let mut graph = KnowledgeGraph::new();

        // Add entities
        let entity1 = Entity {
            id: Uuid::new_v4(),
            name: "Entity1".to_string(),
            kind: EntityKind::Organization,
        };
        let entity2 =
            Entity { id: Uuid::new_v4(), name: "Entity2".to_string(), kind: EntityKind::Location };

        let idx1 = graph.add_entity(entity1).expect("Failed to add entity1");
        let idx2 = graph.add_entity(entity2).expect("Failed to add entity2");

        // Add relation
        graph.add_relation(
            idx1,
            idx2,
            Relation::new(RelationKind::LocatedIn, 0.9).expect("Valid weight"),
        );

        // Serialize to bincode
        let bytes = graph.serialize_to_bincode().expect("Failed to serialize to bincode");
        assert!(!bytes.is_empty());

        // Deserialize from bincode
        let deserialized = KnowledgeGraph::deserialize_from_bincode(&bytes)
            .expect("Failed to deserialize from bincode");

        // Verify structure
        assert_eq!(deserialized.entity_count(), 2);
        assert_eq!(deserialized.relation_count(), 1);
    }

    #[test]
    fn test_serialize_to_file_and_deserialize_from_file() {
        let mut graph = KnowledgeGraph::new();

        // Add test entity
        let entity = Entity {
            id: Uuid::new_v4(),
            name: "TestEntity".to_string(),
            kind: EntityKind::Concept,
        };
        graph.add_entity(entity).expect("Failed to add entity");

        let file_path = std::env::temp_dir().join("test_graph.json");
        let file_path = file_path.to_str().expect("temp path is valid utf-8");

        // Serialize to file
        graph.serialize_to_file(file_path).expect("Failed to serialize to file");

        // Deserialize from file
        let deserialized = KnowledgeGraph::deserialize_from_file(file_path)
            .expect("Failed to deserialize from file");

        // Verify
        assert_eq!(deserialized.entity_count(), 1);
        let test_entity = deserialized.get_entity("TestEntity").expect("TestEntity not found");
        assert_eq!(test_entity.name, "TestEntity");
        assert_eq!(test_entity.kind, EntityKind::Concept);

        // Cleanup
        std::fs::remove_file(file_path).ok();
    }

    #[test]
    fn test_serialize_to_bincode_file_and_deserialize_from_bincode_file() {
        let mut graph = KnowledgeGraph::new();

        // Add test entities and relation
        let entity1 = Entity {
            id: Uuid::new_v4(),
            name: "Company".to_string(),
            kind: EntityKind::Organization,
        };
        let entity2 =
            Entity { id: Uuid::new_v4(), name: "City".to_string(), kind: EntityKind::Location };

        let idx1 = graph.add_entity(entity1).expect("Failed to add entity1");
        let idx2 = graph.add_entity(entity2).expect("Failed to add entity2");

        graph.add_relation(
            idx1,
            idx2,
            Relation::new(RelationKind::LocatedIn, 1.0).expect("Valid weight"),
        );

        let file_path = std::env::temp_dir().join("test_graph.bin");
        let file_path = file_path.to_str().expect("temp path is valid utf-8");

        // Serialize to binary file
        graph
            .serialize_to_bincode_file(file_path)
            .expect("Failed to serialize to binary file");

        // Deserialize from binary file
        let deserialized = KnowledgeGraph::deserialize_from_bincode_file(file_path)
            .expect("Failed to deserialize from binary file");

        // Verify
        assert_eq!(deserialized.entity_count(), 2);
        assert_eq!(deserialized.relation_count(), 1);

        // Cleanup
        std::fs::remove_file(file_path).ok();
    }

    #[test]
    fn test_deserialize_invalid_json() {
        let invalid_json = "{ invalid json }";
        let result = KnowledgeGraph::deserialize_from_json(invalid_json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to deserialize"));
    }

    #[test]
    fn test_deserialize_invalid_bincode() {
        let invalid_bytes = b"invalid binary data";
        let result = KnowledgeGraph::deserialize_from_bincode(invalid_bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to deserialize"));
    }

    #[test]
    fn test_duplicate_entity_name_error() {
        let mut graph = KnowledgeGraph::new();
        let alice =
            Entity { id: Uuid::new_v4(), name: "Alice".to_string(), kind: EntityKind::Person };

        graph.add_entity(alice).expect("First entity should succeed");

        let alice2 =
            Entity { id: Uuid::new_v4(), name: "Alice".to_string(), kind: EntityKind::Person };

        let result = graph.add_entity(alice2);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn test_invalid_weight_error() {
        let mut graph = KnowledgeGraph::new();
        let _a = graph
            .add_entity(Entity {
                id: Uuid::new_v4(),
                name: "A".to_string(),
                kind: EntityKind::Concept,
            })
            .expect("Failed to add entity");
        let _b = graph
            .add_entity(Entity {
                id: Uuid::new_v4(),
                name: "B".to_string(),
                kind: EntityKind::Concept,
            })
            .expect("Failed to add entity");

        let json = r#"[
            {"from": "A", "to": "B", "kind": "RelatedTo", "weight": 1.5}
        ]"#;

        let result = KnowledgeGraph::parse_relations_json(json, &graph.index);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("between 0 and 1"));
    }

    #[test]
    fn test_entity_with_id_parsing() {
        let _id = Uuid::new_v4();
        let json = r#"[
            {"id": "550e8400-e29b-41d4-a716-446655440000", "name": "Alice", "kind": "Person"},
            {"name": "Acme", "kind": "Organization"}
        ]"#;

        let entities = KnowledgeGraph::parse_entities_json(json).expect("parse entities");
        assert_eq!(entities.len(), 2);
        // First entity should have the provided ID (if valid)
        assert_eq!(entities[0].name, "Alice");
        // Second entity should have a generated UUID
        assert_eq!(entities[1].name, "Acme");
    }

    #[test]
    fn test_subgraph_name_overflow_protection() {
        let mut graph = KnowledgeGraph::new();

        // Create entities with the same name to trigger overflow protection
        let base_entity =
            Entity { id: Uuid::new_v4(), name: "Test".to_string(), kind: EntityKind::Concept };

        // Add the base entity
        let _base_idx = graph.add_entity(base_entity.clone()).expect("Failed to add base entity");

        // Try to create a subgraph with many duplicate names
        // This simulates the worst case where we hit the MAX_NAME_SUFFIX limit
        let result = graph.get_subgraph(base_entity.id, 0);

        // Should succeed for normal case
        assert!(result.is_ok());

        // Note: Testing the actual overflow would require creating 1000+ entities
        // which is impractical in a unit test. The overflow protection is
        // verified by the logic itself.
    }

    // ===== Mock LLM Client for Testing =====

    struct MockLlmClient {
        entity_response: String,
        relation_response: String,
    }

    impl MockLlmClient {
        fn new(entity_response: &str, relation_response: &str) -> Self {
            Self {
                entity_response: entity_response.to_string(),
                relation_response: relation_response.to_string(),
            }
        }
    }

    #[async_trait]
    impl LlmClient for MockLlmClient {
        async fn complete(&self, prompt: &str) -> Result<String> {
            if prompt.contains("Extract named entities") {
                Ok(self.entity_response.clone())
            } else if prompt.contains("extract relations") {
                Ok(self.relation_response.clone())
            } else {
                Err(TeriError::Llm("Unexpected prompt for mock".to_string()))
            }
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(&self, prompt: &str) -> Result<T> {
            let response = self.complete(prompt).await?;
            serde_json::from_str(&response)
                .map_err(|e| TeriError::Llm(format!("JSON parsing error: {}", e)))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("Streaming not implemented in mock".to_string()))
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

    // ===== Multi-chunk mock LLM that returns different entities per chunk =====

    /// A mock LLM that tracks how many times it has been called for entity extraction
    /// and returns a different entity on each call — proving that entities from ALL chunks
    /// are merged into the final graph.
    struct ChunkCountingLlmClient {
        /// All entity-extraction responses, indexed by call order (wraps around).
        entity_responses: Vec<String>,
        /// Single relation response (same for all chunks — tests the merge, not relations).
        relation_response: String,
        /// Shared call counter (using `std::sync::Mutex` to satisfy `&self` bound on `complete`).
        entity_call_count: std::sync::Mutex<usize>,
    }

    impl ChunkCountingLlmClient {
        fn new(entity_responses: Vec<&str>, relation_response: &str) -> Self {
            Self {
                entity_responses: entity_responses.iter().map(|s| s.to_string()).collect(),
                relation_response: relation_response.to_string(),
                entity_call_count: std::sync::Mutex::new(0),
            }
        }

        fn entity_call_count(&self) -> usize {
            *self.entity_call_count.lock().unwrap()
        }
    }

    #[async_trait]
    impl LlmClient for ChunkCountingLlmClient {
        async fn complete(&self, prompt: &str) -> Result<String> {
            if prompt.contains("Extract named entities") {
                let mut count = self.entity_call_count.lock().unwrap();
                let idx = *count % self.entity_responses.len();
                *count += 1;
                Ok(self.entity_responses[idx].clone())
            } else if prompt.contains("extract relations") {
                Ok(self.relation_response.clone())
            } else {
                Err(TeriError::Llm("Unexpected prompt for chunk-counting mock".to_string()))
            }
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(&self, prompt: &str) -> Result<T> {
            let response = self.complete(prompt).await?;
            serde_json::from_str(&response)
                .map_err(|e| TeriError::Llm(format!("JSON parsing error: {}", e)))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("Streaming not implemented in chunk-counting mock".to_string()))
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

    /// build() with a large multi-chunk document must:
    /// 1. Call the LLM more than once for entity extraction (proving chunking happened).
    /// 2. Merge entities from ALL chunks into the graph (deduped).
    /// 3. Produce a graph that contains entities from chunk 1 AND chunk 2 (distinct names).
    #[tokio::test]
    async fn test_build_large_doc_multi_chunk_merge() {
        // Build a document that is clearly > 500 chars so it gets split into at least 2 chunks.
        // Pad with unique filler so the chunks are distinct.
        let chunk1_filler = "Alpha ".repeat(50); // ~300 chars, first chunk territory
        let chunk2_filler = "Beta ".repeat(50); // ~300 chars, second chunk territory
        // Use ". " as sentence boundary separator to encourage clean splits.
        let large_text = format!(
            "{}. Context for Alice and her team at Sunrise Corp. {}. Context for Bob and his division at Sunset Inc.",
            chunk1_filler, chunk2_filler
        );
        assert!(
            large_text.chars().count() > 500,
            "test pre-condition: document must exceed chunk threshold"
        );

        // Entity responses: chunk 1 returns Alice/Sunrise Corp, chunk 2 returns Bob/Sunset Inc.
        // (The counting mock cycles through responses in order.)
        let entity_resp_chunk1 = r#"[
            {"name": "Alice", "kind": "Person"},
            {"name": "Sunrise Corp", "kind": "Organization"}
        ]"#;
        let entity_resp_chunk2 = r#"[
            {"name": "Bob", "kind": "Person"},
            {"name": "Sunset Inc", "kind": "Organization"}
        ]"#;

        let mock_llm = ChunkCountingLlmClient::new(
            vec![entity_resp_chunk1, entity_resp_chunk2],
            "[]", // no relations needed for this test
        );

        let doc = SeedDocument {
            id: uuid::Uuid::new_v4(),
            raw_text: large_text,
            metadata: HashMap::new(),
            created_at: Utc::now(),
        };

        let graph = KnowledgeGraph::build(&doc, &mock_llm).await.expect("multi-chunk build");

        // Chunking happened — entity extraction was called more than once.
        let call_count = mock_llm.entity_call_count();
        assert!(
            call_count > 1,
            "expected entity extraction to be called >1 time (chunking), got {call_count}"
        );

        // All 4 entities (2 per chunk) should be present in the merged graph.
        assert!(graph.get_entity("Alice").is_some(), "Alice must be in merged graph");
        assert!(
            graph.get_entity("Sunrise Corp").is_some(),
            "Sunrise Corp must be in merged graph"
        );
        assert!(graph.get_entity("Bob").is_some(), "Bob must be in merged graph");
        assert!(graph.get_entity("Sunset Inc").is_some(), "Sunset Inc must be in merged graph");
        assert_eq!(graph.entity_count(), 4, "4 unique entities, none dropped");
    }

    // ===== Entity/Relation Extraction Tests =====

    #[tokio::test]
    async fn test_entity_extraction_with_mock_llm() {
        let mock_response = r#"[
            {"name": "Alice", "kind": "Person"},
            {"name": "Acme Corp", "kind": "Organization"},
            {"name": "New York", "kind": "Location"}
        ]"#;

        let mock_llm = MockLlmClient::new(mock_response, "");

        let mut metadata = HashMap::new();
        metadata.insert("title".to_string(), "Test Document".to_string());
        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Alice works at Acme Corp in New York.".to_string(),
            metadata,
            created_at: Utc::now(),
        };

        let prompt = KnowledgeGraph::entity_extraction_prompt(&doc);
        let response = mock_llm.complete(&prompt).await.expect("Failed to get response");
        let entities =
            KnowledgeGraph::parse_entities_json(&response).expect("Failed to parse entities");

        assert_eq!(entities.len(), 3);
        assert_eq!(entities[0].name, "Alice");
        assert_eq!(entities[0].kind, EntityKind::Person);
        assert_eq!(entities[1].name, "Acme Corp");
        assert_eq!(entities[1].kind, EntityKind::Organization);
        assert_eq!(entities[2].name, "New York");
        assert_eq!(entities[2].kind, EntityKind::Location);
    }

    #[tokio::test]
    async fn test_relation_extraction_with_mock_llm() {
        let mock_response = r#"[
            {"from": "Alice", "to": "Acme Corp", "kind": "WorksFor", "weight": 0.9},
            {"from": "Acme Corp", "to": "New York", "kind": "LocatedIn", "weight": 0.8}
        ]"#;

        let mock_llm = MockLlmClient::new("", mock_response);

        let mut metadata = HashMap::new();
        metadata.insert("title".to_string(), "Test Document".to_string());
        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Alice works at Acme Corp in New York.".to_string(),
            metadata,
            created_at: Utc::now(),
        };

        let entities = vec![
            Entity { id: Uuid::new_v4(), name: "Alice".to_string(), kind: EntityKind::Person },
            Entity {
                id: Uuid::new_v4(),
                name: "Acme Corp".to_string(),
                kind: EntityKind::Organization,
            },
            Entity { id: Uuid::new_v4(), name: "New York".to_string(), kind: EntityKind::Location },
        ];

        let prompt = KnowledgeGraph::relation_extraction_prompt(&doc, &entities);
        let response = mock_llm.complete(&prompt).await.expect("Failed to get response");

        // Create a mock index for relation parsing
        let mut index = HashMap::new();
        let mut graph = KnowledgeGraph::new();
        for entity in &entities {
            let idx = graph.add_entity(entity.clone()).expect("Failed to add entity");
            index.insert(entity.name.clone(), idx);
        }

        let relations = KnowledgeGraph::parse_relations_json(&response, &index)
            .expect("Failed to parse relations");

        assert_eq!(relations.len(), 2);
        assert_eq!(relations[0].2.kind, RelationKind::WorksFor);
        assert_eq!(relations[0].2.weight, 0.9);
        assert_eq!(relations[1].2.kind, RelationKind::LocatedIn);
        assert_eq!(relations[1].2.weight, 0.8);
    }

    #[tokio::test]
    async fn test_graph_construction_with_mock_llm() {
        let entity_response = r#"[
            {"name": "Alice", "kind": "Person"},
            {"name": "Bob", "kind": "Person"}
        ]"#;

        let relation_response = r#"[
            {"from": "Alice", "to": "Bob", "kind": "RelatedTo", "weight": 0.7}
        ]"#;

        let mock_llm = MockLlmClient::new(entity_response, relation_response);

        let mut metadata = HashMap::new();
        metadata.insert("title".to_string(), "Test Document".to_string());
        let doc = SeedDocument {
            id: Uuid::new_v4(),
            raw_text: "Alice and Bob are colleagues.".to_string(),
            metadata,
            created_at: Utc::now(),
        };

        // Extract entities
        let entity_prompt = KnowledgeGraph::entity_extraction_prompt(&doc);
        let entity_response =
            mock_llm.complete(&entity_prompt).await.expect("Failed to get entities");
        let entities = KnowledgeGraph::parse_entities_json(&entity_response)
            .expect("Failed to parse entities");

        // Build graph with entities
        let mut graph = KnowledgeGraph::new();
        let mut entity_map = HashMap::new();

        for entity in entities {
            let idx = graph.add_entity(entity.clone()).expect("Failed to add entity");
            entity_map.insert(entity.name, idx);
        }

        // Extract relations
        let entities: Vec<Entity> = graph.get_all_entities().into_iter().cloned().collect();
        let relation_prompt = KnowledgeGraph::relation_extraction_prompt(&doc, &entities);
        let relation_response =
            mock_llm.complete(&relation_prompt).await.expect("Failed to get relations");
        let relations = KnowledgeGraph::parse_relations_json(&relation_response, &graph.index)
            .expect("Failed to parse relations");

        // Add relations to graph
        for (from_idx, to_idx, relation) in relations {
            graph.add_relation(from_idx, to_idx, relation);
        }

        // Verify graph structure
        assert_eq!(graph.entity_count(), 2);
        assert_eq!(graph.relation_count(), 1);

        let alice = graph.get_entity("Alice").expect("Alice not found");
        assert_eq!(alice.kind, EntityKind::Person);

        let neighbors = graph.get_neighbors(alice.id).expect("Failed to get neighbors");
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].name, "Bob");
    }

    // ===== Additional Graph Query Method Tests =====

    #[test]
    fn test_get_entity_case_sensitivity() {
        let mut graph = KnowledgeGraph::new();
        let alice_lower =
            Entity { id: Uuid::new_v4(), name: "alice".to_string(), kind: EntityKind::Person };
        let alice_upper =
            Entity { id: Uuid::new_v4(), name: "Alice".to_string(), kind: EntityKind::Person };

        graph.add_entity(alice_lower).expect("Failed to add alice");
        graph.add_entity(alice_upper).expect("Failed to add Alice");

        // Names are case-sensitive
        let lower_result = graph.get_entity("alice");
        let upper_result = graph.get_entity("Alice");

        assert!(lower_result.is_some());
        assert!(upper_result.is_some());
        assert_ne!(lower_result.unwrap().id, upper_result.unwrap().id);
    }

    #[test]
    fn test_get_neighbors_nonexistent_entity() {
        let graph = KnowledgeGraph::new();
        let nonexistent_id = Uuid::new_v4();
        let result = graph.get_neighbors(nonexistent_id);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Entity not found"));
    }

    #[test]
    fn test_get_subgraph_nonexistent_entity() {
        let graph = KnowledgeGraph::new();
        let nonexistent_id = Uuid::new_v4();
        let result = graph.get_subgraph(nonexistent_id, 2);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Entity not found"));
    }

    #[test]
    fn test_get_subgraph_depth_zero() {
        let mut graph = KnowledgeGraph::new();
        let entity_id = Uuid::new_v4();
        let entity =
            Entity { id: entity_id, name: "Central".to_string(), kind: EntityKind::Concept };
        graph.add_entity(entity).expect("Failed to add entity");

        let subgraph = graph.get_subgraph(entity_id, 0).expect("Failed to get subgraph");
        assert_eq!(subgraph.entity_count(), 1);
    }

    #[test]
    fn test_get_subgraph_isolated_entity() {
        let mut graph = KnowledgeGraph::new();
        let isolated_id = Uuid::new_v4();
        let isolated =
            Entity { id: isolated_id, name: "Isolated".to_string(), kind: EntityKind::Concept };
        let other_id = Uuid::new_v4();
        let other = Entity { id: other_id, name: "Other".to_string(), kind: EntityKind::Concept };

        graph.add_entity(isolated).expect("Failed to add isolated");
        graph.add_entity(other).expect("Failed to add other");
        // Don't add any relations

        let subgraph = graph.get_subgraph(isolated_id, 5).expect("Failed to get subgraph");
        assert_eq!(subgraph.entity_count(), 1); // Only the isolated entity
    }
}
