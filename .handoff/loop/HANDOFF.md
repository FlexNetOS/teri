# HANDOFF — MiroFish → teri rust-port-merge

**Resume signal for the autonomous /loop.** Read this + `loop_state.md`, then continue at ITERATE.

## Where we are  (SESSION HAND OFF — budget exhausted 2026-06-14; resume cold from here)
- **Phase:** ITERATE in progress. **8/50 units `[x]`** + the `[≠]`-audit ports green-but-PARITY-PENDING. teri **333 tests green** (clippy `--all-targets -D warnings` clean). ~115/1087 symbols `[x]`.
- **🔴 FIRST ACTION NEXT SESSION (a cycle was interrupted mid-gate):** the `[≠]`-audit FIX is committed GREEN (20e2e48) but its parity-verifier was INTERRUPTED → the 4 ported symbols are `[~]` NOT `[x]`. **Re-run the opus parity gate for S-360/361 (truncated-JSON salvage), S-356 (generate_social graph-enrichment), S-048 (call_batch_with_retry)** — faithfully ported vs MiroFish (oasis_profile_generator.py `_fix_truncated_json:583`/`_try_fix_json:606`/`_build_entity_context:414`; retry.py:195). On PASS → flip those rows `[x]` + re-affirm U-006/U-018 unit-rollup (they own these symbols). The prompt I was about to send the verifier is reconstructable from the commit + findings/parity.md `[≠]-audit` section.
- **Units `[x]`:** U-004 rotating-file logging, U-006 retry, U-008 LLM superset (GAP-6), U-009 SeedIngestor, U-013 text_processor + GAP-U015-1 chunking, U-015 build, U-018 Persona social + OASIS serializers, U-048 sim_end. Enablers: GAP-1 valid_at, GAP-2 vec-sim, GAP-ACTION-TAXONOMY.
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
0. **🔴 FIRST: re-run the parity gate for the `[≠]`-audit ports** (S-360/361/356/048 — see "Where we are"). They're committed GREEN (20e2e48) but parity-pending `[~]`; the gate was interrupted. Flip `[x]` on PASS. (`[≠]` re-audit itself is DONE this session.)
1. **U-024 ReportAgent ReACT** (the biggest extend-Y) — extend report::generate_stream with plan_outline + a per-section graph-tool ReACT loop using KnowledgeGraph::search/get_subgraph + panorama via valid_at + insight via vec-sim; chat mode; section-by-section progress. NOTE GAP-OQ3-EMBED blocks the embeddings-backed semantic-search path until the substrate decision.
2. **remaining extend-Y**: U-001 AppConfig · U-002 serve_cmd · U-049 graceful shutdown · U-036 i18n. **U-012 TaskManager** (then port S-189 build_graph_async into it) · **U-014 OntologyGenerator** (then port S-192 set_ontology, OQ-5/GAP-3 EntityKind::Custom).
4. **GAP-OQ3-EMBED decision** — embedding-generation substrate (add `/v1/embeddings` to shimmy via Airframe, OR a teri `EmbeddingClient` vs an OpenAI-compatible provider).
4. **port-fresh** (the heavy surface): HTTP/axum API routes (U-025/026/027, 59 routes — unblocks the 9 Vue reuse-Y units), simulation lifecycle manager (U-023), community platform adapters + social-sim consuming the Action taxonomy (U-022/028/029/030, resolves GAP-SOCIAL-WORLDSTATE), IPC/interview (U-020), config generator (U-019), ontology (U-014, OQ-5), Vue re-pointed at teri's axum API (OQ-4).
5. **Vue reuse-Y verify (9 units, BLOCKED until the axum API exists)** — U-035/037/038/039/040/042/043/044 + SWEEP-1: verify the kept-Vue shapes against teri's API once routes land.

## Open gaps (flagged `- [!]`, no silent drop)
- **GAP-OQ3-EMBED** — embedding generation substrate (shimmy `/v1/embeddings` absent).
- **GAP-U015-1** — `build()` large-doc chunking (sequenced after U-013).

## Loop discipline
Each /loop iteration: run ITERATE cycles (cycle_budget 3, or to ~50% context) → `/harness-evolution find` → commit → `ScheduleWakeup`. DONE only at 100% parity-ledger `- [x]` + 100% symbol-map `- [x]`/`- [≠]` + both left-behind sweeps clean + teri green + Vue re-pointed at teri API (OQ-4). Then open PR `port/mirofish → develop` (auto-merge armed).
