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
- [x] U-015 · map-onto-substrate · `KnowledgeGraph::build` · map-onto `src/graph` (wire pipeline) · -> teri::graph::KnowledgeGraph::build · refs: extraction helpers, llm, models::task · VERIFIED-IN-TERI 2026-06-14 · build() pipeline landed + parity-verified (5 branches match MiroFish; 156-test baseline intact; findings/parity.md). Open `- [!]` GAP-U015-1: chunking → U-013
- [ ] U-016 · map-onto-substrate · — · map-onto `src/graph` · -> get_entity_with_context/get_entities_by_type · refs: graph::get_subgraph · pending · verify-only vs X (skip fresh port)
- [ ] U-017 · port-fresh · `ZepToolsService` · new module src/services/zep_tools.rs · -> teri::services::zep_tools · refs: GAP-1 valid_at, GAP-2 vec, sim::ipc, report · pending · verify-only vs X (needs valid_at + vec impl)
- [ ] U-018 · extend-Y · `PersonaGenerator` · merge-into `src/agent` · -> teri::agent::Persona (social fields) · refs: agent/mod.rs:14, sim · pending
- [ ] U-019 · port-fresh · `SimConfigGenerator` · new module src/sim/config_generator.rs · -> teri::sim::config_generator · refs: llm, sim::SimConfig · pending
- [ ] U-020 · port-fresh · `InterviewBus` · new module src/sim/ipc.rs · -> teri::sim::ipc · refs: sim::SimEngine::inject_fn · pending
- [ ] U-021 · map-onto-substrate · `KnowledgeGraph::append_episode` · map-onto `src/graph` · -> teri::graph::append_episode · refs: AgentActivity::to_episode_text, sim tick · pending · verify-only vs X (append_episode must land)

