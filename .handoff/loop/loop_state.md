# Loop state — rust-port (MiroFish → teri, port-and-merge)
session_started: 2026-06-14T12:00:00Z
loop: rust-port
branch: port/mirofish
worktree: /home/drdave/Desktop/meta/.worktrees/mirofish-port/teri
source_root: /home/drdave/Desktop/meta/MiroFish
source_toolchain: python    # uv-managed; backend/.venv present; `cd backend && uv run python3 run.py`
rust_target: /home/drdave/Desktop/meta/.worktrees/mirofish-port/teri   # teri (the Rust port lands here)
dest_repo: /home/drdave/Desktop/meta/.worktrees/mirofish-port/teri     # SAME as rust_target — port target IS the merge dest
dest_branch: port/mirofish
dest_worktree: /home/drdave/Desktop/meta/.worktrees/mirofish-port/teri
dest_base: develop
# NOTE: rust_target == dest_repo == teri. The port lands Rust directly into teri's modules, so the
# "port" and "merge" steps collapse into one landing. The merge-ledger tracks class + landing decision;
# the no-downgrade-of-Y gate is teri's 142-test baseline (findings/y-regression.md). SUBSTRATES:
#   ollama/OpenAI endpoint → shimmy (Airframe, OpenAI-compatible) — DONE, map-onto-substrate
#   Zep Cloud SaaS graph/memory → teri petgraph (src/graph) + redb (src/memory) — map-onto-substrate
#   OASIS Python subprocess sim → teri native SimEngine (src/sim) — map-onto-substrate (REIMPLEMENT-in-Y)
cycle_budget: 3
cycles_this_session: 2   # RESUMED 2026-06-17 (6th resume, reset 0); c1=U-019 sub-cycle(b); c2=U-019 sub-cycle(c) agent-config gen
cycles_total: 22
# CYCLE2 (6th resume) 2026-06-17: U-019 sub-cycle (c) agent-config generation — opus FAIL→fix→PASS (S-450/451/452).
#   Added to SimulationConfigGenerator<L> in src/services/simulation_config.rs: S-451 generate_agent_configs_batch
#   (async; entity_list w/ AGENT_SUMMARY_LENGTH char-trunc; byte-verbatim Chinese prompt 1657B + system_prompt 400B
#   incl get_language_instruction + English stance note; LLM-failure→rule fallback no fault; BATCH defaults DIFFER
#   from dataclass: posts 0.5/comments 1.0/active_hours [9..=22]14elem), S-452 generate_agent_config_by_rule (6
#   branches, exact numeric tables + active_hours lists; returns serde_json Value), S-450 assign_initial_post_agents
#   (type_aliases 8-key ordered Vec-of-pairs; round-robin used_indices; influence-max stable tie-break strict >).
#   GATE CAUGHT real downgrade in S-450: alias inner-loop had an UNCONDITIONAL break → when a poster_type matched an
#   alias group whose members were all absent, Rust fell through to influence-max while Python continues to the next
#   group (proven: person + [alumni infl1.0, official infl9.0] → Rust gave 200, Python 100). FIXED: removed the
#   unconditional break (break 'outer stays for success; outer for-loop continues on no-match) + regression test
#   assign_initial_post_agents_continues_past_empty_alias_group → re-verified PASS. U-019 STAYS [ ] (sub-cycle d =
#   generate_config orchestration S-439 + save remains). 29 new tests, teri 659 green, clippy --all-targets clean.
# CYCLE1 (6th resume) 2026-06-17: U-019 sub-cycle (b) + EntityNode DTO — opus PASS (35/35). PART1 (U-016 rows
#   S-198..S-213): src/services/entity_reader.rs — EntityNode {uuid,name,labels,summary,attributes,related_edges,
#   related_nodes} +to_dict(7-key)+get_entity_type() (first label ∉{Entity,Node}); FilteredEntities +to_dict
#   (entity_types=HashSet→Vec faithful to list(set)). NOT teri graph::Entity (distinct read-DTO). ZepEntityReader
#   machinery S-214..S-219 NOT ported (Zep-read→KnowledgeGraph adapter = later unit / DECISION-9). PART2 (U-019 rows
#   S-430..S-449 excl S-439): SimulationConfigGenerator<L:LlmClient> added to src/services/simulation_config.rs —
#   __init__ + consts (MAX_CONTEXT_LENGTH 50000/AGENTS_PER_BATCH 15/TIME_CONFIG_CONTEXT 10000/EVENT_CONFIG_CONTEXT
#   8000/ENTITY_SUMMARY 300/AGENT_SUMMARY 300/ENTITIES_PER_TYPE_DISPLAY 20), summarize_entities, build_context,
#   call_llm_with_retry (3 attempts, temp 0.7-attempt*0.1, REUSES DECISION-7 chat() raw String NOT chat_json, salvage
#   chain), fix_truncated_json (brace/bracket balance), try_fix_config_json (2 regexes via regex crate),
#   generate_time_config+get_default_time_config+parse_time_config, generate_event_config+parse_event_config.
#   PROMPTS byte-verbatim (opus diffed 1207==1207, 562==562). CHAR-based truncation (CJK-safe). scheduled_events=[]
#   confirmed in SOURCE L723. finish_reason: teri chat() can't surface it (DECISION-7 inexpressible) → strategy(a)
#   always-salvage-on-parse-fail subsumes Python length-branch; opus probed edge divergence = unreachable under
#   operative contract (brace-imbalance⟺truncation⟺finish_reason=='length'), Rust salvages strict superset → [≠]-class
#   NOT downgrade. NEITHER U-016 NOR U-019 marked [x] (U-016: ZepEntityReader remains; U-019: c/d + generate_config
#   remain). 51 new tests, teri 630 green, clippy --all-targets clean.
# CYCLE2 (5th resume) 2026-06-17: U-019 sub-cycle (a) config DATA MODEL — opus PASS (byte-identical diff ×3). New
#   src/services/simulation_config.rs: CHINA_TIMEZONE_CONFIG (china_timezone_config()->Value), AgentActivityConfig,
#   TimeSimulationConfig, EventConfig, PlatformConfig, SimulationParameters (+to_dict 13-key decl-order, +to_json
#   2-space ensure_ascii=False). S-374..S-429 [x] (56). U-019 UNIT STAYS [ ] (sub-cycles b/c/d remain: LLM stages,
#   agent-config gen, retry/fix-json/generate_config orchestration). active_hours default=range(8,23)=[8..22] 15 elems.
#   generated_at reuses python_isoformat_local. GLOBAL CARGO CHANGE: serde_json +preserve_order feature — opus
#   verified this is a PARITY GAIN not a regression: MiroFish runs Flask 3.x (JSON_SORT_KEYS=False) + no sort_keys
#   anywhere, so old BTreeMap alphabetical order was a LATENT DIVERGENCE; preserve_order makes ALL teri JSON
#   (U-010/U-011/U-002 /health/U-012/U-014) byte-faithful to Python insertion order. >>> U-011 [≠] JSON-key-order
#   can be RETIRED (now actually-ordered). 19 new tests, teri 579 green, clippy --all-targets clean.
# CYCLE1 (5th resume) 2026-06-17: U-015 COMPLETE — S-189 build_graph_async + S-192 set_ontology (rollup) — opus PASS.
#   DECISION-8 (architect, map-onto teri's native async build over the petgraph 2-pass pipeline). New src/services/
#   graph_builder.rs: build_graph_async(text,ontology,graph_name,chunk params)->task_id (TaskManager::create_task
#   graph_build + metadata{graph_name,chunk_size,text_length}; capture locale via i18n::get_locale; tokio::spawn(
#   with_locale(...)) driving build_with_progress; complete_task w/ teri-native superset result {graph_name,graph_info,
#   chunks_processed,graph:<serialized>}; fail_task on Err). build() byte-IDENTICAL (delegates to build_with_progress->
#   build_with_progress_and_ontology w/ no-op callback; 6 build tests unchanged). OWNER NO-DOWNGRADE OVERRIDE of
#   DECISION-8 #2: architect deferred custom RELATION kinds as [!]; overridden -> ported RelationKind::Custom(String)
#   SYMMETRIC w/ EntityKind::Custom(String) (set_ontology registers BOTH entity+edge types in MiroFish). set_ontology
#   (&mut self,&Value) records BOTH entity+edge ontology type-name sets, wired into extraction PROMPTS + PARSERS
#   (entity_extraction_prompt_with_custom/relation_extraction_prompt_with_custom/parse_entities_json_with_custom) ->
#   NOT inert (opus verified end-to-end: {MediaOutlet,COVERS_TOPIC}->Custom variants, built-in Person/WorksFor still
#   map to built-ins, unknown-unregistered->Other). Custom variants additive (externally-tagged serde, no wire-form
#   regression — opus round-trip tested). 3 [≠] legit (create_graph 10%/wait_for_episodes 60-90%/fetch_graph_info 90%
#   = Zep-SaaS inexpressible; batch_size non-contractual). i18n progress.* keys all pre-existed (SWEEP-2). 15 new
#   tests, teri 560 green, clippy --all-targets clean.
# CYCLE1 (4th resume) 2026-06-17: U-008-extend (DECISION-7) + U-014 OntologyGenerator — opus PASS/PASS.
#   PART1 U-008-extend (extend-Y, additive superset, the faithful full-signature port of S-058/S-059): added
#   chat/chat_json(&[ChatMessage],&ChatOptions) + ChatRole/ChatMessage/ChatOptions to LlmClient, impl on all 3
#   adapters (OpenAI/Anthropic/Gemini), reuses call_api+strip_think/strip_json_fence. NO U-008 regression (complete*/
#   stream byte-identical, 0 deletions in llm.rs; only #[cfg(test)] mocks stubbed). PART2 U-014 (src/services/
#   ontology.rs, NEW src/services/): S-172 prompt verbatim, S-173 to_pascal_case (.capitalize semantics), S-176
#   generate (chat_json system+user + temp 0.3 + max_tokens 4096), S-178 char-based 50000 truncation, S-179
#   validate_and_process ALL branches (PascalCase/upper/remap/dedup/100-char-desc/fallback end-removal/10-caps) —
#   live differential identical. S-180 generate_python_code [≠] opus-accepted (dead code + Zep-Cloud Python codegen
#   for absent substrate; behavior maps onto EntityKind::Custom via future S-192). regex crate added. Reused next by
#   U-019/U-024/U-018. 49 new tests, teri 545 green, clippy --all-targets clean.
# CYCLE1 (3rd resume) 2026-06-17: U-002 + U-003 axum serve — opus PASS. New src/server.rs (create_app Router:
#   /health + CORS + before/after logging middleware + Accept-Language→locale middleware; serve: validate-first +
#   FLASK_HOST/PORT superset addr resolution + graceful ctrl_c shutdown). TRIPLE ROLL-UP: S-005 JSON ensure_ascii
#   (axum Json raw-UTF-8) + S-003 SECRET_KEY (new Config.secret_key field, default mirofish-secret-key, on ApiState)
#   + S-040 get_locale request-context branch (Accept-Language middleware via i18n::is_supported_locale + with_locale)
#   ALL flipped [x]. => U-001 COMPLETE [x], U-002 COMPLETE [x], U-005 COMPLETE [x]. U-003 [~] partial: S-025 /health
#   [x] (matched actual source {status:ok,service:teri}, ledger {status:healthy} was WRONG); S-024 create_app [~]
#   pending 3 blueprints (U-025/026/027) + register_cleanup (U-023/U-049) — recorded NO stub routes. 2 [≠] accepted
#   (Windows UTF-8 reconfigure; Flask debug/threaded WSGI artifacts). validate_locale routed through i18n single-
#   source-of-truth (removed latent drift). tower 0.4->0.5 (axum 0.7 req). 17 server tests, teri 496 green, clippy clean.
# CYCLE3 (resumed) 2026-06-17: U-010 action_logger port-fresh — opus gate FAIL→fix→PASS (twice), 28/28 [x].
#   New src/sim/action_logger.rs. PlatformActionLogger JSONL (per-event key-sets exact, ensure_ascii=False,
#   total_rounds=hours(def72)*2), SimulationLogManager (dual-sink simulation.log, mode=w, INFO→debug-suppressed),
#   legacy ActionLogger (+platform field), global singleton get_logger (OnceLock<Mutex<Option>>). Shared
#   python_isoformat_local made pub(crate). GATE CAUGHT: (1) S-083 log() narrowed Python getattr method-name
#   resolution (critical/fatal/warn/exception→INFO); fixed full alias map + LogLevel::Critical. (2) S-096/097
#   global-singleton test race under parallelism; fixed ENV_LOCK-style mutex + reset (33/33 ×3). One [≠] ACCEPTED:
#   logger.exception() trailing sys.exc_info() traceback line (inexpressible Python ambient state, level ported,
#   0 call sites). 33 module tests, teri 479 green, clippy --all-targets clean.
# CYCLE2 (resumed) 2026-06-17: U-011 ProjectManager port-fresh — opus gate FAIL→fix→PASS, 40/40 [x].
#   New src/models/project.rs (+src/models/mod.rs). ProjectStatus/Project(15-key to_dict, ensure_ascii=False)/
#   from_dict(.get tolerance, project_id REQUIRED→Err)/create_project(proj_+12hex)/save/get(missing→None,corrupt→Err)/
#   list(created_at-desc+limit)/delete(absent→false)/save_file_to_project(FileStorage→&[u8])/extracted_text/
#   get_project_files. PROJECTS_DIR from config.upload_folder. chrono::Local naive (matches datetime.now()) + µs-omit.
#   GATE CAUGHT 2 real downgrades: (1) create_project returned a CLONE→stale updated_at; fixed same-object mutate.
#   (2) from_dict strict Vec<ProjectFile> collapsed legacy files list to []; fixed Vec<Value> verbatim. 1 [≠]
#   JSON key-order (non-contractual). 23 module tests, teri 446 green, clippy --all-targets clean.
# CYCLE1 (resumed session) 2026-06-17: U-005 locale/i18n port-fresh — opus gate PASS. Landed src/i18n/mod.rs
#   (+ byte-identical embed src/i18n/locales/{zh,en,languages}.json). 6/7 symbols [x]: S-036/037/038/039/041/042
#   (t/t_args 17-input differential match, get_language_instruction 7-locale, with_locale/LOCALE task-local replacing
#   thread-local — the faithful tokio substrate, OnceLock {en,zh} translations, include_str! embed). S-040 get_locale
#   [~]: task-local fallback branch verified; has_request_context()->Accept-Language branch PENDING-U-002/U-003 (axum)
#   honestly recorded (U-001/SECRET_KEY precedent). ROLLED UP U-012: S-163/S-164 task messages now route through
#   i18n::t() -> U-012 29/29 COMPLETE [x]. 24 new i18n tests, teri 423 green, clippy --all-targets clean.
# CYCLE3 2026-06-17: U-012 TaskManager port-fresh PARTIAL — 27/29 symbols [x]; src/task.rs (TaskStatus/Task/to_dict/
#   singleton OnceLock+parking_lot/create/get/update/complete/fail/list/cleanup). Gate caught real bug S-155
#   (Python isoformat omits .000000 when µs==0) -> fixed via python_isoformat(). S-163/S-164 message stay [~]
#   pending-U-005 (MiroFish uses t('progress.task*'); zh placeholder until locale subsystem lands — NOT silent skip).
#   Unit [~] partial, rolls up when U-005. Unblocks S-189->U-012. 399 green. ALSO this session: shimmy /v1/embeddings
#   (FlexNetOS/shimmy#6 MERGED) + teri EmbeddingClient -> GAP-OQ3-EMBED RESOLVED. + owner LLM_MODEL_NAME resolved.
HANDOFF: (active session, resumed 2026-06-17) — cycle1: [≠]-audit gate CLEARED (S-360/361/048 PASS as ported; S-356 FAILED→ported related_edges Part2→re-verified PASS; all 4 [x]). cycle2: U-001 AppConfig PARTIAL extend-Y — 20 fields PASS [x] + S-001 [≠]; S-003/S-005 pending-U-002/U-003 (no axum surface yet); unit stays [~]. Fixed porter's flaky env-tests via ENV_LOCK mutex. OWNER-RESOLVED LLM_MODEL_NAME: shimmy replaces Ollama -> default model=OpenThinker3-7B + base_url=shimmy local 11435/v1 (Gemma-4-12B documented alt). 364 green (+2). Next: U-002/U-003 (axum serve — rolls up U-001) OR U-012 TaskManager OR U-005 locale.
ledger: parity 15/50 units verified [x] (U-001✅, U-002✅, U-004, U-005✅, U-006, U-008(+DECISION-7 extend), U-009, U-010, U-011, U-012, U-013, U-014✅, U-015, U-018, U-048) + U-003 [~] partial (S-025 [x], S-024 pending U-025/026/027+U-023/049) + GAP-1/2/6/U015-1/OQ3-EMBED/ACTION-TAXONOMY resolved; [≠]-audit symbol ports S-360/361/356/048 all [x] parity-verified (opus gate, resume 2026-06-17)   # 545 green
# [≠] RE-AUDIT DONE (owner-flagged): 47 [≠] re-challenged under tightened bar (harness PR #34). 4 disguised
#   skips ported [~]; S-189->U-012 + S-192->U-014 reclassified pending; Zep-SaaS/jitter/REFRESH/json-superset keep.
# NO-DOWNGRADE CORRECTION (owner-flagged): [≠] is for genuinely-inexpressible/non-contractual ONLY,
#   never "destination won't use it". U-018 to_reddit/twitter_format/to_dict were wrongly [≠]-skipped ->
#   PORTED. U-004 rotating-file logging same error -> reclassify extend-Y, port next. Audit other [≠].
# U-023 sweep TODO (tracked, not lost): gender 中文-normalization (S-371), truncated-JSON salvage (S-360/361).
symbols: ~60/1087 [x] + ~19 [≠] of 1087 mapped   # +U-001 S-002/S-004/S-006..S-022 [x] (20) + S-001 [≠]; S-003/S-005 pending-U-002/U-003; +S-360/361/356/048 [x] ([≠]-audit); +S-TAX-001..019,021 [x] (social Action taxonomy); S-TAX-020 REFRESH [≠]; +U-005 S-036/037/038/039/041/042 [x] (6), S-040 [~] pending-U-002/U-003; +U-012 S-163/S-164 [x] (rolled up by U-005); +U-011 S-098..S-137 [x] (40); +U-010 S-070..S-097 [x] (28, S-083 carries 1 [≠] exception-traceback sub-behavior); +U-002/U-003 S-023/S-025 [x], S-024 [~]; ROLLED UP S-003/S-005 (U-001) + S-040 (U-005) [x]; +2 [≠] (win-utf8, flask-debug/threaded) (2026-06-17, 3rd resume); +U-014 S-172..S-179 [x] (8), S-180 [≠] (generate_python_code dead/Zep-substrate); +U-008-extend DECISION-7 chat/chat_json (faithful S-058/S-059 full-signature, extend-Y) (2026-06-17, 4th resume)
gaps_open: GAP-SOCIAL-WORLDSTATE [!] (rich social-world-state timeline/posts/engagement deferred to U-022/028/029/030)
# GAP-OQ3-EMBED RESOLVED 2026-06-17 (owner chose (a)): shimmy gained real /v1/embeddings (candle BERT all-MiniLM-L6-v2, FlexNetOS/shimmy#6 MERGED) + teri EmbeddingClient (src/embedding.rs) wired to it. embed_model default ->all-MiniLM-L6-v2. 371 green. Unblocks U-017/021/024 vec-search consumers.
# GAP-U015-1 RESOLVED earlier (build() large-doc chunking, cycle-5).
action_taxonomy: Action::Social(SocialAction) — 12 OASIS types (Trend included; Like/Dislike carry TargetKind{Post,Comment}); REFRESH [≠] (FILTERED_ACTIONS). READY for U-022/028/029/030.
# TRUE BASELINE CORRECTION: DISCOVER baseline.md claimed 142 tests GREEN on c894de8 — FALSE-GREEN.
#   c894de8 (PR #4 merge) was a BAD MERGE: duplicated api_key field (config.rs) + dead from_env/dup
#   block (main.rs) => did NOT compile. Cycle 1 repaired both to clean fix-branch (d433f95) versions.
#   TRUE no-downgrade baseline going forward = 156 tests green (verified). Cargo.toml has a worktree-only
#   [workspace] build-aid (teri is a meta-root member; review before develop->main promotion).
merge: 0/50 merged+reverified-in-Y (target==dest, so "merge" == landed-in-teri + teri-not-regressed)
classes: port-fresh=17 extend-Y=6 reuse-Y=18 map-onto-substrate=13   # architect's ORIGINAL (provisional, harness PR #33)
# CYCLE-4 RECLASSIFICATION (differential gate, authoritative): the 6 backend reuse-Y units were ALL refuted —
#   U-006/U-008 -> extend-Y DONE [x]; U-009 -> extend-Y (pending); U-013 -> port-fresh (pending, unblocks GAP-U015-1);
#   U-004 -> [≠] console-by-design; U-048 -> extend-Y (pending sim_end). 9 Vue reuse-Y units BLOCKED on the axum API.
# status: cycle 4 done — LLM adapter (U-006+U-008) PROVEN superset, GAP-6 resolved, 242 green.
# OQ resolutions (all no-downgrade): OQ-1 dual-platform=BOTH (MultiPlatformRunner + 2 SimEngine tokio::join!);
#   OQ-2 Relation.valid_at temporal window; OQ-3 query_vec_similarity via shimmy embeddings;
#   OQ-4 Vue frontend IN SCOPE, kept + re-pointed at teri axum API (not WASM);
#   OQ-5 EntityKind::Custom + port ontology generator; OQ-6 fix ARCHITECTURE.md/TODO.md drift.
# Gaps flagged [!] (no silent drop): GAP-1 valid_at, GAP-2 vec-similarity, GAP-3 EntityKind::Custom,
#   GAP-4 per-platform action matrix, GAP-5 action-arg enrichment, GAP-6 <think> strip.
# Source-runnable caveat: differential parity vs MiroFish needs `cd backend && uv sync` (~2min, torch) +
#   LLM_API_KEY; Zep/OASIS-path units are map-onto-substrate (verified as behavioral equivalence of the
#   mapped teri path, not by running MiroFish's Zep/OASIS path which needs external creds).
last_item: (none — DISCOVER complete)
status: HAND OFF (5th resume, 2026-06-17) at clean 2-cycle boundary — U-015 COMPLETE (S-189+S-192) + U-019 sub-cycle (a) data model [x]; teri 579 green, clippy --all-targets clean. Stopped before U-019 sub-cycle (b) because it is large + dependency-blocked on EntityNode (needs DECISION-9 architect call) — better started fresh. NEXT: port EntityNode (DECISION-9), then U-019 sub-cycle (b) LLM stages. HEAD=a0ec455 + handoff checkpoint. develop tip repair still staged (9836238, not yet PR'd).
loop_iteration: 3 (ITERATE c3 Action taxonomy + eval) done; next = 4 (reuse-Y verify-only quick-wins, then extend-Y/port-fresh)
next_iterate: reuse-Y verify-only quick-wins (18 units — differential verify teri's existing symbols vs MiroFish; mark [x] or reclassify extend-Y). Then U-013/text_processor, extend-Y, port-fresh (HTTP API, sim lifecycle, community adapters+social-sim, IPC, config-gen, ontology, Vue re-point).
last_update: 2026-06-14T12:30:00Z
next_iterate: U-015 (wire KnowledgeGraph::build) -> OQ-2/OQ-3 (Relation.valid_at + query_vec_similarity) -> Action enum social variants (unlocks U-022/028/029/030)
