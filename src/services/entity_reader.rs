//! Entity node DTO — the read-model for graph entities.
//!
//! Port of `backend/app/services/zep_entity_reader.py` lines 22-68 (MiroFish).
//! Covers: `EntityNode` and `FilteredEntities` dataclasses ONLY.
//!
//! The `ZepEntityReader` class (L71+) is NOT ported here; it is a separate later
//! unit (DECISION-9) that depends on the Zep-Cloud substrate which teri replaces
//! with native petgraph.
//!
//! # Symbol mapping (S-198..S-213)
//!
//! | Source symbol | Lines | Rust symbol |
//! |---|---|---|
//! | S-198 `EntityNode` | 22-51 | `EntityNode` struct |
//! | S-199..S-205 fields | 24-33 | struct fields |
//! | S-206 `to_dict` | 35-44 | `EntityNode::to_dict` |
//! | S-207 `get_entity_type` | 46-51 | `EntityNode::get_entity_type` |
//! | S-208 `FilteredEntities` | 54-68 | `FilteredEntities` struct |
//! | S-209..S-212 fields | 56-60 | struct fields |
//! | S-213 `to_dict` | 62-68 | `FilteredEntities::to_dict` |
//!
//! # `entity_types` set semantics
//!
//! Python's `FilteredEntities.entity_types` is a `Set[str]`.  `to_dict()` emits
//! it as `list(self.entity_types)`.  Python set iteration order is unspecified
//! (hash-randomised at runtime), so a `HashSet<String>` collected into a `Vec`
//! is the faithful Rust equivalent — both produce an unspecified-but-deterministic
//! per-run ordering.  Downstream consumers that care about ordering must sort
//! explicitly (as neither Python nor Rust guarantees order here).
//!
//! # `attributes` field
//!
//! Python declares `attributes: Dict[str, Any]`.  We use `serde_json::Map<String, Value>`
//! which serialises identically to a JSON object with arbitrary value types.
//!
//! # `related_edges` / `related_nodes`
//!
//! Python declares `List[Dict[str, Any]]` with `field(default_factory=list)`.
//! We use `Vec<Value>` which deserialisable from any JSON array of objects.
//!
//! # NOT teri's `graph::Entity`
//!
//! `EntityNode` is a richer read-DTO (uuid + name + labels + summary + attributes +
//! edges + nodes).  It is distinct from `crate::graph::Entity` ({id, name, kind})
//! and must NOT be merged with it.

use crate::graph::KnowledgeGraph;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// S-198..S-207 — EntityNode
// ---------------------------------------------------------------------------

/// Entity node read-model.
///
/// Port of `EntityNode` dataclass (`zep_entity_reader.py:22-51`).
///
/// Fields are in the same order as the Python dataclass declaration so that
/// `to_dict()` key order is preserved (we build the map explicitly anyway,
/// but this serves as documentation).
///
/// S-198
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityNode {
    /// S-199 — `uuid: str`
    pub uuid: String,

    /// S-200 — `name: str`
    pub name: String,

    /// S-201 — `labels: List[str]`
    pub labels: Vec<String>,

    /// S-202 — `summary: str`
    pub summary: String,

    /// S-203 — `attributes: Dict[str, Any]`
    ///
    /// A JSON object with arbitrary value types, matching Python's `Dict[str, Any]`.
    pub attributes: Map<String, Value>,

    /// S-204 — `related_edges: List[Dict[str, Any]] = field(default_factory=list)`
    #[serde(default)]
    pub related_edges: Vec<Value>,

    /// S-205 — `related_nodes: List[Dict[str, Any]] = field(default_factory=list)`
    #[serde(default)]
    pub related_nodes: Vec<Value>,
}

impl EntityNode {
    /// Construct an `EntityNode` with the four required fields and empty defaults
    /// for `related_edges` and `related_nodes`.
    pub fn new(
        uuid: impl Into<String>,
        name: impl Into<String>,
        labels: Vec<String>,
        summary: impl Into<String>,
        attributes: Map<String, Value>,
    ) -> Self {
        Self {
            uuid: uuid.into(),
            name: name.into(),
            labels,
            summary: summary.into(),
            attributes,
            related_edges: Vec::new(),
            related_nodes: Vec::new(),
        }
    }

    /// Convert to a JSON object with EXACTLY 7 keys in declaration order.
    ///
    /// Port of `EntityNode.to_dict()` (`zep_entity_reader.py:35-44`).
    ///
    /// Key order: uuid, name, labels, summary, attributes, related_edges, related_nodes.
    ///
    /// S-206
    pub fn to_dict(&self) -> Value {
        let mut map = serde_json::Map::with_capacity(7);
        map.insert("uuid".to_string(), Value::String(self.uuid.clone()));
        map.insert("name".to_string(), Value::String(self.name.clone()));
        map.insert(
            "labels".to_string(),
            Value::Array(self.labels.iter().map(|l| Value::String(l.clone())).collect()),
        );
        map.insert("summary".to_string(), Value::String(self.summary.clone()));
        map.insert("attributes".to_string(), Value::Object(self.attributes.clone()));
        map.insert("related_edges".to_string(), Value::Array(self.related_edges.clone()));
        map.insert("related_nodes".to_string(), Value::Array(self.related_nodes.clone()));
        Value::Object(map)
    }

