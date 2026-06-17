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

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;

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
        map.insert(
            "attributes".to_string(),
            Value::Object(self.attributes.clone()),
        );
        map.insert(
            "related_edges".to_string(),
            Value::Array(self.related_edges.clone()),
        );
        map.insert(
            "related_nodes".to_string(),
            Value::Array(self.related_nodes.clone()),
        );
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
        let types_arr: Vec<Value> = self
            .entity_types
            .iter()
            .map(|t| Value::String(t.clone()))
            .collect();
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
            ["uuid", "name", "labels", "summary", "attributes", "related_edges", "related_nodes"]
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
        let mut type_strings: Vec<String> = types_arr
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
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
