# Teri — Final Parity Sprint

**Goal:** close every remaining MiroFish-parity gap, light up the full L2–L5 autonomy loop,
and land the workspace on the latest toolchain (nightly + `rustc_codegen_gcc` + `wild` + `kache`,
plus the CUDA-nightly path via `cuda-oxide`). When this sprint closes, teri is the *complete*
upgrade over MiroFish — engine, UI, community loop, autonomy, and build stack — with nothing
left on the parity ledger.

**Ground truth:** `FEATURE-PARITY.md` (the authoritative gap ledger) and `RUNBOOK.md` §12.
This file is the *execution order* over that ledger; it does not introduce new scope, it
sequences and closes the open `TASK-*` items, then finishes with the toolchain upgrade.

**Discipline (binding, from CLAUDE.md):**
- Fresh worktree per slice (`git worktree add .worktrees/<slug> -b <slug> origin/main`); never on
  the shared canonical checkout.
- One vertical slice per PR; PR → auto-merge on green. Never merge red.
- Full unfiltered `cargo test` summary in every PR body.
- `cargo fmt --all && cargo clippy --all-targets -- -D warnings` green before push.
- Witness milestones (`/checkpoint`), close segments (`/handoff`).

**Definition of done (sprint):** every box below ☑; `FEATURE-PARITY.md` shows no open `TASK-*`;
`RUNBOOK.md` §12 parity matrix all ✅/➕; toolchain phase verified on both local (meta-inherited)
and standalone-CI build paths.

---

## Slice plan (ordered; each row = one PR)

| # | Slice | Closes | Risk | Gate |
|---|-------|--------|------|------|
| S0 | Doc/backlog truth | TASK-DOC-1 | low | docs only |
| S1 | UI live-verify + envelope/CORS fixes | TASK-UI-1 | low | manual + smoke |
| S2 | UI SSE adoption | TASK-UI-2 | med | UI smoke |
| S3 | UI branding + chunk-split + dual-import fix | TASK-UI-3 | low | `npm run build` |
| S4 | sqlx/Postgres-backed `IntelligenceStore` | TASK-SEAM-2 | med | pebesen-ci |
| S5 | LLM-pipeline-middle loop E2E | TASK-SEAM-3 | med | gated integ test |
| S6 | Persona memory injection + bilingual + randomization | TASK-SIM-1 | high | cargo test |
| S7 | Per-platform action split + enriched `action_args` | TASK-SIM-2 | med | cargo test |
| S8 | Social-DB producer + `sqlite`-default serve | TASK-SIM-3 | high | cargo test |
| S9 | Provider-agnostic `serve` ApiState | TASK-SIM-4 | med | cargo test |
| S10 | `register_cleanup` shutdown hook | TASK-SIM-5 | low | cargo test |
| S11 | `json_object`/`finish_reason` + ontology constraints + entity-search enrich | TASK-SIM-6 | med | cargo test |
| S12 | Autonomy orchestrator (DECIDE layer) | TASK-AUTO-1 | high | cargo test |
| S13 | Calibration loop (calibrated `confidence`) | TASK-AUTO-2 | med | cargo test |
| **S14** | **Toolchain upgrade — nightly + codegen-gcc + wild + kache + CUDA-nightly** | (new) | high | full CI + local |

Sequencing rationale: **truth first** (S0), **close the two structural follow-on areas** (UI S1–S3,
seam S4–S5), **then engine fidelity** (S6–S11) which the autonomy loop depends on for honest output,
**then autonomy** (S12–S13) which turns the loop self-driving, **and finally the toolchain** (S14) so
every prior slice has already proven green on the *current* stable pin before we move the floor —
the toolchain change is isolated as the last variable.

---

## Phase A — Truth & structural follow-ups

### S0 · TASK-DOC-1 — refresh stale TODO.md
`TODO.md` is dated 2026-06-12 and still says "pipeline pending" (false). Rewrite it to point at
`FEATURE-PARITY.md` as the live backlog, or delete it in favor of this `SPRINT.md`.
**Done:** no doc in-tree contradicts current code state.

