# MiroFish → teri Merge Ledger

**Architect:** rust-port-architect · **Date:** 2026-06-14 · **target==dest==teri** (port and merge collapse).
**Schema:** `- [ ] <unit-id> · <class> · <rust-symbol or —> · <landing> · -> <teri-target> · refs: intra-teri · status`
**Classes:** `port-fresh` | `extend-Y` | `reuse-Y` | `map-onto-substrate`.
`reuse-Y`/`map-onto-substrate` rows are **verify-only vs MiroFish X (skip fresh port)** — confirm teri's existing surface matches X behavior; do not re-implement.
Status legend: `pending` (not started).

---

## Layer 0 — Config / Bootstrap
- [ ] U-001 · extend-Y · `AppConfig` · merge-into `teri::config` · -> teri::config::AppConfig · refs: main.rs, all consumers · pending
- [ ] U-002 · extend-Y · `serve_cmd` · merge-into `teri::main` · -> teri::main::serve_cmd · refs: server::create_app · pending
- [ ] U-003 · port-fresh · `create_app` · new module src/server/mod.rs · -> teri::server::create_app · refs: api::{graph,simulation,report}, /health · pending

## Layer 1 — Utilities
- [ ] U-004 · reuse-Y · — · verify-only `teri` tracing · -> tracing setup (main.rs) · refs: — · pending · verify-only vs X (skip fresh port)
- [ ] U-005 · port-fresh · `i18n::t` · new module src/i18n/mod.rs · -> teri::i18n · refs: report, api, SWEEP-2 · pending
- [ ] U-006 · reuse-Y · — · verify-only `src/llm.rs` · -> OpenAiAdapter retry (llm.rs:35) · refs: — · pending · verify-only vs X (skip fresh port)
- [ ] U-007 · map-onto-substrate · — · map-onto `src/graph` · -> KnowledgeGraph::get_all_entities/edges · refs: graph · pending · verify-only vs X (skip fresh port)
- [ ] U-008 · reuse-Y · — · verify-only `src/llm.rs` · -> OpenAiAdapter::complete/complete_json · refs: GAP-6 <think> add · pending · verify-only vs X (one ADD: <think> strip)
- [ ] U-009 · reuse-Y · — · verify-only `src/seed` · -> SeedIngestor::from_file · refs: — · pending · verify-only vs X (skip fresh port)
- [ ] U-010 · port-fresh · `PlatformActionLogger` · new module src/sim/action_logger.rs · -> teri::sim::action_logger · refs: sim::SimEngine · pending

## Layer 2 — Models
- [ ] U-011 · port-fresh · `ProjectManager` · new module src/models/project.rs · -> teri::models::project · refs: api::graph · pending
- [ ] U-012 · port-fresh · `TaskManager` · new module src/models/task.rs · -> teri::models::task · refs: api::*, sim::manager · pending

## Layer 3 — External Integration Services
- [ ] U-013 · reuse-Y · — · verify-only `src/seed` · -> seed chunk/preprocess · refs: — · pending · verify-only vs X (skip fresh port)
- [ ] U-014 · port-fresh · `OntologyGenerator` · new module src/graph/ontology.rs · -> teri::graph::ontology · refs: graph (EntityKind::Custom), llm · pending
- [ ] U-015 · map-onto-substrate · `KnowledgeGraph::build` · map-onto `src/graph` (wire pipeline) · -> teri::graph::KnowledgeGraph::build · refs: extraction helpers, llm, models::task · pending · verify-only vs X (build() pipeline must land first)
- [ ] U-016 · map-onto-substrate · — · map-onto `src/graph` · -> get_entity_with_context/get_entities_by_type · refs: graph::get_subgraph · pending · verify-only vs X (skip fresh port)
- [ ] U-017 · map-onto-substrate · — · map-onto `src/graph`+`src/report` · -> graph::search/panorama_search/insight_forge · refs: GAP-1 valid_at, GAP-2 vec, sim::ipc · pending · verify-only vs X (needs valid_at + vec impl)
- [ ] U-018 · extend-Y · `PersonaGenerator` · merge-into `src/agent` · -> teri::agent::Persona (social fields) · refs: agent/mod.rs:14, sim · pending
- [ ] U-019 · port-fresh · `SimConfigGenerator` · new module src/sim/config_generator.rs · -> teri::sim::config_generator · refs: llm, sim::SimConfig · pending
- [ ] U-020 · port-fresh · `InterviewBus` · new module src/sim/ipc.rs · -> teri::sim::ipc · refs: sim::SimEngine::inject_fn · pending
- [ ] U-021 · map-onto-substrate · `KnowledgeGraph::append_episode` · map-onto `src/graph` · -> teri::graph::append_episode · refs: AgentActivity::to_episode_text, sim tick · pending · verify-only vs X (append_episode must land)

