# CLAUDE.md — teri (Swarm Intelligence Prediction Engine)

Autonomous-operation guidance for agents working in this repo. This replaces the upstream
interactive plan-review prompt (which paused for user input at every section — incompatible with
the meta workspace's autonomous loop). Its engineering preferences are preserved below verbatim
in spirit; they still govern every change.

## Engineering preferences (inherited, binding)

- **DRY is important** — flag repetition aggressively.
- **Well-tested code is non-negotiable** — too many tests beats too few.
- **"Engineered enough"** — not under-engineered (fragile, hacky), not over-engineered
  (premature abstraction, unnecessary complexity).
- **Err toward handling more edge cases**, not fewer; thoughtfulness > speed.
- **Explicit over clever.**

## How work lands here (meta workspace discipline)

- Fresh git worktree per change (`git worktree add .worktrees/<slug> -b <slug> origin/main`);
  never on the shared checkout (another session may hold it).
- One vertical slice per PR; PRs to `main` with auto-merge on green checks. Never merge red.
- Witness milestones in the handoff kernel (`/checkpoint`) and use `/handoff` to close segments —
  hand-written handoff markdown at repo root is guard-denied workspace-wide (ADR-0004).
- Full unfiltered test summaries in PR bodies (`cargo test` count, failures verbatim).

## What teri is (state of truth, 2026-06-12)

A Rust rewrite of MiroFish (AGPL upstream; this is an MIT independent reimplementation — parity by
spec, never by code copy). The five-stage pipeline mirrors upstream: seed → graph → agents → sim →
report. **140+ tests green.** Real today: seed ingestion (pdf/md/txt/json/url), LLM adapter layer
(OpenAI-compatible/Anthropic/Gemini, retry/backoff), persona generation (minijinja), two-phase sim
loop with bounded tokio concurrency, report generation, redb memory store. Placeholder today:
`KnowledgeGraph::build` orchestration (P1 keystone), pipeline/API wiring in `main.rs` (both
subcommands preflight then bail with explicit errors), memory write-back hooks.

The build order and parity matrix live in `~/Desktop/meta/MIROFISH-PORT-PLAN.md` (P1 wire-the-spine
→ P2 parity-core → P3 serve+estate → P4 scale+provenance). Do not re-derive scope — extend that plan.

## Secrets contract (owner architecture intent)

teri NEVER documents or expects raw `export LLM_API_KEY` workflows. The key arrives via **envctl
injection**: canonical form `env-ctl run --provider <p> -- teri run …` (vault-held key, child-env
only; envctl's data-plane phase). Until that phase lands, the vault registration is
`env-ctl secret add teri-llm --provider <p> --value-stdin`; local development may use `.env`
(gitignored). The missing-key error message points at this contract — keep it that way.

## Inference backend guard

`teri run`/`serve` preflight the backend (`src/preflight.rs`): list `/models`, then a 1-token
completion probe. Stub/canned backends are REFUSED — shimmy's SafeTensors engine returns
"Full transformer inference coming soon!" and a swarm pointed at it would fabricate an entire
simulation from canned text. Only GGUF-served (or real) backends pass. Extend `STUB_MARKERS` when
new stub engines appear; never weaken the guard to make a run proceed.

## Build, test, verify

```bash
cargo build                 # debug build
cargo test                  # full suite (keep it green; add tests with every change)
cargo fmt --all && cargo clippy --all-targets -- -D warnings
./target/debug/teri --help  # MUST work keyless (exit 0) — regression-probe this after CLI changes
```

CLI exit codes: usage errors = 2 (clap), runtime errors = 1, success/help = 0. Config loads only
inside commands — never before argument parsing.