### S1 · TASK-UI-1 — UI live smoke
`teri serve` + `npm run dev`; drive all 5 wizard steps against the running engine. Catch what the
static build can't: `{success,data,error}` envelope mismatches, field-name drift, CORS, `Accept-Language`.
**Done:** all 5 steps complete a real run against a live (mock-backed) engine; fixes committed.

### S2 · TASK-UI-2 — UI SSE adoption
Replace MiroFish's `from_line` log polling (Step4/Step5) and 30s graph re-polling (SimulationRun)
with `EventSource` against teri's `/agent-log/sse`, `/console-log/sse`, `/events`, `/ticks/sse`.
**Done:** live tails stream over SSE; polling fallbacks removed or gated.

### S3 · TASK-UI-3 — UI polish
teri-native mark (replace renamed MiroFish logo); code-split the large d3 chunk; resolve the
`pendingUpload.js` dual-import warning.
**Done:** `npm run build` clean, no dual-import warning, teri branding.

### S4 · TASK-SEAM-2 — sqlx-backed IntelligenceStore
Make `IntelligenceStore` a trait; keep the in-memory impl, add a Postgres impl (tables `predictions`,
`prediction_actions` per the `// SQLX SLOT:` markers). `SQLX_OFFLINE=true` + `.sqlx` for offline CI.
**Done:** both impls pass the same behavior test; pebesen-ci green.

### S5 · TASK-SEAM-3 — LLM-pipeline-middle loop E2E
The feedback half is done (`tests/community_loop_e2e.rs`). Add the middle: signal → seed →
`pipeline::run_pipeline` → report → feedback, as a **gated** integration test against a mock
inference backend (honors the backend-honesty guard; never canned-text).
**Done:** one E2E test exercises the full community→prediction→community loop.

---

## Phase B — Simulation fidelity (engine)

### S6 · TASK-SIM-1 — persona generation (biggest fidelity gap)
Persona memory injection (个人记忆/机构记忆), bilingual two-prompt strategy (individual-vs-group,
system prompt, `response_format=json_object`, temp ramp `0.7−attempt×0.1`, 3-attempt loop,
`finish_reason=="length"` truncation), and randomization (karma 500–5000, random age/gender/mbti/country).
Refs: `oasis_profile_generator.py:497-772,262-265,786-845` → `agent/mod.rs:1404-1426`.

### S7 · TASK-SIM-2 — action split + enriched args
Per-platform allowed-action set (TWITTER 6 vs REDDIT 13) gated at decision time; enrich `actions.jsonl`
with `post_content`/`author_name`/`quote_content`/`comment_content`/`target_user_name` resolved at
log time from `social_world.rs`. Refs: `run_parallel_simulation.py:178-202,749-981` → `sim/mod.rs`.

### S8 · TASK-SIM-3 — social-DB producer + sqlite-default serve
Land the OASIS-equivalent social-DB *producer* so `/interview/history`, `/posts`, `/comments` return
populated results; ship `serve` with the `sqlite` feature on. (GAP-U026-SOCIALDB.)

### S9 · TASK-SIM-4 — provider-agnostic serve
Generalize serve's `ApiState` off concrete `SimulationRunner<OpenAiAdapter>` (`api/report.rs:1178`)
so Anthropic/Gemini-backed sims support deep interaction under `serve`. Aligns serve with
`teri run`'s provider selection.

### S10 · TASK-SIM-5 — shutdown hook
tokio signal / axum graceful-shutdown → `cleanup_all` on SIGTERM/SIGINT/SIGHUP; no orphaned sims.

### S11 · TASK-SIM-6 — structured-output + ontology + enrichment
`LlmClient` structured-output request shape (`json_object`/`finish_reason`); `set_ontology`
reserved-names remap + source_targets edge constraints; per-entity context-search enrichment wired to
teri's semantic-recall/graph-search.

---

## Phase C — Autonomy (L2–L5)

### S12 · TASK-AUTO-1 — autonomy orchestrator (DECIDE)
Watch community adapters; debounce signal deltas into `(seed, query)` jobs; schedule headless
`pipeline::run_pipeline` runs under a compute budget; continuity/resume + witnessed audit trail.
This is the layer that makes teri self-driving (see `docs/AGENTIC-STORY.md`).

