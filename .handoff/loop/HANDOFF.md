# HANDOFF — MiroFish → teri rust-port-merge

**Resume signal for the autonomous /loop.** Read this + `loop_state.md`, then continue at ITERATE.

## Where we are
- **Phase:** ITERATE in progress. **9 cycles done** (through loop iteration 6). Next = the `[≠]` re-audit, then U-024 ReportAgent ReACT + the heavy port-fresh.
- **Done so far (parity-verified `[x]`, 8/50 units):** U-004 rotating-file logging, U-006 retry, U-008 LLM superset (GAP-6 resolved), U-009 SeedIngestor (encoding/.md/is_supported/multi-file), U-013 text_processor + GAP-U015-1 build() chunking, U-015 build extraction, U-018 Persona social fields + OASIS serializers, U-048 sim_end signal. Enablers: GAP-1 valid_at, GAP-2 vec-sim, GAP-ACTION-TAXONOMY. teri **315 tests green**. ~115/1087 symbols `[x]`.
- **⚠️ NO-DOWNGRADE CORRECTION (owner-flagged, cycle 8-9):** `[≠]` was being misused to skip portable features ("dest won't use it"). FIXED: U-018 OASIS serializers + U-004 rotating-file logging were wrongly `[≠]`-skipped → both now PORTED. Harness PR #34 tightened the `[≠]` bar (verifier must CHALLENGE every `[≠]`, FAIL disguised skips; "when in doubt PORT IT"). **QUEUED: interim `[≠]` re-audit** (proposed-upgrades.md §D) of this run's ~20 existing `[≠]` rows under the tightened bar — prioritize U-018-adjacent + Zep-mechanics (S-189/193/194); port any disguised skip.
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
1. **`[≠]` RE-AUDIT (NEXT — owner-flagged priority)** — under the tightened bar (harness PR #34, proposed-upgrades.md §D), re-challenge this run's ~20 existing `[≠]` rows. For each: is it genuinely INEXPRESSIBLE / NON-CONTRACTUAL / strict-SUPERSET, or a disguised feature-skip? PORT any disguised skip. Priority: U-018's retained `[≠]` (esp. the gender 中文-normalization S-371 + truncated-JSON salvage S-360/361 tracked for this), Zep-mechanics S-189/193/194. Legitimate keepers: retry jitter, REFRESH (FILTERED_ACTIONS), is_supported json-superset.
2. **U-024 ReportAgent ReACT** (the biggest extend-Y) — extend report::generate_stream with plan_outline + a per-section graph-tool ReACT loop using KnowledgeGraph::search/get_subgraph + panorama via valid_at + insight via vec-sim; chat mode; section-by-section progress. NOTE GAP-OQ3-EMBED blocks the embeddings-backed semantic-search path until the substrate decision.
3. **remaining extend-Y**: U-001 AppConfig · U-002 serve_cmd · U-049 graceful shutdown · U-036 i18n.
4. **GAP-OQ3-EMBED decision** — embedding-generation substrate (add `/v1/embeddings` to shimmy via Airframe, OR a teri `EmbeddingClient` vs an OpenAI-compatible provider).
4. **port-fresh** (the heavy surface): HTTP/axum API routes (U-025/026/027, 59 routes — unblocks the 9 Vue reuse-Y units), simulation lifecycle manager (U-023), community platform adapters + social-sim consuming the Action taxonomy (U-022/028/029/030, resolves GAP-SOCIAL-WORLDSTATE), IPC/interview (U-020), config generator (U-019), ontology (U-014, OQ-5), Vue re-pointed at teri's axum API (OQ-4).
5. **Vue reuse-Y verify (9 units, BLOCKED until the axum API exists)** — U-035/037/038/039/040/042/043/044 + SWEEP-1: verify the kept-Vue shapes against teri's API once routes land.

## Open gaps (flagged `- [!]`, no silent drop)
- **GAP-OQ3-EMBED** — embedding generation substrate (shimmy `/v1/embeddings` absent).
- **GAP-U015-1** — `build()` large-doc chunking (sequenced after U-013).

## Loop discipline
Each /loop iteration: run ITERATE cycles (cycle_budget 3, or to ~50% context) → `/harness-evolution find` → commit → `ScheduleWakeup`. DONE only at 100% parity-ledger `- [x]` + 100% symbol-map `- [x]`/`- [≠]` + both left-behind sweeps clean + teri green + Vue re-pointed at teri API (OQ-4). Then open PR `port/mirofish → develop` (auto-merge armed).
