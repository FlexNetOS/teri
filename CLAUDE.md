# Teri — Autonomous Development Guidance

## Architecture Overview

Teri is a Rust-native swarm intelligence prediction engine. The core architecture has three layers:

1. **CLI layer** (main.rs) — clap-based CLI with arg-parse-before-config discipline (--help works keyless)
2. **Config layer** (config.rs) — lazy env-driven configuration with envctl auto-injection support via agent-env.toml
3. **Runtime layer** (lib.rs + modules) — LLM adapter abstraction, simulation engine, agent pool, persistence

### Key Design Decisions
- **LLM provider agnostic**: `LlmClient` trait + concrete adapters (OpenAI, Anthropic, Gemini)
- **Config = env vars only**: no config files, no secrets on disk. Keys flow via envctl when available.
- **Stub guard mandatory**: all simulation paths preflight-check backend; stub backends are refused

## Dev Commands

```bash
cargo check        # Fast compilation check
cargo test         # Run all tests
cargo clippy       # Linting
cargo build --release  # Release binary at ./target/release/teri
```

## Envctl Integration

For secret injection, use envctl:
```bash
envctl run -- teri run --seed ... --query ...
envctl run -- teri serve --addr ...
```

The `agent-env.toml` file declares teri's required secrets.

## Stub Backend Guard

Both `teri run` and `teri serve` fail-closed through `preflight::verify_backend` before any
work (`run` before the pipeline, `serve` before binding). The guard:
- `GET {LLM_BASE_URL}/models` — refuses an unreachable backend or an empty model list
- 1-token `/chat/completions` probe — refuses canned stub text (`STUB_MARKERS`)
- on refusal, errors `inference backend unreachable …` / `backend … lists no models` /
  `REFUSING stub inference backend …`; never weaken it to make a run proceed
- `--help`/`--version` stay keyless (config + guard load only inside `run`/`serve`)

## Coding Conventions

- Error types use thiserror; prefer TeriError variants over anyhow
- All modules must be re-exported in lib.rs
- Config errors distinguish between Config (hard) and ConfigMissing (graceful degradation)
- Never write secrets to disk or config files
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

## What teri is (state of truth, refreshed 2026-06-22)

A Rust rewrite of MiroFish (AGPL upstream; teri is itself licensed AGPL-3.0-or-later, keeping faith
with that copyleft intent — see LICENSE). The five-stage pipeline mirrors upstream: seed → graph → agents → sim →
report. **The full `cargo test` suite is green (1700+ tests)** (was ~140 at the 2026-06-12
snapshot). All five stages and their
services are implemented and tested, and the **full pipeline runs today via `teri serve` + the
REST API** (`/api/graph` build → `/api/simulation` prepare/start → `/api/report` generate/chat).
Real today: seed ingestion (pdf/md/txt/json/url), `KnowledgeGraph::build` orchestration (LLM
ontology + 2-pass entity/relation extraction), native petgraph graph store, OASIS persona +
sim-config generation, native in-process `SimEngine` (two-phase ticks, dual Twitter/Reddit,
graph-memory write-back), ReACT `ReportAgent` + graph tools (InsightForge analog), interview/chat
endpoints, the axum HTTP server, embeddings + real cosine semantic recall (redb), LLM adapters.

**`teri run` is now fully wired** (`main.rs` → `pipeline::run_pipeline`, the in-process
`seed → graph → agents → sim → report` composition, tested in `tests/pipeline_run.rs`). The former
gap list is also closed: runtime provider selection (`build_llm` → `ProviderAdapter::from_config`),
native Anthropic/Gemini streaming, live `text/event-stream` routes, agent-LTM write-back from the
sim loop, English-default i18n (en/zh), and per-tick knowledge-graph context for agents all landed.
The remaining backlog (i18n breadth beyond en/zh, `/api/graph/data` `links` shape, the
`scheduled_events` stub) is in **`RUNBOOK.md` §13**; the parity verification is **`RUNBOOK.md` §12**.

The build order lives in `~/Desktop/meta/MIROFISH-PORT-PLAN.md` (P1 wire-the-spine → P2
parity-core → P3 serve+estate → P4 scale+provenance) — extend that plan; its **parity matrix is
stale** (superseded by `RUNBOOK.md` §12).

## Secrets contract (owner architecture intent)

teri NEVER documents or expects raw `export LLM_API_KEY` workflows. The key arrives via **envctl
injection**: canonical form `env-ctl run --provider <p> -- teri run …` (vault-held key, child-env
only; envctl's data-plane phase). Until that phase lands, the vault registration is
`env-ctl secret add teri-llm --provider <p> --value-stdin`; local development may use `.env`
(gitignored). The missing-key error message points at this contract — keep it that way.

## Inference backend guard

Both `teri run` **and** `teri serve` preflight the backend **fail-closed** through the single
guard `preflight::verify_backend` (`src/main.rs::preflight_backend` is the shared call site) —
`run` before the pipeline, `serve` before binding the socket. There is **one** guard; the former
weaker `lib.rs::preflight_check_backend` (a `/health`-body scan that silently accepted unreachable
backends and never guarded `serve`) was deleted — see [ADR-0004.1](docs/adr/adr-0004.1-backend-honesty-guard.md).

The guard:
1. `GET {LLM_BASE_URL}/models` — REFUSES an **unreachable** backend or one that **lists no models**.
2. 1-token `/chat/completions` probe — REFUSES canned stub text (matched against `STUB_MARKERS`).
   shimmy's SafeTensors engine returns "Full transformer inference coming soon!", and a swarm on
   canned text fabricates an entire simulation. Markers only match engines that ignore `max_tokens`
   (a real 1-token reply can't contain a multi-word marker), so false positives are impossible.

Refusals exit 1 before any work: `inference backend unreachable …` / `backend … lists no models` /
`REFUSING stub inference backend …`. `teri serve` therefore **refuses to boot** against a
stub/unreachable backend. Extend `STUB_MARKERS` when a new stub engine appears; **never weaken the
guard** to make a run proceed.

## Build, test, verify

```bash
cargo build                 # debug build
cargo test                  # full suite (keep it green; add tests with every change)
cargo fmt --all && cargo clippy --all-targets -- -D warnings
./target/debug/teri --help  # MUST work keyless (exit 0) — regression-probe this after CLI changes
```

## Living research map (auto-refreshed each session)

`scripts/gen-research-map.sh` regenerates `code-research/references/research-ledger.md` — an
always-current structural map of the repo (entry points, module inventory, HTTP route count,
test-fn count, code-intelligence stats). It is **gitignored and regenerated at every session
start** (wired into `.codex/hooks.json` SessionStart → `teri-context-session-start.sh`, which
also surfaces a one-line pointer into agent context). Run it by hand anytime; add `--reindex`
to rebuild the `git-kb code` index first.

Treat the map as a **navigation aid**, not a source of truth — the authoritative parity verdict
and capability matrix live in `RUNBOOK.md` §12, architecture/guards in this file. Never hand-edit
the generated ledger (edits are overwritten); change `scripts/gen-research-map.sh` instead.

CLI exit codes: usage errors = 2 (clap), runtime errors = 1, success/help = 0. Config loads only
inside commands — never before argument parsing.
