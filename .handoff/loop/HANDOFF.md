# HANDOFF — MiroFish → teri rust-port-merge

**Resume signal for the autonomous /loop.** Read this + `loop_state.md`, then continue at ITERATE.

## Where we are
- **Phase:** ITERATE in progress. **3 cycles done** (through loop iteration 3). Next = reuse-Y verify-only quick-wins.
- **Done so far:** U-015 (build 2-pass extraction) `[x]`; GAP-1 Relation.valid_at + GAP-2 query_vec_similarity `[x]`; GAP-ACTION-TAXONOMY (Action::Social — 12 OASIS types incl Trend + TargetKind{Post,Comment}) `[x]`. teri **220 tests green**. ~28/1087 symbols `[x]`.
- **Harness upgrades merged:** harness_hub PR #30 (venv-exclude+AST fallback, drop-resilience), #31 (build-health EXECUTES baseline + fail-closed on red tip; relay-noise return-integrity), #32 (porter prove-before-collapse/omit discriminant rule).
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
1. **reuse-Y verify-only quick-wins (NEXT — 18 units, fast ledger advance).** teri already provides these fully; per the merge-ledger they need DIFFERENTIAL VERIFICATION vs MiroFish (NOT a fresh port) — if teri's existing symbol matches the source contract → mark `- [x]` (verify-only); if it diverges → reclassify `extend-Y` and port the missing behavior. Candidates from target-architecture.md/merge-ledger.md: the LLM-adapter units (U-008 `LlmClient` superset — but note `<think>` strip GAP-6 may make it extend-Y), seed ingestor, persistent-memory units, sim-tick-loop reuse, the streaming infra. Batch several per cycle (they're quick). Use the opus parity-verifier per unit (reuse is never trusted).
2. **extend-Y units** (6): agent social-persona fields, graph episodic wiring, report ReACT, etc.
3. **U-013 text_processor** (chunking) → then extend `build()` for large-doc chunking (resolves GAP-U015-1).
4. **GAP-OQ3-EMBED decision** — embedding-generation substrate (add `/v1/embeddings` to shimmy via Airframe, OR a teri `EmbeddingClient` vs an OpenAI-compatible provider).
5. **port-fresh** (17, the heavy surface): HTTP/axum API routes (U-025/026/027, 59 routes), simulation lifecycle manager (U-023), community platform adapters + the social-sim that consumes the Action taxonomy (U-022/028/029/030, resolves GAP-SOCIAL-WORLDSTATE), IPC/interview (U-020), simulation config generator (U-019), ontology generator (U-014, OQ-5), Vue frontend re-pointed at teri's axum API (OQ-4).

## Open gaps (flagged `- [!]`, no silent drop)
- **GAP-OQ3-EMBED** — embedding generation substrate (shimmy `/v1/embeddings` absent).
- **GAP-U015-1** — `build()` large-doc chunking (sequenced after U-013).

## Loop discipline
Each /loop iteration: run ITERATE cycles (cycle_budget 3, or to ~50% context) → `/harness-evolution find` → commit → `ScheduleWakeup`. DONE only at 100% parity-ledger `- [x]` + 100% symbol-map `- [x]`/`- [≠]` + both left-behind sweeps clean + teri green + Vue re-pointed at teri API (OQ-4). Then open PR `port/mirofish → develop` (auto-merge armed).