### S13 · TASK-AUTO-2 — calibration loop
Turn actioned/accurate outcomes into per-community confidence weights (persist in redb); upgrade
report `confidence` from synthesized metadata → calibrated probability. Updates the README "Status"
caveat once calibration is real.

---

## Phase D — Toolchain upgrade (the finish) · S14

Owner standing rule: **always the latest toolchain.** End state for teri (matching meta's Epic-H
build stack and adding the GPU path):

**Target stack — single toolchain, no duplicates (owner rule)**
- **Channel:** `nightly` — the **only** toolchain. `cuda-oxide` *requires* nightly, so nightly is
  mandatory, not a choice weighed against stable. Floating `nightly` (always-latest), refreshed on the
  meta cadence — **not** date-pinned, and **no** stable fallback toolchain kept alongside it.
- **No duplicate toolchains:** the existing rustup installs (`stable`, `1.94.1`, `1.96.0`) are
  duplicates and get consolidated away — `nightly` is the single resolved toolchain for teri *and*
  the meta workspace. The toolchain + compiler cache are owned by **meta/envctl paths**, not
  user-global `~/.rustup`/`~/.cargo`.
- **Codegen:** `rustc_codegen_gcc` as an optional codegen *backend* (perf builds) on the one nightly
  toolchain — a backend component, not a second toolchain; LLVM stays the default codegen so
  `cargo`/CI remain portable.
- **Linker:** `clang` + `--ld-path=wild` — already in meta `.cargo/config.toml`; teri inherits it
  inside the meta tree but **not** in standalone CI (CI clones teri alone).
- **rustc-wrapper:** `kache 0.6.0` — same inheritance caveat as wild.
- **CUDA:** nightly CUDA via `cuda-oxide` (clang `llc` = `CUDA_OXIDE_LLC` for Rust→PTX), gated behind
  a `cuda` feature so the default build needs no GPU.

