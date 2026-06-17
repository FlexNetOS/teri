//! Teri service modules — port of `backend/app/services/` (MiroFish).
//!
//! Each sub-module is a high-level service that orchestrates LLM calls,
//! validation, and post-processing to implement a discrete feature.

pub mod graph_builder;
pub mod ontology;
pub mod simulation_config;
