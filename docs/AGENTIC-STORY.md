# Teri — Agentic Story (path to full autonomy)

How Teri goes from a human-driven studio to a **fully autonomous prediction service** — a loop that
seeds itself, predicts, acts, and self-calibrates with no human in the inner loop. This is the
target operating model; each layer below maps to concrete seams already in the codebase.

## The autonomy ladder

```
 L0  Manual        human drives every step in the UI/CLI
 L1  Scripted      `teri run` one-shot in a cron/CI job (exists today)
 L2  Triggered     community signal change auto-starts a run (CommunityAdapter)
 L3  Closed-loop   predictions auto-pushed back + actioned (CommunityFeedback)
 L4  Self-calib.   actioned outcomes tune confidence per community (calibration loop)
 L5  Self-directed an orchestrator decides WHAT to predict next, allocates compute,
                   spawns runs, and curates the backlog — no human in the inner loop
```

## The two agent layers (don't conflate them)

1. **In-simulation agents** — the swarm. Thousands of persona agents interacting inside `sim/`.
   These already exist and are the *subject* of prediction.
2. **The operating agent(s)** — the autonomy layer *around* the engine that decides when/what to run
   and what to do with the result. This is what makes Teri "completely autonomous."

## The autonomous loop (target)

```
        ┌──────────────────────────────────────────────────────────────┐
        │                     TERI AUTONOMY LOOP                         │
        │                                                               │
   ┌────▼─────┐   signal     ┌──────────┐   run     ┌──────────┐        │
   │ SENSE    │─────────────►│ DECIDE   │──────────►│ PREDICT  │        │
   │ Community │  topic/      │ what to  │  seed+    │ pipeline │        │
   │ Adapter  │  churn/risk  │ predict, │  query    │ (5 stage)│        │
   │ + watchers│  deltas      │ when     │           └────┬─────┘        │
   └────▲─────┘              └──────────┘                │ report       │
        │                                                ▼              │
   ┌────┴─────┐   calibrate   ┌──────────┐   push    ┌──────────┐       │
   │ LEARN    │◄──────────────│ OBSERVE  │◄──────────│ ACT      │       │
   │ tune     │  actioned?    │ outcome  │  feedback │ Community │       │
   │ confidence│  accurate?   │ vs truth │           │ Feedback │       │
   └──────────┘              └──────────┘            └──────────┘       │
        └──────────────────────────────────────────────────────────────┘
```

## How each stage becomes autonomous (with the real seam)

| Loop stage | Mechanism | Seam in code |
|---|---|---|
| **SENSE** | Poll community signal; detect deltas worth predicting | `CommunityAdapter::fetch_signal/topics` (`seed/community/`, planned) |
| **DECIDE** | Policy that turns a signal delta into a `(seed, query)` job | autonomy orchestrator (new; sits above `pipeline.rs`) |
| **PREDICT** | Run the 5-stage pipeline headless | `pipeline::run_pipeline` (exists) — provider-agnostic |
| **ACT** | Push report-derived signals back to the platform | `CommunityFeedback::push_*` → pebesen `intelligence` crate |
| **OBSERVE** | Did a moderator action the prediction? Was it accurate? | pebesen `intelligence` action endpoint (planned receiver) |
| **LEARN** | Adjust per-community confidence weighting | calibration loop (new; persists weights in redb) |

## What already supports autonomy today

- **Headless, scriptable engine** — `teri run` composes the full pipeline and emits `verdict.json`;
  no interactive prompts (CLI parses before config; help is keyless).
- **Provider-agnostic inference** — `LlmClient` + `ProviderAdapter::from_config` so the loop can run
  against any backend (hosted or local shimmy/ruvllm).
- **Fail-closed honesty** — `verify_backend` guarantees the loop never fabricates a run on a stub
  backend (critical for an unattended loop — silent garbage is worse than a hard stop).
- **In-process, single-binary** — no Python/subprocess/Docker orchestration to babysit; one process
  the orchestrator can supervise.
- **Live observability** — SSE for ticks, logs, and report events lets a supervising agent watch a
  run's health in real time instead of polling blind.

## What must be built for L2–L5 (backlog → see FEATURE-PARITY.md)

1. **CommunityAdapter / CommunityFeedback** (teri) + **prediction receiver** (pebesen `intelligence`)
   — the SENSE and ACT seams. *(this slice)*
2. **Autonomy orchestrator** — a supervising loop (the DECIDE layer): watches adapters, debounces
   signal deltas into jobs, schedules runs, enforces a compute budget, writes results to the backlog.
3. **Calibration loop** — turn actioned/accurate outcomes into per-community confidence weights;
   this is also what upgrades report `confidence` from "synthesized metadata" to a calibrated number.
4. **Continuity/Resume** — checkpoint the loop so a restart resumes mid-cycle (mirror the meta
   workspace's handoff-kernel pattern: durable state + resume signal).
5. **Guardrails** — rate limits, action-approval gates for high-impact pushes, and an audit trail so
   an autonomous push is always traceable to the run + seed that produced it.

## Design stance

- **Engine stays a library + binary; autonomy is a thin layer above it.** The orchestrator calls
  `pipeline::run_pipeline` and the `Community*` traits — it never reaches inside the stages. This
  keeps the autonomous and human-driven paths on the same verified core.
- **Every autonomous action is witnessed.** No prediction is pushed to a live community without a
  durable record of the run, seed, and confidence that justified it — fail-closed, like the backend
  guard.
