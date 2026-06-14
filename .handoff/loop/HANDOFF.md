# HANDOFF — MiroFish → teri rust-port-merge

**Resume signal for the autonomous /loop.** Read this + `loop_state.md`, then continue at ITERATE.

## Where we are
- **Phase:** ITERATE in progress. **2 cycles done** (loop iteration 2). Next = ITERATE cycle 3.
- **Done so far:** U-015 (build 2-pass extraction) `[x]`; GAP-1 Relation.valid_at + GAP-2 query_vec_similarity `[x]`. teri **171 tests green**.
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
1. **Action enum social variants (NEXT)** — extend teri's `Action` enum (sim/mod.rs:9, currently Speak/Move/Interact/Observe/Think) with the 12 MiroFish/OASIS social action types: CREATE_POST, LIKE_POST, DISLIKE_POST, REPOST/RETWEET, QUOTE_POST, FOLLOW, CREATE_COMMENT, LIKE_COMMENT, DISLIKE_COMMENT, SEARCH_POSTS, SEARCH_USER, MUTE (+ DO_NOTHING). **No-downgrade: KEEP the 5 generic variants.** Touches sim Action + agent parse_and_validate_action + WorldState::apply + their tests. SCOPE TIGHTLY: the enum + parser + serialization (the action *taxonomy*); defer full social-world-state application (timeline/posts/engagement) to the dedicated social-sim unit (flag, don't drop). Unlocks U-022/028/029/030. Evidence: MiroFish U-021 `AgentActivity.to_episode_text` (12 action types).
2. **U-013 text_processor** (chunking) → then extend `build()` for large-doc chunking (resolves GAP-U015-1).
3. **GAP-OQ3-EMBED** — decide embedding-generation substrate (add `/v1/embeddings` to shimmy via Airframe, OR a teri `EmbeddingClient` vs an OpenAI-compatible provider) — then embedding generation feeds `query_vec_similarity`.
4. Then continue the ledger: reuse-Y verify-only quick-wins (18 units) → extend-Y → port-fresh (HTTP API, lifecycle mgr, community adapters, IPC/interview, config-gen, ontology, Vue re-point).

## Open gaps (flagged `- [!]`, no silent drop)
- **GAP-OQ3-EMBED** — embedding generation substrate (shimmy `/v1/embeddings` absent).
- **GAP-U015-1** — `build()` large-doc chunking (sequenced after U-013).

## Loop discipline
Each /loop iteration: run ITERATE cycles (cycle_budget 3, or to ~50% context) → `/harness-evolution find` → commit → `ScheduleWakeup`. DONE only at 100% parity-ledger `- [x]` + 100% symbol-map `- [x]`/`- [≠]` + both left-behind sweeps clean + teri green + Vue re-pointed at teri API (OQ-4). Then open PR `port/mirofish → develop` (auto-merge armed).