## Layer 4-6 — Runner / Manager / Report
- [ ] U-022 · map-onto-substrate · `SimRunner` · map-onto `src/sim::SimEngine` · -> teri::sim::SimEngine · refs: action_logger, ipc, broadcast · pending · verify-only vs X (native — no subprocess)
- [ ] U-023 · port-fresh · `SimulationManager` · new module src/sim/manager.rs · -> teri::sim::manager · refs: models::task, graph, agent, config_generator · pending
- [ ] U-024 · extend-Y · `ReportAgent` · merge-into `src/report` · -> teri::report::ReportAgent · refs: report::generate_stream, graph search (U-017), llm · pending

## Layer 7 — HTTP API
- [ ] U-025 · port-fresh · `graph routes` · new module src/api/graph.rs · -> teri::api::graph · refs: models::{project,task}, graph::ontology, graph::build · pending
- [ ] U-026 · port-fresh · `simulation routes` · new module src/api/simulation.rs · -> teri::api::simulation · refs: sim::{manager,SimEngine}, api::streaming, models::project · pending
- [ ] U-027 · port-fresh · `report routes` · new module src/api/report.rs · -> teri::api::report · refs: report::ReportAgent, graph search · pending

## Layer 8 — Simulation Platforms (native; was OASIS subprocess)
- [ ] U-028 · map-onto-substrate · `TwitterPlatform` · map-onto `src/sim::SimEngine` · -> teri::sim::platform (Twitter) · refs: Action enum, action_logger · pending · verify-only vs X (reimplement OASIS twitter natively)
- [ ] U-029 · map-onto-substrate · `RedditPlatform` · map-onto `src/sim::SimEngine` · -> teri::sim::platform (Reddit) · refs: Action enum, action_logger · pending · verify-only vs X (reimplement OASIS reddit natively)
- [ ] U-030 · map-onto-substrate · `MultiPlatformRunner` · map-onto `src/sim` top-runner · -> teri::sim::multi_runner · refs: two SimEngine (tokio::join), OQ-1 · pending · verify-only vs X (dual-platform native)

