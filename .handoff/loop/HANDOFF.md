# HANDOFF — MiroFish → teri rust-port-merge

**Resume signal for the autonomous /loop.** Read this + `loop_state.md`, then continue at ITERATE.

## Where we are
- **Phase:** ITERATE in progress. **6 cycles done** (through loop iteration 5). Next = U-048 (sim_end stream signal), then the extend-Y units.
- **Done so far (parity-verified `[x]`, 5/50 units):** U-006 retry (recovery-proven), U-008 chat/chat_json (LLM PROVEN superset, GAP-6 resolved), U-009 SeedIngestor (encoding fallback GBK/1252 + .md + is_supported + multi-file), U-013 text_processor (split_text/preprocess/stats) + GAP-U015-1 build() chunking, U-015 build extraction. Enablers: GAP-1 valid_at, GAP-2 vec-sim, GAP-ACTION-TAXONOMY (Action::Social 12 OASIS types). teri **275 tests green**. ~49/1087 symbols `[x]`.
- **Reuse-Y reality check (cycle 4):** the differential gate refuted reuse-Y for ALL 6 backend units → small ports, not free wins. Reclassified: U-006/U-008 `[x]` DONE; **U-009 extend-Y** (encoding-fallback + `.md` + `is_supported` + multi-file), **U-013 port-fresh** (teri has NO text chunking → new `src/seed/text_processor.rs`, `split_text` 500/50 — **unblocks GAP-U015-1**), **U-004 `[≠]`** (console-by-design), **U-048 extend-Y** (add in-band `sim_end` stream signal). 9 Vue reuse-Y units are BLOCKED on the teri axum API (defer until U-025/026/027 exist).
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
1. **U-048 (extend-Y, NEXT)** — add in-band end-of-sim terminal signal to stream subscribers (`StreamEvent::sim_end` at sim/mod.rs:~496 + api/mod.rs:~82). ← action_logger.py:105 / simulation_runner.py:623. (U-013 ✅ + U-009 ✅ done.)
2. **extend-Y units** (the substantive ones next): **U-018 Persona social fields** (handle/follower-count/platform/posting-style — agent/mod.rs:14, consumed by the social-sim) · **U-024 ReportAgent ReACT** (plan_outline + per-section graph-tool ReACT loop using KnowledgeGraph::search/get_subgraph + panorama via valid_at + insight via vec-sim; extends report::generate_stream) · U-001 AppConfig · U-002 serve_cmd · U-049 graceful shutdown · U-036 i18n.
3. **GAP-OQ3-EMBED decision** — embedding-generation substrate (add `/v1/embeddings` to shimmy via Airframe, OR a teri `EmbeddingClient` vs an OpenAI-compatible provider).
4. **port-fresh** (the heavy surface): HTTP/axum API routes (U-025/026/027, 59 routes — unblocks the 9 Vue reuse-Y units), simulation lifecycle manager (U-023), community platform adapters + social-sim consuming the Action taxonomy (U-022/028/029/030, resolves GAP-SOCIAL-WORLDSTATE), IPC/interview (U-020), config generator (U-019), ontology (U-014, OQ-5), Vue re-pointed at teri's axum API (OQ-4).
5. **Vue reuse-Y verify (9 units, BLOCKED until the axum API exists)** — U-035/037/038/039/040/042/043/044 + SWEEP-1: verify the kept-Vue shapes against teri's API once routes land.

## Open gaps (flagged `- [!]`, no silent drop)
- **GAP-OQ3-EMBED** — embedding generation substrate (shimmy `/v1/embeddings` absent).
- **GAP-U015-1** — `build()` large-doc chunking (sequenced after U-013).

## Loop discipline
Each /loop iteration: run ITERATE cycles (cycle_budget 3, or to ~50% context) → `/harness-evolution find` → commit → `ScheduleWakeup`. DONE only at 100% parity-ledger `- [x]` + 100% symbol-map `- [x]`/`- [≠]` + both left-behind sweeps clean + teri green + Vue re-pointed at teri API (OQ-4). Then open PR `port/mirofish → develop` (auto-merge armed).
