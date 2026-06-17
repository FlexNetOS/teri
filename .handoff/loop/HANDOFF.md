# HANDOFF — MiroFish → teri rust-port-merge

**Resume signal for the autonomous /loop.** Read this + `loop_state.md`, then continue at ITERATE.

## Where we are  (SESSION HAND OFF — cycle budget reached 2026-06-17; resume cold from here)
- **Phase:** ITERATE in progress. **8/50 units `[x]` + U-001 `[~]` + U-012 `[~]` (partial)**. teri **399 tests green** (clippy `--all-targets -D warnings` clean). ~80/1087 symbols `[x]`.
- **THIS SESSION (2026-06-17) — 3 ITERATE cycles + 2 owner-directed features, all committed + opus-gated:**
  - **cycle1 (8239b62):** cleared the interrupted `[≠]`-audit gate — S-360/361/048 PASS as ported; S-356 FAILED (gate caught dropped `related_edges` Part 2) → ported `get_neighbor_relations` + directional relation lines → re-verified PASS. All 4 `[x]`; U-006/U-018 rollups clean.
  - **cycle2 (d0f4953):** U-001 AppConfig extend-Y partial (20 fields `[x]`; details below).
  - **cycle3 (f225c17):** U-012 TaskManager port-fresh partial (`src/task.rs`, 27/29 `[x]`; details below).
  - **owner feature A (c3747cf):** LLM model default resolved — shimmy replaces Ollama → `model="OpenThinker3-7B"` + `base_url=http://127.0.0.1:11435/v1`, `Gemma-4-12B` documented alt (config.rs:63-76).
  - **owner feature B (FlexNetOS/shimmy#6 MERGED + teri ee2c9bd):** **GAP-OQ3-EMBED RESOLVED** — shimmy gained a real `/v1/embeddings` (candle BERT all-MiniLM-L6-v2) + teri `EmbeddingClient` (`src/embedding.rs`) wired to it; embed_model default → `all-MiniLM-L6-v2`. Both halves of vec-similarity now exist.
- **🔴 FIRST ACTION NEXT SESSION — recommend U-005 locale (deps:none):** porting U-005 (i18n thread-local `t()`, S-036..S-042) ROLLS UP U-012 (its S-163/S-164 `message` route through `t()`) AND feeds U-036, AND is needed by many service units. High leverage. THEN: **S-189 build_graph_async → U-012** (now unblocked — port the async-task-with-progress wrapper into the new TaskManager); **U-002/U-003 axum serve** (rolls up U-001's S-003/S-005, unblocks HTTP API + 9 Vue units); other leaves U-010 action_logger / U-011 ProjectManager (deps:none).
- **U-001 partial:** 20 value-fields `[x]` + S-001 `[≠]` (dotenvy superset); **S-003 SECRET_KEY + S-005 JSON_AS_ASCII `[ ]` pending-U-002/U-003** (no live axum surface yet). Rolls up at U-002/U-003. (d0f4953)
- **U-012 partial:** 27/29 `[x]` (`src/task.rs` — TaskStatus/Task/to_dict/singleton OnceLock+parking_lot/create/get/update/complete/fail/list/cleanup; gate caught+fixed S-155 isoformat µs==0 bug via `python_isoformat()`). **S-163/S-164 `message` `[~]` pending-U-005** (MiroFish uses `t('progress.task*')` locale lookup; zh placeholder emitted until U-005 lands then route through `t()` — recorded, NOT silent-skipped). Rolls up at U-005. (f225c17)
- **Units `[x]`:** U-004 logging, U-006 retry (+S-048), U-008 LLM superset (GAP-6), U-009 SeedIngestor, U-013 text_processor (+GAP-U015-1), U-015 build, U-018 Persona social + OASIS serializers (+S-356/360/361), U-048 sim_end. **Partial:** U-001, U-012. Enablers: GAP-1 valid_at, GAP-2 vec-sim, GAP-ACTION-TAXONOMY, **GAP-OQ3-EMBED ✅, GAP-U015-1 ✅**. New modules: `src/embedding.rs`, `src/task.rs`.
- **`[≠]` RE-AUDIT (owner-flagged) — DONE this session** (findings/parity.md `[≠] RE-AUDIT`): re-challenged all 47 `[≠]` under the tightened bar (harness PR #34). 4 disguised skips → ported (above, parity-pending). 2 reclassified to pending deps: **S-189 build_graph_async → U-012** (TaskManager), **S-192 set_ontology → U-014** (OntologyGenerator) — port WITH those units, not `[≠]`. Legit keepers HOLD: Zep-SaaS lifecycle (create_graph/_wait_for_episodes/delete_graph/GraphInfo DTOs), retry jitter, REFRESH (FILTERED_ACTIONS), is_supported json-superset, const-arrays-inline, prompt-folding. **One still-flagged:** S-371 gender 中文-normalization KEEP-`[≠]` (b) BUT must port IF/when OASIS export is consumed (sole caller is OASIS export).
- 9 Vue reuse-Y units BLOCKED on the teri axum API (defer until U-025/026/027 exist).
- **Harness upgrades merged:** harness_hub PR #30, #31, #32, **#33** (reuse-Y is now PROVISIONAL — budget as differential-verify+probable-small-port, not free win).
- **⚠️ develop tip was BROKEN:** `c894de8` (PR #4 merge) was a bad merge (dup `api_key` in config.rs + dead `from_env`/dup block in main.rs) — did NOT compile. **Repaired on `port/mirofish` (commit 9836238)** by restoring the clean fix-branch versions, but **NOT yet PR'd to develop** — develop's tip is still broken until this lands. Consider a clean repair-only hotfix PR (without the Cargo.toml `[workspace]` worktree-aid) to unbreak develop.
- **Branch:** `port/mirofish` off `develop@c894de8` in worktree `/home/drdave/Desktop/meta/.worktrees/mirofish-port/teri`. Commits: 9836238 (repair) · 064655c (U-015) · 4cdfd0d (valid_at+vec_sim).
- **Cargo.toml `[workspace]`** = worktree-only build aid (teri is a meta-root member); strip before develop→main promote.
- **Source:** `/home/drdave/Desktop/meta/MiroFish` (Python; real source = backend/app, backend/scripts, backend/run.py, frontend/src, locales — **NOT** backend/.venv).
- **Target==Dest:** teri (port lands Rust directly into teri modules; no separate merge repo).
- **Substrates locked:** ollama→shimmy (done) · Zep Cloud→petgraph+redb · OASIS subprocess→native SimEngine.

## State files (all committed at 897ba5d)
- `parity-ledger.md` — 50 units U-001..U-050 + SWEEP-1..4, all `- [ ]`.
- `symbol-map.md` — 1,087 symbols S-001..S-1087, all `- [ ]` (the symbol-grain DONE gate).
- `target-architecture.md` — per-unit class + 3 Port-and-map blocks + OQ-1..6 resolutions + module layout + idiom map.
- `merge-ledger.md` — class-tagged rows (reuse-Y 18 · port-fresh 17 · map-onto 13 · extend-Y 6).
- `reports/research.md` — X⟷Y reuse map. `reports/cross-repo-refs.md` — teri↔shimmy + intra-teri blast radius.
- `baseline.md` / `findings/y-regression.md` — **teri 142-test baseline = the no-downgrade-of-Y gate**.
- `evaluation.md` / `proposed-upgrades.md` — evolution-find output.

## No-downgrade gate (every ITERATE cycle must hold)
- teri `cargo build` + `cargo clippy -D warnings` + `cargo test` green; **142 baseline tests never regress**.
- Each unit differentially parity-verified vs MiroFish (or, for map-onto, behavioral equivalence of the mapped teri path). Never stub, never drop a branch, never fake a `- [x]`.

## Next ITERATE units
0. **✅ DONE this session:** the `[≠]`-audit parity gate (S-360/361/356/048, all `[x]`) AND **U-001 AppConfig** (partial `[~]`, 20 fields `[x]`). Both committed + verified.
1. **🔴 NEXT (recommended): U-002 `serve_cmd` + U-003 `create_app` (axum server)** — porting these brings SECRET_KEY/JSON_AS_ASCII into a live HTTP surface → **rolls U-001 up to `[x]`** and unblocks U-025/026/027 (HTTP API, 59 routes) + the 9 Vue reuse-Y units. Alternatively the smaller self-contained leaves: **U-012 TaskManager** (deps:none → then port reclassified S-189 build_graph_async) · **U-005 locale** (deps:none, i18n thread-local → feeds U-036).
2. **U-024 ReportAgent ReACT** (the biggest extend-Y) — extend report::generate_stream with plan_outline + a per-section graph-tool ReACT loop using KnowledgeGraph::search/get_subgraph + panorama via valid_at + insight via vec-sim; chat mode; section-by-section progress. NOTE GAP-OQ3-EMBED blocks the embeddings-backed semantic-search path until the substrate decision.
3. **remaining extend-Y / deps**: U-049 graceful shutdown · U-036 i18n · **U-014 OntologyGenerator** (then port S-192 set_ontology, OQ-5/GAP-3 EntityKind::Custom).
4. **port-fresh** (the heavy surface): HTTP/axum API routes (U-025/026/027, 59 routes — unblocks the 9 Vue reuse-Y units), simulation lifecycle manager (U-023), community platform adapters + social-sim consuming the Action taxonomy (U-022/028/029/030, resolves GAP-SOCIAL-WORLDSTATE), IPC/interview (U-020), config generator (U-019), ontology (U-014, OQ-5), Vue re-pointed at teri's axum API (OQ-4).
5. **Vue reuse-Y verify (9 units, BLOCKED until the axum API exists)** — U-035/037/038/039/040/042/043/044 + SWEEP-1: verify the kept-Vue shapes against teri's API once routes land.

## Open gaps (flagged `- [!]`, no silent drop)
- **GAP-SOCIAL-WORLDSTATE** — rich social world-state (timeline/posts/engagement) deferred to U-022/028/029/030.
- ✅ **GAP-OQ3-EMBED RESOLVED 2026-06-17** — owner chose (a): shimmy gained real `/v1/embeddings` (candle BERT all-MiniLM-L6-v2, FlexNetOS/shimmy#6 MERGED) + teri `EmbeddingClient` (`src/embedding.rs`) wired to it; embed_model default → `all-MiniLM-L6-v2`. Unblocks U-017/021/024 vec-search consumers.
- ✅ **GAP-U015-1 RESOLVED** (cycle-5) — `build()` large-doc chunking.

## Loop discipline
Each /loop iteration: run ITERATE cycles (cycle_budget 3, or to ~50% context) → `/harness-evolution find` → commit → `ScheduleWakeup`. DONE only at 100% parity-ledger `- [x]` + 100% symbol-map `- [x]`/`- [≠]` + both left-behind sweeps clean + teri green + Vue re-pointed at teri API (OQ-4). Then open PR `port/mirofish → develop` (auto-merge armed).
