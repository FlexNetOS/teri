# HANDOFF — MiroFish → teri rust-port-merge

**Resume signal for the autonomous /loop.** Read this + `loop_state.md`, then continue at ITERATE.

## Where we are
- **Phase:** ITERATE in progress. **4 cycles done** (through loop iteration 4). Next = the extend-Y/port-fresh ports surfaced by the reuse-Y differential verification.
- **Done so far (parity-verified `[x]`):** U-015 (build extraction); GAP-1 Relation.valid_at; GAP-2 query_vec_similarity; GAP-ACTION-TAXONOMY (Action::Social 12 OASIS types); **U-006 retry (recovery-proven) + U-008 chat/chat_json — teri LLM adapter is now a PROVEN superset (GAP-6 resolved: `<think>`+JSON-fence strip, all 3 adapters)**. teri **242 tests green**. ~36/1087 symbols `[x]`.
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
1. **Small extend-Y/port-fresh completions surfaced by cycle-4 (NEXT):**
   - **U-013 (port-fresh)** — new `src/seed/text_processor.rs`: `split_text(chunk_size=500, overlap=50, UTF-8-safe char windows + sentence-boundary backtrack)` + `preprocess_text` (CRLF→LF, collapse blank runs, trim) + `get_text_stats`. ← text_processor.py / file_parser.py:161-202. **Then resolve GAP-U015-1**: extend `build()` (graph/mod.rs:237) to split→extract-per-chunk→merge for large docs.
   - **U-009 (extend-Y)** — `SeedIngestor`: add multi-encoding fallback (UTF-8→GBK/Latin-1), `.md`/`.markdown` dispatch, `is_supported` ext gate, multi-file concat. ← file_parser.py.
   - **U-048 (extend-Y)** — add in-band end-of-sim terminal signal to stream subscribers (`StreamEvent::sim_end` at sim/mod.rs:496 + api/mod.rs:82). ← action_logger.py:105.
2. **extend-Y units** (6): U-001 AppConfig, U-002 serve_cmd, U-018 Persona social fields, U-024 ReportAgent ReACT, U-036 i18n, U-041 SimulationRunView, U-049 graceful shutdown.
3. **GAP-OQ3-EMBED decision** — embedding-generation substrate (add `/v1/embeddings` to shimmy via Airframe, OR a teri `EmbeddingClient` vs an OpenAI-compatible provider).
4. **port-fresh** (the heavy surface): HTTP/axum API routes (U-025/026/027, 59 routes — unblocks the 9 Vue reuse-Y units), simulation lifecycle manager (U-023), community platform adapters + social-sim consuming the Action taxonomy (U-022/028/029/030, resolves GAP-SOCIAL-WORLDSTATE), IPC/interview (U-020), config generator (U-019), ontology (U-014, OQ-5), Vue re-pointed at teri's axum API (OQ-4).
5. **Vue reuse-Y verify (9 units, BLOCKED until the axum API exists)** — U-035/037/038/039/040/042/043/044 + SWEEP-1: verify the kept-Vue shapes against teri's API once routes land.

## Open gaps (flagged `- [!]`, no silent drop)
- **GAP-OQ3-EMBED** — embedding generation substrate (shimmy `/v1/embeddings` absent).
- **GAP-U015-1** — `build()` large-doc chunking (sequenced after U-013).

## Loop discipline
Each /loop iteration: run ITERATE cycles (cycle_budget 3, or to ~50% context) → `/harness-evolution find` → commit → `ScheduleWakeup`. DONE only at 100% parity-ledger `- [x]` + 100% symbol-map `- [x]`/`- [≠]` + both left-behind sweeps clean + teri green + Vue re-pointed at teri API (OQ-4). Then open PR `port/mirofish → develop` (auto-merge armed).