## Layer 9 — Frontend (Vue kept, re-pointed — OQ-4)
- [ ] U-031 · port-fresh · `axios client` · keep-Vue frontend/src/api/index.js · -> teri axum API baseURL · refs: i18n Accept-Language · pending
- [ ] U-032 · port-fresh · `graph client` · keep-Vue · -> teri /api/graph · refs: U-025 routes · pending
- [ ] U-033 · port-fresh · `simulation client` · keep-Vue · -> teri /api/simulation · refs: U-026 routes · pending
- [ ] U-034 · port-fresh · `report client` · keep-Vue · -> teri /api/report · refs: U-027 routes · pending
- [ ] U-035 · reuse-Y · — · keep-Vue (unchanged) · -> Vue reactive store · refs: — · pending · verify-only vs X (skip fresh port)
- [ ] U-036 · extend-Y · — · keep-Vue + `src/i18n` · -> vue-i18n + locales/*.json · refs: SWEEP-2 · pending
- [ ] U-037 · reuse-Y · — · keep-Vue (unchanged) · -> vue-router · refs: — · pending · verify-only vs X (skip fresh port)
- [ ] U-038 · reuse-Y · — · keep-Vue (unchanged) · -> Home.vue · refs: U-032 · pending · verify-only vs X (skip fresh port)
- [ ] U-039 · reuse-Y · — · keep-Vue · -> MainView.vue · refs: U-032/U-033 · pending · verify-only vs X (verify task-poll shapes)
- [ ] U-040 · reuse-Y · — · keep-Vue · -> SimulationView.vue · refs: U-033 · pending · verify-only vs X (verify config/profile shapes)
- [ ] U-041 · extend-Y · — · keep-Vue · -> SimulationRunView.vue · refs: U-033/U-034, SSE · pending
- [ ] U-042 · reuse-Y · — · keep-Vue · -> ReportView.vue · refs: U-034 · pending · verify-only vs X (verify agent-log from_line)
- [ ] U-043 · reuse-Y · — · keep-Vue · -> InteractionView.vue · refs: U-034 · pending · verify-only vs X (verify chat shape)
- [ ] U-044 · reuse-Y · — · keep-Vue (unchanged) · -> main.js+App.vue · refs: U-036/U-037 · pending · verify-only vs X (skip fresh port)

## Layer 10 — Runtime / Concurrency contracts
- [ ] U-045 · map-onto-substrate · — · map-onto tokio runtime · -> tokio tasks · refs: sim, graph · pending · verify-only vs X (daemon threads → tokio tasks)
- [ ] U-046 · map-onto-substrate · `CancellationToken` · map-onto SimEngine · -> tokio_util CancellationToken · refs: sim::SimEngine · pending · verify-only vs X (no subprocess)
- [ ] U-047 · map-onto-substrate · — · map-onto channels · -> InterviewBus (U-020) · refs: sim::ipc · pending · verify-only vs X (file-IPC → channels)
- [ ] U-048 · reuse-Y · — · verify-only `src/sim` broadcast · -> SimEngine::subscribe_with_history · refs: — · pending · verify-only vs X (skip fresh port)
- [ ] U-049 · extend-Y · `shutdown handler` · merge-into `src/main`+`src/server` · -> graceful shutdown · refs: sim CancellationToken · pending
- [ ] U-050 · port-fresh · `LOCALE task-local` · new src/i18n (task-local) · -> teri::i18n::LOCALE · refs: U-005, spawn sites · pending

## Sweep items
- [ ] SWEEP-1 · reuse-Y · — · keep-Vue · -> 6 step components · refs: re-pointed clients · pending · verify-only vs X (skip fresh port)
- [ ] SWEEP-2 · port-fresh · `locales` · src/i18n/locales + keep-Vue · -> zh/en keys · refs: U-005, U-036 · pending
- [ ] SWEEP-3 · map-onto-substrate · `SimEngine` · map-onto `src/sim::SimEngine` · -> native SimEngine · refs: U-022/U-028/U-029/U-030 · pending · verify-only vs X (RECLASSIFIED: reimplement OASIS natively, was "Python subprocess")
- [ ] SWEEP-4 · — · — · (drop, non-prod) · -> — · refs: — · `- [≠]` intentional-divergence: utility test script, not production surface

---

## Cross-cutting gap rows (no silent drop — each blocks specific units)
- [!] GAP-1 · `Relation.valid_at: Option<(u64,Option<u64>)>` · blocks U-017/U-021 panorama_search active/historical · OQ-2
- [!] GAP-2 · `query_vec_similarity` impl via shimmy embeddings · blocks U-017 insight_forge semantic search · OQ-3
- [!] GAP-3 · `EntityKind::Custom(String)` dynamic kinds · blocks U-014 ontology generator · OQ-5
- [!] GAP-4 · per-platform action-validity matrix · blocks U-028/U-029 · Decision-2
- [!] GAP-5 · action-arg enrichment (post/user/comment) · informs U-030 · Decision-2
- [!] GAP-6 · `<think>...</think>` strip in OpenAiAdapter · blocks U-008 parity · Decision-3