## Layer 4-6 — Runner / Manager / Report
- [ ] U-022 · map-onto-substrate · `SimRunner` · map-onto `src/sim::SimEngine` · -> teri::sim::SimEngine · refs: action_logger, ipc, broadcast · pending · verify-only vs X (native — no subprocess)
- [ ] U-023 · port-fresh · `SimulationManager` · new module src/sim/manager.rs · -> teri::sim::manager · refs: models::task, graph, agent, config_generator · pending
- [ ] U-024 · extend-Y (CONFIRMED, not reuse-Y) · `ReportAgent` · merge-into `src/report` · -> teri::report::ReportAgent · refs: report::generate_stream, graph search (U-017), llm · pending · ARCH: findings/u024-architecture.md — BOTH paths coexist (template `PredictionReport` + ReACT `Report`); ZepTools wiring = `ReportTools<'g>{graph:&KnowledgeGraph}` facade (DECISION-9, zero blast radius, leaf struct), NOT Arc; graph_id=[≠] handle/PORTED label. Sub-cycles a→i: (a)data-model✓[parity PASS] (b)ReportTools-wiring✓[BLOCKER CLEARED — parity PASS, is_invalid bug fixed] (b2)insight_forge[OQ-3] (c)tool-dispatch✓[parity FAIL→fix→PASS: gate caught tier-1-normalization + int-coercion downgrades, both fixed] (d)plan_outline✓[parity PASS 7 surfaces; stateful ReportAgent+new_react; FIXED get_graph_statistics HashMap order downgrade] (e)ReACT-loop✓[parity PASS 12 surfaces; full branch ladder verbatim; 7 report_logger sites deferred to (g)] (f)ReportManager✓[parity PASS 7 surfaces, 29 methods byte-identical; regex content-shaping, JSON formats, back-compat; FIXED updated_at→python_isoformat_local] (g1)ReportLogger✓[parity FAIL→fix→PASS — new src/report/logger.rs, 13 helpers + log core (agent_log.jsonl compact, key-order timestamp/elapsed_seconds/report_id/action/stage/section_title/section_index/details, python_isoformat_local ts, 2dp banker's elapsed); wired 7 jsonl markers in generate_section_react (Option<ReportLogger> field, None=no-op); gate caught *_length byte-vs-char downgrade (.len()→.chars().count() ×4, Python len(str)=chars), fixed] (g2)ReportConsoleLogger✓[parity PASS 8 surfaces — architect-decided (findings/u024-g2-console-logger.md): per-report tracing Layer gated on process-global sink (1:1 of Python FileHandler-on-named-loggers); new src/report/console_logger.rs + additive src/logging.rs init; format [%H:%M:%S] LEVEL: msg, WARN→WARNING (#1 trap, correct), INFO+ floor, targets teri::report exact + teri::services::zep_tools prefix; 17 loop emissions wired (unconditional); verifier PROVED real capture (refuted test-skip concern, dumped console_log.txt); fwd-dep [!] zep_tools producers] | (g) COMPLETE (g1+g2). (h)generate_report [ARCH: findings/u024-h-generate-report.md — ReportSink=TRAIT (orthogonal to teri generate_stream, no downgrade); decomposed h1-h4. h1✓[parity PASS — new src/report/sink.rs (ReportEvent{progress:i32}/ReportStage lowercase/ReportSink trait/NullSink); FIXED update_progress u32→i32 parity bug (Python writes -1 on failed path, byte-identical progress.json); +console_logger field on ReportAgent; +ensure_report_folder pub +upload_folder accessor] h2✓[parity FAIL→fix→PASS (5+ surfaces) — generate_report skeleton: report_id uuid12-shape, status machine Pending→Planning→Generating→Completed/Failed, loggers create (log_start/planning_start/planning_complete/report_complete/error), plan_outline via prog//5 sink-closure (RefCell bridge: ProgressCallback is Fn, ReportSink::event is &mut), save_outline, placeholder assemble, both console_logger.close tails. GATE CAUGHT error-tail downgrade: built FRESH failed_report dropping outline/markdown_content/completed_at vs Python except mutating SAME report (retains them in FAILED meta.json) — FIXED by hoisting report before try-body so error arm mutates same object; new test drives EISDIR post-planning failure, asserts on-disk meta.json non-null outline. S-763 stays [~]→h2-skeleton-PASS (full contract completes at h3 section loop)] h3[per-section streaming loop + save_section-immediately + real assemble]←NEXT h4[U-027 SSE sink adapter]] interview_agents needs U-020 (d)plan_outline (e)ReACT-loop (f)ReportManager (g)loggers/Sink (h)generate_report (i)chat. Risks: [!]insight_forge/OQ-3, [!]interview_agents/U-020, text-tool-protocol-not-native (PORT verbatim).

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
- [!] GAP-6 · `<think>...</think>` strip in OpenAiAdapter · blocks U-008 parity · Decision-3 → **RESOLVED cycle-4** (strip_think + strip_json_fence, all 3 adapters)

---

## Cycle-4 verification reclassifications (AUTHORITATIVE over the reuse-Y rows above)
The differential gate (reuse is never trusted) refuted the reuse-Y class for every backend unit checked — findings in `findings/reuse-verify-{llm,seed,infra}.md`:
- [x] U-006 · reuse-Y→**extend-Y DONE** · retry recovery+cap+no-spurious tested, MAX_BACKOFF_SECS clamp; jitter `- [≠]`.
- [x] U-008 · reuse-Y→**extend-Y DONE** · `strip_think`+`strip_json_fence` across all 3 adapters; teri LLM = proven superset; GAP-6 resolved.
- [~] U-008 · **extend-Y (pending, DECISION-7)** · additive parameterized `chat`/`chat_json(&[ChatMessage], &ChatOptions)` on all 3 adapters (system+user roles, opt temperature/max_tokens) — `complete`/`complete_json`/`stream` byte-identical (no regression). Built once; reused by U-014 (trigger), U-019, U-024, U-018. Port before U-014. ← `chat_json` callers in `ontology_generator.py:217`, `llm_client.py:35-102`.
- [~] U-009 · reuse-Y→**extend-Y (pending)** · add encoding-fallback (GBK/Latin-1) + `.md` dispatch + `is_supported` gate + multi-file concat. ← file_parser.py.
- [~] U-013 · reuse-Y→**port-fresh (pending)** · teri has NO text chunking → new `src/seed/text_processor.rs` (`split_text` 500/50 + `preprocess` + `stats`). **Unblocks GAP-U015-1.** ← text_processor.py.
- [≠] U-004 · reuse-Y→**intentional-divergence** · teri logs to stdout by design (operators redirect; MiroFish itself does at simulation_runner.py:427); rotating-file appender omitted (owner-optional `tracing-appender` add).
- [~] U-048 (JSONL action-log streaming) · reuse-Y→**extend-Y (pending)** · 3/4 behaviors confirmed (ordering, no-loss/catch-up, backpressure); add in-band end-of-sim terminal signal to stream subscribers (`StreamEvent::sim_end` at sim/mod.rs:496 + api/mod.rs:82). ← action_logger.py:105.
