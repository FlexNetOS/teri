# HANDOFF — MiroFish → teri rust-port-merge

**Resume signal for the autonomous /loop.** Read this + `loop_state.md`, then continue at ITERATE.

## Where we are  (SESSION HAND OFF — cycle budget reached 2026-06-17; resume cold from here)
- **Phase:** ITERATE in progress. **8/50 units `[x]` + U-001 `[~]` (partial)**. teri **362 tests green** (clippy `--all-targets -D warnings` clean, env-test flakiness fixed). ~52/1087 symbols `[x]`.
- **✅ PRIOR FIRST-ACTION DONE (this session):** the `[≠]`-audit parity gate is CLEARED. Re-ran the opus gate on the 4 ports from 20e2e48: **S-360/S-361/S-048 PASS as ported**; **S-356 FAILED** (the gate caught a dropped Part 2 — `related_edges` relationship/fact section narrowed) → ported the fix (`KnowledgeGraph::get_neighbor_relations` + directional `_relation_line` mirroring MiroFish :443-451 + `### Related Facts and Relationships` section + 2 direction tests; fact-branch adjudicated (a) faithful = Zep-server-derived, S-355-class `[≠]` boundary) → **re-verified PASS**. All 4 now `[x]`; U-006 + U-018 rollups re-affirmed clean (commit 8239b62).
- **🔴 FIRST ACTION NEXT SESSION:** continue ITERATE at the next leaf unit. Best picks: **U-002/U-003 (axum `serve`)** — porting these rolls up U-001 (they bring SECRET_KEY/JSON_AS_ASCII into a live HTTP surface) and unblock the 9 Vue reuse-Y units + the HTTP API; OR **U-012 TaskManager** (deps:none, unblocks reclassified S-189 build_graph_async); OR **U-005 locale** (deps:none, i18n thread-local, feeds U-036). No interrupted gate this time — clean boundary.
- **✅ OWNER DECISION RESOLVED 2026-06-17 (`[!]` closed):** MiroFish's Ollama models are replaced by **shimmy**, so teri's default LLM is now a shimmy-served top reasoning model: `model="OpenThinker3-7B"` (8–16GB VRAM, top text/math/code reasoning) + `base_url=http://127.0.0.1:11435/v1` (shimmy local), with `Gemma-4-12B` documented as the 16–24GB multimodal/tool-calling alt (config.rs:63-76). 2 new default tests. 364 green. (embed_model unchanged — still GAP-OQ3-EMBED.)
- **U-001 partial detail:** 20 value-fields `[x]` + S-001 `[≠]` (dotenvy superset); **S-003 SECRET_KEY + S-005 JSON_AS_ASCII stay `[ ]` pending-U-002/U-003** (verified: teri has no live axum surface today — `serve_cmd` = "API server not yet implemented" main.rs:102 — so non-contractual; recorded, NOT `[≠]`-dropped). U-001 → `[x]` once U-002/U-003 land. (commit d0f4953)
- **Units `[x]`:** U-004 rotating-file logging, U-006 retry (+S-048 batch-retry), U-008 LLM superset (GAP-6), U-009 SeedIngestor, U-013 text_processor + GAP-U015-1 chunking, U-015 build, U-018 Persona social + OASIS serializers (+S-356/360/361), U-048 sim_end. **U-001 `[~]` partial** (20 fields `[x]`). Enablers: GAP-1 valid_at, GAP-2 vec-sim, GAP-ACTION-TAXONOMY.
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
4. **GAP-OQ3-EMBED decision** — embedding-generation substrate (add `/v1/embeddings` to shimmy via Airframe, OR a teri `EmbeddingClient` vs an OpenAI-compatible provider).
4. **port-fresh** (the heavy surface): HTTP/axum API routes (U-025/026/027, 59 routes — unblocks the 9 Vue reuse-Y units), simulation lifecycle manager (U-023), community platform adapters + social-sim consuming the Action taxonomy (U-022/028/029/030, resolves GAP-SOCIAL-WORLDSTATE), IPC/interview (U-020), config generator (U-019), ontology (U-014, OQ-5), Vue re-pointed at teri's axum API (OQ-4).
5. **Vue reuse-Y verify (9 units, BLOCKED until the axum API exists)** — U-035/037/038/039/040/042/043/044 + SWEEP-1: verify the kept-Vue shapes against teri's API once routes land.

## Open gaps (flagged `- [!]`, no silent drop)
- **GAP-OQ3-EMBED** — embedding generation substrate (shimmy `/v1/embeddings` absent).
- **GAP-U015-1** — `build()` large-doc chunking (sequenced after U-013).

## Loop discipline
Each /loop iteration: run ITERATE cycles (cycle_budget 3, or to ~50% context) → `/harness-evolution find` → commit → `ScheduleWakeup`. DONE only at 100% parity-ledger `- [x]` + 100% symbol-map `- [x]`/`- [≠]` + both left-behind sweeps clean + teri green + Vue re-pointed at teri API (OQ-4). Then open PR `port/mirofish → develop` (auto-merge armed).