**Work items**
1. **Bump `rust-toolchain.toml`** `1.94.1` → `nightly` (floating channel), keep `edition 2024`.
   Add `components = ["rustfmt","clippy","rustc-codegen-gcc"]` (or install codegen-gcc via the
   meta toolchain component if it's distributed that way — verify availability first). teri's pin and
   meta's channel must resolve to the **same single nightly** — no per-repo toolchain divergence.
1b. **Consolidate duplicate toolchains:** remove the now-redundant `stable` / `1.94.1` / `1.96.0`
   rustup installs so `nightly` is the only toolchain; ensure it lives under the meta/envctl-owned
   path, not user-global `~/.rustup`. (Coordinate with meta — meta's `rust-toolchain.toml` moves
   `stable` → `nightly` in the same window so the workspace floor is uniform.)
2. **Fix the standalone-CI inheritance gap (load-bearing):** teri's CI clones teri *alone*, so it does
   **not** see meta's `.cargo/config.toml` (wild + kache). Decide per-repo:
   - Add a teri-local `.cargo/config.toml` mirroring the wild+kache lines **guarded** so CI without
     `wild`/`kache` on PATH still builds (CI installs them, or the config is feature/env-gated), **or**
   - Install `wild` + `kache` in `ci.yml`/`pebesen-ci.yml` before build and keep config meta-only.
   Whichever is chosen, **preflight (`scripts/preflight.sh`) and CI must agree** — preflight already
   mirrors per-repo CI clippy flags; the toolchain bump must not desync them.
3. **Nightly lint/fmt drift:** nightly clippy/rustfmt surface new lints. Run
   `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings` on nightly, fix drift,
   in the **same** slice so CI doesn't go red on merge.
4. **`cuda-oxide` path:** add a `cuda` feature + the cuda-oxide dependency wiring; ensure
   `cargo build` (no features) is unaffected and `cargo build --features cuda` compiles where
   `CUDA_OXIDE_LLC`/`llc` are present. Document the GPU build in `RUNBOOK.md`.
5. **pebesen crates:** they use `edition.workspace`/`rust-version.workspace` from teri's root
   `Cargo.toml`; bumping the workspace toolchain must keep pebesen-ci green (`SQLX_OFFLINE=true`).
6. **Update CI** `dtolnay/rust-toolchain@stable` → the pinned nightly in `ci.yml` and
   `pebesen-ci.yml`; add codegen-gcc/wild/kache install steps per decision (2).
7. **Bench/verify** a release build with codegen-gcc vs LLVM to confirm the perf path works (record
   numbers in the PR); confirm `cuda` feature builds.

**Done (S14):**
- ☑ `rust-toolchain.toml` = floating `nightly` (single toolchain, no date-pin, no stable fallback);
  `cargo build`, `cargo test` (1764 default + all-features), `cargo clippy --all-targets
  --all-features -- -D warnings`, `cargo fmt --all --check` all **green on nightly 1.98.0**
  (teri + pebesen). Nightly `unnecessary_sort_by` drift fixed in the same slice.
- ☑ CI toolchains bumped to `nightly` (`ci.yml`, `pebesen-ci.yml`, `promote.yml`).
- ☑ wild+kache resolved as an intentional **meta-tree-inherited** perf path; teri ships no repo-local
  `.cargo/config.toml`, so standalone CI + preflight agree on the default LLVM linker (no inheritance
  gap, no brittle CI install). Documented in `RUNBOOK.md §3.1`.
- ☑ `rustc_codegen_gcc` documented as an on-demand perf backend (NOT pinned in `rust-toolchain.toml`
  — a floating nightly doesn't build it every day, and a missing pinned component is a fatal rustup
  error). `RUNBOOK.md §3.1`.
- ☑ `./target/*/teri --help` still exit 0 keyless; backend-honesty guard untouched.
- ☑ **License corrected → AGPL-3.0-or-later** (owner decision 2026-06-24). teri's stale `MIT` +
  upstream-port attribution (`Kresna Sucandra` / `SHA888/teri`) was wrong; set to
  `AGPL-3.0-or-later`, `FlexNetOS`, `FlexNetOS/teri`, with the canonical AGPL `LICENSE` added.
  Copyleft + network-use clause = reciprocity (no SaaS freeloading), faithful to MiroFish's AGPL.
- ☑ **GPU codegen path = [NVlabs/cuda-oxide](https://github.com/NVlabs/cuda-oxide)** documented as
  the third codegen backend on the nightly toolchain (LLVM default · codegen-gcc CPU · cuda-oxide
  GPU). It's a custom `rustc` backend that compiles GPU kernels in pure Rust (Rust→MIR→Pliron→LLVM→
  PTX via `cargo oxide build`, `llc`/`CUDA_OXIDE_LLC`) — *the* reason teri must be nightly. Apache-2.0
  (not GPL). teri has no GPU kernels yet, so no GPU build target lands here; authoring the first
  cuda-oxide kernels is a follow-up feature. `RUNBOOK.md §3.1`.
  *(Correction: an earlier draft of this slice wrongly added the unrelated `cudarc` driver-bindings
  crate after mis-identifying "cuda-oxide" as the abandoned Protryon GPL crate. Reverted — cuda-oxide
  is NVlabs' Rust→PTX compiler, not a runtime binding, and is not interchangeable with cudarc.)*

**Risk notes**
- Floating nightly is a moving floor — this is why S14 is **last**: all feature work (S0–S13) lands on
  the current `1.94.1` pin first, so any nightly breakage is isolated to this one slice, not smeared
  across the sprint.
- Single toolchain means **no stable fallback** — once teri is on nightly there is no stable build to
  retreat to. So S14 does not merge until nightly CI (`ci.yml` + `pebesen-ci.yml`) is fully green;
  the `--help` keyless probe and the backend-honesty guard must both pass on nightly before merge.
- Keep the backend-honesty guard and the preflight gate intact across the bump — never weaken a guard
  to make the new toolchain pass.

---

## Tracking

As each slice merges, tick its `TASK-*` in `FEATURE-PARITY.md` and update `RUNBOOK.md` §12. This
file is done when the slice table above is all ☑ and the parity ledger has no open task.