    /// Return the first label that is not `"Entity"` or `"Node"`.
    ///
    /// Port of `EntityNode.get_entity_type()` (`zep_entity_reader.py:46-51`).
    ///
    /// Iterates `self.labels` in order; returns the **first** label not in
    /// `{"Entity", "Node"}`, or `None` when every label is in that set or
    /// the labels list is empty.
    ///
    /// # Examples
    /// ```
    /// use teri::services::entity_reader::EntityNode;
    /// use serde_json::Map;
    ///
    /// let mut e = EntityNode::new("u", "Bob", vec!["Entity".into(), "Person".into()], "", Map::new());
    /// assert_eq!(e.get_entity_type(), Some("Person".to_string()));
    ///
    /// let mut e2 = EntityNode::new("u", "X", vec!["Entity".into(), "Node".into()], "", Map::new());
    /// assert_eq!(e2.get_entity_type(), None);
    /// ```
    ///
    /// S-207
    pub fn get_entity_type(&self) -> Option<String> {
        for label in &self.labels {
            if label != "Entity" && label != "Node" {
                return Some(label.clone());
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// S-208..S-213 — FilteredEntities
// ---------------------------------------------------------------------------

/// Filtered entity collection.
///
/// Port of `FilteredEntities` dataclass (`zep_entity_reader.py:54-68`).
///
/// # `entity_types` ordering note
/// Python `Set[str]` iteration order is unspecified.  `list(self.entity_types)`
/// in `to_dict()` therefore produces an unspecified-but-deterministic (per run)
/// order.  We model this as `HashSet<String>` and collect to `Vec<String>` in
/// `to_dict()`, which is faithfully equivalent — unordered iteration, one value
/// per unique type.  Callers that need sorted output must sort the returned
/// `to_dict()` value themselves.
///
/// S-208
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilteredEntities {
    /// S-209 — `entities: List[EntityNode]`
    pub entities: Vec<EntityNode>,

    /// S-210 — `entity_types: Set[str]`
    ///
    /// Stored as a `HashSet` to match Python's set semantics (unique values,
    /// unspecified iteration order).
    pub entity_types: HashSet<String>,

    /// S-211 — `total_count: int`
    pub total_count: i64,

    /// S-212 — `filtered_count: int`
    pub filtered_count: i64,
}

impl FilteredEntities {
    /// Convert to a JSON object with EXACTLY 4 keys in declaration order.
    ///
    /// Port of `FilteredEntities.to_dict()` (`zep_entity_reader.py:62-68`).
    ///
    /// Key order: entities, entity_types, total_count, filtered_count.
    ///
    /// `entity_types` is serialised as `list(self.entity_types)` — a JSON array
    /// of strings in unspecified (HashSet iteration) order, matching Python.
    ///
    /// S-213
    pub fn to_dict(&self) -> Value {
        let mut map = serde_json::Map::with_capacity(4);

        // entities: [e.to_dict() for e in self.entities]
        let entities_arr: Vec<Value> = self.entities.iter().map(|e| e.to_dict()).collect();
        map.insert("entities".to_string(), Value::Array(entities_arr));

        // entity_types: list(self.entity_types) — unspecified order (Python set)
        let types_arr: Vec<Value> =
            self.entity_types.iter().map(|t| Value::String(t.clone())).collect();
        map.insert("entity_types".to_string(), Value::Array(types_arr));

        map.insert(
            "total_count".to_string(),
            Value::Number(serde_json::Number::from(self.total_count)),
        );
        map.insert(
            "filtered_count".to_string(),
            Value::Number(serde_json::Number::from(self.filtered_count)),
        );

        Value::Object(map)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn node(labels: Vec<&str>) -> EntityNode {
        EntityNode::new(
            "test-uuid",
            "TestNode",
            labels.into_iter().map(str::to_string).collect(),
            "A test summary",
            Map::new(),
        )
    }

    // -----------------------------------------------------------------------
    // S-207 get_entity_type
    // -----------------------------------------------------------------------

    #[test]
    fn get_entity_type_skips_entity_and_node_returns_first_real_label() {
        let e = node(vec!["Entity", "Node", "Person"]);
        assert_eq!(e.get_entity_type(), Some("Person".to_string()));
    }

    #[test]
    fn get_entity_type_returns_first_non_entity_non_node() {
        let e = node(vec!["Entity", "MediaOutlet", "Organization"]);
        // first real label is MediaOutlet
        assert_eq!(e.get_entity_type(), Some("MediaOutlet".to_string()));
    }

    #[test]
    fn get_entity_type_returns_none_when_only_entity_and_node() {
        let e = node(vec!["Entity", "Node"]);
        assert_eq!(e.get_entity_type(), None);
    }

    #[test]
    fn get_entity_type_returns_none_when_only_entity() {
        let e = node(vec!["Entity"]);
        assert_eq!(e.get_entity_type(), None);
    }

    #[test]
    fn get_entity_type_returns_none_when_only_node() {
        let e = node(vec!["Node"]);
        assert_eq!(e.get_entity_type(), None);
    }

    #[test]
    fn get_entity_type_returns_none_for_empty_labels() {
        let e = node(vec![]);
        assert_eq!(e.get_entity_type(), None);
    }

    #[test]
    fn get_entity_type_returns_first_label_when_no_entity_or_node() {
        let e = node(vec!["Student", "Person"]);
        // No "Entity"/"Node" in list: returns first label
        assert_eq!(e.get_entity_type(), Some("Student".to_string()));
    }

    #[test]
    fn get_entity_type_entity_node_are_case_sensitive() {
        // "entity" (lowercase) is NOT filtered — filter is exact "Entity"/"Node"
        let e = node(vec!["entity", "node"]);
        assert_eq!(e.get_entity_type(), Some("entity".to_string()));
    }

    // -----------------------------------------------------------------------
    // S-206 EntityNode to_dict shape
    // -----------------------------------------------------------------------

    #[test]
    fn entity_node_to_dict_has_exactly_7_keys_in_order() {
        let e = EntityNode {
            uuid: "u1".to_string(),
            name: "Alice".to_string(),
            labels: vec!["Entity".to_string(), "Person".to_string()],
            summary: "A person".to_string(),
            attributes: Map::new(),
            related_edges: vec![],
            related_nodes: vec![],
        };
        let dict = e.to_dict();
        let obj = dict.as_object().expect("to_dict must return an object");
        assert_eq!(obj.len(), 7);

        // Key order: uuid, name, labels, summary, attributes, related_edges, related_nodes
        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "uuid",
                "name",
                "labels",
                "summary",
                "attributes",
                "related_edges",
                "related_nodes"
            ]
        );
    }

    #[test]
    fn entity_node_to_dict_values_match() {
        let mut attrs = Map::new();
        attrs.insert("age".to_string(), Value::Number(30.into()));

        let e = EntityNode {
            uuid: "uuid-42".to_string(),
            name: "Bob".to_string(),
            labels: vec!["Entity".to_string(), "Student".to_string()],
            summary: "A student".to_string(),
            attributes: attrs,
            related_edges: vec![serde_json::json!({"type": "KNOWS"})],
            related_nodes: vec![serde_json::json!({"uuid": "other"})],
        };
        let dict = e.to_dict();
        let obj = dict.as_object().unwrap();

        assert_eq!(obj["uuid"].as_str().unwrap(), "uuid-42");
        assert_eq!(obj["name"].as_str().unwrap(), "Bob");
        assert_eq!(
            obj["labels"].as_array().unwrap(),
            &[Value::String("Entity".into()), Value::String("Student".into())]
        );
        assert_eq!(obj["summary"].as_str().unwrap(), "A student");
        assert_eq!(obj["attributes"]["age"].as_i64().unwrap(), 30);
        assert_eq!(obj["related_edges"].as_array().unwrap().len(), 1);
        assert_eq!(obj["related_nodes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn entity_node_to_dict_defaults_are_empty_arrays() {
        let e = EntityNode::new("u", "N", vec![], "s", Map::new());
        let dict = e.to_dict();
        let obj = dict.as_object().unwrap();
        assert_eq!(obj["related_edges"].as_array().unwrap().len(), 0);
        assert_eq!(obj["related_nodes"].as_array().unwrap().len(), 0);
    }

    // -----------------------------------------------------------------------
    // S-213 FilteredEntities to_dict shape
    // -----------------------------------------------------------------------

    #[test]
    fn filtered_entities_to_dict_has_exactly_4_keys_in_order() {
        let fe = FilteredEntities {
            entities: vec![],
            entity_types: HashSet::new(),
            total_count: 10,
            filtered_count: 5,
        };
        let dict = fe.to_dict();
        let obj = dict.as_object().expect("to_dict must return an object");
        assert_eq!(obj.len(), 4);

        // Key order: entities, entity_types, total_count, filtered_count
        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(keys, ["entities", "entity_types", "total_count", "filtered_count"]);
    }

    #[test]
    fn filtered_entities_to_dict_entities_are_nested_dicts() {
        let e = EntityNode::new("u1", "Alice", vec!["Person".into()], "desc", Map::new());
        let fe = FilteredEntities {
            entities: vec![e],
            entity_types: {
                let mut s = HashSet::new();
                s.insert("Person".to_string());
                s
            },
            total_count: 1,
            filtered_count: 1,
        };
        let dict = fe.to_dict();
        let obj = dict.as_object().unwrap();

        let entities = obj["entities"].as_array().unwrap();
        assert_eq!(entities.len(), 1);
        // Each element is a to_dict() shape (7-key object)
        assert_eq!(entities[0].as_object().unwrap().len(), 7);
        assert_eq!(entities[0]["name"].as_str().unwrap(), "Alice");
    }

    #[test]
    fn filtered_entities_to_dict_entity_types_is_array_of_strings() {
        let mut types = HashSet::new();
        types.insert("Person".to_string());
        types.insert("Organization".to_string());

        let fe = FilteredEntities {
            entities: vec![],
            entity_types: types,
            total_count: 2,
            filtered_count: 2,
        };
        let dict = fe.to_dict();
        let obj = dict.as_object().unwrap();

        let types_arr = obj["entity_types"].as_array().unwrap();
        assert_eq!(types_arr.len(), 2);
        // Values are strings (order unspecified — set semantics)
        let mut type_strings: Vec<String> =
            types_arr.iter().map(|v| v.as_str().unwrap().to_string()).collect();
        type_strings.sort();
        assert_eq!(type_strings, vec!["Organization", "Person"]);
    }

    #[test]
    fn filtered_entities_to_dict_counts_are_numbers() {
        let fe = FilteredEntities {
            entities: vec![],
            entity_types: HashSet::new(),
            total_count: 100,
            filtered_count: 42,
        };
        let dict = fe.to_dict();
        let obj = dict.as_object().unwrap();
        assert_eq!(obj["total_count"].as_i64().unwrap(), 100);
        assert_eq!(obj["filtered_count"].as_i64().unwrap(), 42);
    }

    #[test]
    fn entity_node_related_edges_and_nodes_defaults_to_empty() {
        // Python: field(default_factory=list) → []
        let e = EntityNode::new("u", "X", vec![], "", Map::new());
        assert!(e.related_edges.is_empty());
        assert!(e.related_nodes.is_empty());

        let dict = e.to_dict();
        let obj = dict.as_object().unwrap();
        assert!(obj["related_edges"].as_array().unwrap().is_empty());
        assert!(obj["related_nodes"].as_array().unwrap().is_empty());
    }
}

// ---------------------------------------------------------------------------
// S-214..S-222 — KnowledgeGraphEntityReader
//
// Port of `ZepEntityReader` (`zep_entity_reader.py:71-435`).
//
// ## Design contract (DECISION-9)
//
// The Zep Cloud substrate is replaced by teri's native petgraph `KnowledgeGraph`.
// The reader borrows the graph for its lifetime; there is no network call, no API
// key, no `graph_id` parameter — those are `[≠]` inexpressible (Zep-auth/server
// artifacts).
//
// ### `[≠]` fields — every one corresponds to a DECISION-9 record, never a silent skip
//
// | Field | Source | Justification |
// |---|---|---|
// | `EntityNode.summary` | `""` | Zep auto-generates per-entity summaries server-side during ingestion; teri extraction produces none. Consumer (U-018 `_generate_profile_with_llm`) falls back to `"A {type} named {name}"`. DECISION-9 Q2. |
// | `EntityNode.attributes` | `{}` | teri `Entity` has no KV attribute bag; Zep attributes are server-extracted. Consumer guards `if entity.attributes:` → skips block (graceful). DECISION-9 Q2. |
// | edge `fact` | `""` | Zep's `fact` is an LLM-generated natural-language sentence produced during Zep ingestion; teri stores only `(kind, weight)`. Consumer falls back to `edge_name`+`direction` template. DECISION-9 Q4. |
// | edge `uuid` | `""` | Edge UUID is read by NO consumer of `get_all_edges`/`get_node_edges` in MiroFish. teri `Relation` has no uuid. DECISION-9 Q4. |
// | edge `attributes` | `{}` | No consumer reads it; no source on `Relation`. DECISION-9 Q4. |
// | `_call_with_retry` | not ported | An in-process petgraph read has no I/O — there is no transient failure to retry. The `except→None`/`except→[]` *fallbacks* ARE ported (observable contracts). DECISION-9 Q9. |
// | `ZepEntityReader.__init__` (api_key, Zep client) | not ported | Zep-auth for a network client; in-process petgraph read has no auth. DECISION-9 Q1. |
// | `graph_id` param on every method | not ported | Zep server-graph selector; the bound `&KnowledgeGraph` is the teri selector. DECISION-9 Q1. |
//
// ---------------------------------------------------------------------------

/// Reader that queries a native `KnowledgeGraph` using the same observable interface
/// that `ZepEntityReader` exposed against Zep Cloud.
///
/// Port of `ZepEntityReader` (`zep_entity_reader.py:71-435`). See module-level `[≠]`
/// table for inexpressible fields (each has a DECISION-9 record and a consumer-side
/// graceful fallback).
///
/// # Lifetime
/// The reader borrows the graph; it cannot outlive the `KnowledgeGraph` reference.
///
/// S-214 (`ZepEntityReader` class)
pub struct KnowledgeGraphEntityReader<'a> {
    /// The knowledge graph being read.  Replaces the `graph_id: str` Zep server-handle
    /// that every Python method accepted; the bound reference IS the selector.
    graph: &'a KnowledgeGraph,
}

impl<'a> KnowledgeGraphEntityReader<'a> {
    /// Create a reader over `graph`.
    ///
    /// Replaces `ZepEntityReader.__init__(api_key)` (S-215).  No api_key, no Zep client —
    /// `[≠]` inexpressible (DECISION-9 Q1): in-process petgraph read has no auth, no client.
    ///
    /// S-215
    pub fn new(graph: &'a KnowledgeGraph) -> Self {
        Self { graph }
    }

    // -----------------------------------------------------------------------
    // S-217  get_all_nodes
    // -----------------------------------------------------------------------

    /// Returns all entities as node dicts.
    ///
    /// Port of `ZepEntityReader.get_all_nodes(graph_id)` (`zep_entity_reader.py:127-152`).
    ///
    /// Each dict has 5 keys: `uuid`, `name`, `labels`, `summary`, `attributes`.
    ///
    /// - `summary` is `""` — `[≠]` DECISION-9 Q2: teri extraction produces no per-entity summary.
    /// - `attributes` is `{}` — `[≠]` DECISION-9 Q2: teri `Entity` has no attribute bag.
    /// - `labels` is `[entity.kind.to_string()]` — PORT (mapped): 1-element vec carrying the
    ///   EntityKind Display token (built-ins lowercase, Custom verbatim). DECISION-9 Q2.
    ///
    /// `graph_id: str` is not a parameter — `[≠]` inexpressible (DECISION-9 Q1).
    ///
    /// S-217
    pub fn get_all_nodes(&self) -> Vec<Value> {
        self.graph
            .get_all_entities()
            .into_iter()
            .map(|entity| build_node_dict(entity.id, &entity.name, &entity.kind.to_string()))
            .collect()
    }

    // -----------------------------------------------------------------------
    // S-218  get_all_edges
    // -----------------------------------------------------------------------

    /// Returns all relations as full edge dicts.
    ///
    /// Port of `ZepEntityReader.get_all_edges(graph_id)` (`zep_entity_reader.py:154-180`).
    ///
    /// Each dict has 6 keys: `uuid`, `name`, `fact`, `source_node_uuid`,
    /// `target_node_uuid`, `attributes`.
    ///
    /// - `uuid` is `""` — `[≠]` DECISION-9 Q4: `Relation` has no uuid; no consumer reads it.
    /// - `fact` is `""` — `[≠]` DECISION-9 Q4: consumer uses `edge_name`+`direction` fallback.
    /// - `attributes` is `{}` — `[≠]` DECISION-9 Q4: no source, no consumer reads it.
    /// - `name` ← `relation.kind.to_string()` — PORT (mapped).
    ///
    /// S-218
    pub fn get_all_edges(&self) -> Vec<Value> {
        self.graph
            .get_all_edges()
            .into_iter()
            .map(|(from_id, to_id, relation)| {
                build_full_edge_dict(&relation.kind.to_string(), &from_id, &to_id)
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // S-219  get_node_edges
    // -----------------------------------------------------------------------

    /// Returns all incident edges for the entity identified by `node_uuid`.
    ///
    /// Port of `ZepEntityReader.get_node_edges(node_uuid)` (`zep_entity_reader.py:182-213`).
    ///
    /// ## Error contract (PORTED — DECISION-9 Q8/Q9)
    /// - If `node_uuid` cannot be parsed as a UUID, returns `[]`.
    /// - If the UUID is not found in the graph, returns `[]`.
    /// - Any other internal error also returns `[]`.
    ///
    /// The retry/backoff from `_call_with_retry` is `[≠]` non-contractual (DECISION-9 Q9):
    /// an in-process petgraph lookup has no transient I/O failures to retry.
    ///
    /// Each dict has the FULL shape (`uuid=""`, `name`, `fact=""`, `source_node_uuid`,
    /// `target_node_uuid`, `attributes={}`).
    ///
    /// S-219
    pub fn get_node_edges(&self, node_uuid: &str) -> Vec<Value> {
        // Parse uuid; invalid string → except→[] (PORTED error contract)
        let id = match node_uuid.parse::<Uuid>() {
            Ok(id) => id,
            Err(_) => return Vec::new(),
        };

        // Missing entity → except→[] (PORTED error contract)
        match self.graph.get_neighbor_relations(id) {
            Ok(rels) => rels
                .into_iter()
                .map(|(neighbor, relation, is_outgoing)| {
                    let (from_id, to_id) =
                        if is_outgoing { (id, neighbor.id) } else { (neighbor.id, id) };
                    build_full_edge_dict(&relation.kind.to_string(), &from_id, &to_id)
                })
                .collect(),
            // entity not found → except→[] (PORTED error contract)
            Err(_) => Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // S-220  filter_defined_entities
    // -----------------------------------------------------------------------

    /// Filters entities to those with a "custom" (non-base) kind label, optionally
    /// restricted to a caller-supplied set of type names, and optionally enriched with
    /// incident edges and related-node summaries.
    ///
    /// Port of `ZepEntityReader.filter_defined_entities(graph_id, defined_entity_types,
    /// enrich_with_edges)` (`zep_entity_reader.py:215-331`).
    ///
    /// ## Filter logic (DECISION-9 Q3 / Q5)
    ///
    /// In MiroFish, nodes whose `labels` are only `{"Entity", "Node"}` are skipped — they
    /// have no custom type.  In teri, every `Entity` carries a typed `kind`, so `labels` is
    /// always `[kind.to_string()]` — a 1-element vec of the real kind Display token.  The
    /// token for built-in kinds is lowercase (`"person"`, `"organization"`, …); for
    /// `Custom(name)` it is the PascalCase name verbatim.  None of these equals the exact
    /// strings `"Entity"` or `"Node"`, so the skip branch is an **always-pass** in teri —
    /// but the code is ported verbatim (not deleted), keeping faithful branch coverage and
    /// correctness for any future label-set change.
    ///
    /// `defined_entity_types` match is against the EntityKind Display string:
    /// `"person"` matches `EntityKind::Person`, `"MediaOutlet"` matches
    /// `EntityKind::Custom("MediaOutlet")` — same divergence accepted in DECISION-8.
    ///
    /// ## `[≠]` fields on returned `EntityNode`
    /// `summary=""`, `attributes={}` — see module-level table.
    ///
    /// S-220
    pub fn filter_defined_entities(
        &self,
        defined_entity_types: Option<&[String]>,
        enrich_with_edges: bool,
    ) -> FilteredEntities {
        let all_entities = self.graph.get_all_entities();
        let total_count = self.graph.entity_count() as i64;

        // Build an id→entity map for O(1) related-node resolution.
        let entity_map: HashMap<Uuid, &crate::graph::Entity> =
            all_entities.iter().map(|e| (e.id, *e)).collect();

        let mut filtered_entities: Vec<EntityNode> = Vec::new();
        let mut entity_types_found: HashSet<String> = HashSet::new();

        for entity in &all_entities {
            let labels = vec![entity.kind.to_string()];

            // MiroFish skip logic (Q3): labels - {"Entity","Node"} → custom_labels.
            // In teri this is always-pass (no teri Display token equals "Entity"/"Node"),
            // but the branch is ported verbatim per DECISION-9 Q3 / "PORT with unreachable branch".
            let custom_labels: Vec<&str> = labels
                .iter()
                .map(String::as_str)
                .filter(|l| *l != "Entity" && *l != "Node")
                .collect();

            if custom_labels.is_empty() {
                // Only base labels — skip (always-pass in teri; ported verbatim).
                continue;
            }

            // If defined_entity_types provided, check for a matching label.
            let entity_type: String = if let Some(types) = defined_entity_types {
                let matching: Vec<&&str> =
                    custom_labels.iter().filter(|l| types.iter().any(|t| t == **l)).collect();
                if matching.is_empty() {
                    continue;
                }
                (*matching[0]).to_string()
            } else {
                custom_labels[0].to_string()
            };

            entity_types_found.insert(entity_type);

            // Build EntityNode — [≠] fields summary="" and attributes={} per DECISION-9 Q2.
            let mut node = EntityNode::new(
                entity.id.to_string(),
                entity.name.clone(),
                labels,
                "",         // [≠] summary — DECISION-9 Q2: no per-entity summary in teri
                Map::new(), // [≠] attributes — DECISION-9 Q2: no attribute bag in teri
            );

            if enrich_with_edges {
                enrich_entity_node(&mut node, entity.id, self.graph, &entity_map);
            }

            filtered_entities.push(node);
        }

        let filtered_count = filtered_entities.len() as i64;

        FilteredEntities {
            entities: filtered_entities,
            entity_types: entity_types_found,
            total_count,
            filtered_count,
        }
    }

    // -----------------------------------------------------------------------
    // S-221  get_entity_with_context
    // -----------------------------------------------------------------------

    /// Returns a single `EntityNode` with full edge+related-node context, or `None`
    /// when the uuid is missing or invalid.
    ///
    /// Port of `ZepEntityReader.get_entity_with_context(graph_id, entity_uuid)`
    /// (`zep_entity_reader.py:333-411`).
    ///
    /// ## Error contract (PORTED — DECISION-9 Q7/Q9)
    /// - UUID parse failure → `None`.
    /// - UUID not in graph → `None`.
    /// - Any other internal error → `None`.
    ///
    /// The retry/backoff (`_call_with_retry`) is `[≠]` non-contractual (DECISION-9 Q9):
    /// an in-process petgraph lookup has no transient I/O.
    ///
    /// ## `[≠]` fields
    /// `summary=""`, `attributes={}` — see module-level table.
    ///
    /// S-221
    pub fn get_entity_with_context(&self, entity_uuid: &str) -> Option<EntityNode> {
        // Parse uuid; invalid → except→None (PORTED error contract)
        let id = entity_uuid.parse::<Uuid>().ok()?;

        // Look up entity; missing → except→None (PORTED error contract)
        let entity = self.graph.get_entity_by_id(id)?;

        // Build id→entity map for related-node resolution.
        let all_entities = self.graph.get_all_entities();
        let entity_map: HashMap<Uuid, &crate::graph::Entity> =
            all_entities.iter().map(|e| (e.id, *e)).collect();

        let mut node = EntityNode::new(
            entity.id.to_string(),
            entity.name.clone(),
            vec![entity.kind.to_string()],
            "",         // [≠] summary — DECISION-9 Q2
            Map::new(), // [≠] attributes — DECISION-9 Q2
        );

        // Enrich with edges and related nodes; any internal error → None (PORTED contract).
        enrich_entity_node(&mut node, id, self.graph, &entity_map);

        Some(node)
    }

    // -----------------------------------------------------------------------
    // S-222  get_entities_by_type
    // -----------------------------------------------------------------------

    /// Returns all entities of the given `entity_type`.
    ///
    /// Port of `ZepEntityReader.get_entities_by_type(graph_id, entity_type, enrich_with_edges)`
    /// (`zep_entity_reader.py:413-435`).
    ///
    /// Delegates to `filter_defined_entities(Some(&[entity_type.to_string()]), enrich_with_edges)`
    /// and returns `.entities`.  1:1 port of the Python delegation (L413-435).
    ///
    /// S-222
    pub fn get_entities_by_type(
        &self,
        entity_type: &str,
        enrich_with_edges: bool,
    ) -> Vec<EntityNode> {
        self.filter_defined_entities(Some(&[entity_type.to_string()]), enrich_with_edges)
            .entities
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a node dict with 5 keys: uuid, name, labels, summary="", attributes={}.
///
/// `summary=""` and `attributes={}` are `[≠]` DECISION-9 Q2 fields emitted as specified
/// (inexpressible Zep server-generated fields; consumers have explicit graceful fallbacks).
fn build_node_dict(id: Uuid, name: &str, kind_str: &str) -> Value {
    let mut map = serde_json::Map::with_capacity(5);
    map.insert("uuid".to_string(), Value::String(id.to_string()));
    map.insert("name".to_string(), Value::String(name.to_string()));
    map.insert("labels".to_string(), Value::Array(vec![Value::String(kind_str.to_string())]));
    // [≠] summary — DECISION-9 Q2: teri extraction produces no per-entity summary
    map.insert("summary".to_string(), Value::String(String::new()));
    // [≠] attributes — DECISION-9 Q2: teri Entity has no attribute bag
    map.insert("attributes".to_string(), Value::Object(serde_json::Map::new()));
    Value::Object(map)
}

/// Build a full edge dict with 6 keys: uuid="", name, fact="", source_node_uuid,
/// target_node_uuid, attributes={}.
///
/// - `uuid=""` — `[≠]` DECISION-9 Q4: `Relation` has no uuid; no consumer reads it.
/// - `fact=""` — `[≠]` DECISION-9 Q4: Zep LLM-generated sentence; no portable source.
/// - `attributes={}` — `[≠]` DECISION-9 Q4: no source, no consumer reads it.
fn build_full_edge_dict(kind_str: &str, from_id: &Uuid, to_id: &Uuid) -> Value {
    let mut map = serde_json::Map::with_capacity(6);
    // [≠] uuid — DECISION-9 Q4: Relation has no uuid; no consumer reads this field
    map.insert("uuid".to_string(), Value::String(String::new()));
    map.insert("name".to_string(), Value::String(kind_str.to_string()));
    // [≠] fact — DECISION-9 Q4: Zep LLM-generated sentence; consumer uses edge_name fallback
    map.insert("fact".to_string(), Value::String(String::new()));
    map.insert("source_node_uuid".to_string(), Value::String(from_id.to_string()));
    map.insert("target_node_uuid".to_string(), Value::String(to_id.to_string()));
    // [≠] attributes — DECISION-9 Q4: no source on Relation, no consumer reads it
    map.insert("attributes".to_string(), Value::Object(serde_json::Map::new()));
    Value::Object(map)
}

/// Enrich a `EntityNode` with incident edges and related-node summaries.
///
/// Uses `get_neighbor_relations` (O(degree)) — provably equivalent to MiroFish's O(n·e)
/// `all_edges` scan (DECISION-9 Q5): same edge set, same direction labels, same opposite
/// endpoints, but O(degree) per node instead of O(n·e).
///
/// `related_nodes` dedup: endpoint ids are collected into a `HashSet` first (matching
/// MiroFish's `related_node_uuids: set()` semantics) so a multi-edge pair yields one entry.
fn enrich_entity_node(
    node: &mut EntityNode,
    entity_id: Uuid,
    graph: &KnowledgeGraph,
    entity_map: &HashMap<Uuid, &crate::graph::Entity>,
) {
    let rels = match graph.get_neighbor_relations(entity_id) {
        Ok(r) => r,
        // entity not found — treat as no edges (shouldn't happen since we just iterated it)
        Err(_) => return,
    };

    let mut related_edges: Vec<Value> = Vec::new();
    let mut related_node_ids: HashSet<Uuid> = HashSet::new();

    for (neighbor, relation, is_outgoing) in rels {
        let direction = if is_outgoing { "outgoing" } else { "incoming" };
        let edge_name = relation.kind.to_string();

        // related_edges dict shape: {direction, edge_name, fact="", target_node_uuid|source_node_uuid}
        // MiroFish outgoing: {direction, edge_name, fact, target_node_uuid}
        // MiroFish incoming: {direction, edge_name, fact, source_node_uuid}
        let mut edge_map = serde_json::Map::with_capacity(4);
        edge_map.insert("direction".to_string(), Value::String(direction.to_string()));
        edge_map.insert("edge_name".to_string(), Value::String(edge_name));
        // [≠] fact — DECISION-9 Q4: empty; consumer falls back to edge_name+direction template
        edge_map.insert("fact".to_string(), Value::String(String::new()));
        if is_outgoing {
            edge_map.insert("target_node_uuid".to_string(), Value::String(neighbor.id.to_string()));
        } else {
            edge_map.insert("source_node_uuid".to_string(), Value::String(neighbor.id.to_string()));
        }
        related_edges.push(Value::Object(edge_map));
        related_node_ids.insert(neighbor.id);
    }

    node.related_edges = related_edges;

    // Resolve related node ids to {uuid, name, labels, summary} dicts.
    // summary="" here too — [≠] DECISION-9 Q4 / Q2 (consumer at L467-470 omits summary suffix).
    let related_nodes: Vec<Value> = related_node_ids
        .iter()
        .filter_map(|rid| entity_map.get(rid).copied())
        .map(|rel_entity| {
            let mut rn = serde_json::Map::with_capacity(4);
            rn.insert("uuid".to_string(), Value::String(rel_entity.id.to_string()));
            rn.insert("name".to_string(), Value::String(rel_entity.name.clone()));
            rn.insert(
                "labels".to_string(),
                Value::Array(vec![Value::String(rel_entity.kind.to_string())]),
            );
            // [≠] summary — DECISION-9 Q2/Q4: no per-entity summary; consumer omits summary suffix gracefully
            rn.insert("summary".to_string(), Value::String(String::new()));
            Value::Object(rn)
        })
        .collect();

    node.related_nodes = related_nodes;
}

// ---------------------------------------------------------------------------
// Tests for KnowledgeGraphEntityReader (S-214..S-222)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod reader_tests {
    use super::*;
    use crate::graph::{Entity, EntityKind, KnowledgeGraph, Relation, RelationKind};

    /// Build a small test graph:
    ///
    /// ```
    ///   Alice (Person) --[WorksFor]--> Acme (Organization)
    ///   Alice (Person) --[RelatedTo]--> Events (Custom("Conference"))
    ///   Bob   (Person) --[WorksFor]--> Acme (Organization)
    /// ```
    fn make_test_graph() -> KnowledgeGraph {
        let mut g = KnowledgeGraph::new();
        let alice = Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            name: "Alice".to_string(),
            kind: EntityKind::Person,
        };
        let acme = Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
            name: "Acme".to_string(),
            kind: EntityKind::Organization,
        };
        let conf = Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(),
            name: "RustConf".to_string(),
            kind: EntityKind::Custom("Conference".to_string()),
        };
        let bob = Entity {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000004").unwrap(),
            name: "Bob".to_string(),
            kind: EntityKind::Person,
        };
        let idx_alice = g.add_entity(alice).unwrap();
        let idx_acme = g.add_entity(acme).unwrap();
        let idx_conf = g.add_entity(conf).unwrap();
        let idx_bob = g.add_entity(bob).unwrap();

        let works_for = Relation::new(RelationKind::WorksFor, 1.0).unwrap();
        let related_to = Relation::new(RelationKind::RelatedTo, 0.5).unwrap();
        let bob_works = Relation::new(RelationKind::WorksFor, 1.0).unwrap();

        g.add_relation(idx_alice, idx_acme, works_for);
        g.add_relation(idx_alice, idx_conf, related_to);
        g.add_relation(idx_bob, idx_acme, bob_works);
        g
    }

    /// Regression (opus parity gate, U-016 FAIL→fix): a self-loop `X→X` is returned by
    /// petgraph in BOTH directed passes, so `get_neighbor_relations` previously emitted it
    /// TWICE, double-counting it in every reader path. MiroFish's exclusive `if/elif`
    /// (`zep_entity_reader.py:288-303`) classifies a single edge once — a self-loop hits the
    /// `if source==node` branch (outgoing) and never the `elif`. The fix skips self-loops in
    /// the incoming pass so all three reader paths emit it exactly once, as outgoing.
    #[test]
    fn self_loop_edge_emitted_once_as_outgoing() {
        let mut g = KnowledgeGraph::new();
        let acme_uuid = "00000000-0000-0000-0000-0000000000aa";
        let acme = Entity {
            id: Uuid::parse_str(acme_uuid).unwrap(),
            name: "Acme".to_string(),
            kind: EntityKind::Organization,
        };
        let idx = g.add_entity(acme).unwrap();
        g.add_relation(idx, idx, Relation::new(RelationKind::RelatedTo, 1.0).unwrap());

        let r = KnowledgeGraphEntityReader::new(&g);

        // get_node_edges: the self-loop appears once (was 2 before the fix).
        let edges = r.get_node_edges(acme_uuid);
        assert_eq!(edges.len(), 1, "self-loop must not be double-counted in get_node_edges");

        // filter_defined_entities enrichment: related_edges has the self-loop once, outgoing.
        let filtered = r.filter_defined_entities(None, true);
        let node = filtered.entities.iter().find(|e| e.uuid == acme_uuid).unwrap();
        assert_eq!(node.related_edges.len(), 1, "enriched self-loop must appear once");
        assert_eq!(node.related_edges[0]["direction"], "outgoing");
        assert_eq!(node.related_edges[0]["target_node_uuid"], acme_uuid);

        // get_entity_with_context: same single outgoing self-loop.
        let ctx = r.get_entity_with_context(acme_uuid).unwrap();
        assert_eq!(ctx.related_edges.len(), 1);
        assert_eq!(ctx.related_edges[0]["direction"], "outgoing");
    }

    // -----------------------------------------------------------------------
    // get_all_nodes
    // -----------------------------------------------------------------------

    #[test]
    fn get_all_nodes_returns_all_entities() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let nodes = r.get_all_nodes();
        assert_eq!(nodes.len(), 4);
    }

    #[test]
    fn get_all_nodes_dict_shape_has_5_keys() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let nodes = r.get_all_nodes();
        for n in &nodes {
            let obj = n.as_object().unwrap();
            assert_eq!(obj.len(), 5, "each node dict must have exactly 5 keys");
            assert!(obj.contains_key("uuid"));
            assert!(obj.contains_key("name"));
            assert!(obj.contains_key("labels"));
            assert!(obj.contains_key("summary"));
            assert!(obj.contains_key("attributes"));
        }
    }

    #[test]
    fn get_all_nodes_summary_is_empty_per_decision_9() {
        // [≠] DECISION-9 Q2: summary is always "" (inexpressible Zep field)
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        for n in r.get_all_nodes() {
            assert_eq!(n["summary"].as_str().unwrap(), "", "[≠] summary must be empty");
        }
    }

    #[test]
    fn get_all_nodes_attributes_is_empty_per_decision_9() {
        // [≠] DECISION-9 Q2: attributes is always {} (inexpressible Zep field)
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        for n in r.get_all_nodes() {
            let attrs = n["attributes"].as_object().unwrap();
            assert!(attrs.is_empty(), "[≠] attributes must be empty map");
        }
    }

    #[test]
    fn get_all_nodes_labels_is_one_element_kind_display() {
        // labels = [kind.to_string()] — PORT (mapped), DECISION-9 Q2
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let nodes = r.get_all_nodes();
        // All nodes should have exactly 1 label
        for n in &nodes {
            let labels = n["labels"].as_array().unwrap();
            assert_eq!(labels.len(), 1);
        }
        // Find Alice's node and check label = "person"
        let alice = nodes.iter().find(|n| n["name"].as_str().unwrap() == "Alice").unwrap();
        assert_eq!(alice["labels"][0].as_str().unwrap(), "person");
        // Find RustConf's node and check label = "Conference" (Custom PascalCase verbatim)
        let conf = nodes.iter().find(|n| n["name"].as_str().unwrap() == "RustConf").unwrap();
        assert_eq!(conf["labels"][0].as_str().unwrap(), "Conference");
    }

    // -----------------------------------------------------------------------
    // get_all_edges
    // -----------------------------------------------------------------------

    #[test]
    fn get_all_edges_returns_all_relations() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let edges = r.get_all_edges();
        assert_eq!(edges.len(), 3);
    }

    #[test]
    fn get_all_edges_dict_shape_has_6_keys() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        for e in r.get_all_edges() {
            let obj = e.as_object().unwrap();
            assert_eq!(obj.len(), 6);
            assert!(obj.contains_key("uuid"));
            assert!(obj.contains_key("name"));
            assert!(obj.contains_key("fact"));
            assert!(obj.contains_key("source_node_uuid"));
            assert!(obj.contains_key("target_node_uuid"));
            assert!(obj.contains_key("attributes"));
        }
    }

    #[test]
    fn get_all_edges_inexpressible_fields_per_decision_9() {
        // [≠] DECISION-9 Q4: uuid="", fact="", attributes={}
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        for e in r.get_all_edges() {
            assert_eq!(e["uuid"].as_str().unwrap(), "", "[≠] edge uuid must be empty");
            assert_eq!(e["fact"].as_str().unwrap(), "", "[≠] fact must be empty");
            let attrs = e["attributes"].as_object().unwrap();
            assert!(attrs.is_empty(), "[≠] edge attributes must be empty");
        }
    }

    #[test]
    fn get_all_edges_name_is_relation_kind_display() {
        // name ← relation.kind.to_string() — PORT (mapped), DECISION-9 Q4
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let names: HashSet<String> = r
            .get_all_edges()
            .iter()
            .map(|e| e["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains("WorksFor"));
        assert!(names.contains("RelatedTo"));
    }

    // -----------------------------------------------------------------------
    // get_node_edges
    // -----------------------------------------------------------------------

    #[test]
    fn get_node_edges_returns_incident_edges_for_known_entity() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let alice_uuid = "00000000-0000-0000-0000-000000000001";
        let edges = r.get_node_edges(alice_uuid);
        // Alice has 2 outgoing edges: WorksFor → Acme, RelatedTo → RustConf
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn get_node_edges_returns_empty_for_invalid_uuid() {
        // except→[] error contract — DECISION-9 Q8/Q9
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let edges = r.get_node_edges("not-a-uuid");
        assert!(edges.is_empty(), "invalid uuid must return []");
    }

    #[test]
    fn get_node_edges_returns_empty_for_missing_uuid() {
        // except→[] error contract — DECISION-9 Q8/Q9
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let edges = r.get_node_edges("ffffffff-ffff-ffff-ffff-ffffffffffff");
        assert!(edges.is_empty(), "missing uuid must return []");
    }

    #[test]
    fn get_node_edges_dict_shape_has_6_keys() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let acme_uuid = "00000000-0000-0000-0000-000000000002";
        let edges = r.get_node_edges(acme_uuid);
        // Acme has 2 incoming edges (from Alice and Bob)
        assert_eq!(edges.len(), 2);
        for e in &edges {
            let obj = e.as_object().unwrap();
            assert_eq!(obj.len(), 6);
        }
    }

    #[test]
    fn get_node_edges_source_target_direction_correct() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let alice_uuid = "00000000-0000-0000-0000-000000000001";
        let acme_uuid = "00000000-0000-0000-0000-000000000002";
        let edges = r.get_node_edges(alice_uuid);
        // All should be outgoing with source=alice, target=neighbour
        for e in &edges {
            assert_eq!(
                e["source_node_uuid"].as_str().unwrap(),
                alice_uuid,
                "outgoing edge source must be alice"
            );
        }
        // From Acme's perspective these same edges are incoming
        let acme_edges = r.get_node_edges(acme_uuid);
        for e in &acme_edges {
            // target_node_uuid should be acme for incoming edges
            assert_eq!(
                e["target_node_uuid"].as_str().unwrap(),
                acme_uuid,
                "incoming edge target must be acme"
            );
        }
    }

    // -----------------------------------------------------------------------
    // filter_defined_entities
    // -----------------------------------------------------------------------

    #[test]
    fn filter_defined_entities_no_filter_returns_all_typed() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let fe = r.filter_defined_entities(None, false);
        // All 4 entities have typed kinds — none skipped
        assert_eq!(fe.total_count, 4);
        assert_eq!(fe.filtered_count, 4);
        assert_eq!(fe.entities.len(), 4);
    }

    #[test]
    fn filter_defined_entities_entity_types_set_correct() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let fe = r.filter_defined_entities(None, false);
        // entity_types: "person", "organization", "Conference"
        assert!(fe.entity_types.contains("person"));
        assert!(fe.entity_types.contains("organization"));
        assert!(fe.entity_types.contains("Conference"));
        assert_eq!(fe.entity_types.len(), 3);
    }

    #[test]
    fn filter_defined_entities_with_type_filter_keeps_matching() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let types = vec!["person".to_string()];
        let fe = r.filter_defined_entities(Some(&types), false);
        // Alice and Bob are persons
        assert_eq!(fe.filtered_count, 2);
        assert_eq!(fe.entities.len(), 2);
        assert_eq!(fe.entity_types.len(), 1);
        assert!(fe.entity_types.contains("person"));
    }

    #[test]
    fn filter_defined_entities_custom_kind_filter() {
        // "Conference" is EntityKind::Custom("Conference") — Display = "Conference"
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let types = vec!["Conference".to_string()];
        let fe = r.filter_defined_entities(Some(&types), false);
        assert_eq!(fe.filtered_count, 1);
        assert_eq!(fe.entities[0].name, "RustConf");
        assert!(fe.entity_types.contains("Conference"));
    }

    #[test]
    fn filter_defined_entities_no_match_returns_empty() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let types = vec!["Student".to_string()];
        let fe = r.filter_defined_entities(Some(&types), false);
        assert_eq!(fe.filtered_count, 0);
        assert!(fe.entities.is_empty());
        assert!(fe.entity_types.is_empty());
        // total_count still reflects all entities
        assert_eq!(fe.total_count, 4);
    }

    #[test]
    fn filter_defined_entities_total_count_is_all_entities() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let types = vec!["person".to_string()];
        let fe = r.filter_defined_entities(Some(&types), false);
        // total = 4 (all), filtered = 2 (persons)
        assert_eq!(fe.total_count, 4);
        assert_eq!(fe.filtered_count, 2);
    }

    #[test]
    fn filter_defined_entities_enrich_populates_related_edges() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let types = vec!["person".to_string()];
        let fe = r.filter_defined_entities(Some(&types), true);
        // Alice has 2 outgoing edges; Bob has 1 outgoing edge
        let alice = fe.entities.iter().find(|e| e.name == "Alice").unwrap();
        assert_eq!(alice.related_edges.len(), 2);
        let bob = fe.entities.iter().find(|e| e.name == "Bob").unwrap();
        assert_eq!(bob.related_edges.len(), 1);
    }

    #[test]
    fn filter_defined_entities_related_edge_direction_field() {
        // direction = "outgoing" or "incoming" — PORT, DECISION-9 Q4
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let types = vec!["person".to_string()];
        let fe = r.filter_defined_entities(Some(&types), true);
        let alice = fe.entities.iter().find(|e| e.name == "Alice").unwrap();
        // Alice's edges should all be outgoing
        for edge in &alice.related_edges {
            assert_eq!(
                edge["direction"].as_str().unwrap(),
                "outgoing",
                "alice edges must be outgoing"
            );
        }
    }

    #[test]
    fn filter_defined_entities_related_edge_fact_empty_per_decision_9() {
        // [≠] DECISION-9 Q4: fact="" on related_edges
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let fe = r.filter_defined_entities(None, true);
        for entity in &fe.entities {
            for edge in &entity.related_edges {
                assert_eq!(
                    edge["fact"].as_str().unwrap(),
                    "",
                    "[≠] related_edge fact must be empty"
                );
            }
        }
    }

    #[test]
    fn filter_defined_entities_related_nodes_populated() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let types = vec!["person".to_string()];
        let fe = r.filter_defined_entities(Some(&types), true);
        let alice = fe.entities.iter().find(|e| e.name == "Alice").unwrap();
        // Alice connects to Acme and RustConf — 2 related nodes
        assert_eq!(alice.related_nodes.len(), 2);
    }

    #[test]
    fn filter_defined_entities_related_node_shape() {
        // related_node = {uuid, name, labels, summary=""} — DECISION-9 Q4/Q5
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let types = vec!["person".to_string()];
        let fe = r.filter_defined_entities(Some(&types), true);
        let alice = fe.entities.iter().find(|e| e.name == "Alice").unwrap();
        for rn in &alice.related_nodes {
            let obj = rn.as_object().unwrap();
            assert_eq!(obj.len(), 4);
            assert!(obj.contains_key("uuid"));
            assert!(obj.contains_key("name"));
            assert!(obj.contains_key("labels"));
            assert!(obj.contains_key("summary"));
            // [≠] summary="" — DECISION-9 Q2/Q4
            assert_eq!(rn["summary"].as_str().unwrap(), "");
        }
    }

    #[test]
    fn filter_defined_entities_entity_node_summary_and_attributes_empty() {
        // [≠] DECISION-9 Q2: summary="" and attributes={} on kept EntityNodes
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let fe = r.filter_defined_entities(None, false);
        for e in &fe.entities {
            assert_eq!(e.summary, "", "[≠] EntityNode summary must be empty");
            assert!(e.attributes.is_empty(), "[≠] EntityNode attributes must be empty map");
        }
    }

    // -----------------------------------------------------------------------
    // get_entity_with_context
    // -----------------------------------------------------------------------

    #[test]
    fn get_entity_with_context_returns_entity_for_known_uuid() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let alice_uuid = "00000000-0000-0000-0000-000000000001";
        let result = r.get_entity_with_context(alice_uuid);
        assert!(result.is_some());
        let node = result.unwrap();
        assert_eq!(node.name, "Alice");
        assert_eq!(node.uuid, alice_uuid);
    }

    #[test]
    fn get_entity_with_context_returns_none_for_missing_uuid() {
        // except→None error contract — DECISION-9 Q7/Q9
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let result = r.get_entity_with_context("ffffffff-ffff-ffff-ffff-ffffffffffff");
        assert!(result.is_none(), "missing uuid must return None");
    }

    #[test]
    fn get_entity_with_context_returns_none_for_invalid_uuid() {
        // except→None error contract — DECISION-9 Q7/Q9
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let result = r.get_entity_with_context("garbage-uuid-string");
        assert!(result.is_none(), "invalid uuid must return None");
    }

    #[test]
    fn get_entity_with_context_enriches_with_edges_and_related_nodes() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let alice_uuid = "00000000-0000-0000-0000-000000000001";
        let node = r.get_entity_with_context(alice_uuid).unwrap();
        // Alice has 2 outgoing relations → 2 related_edges, 2 related_nodes
        assert_eq!(node.related_edges.len(), 2);
        assert_eq!(node.related_nodes.len(), 2);
    }

    #[test]
    fn get_entity_with_context_labels_is_kind_display() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let alice_uuid = "00000000-0000-0000-0000-000000000001";
        let node = r.get_entity_with_context(alice_uuid).unwrap();
        assert_eq!(node.labels, vec!["person"]);
    }

    #[test]
    fn get_entity_with_context_summary_empty_per_decision_9() {
        // [≠] DECISION-9 Q2: summary=""
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let node = r.get_entity_with_context("00000000-0000-0000-0000-000000000001").unwrap();
        assert_eq!(node.summary, "", "[≠] summary must be empty");
    }

    #[test]
    fn get_entity_with_context_attributes_empty_per_decision_9() {
        // [≠] DECISION-9 Q2: attributes={}
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let node = r.get_entity_with_context("00000000-0000-0000-0000-000000000001").unwrap();
        assert!(node.attributes.is_empty(), "[≠] attributes must be empty map");
    }

    // -----------------------------------------------------------------------
    // get_entities_by_type
    // -----------------------------------------------------------------------

    #[test]
    fn get_entities_by_type_delegates_to_filter_correctly() {
        // 1:1 delegation — DECISION-9 Q5 / S-222
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let entities = r.get_entities_by_type("person", false);
        assert_eq!(entities.len(), 2);
        let names: HashSet<String> = entities.iter().map(|e| e.name.clone()).collect();
        assert!(names.contains("Alice"));
        assert!(names.contains("Bob"));
    }

    #[test]
    fn get_entities_by_type_custom_kind() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let entities = r.get_entities_by_type("Conference", false);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "RustConf");
    }

    #[test]
    fn get_entities_by_type_unknown_type_returns_empty() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let entities = r.get_entities_by_type("Spaceship", false);
        assert!(entities.is_empty());
    }

    #[test]
    fn get_entities_by_type_enrich_with_edges_true() {
        let g = make_test_graph();
        let r = KnowledgeGraphEntityReader::new(&g);
        let entities = r.get_entities_by_type("organization", true);
        // Acme has 2 incoming edges (from Alice and Bob)
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].related_edges.len(), 2);
    }

    // -----------------------------------------------------------------------
    // get_entity_by_id (new additive KnowledgeGraph accessor, DECISION-9 Q6)
    // -----------------------------------------------------------------------

    #[test]
    fn get_entity_by_id_returns_entity_for_known_id() {
        let g = make_test_graph();
        let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let entity = g.get_entity_by_id(id);
        assert!(entity.is_some());
        assert_eq!(entity.unwrap().name, "Alice");
    }

    #[test]
    fn get_entity_by_id_returns_none_for_unknown_id() {
        let g = make_test_graph();
        let id = Uuid::parse_str("ffffffff-ffff-ffff-ffff-ffffffffffff").unwrap();
        assert!(g.get_entity_by_id(id).is_none());
    }
}
