# Teri — TODO

> **Status (2026-06-23):** Teri is a real, near-complete Rust port of MiroFish — **1671 tests
> green**, all five pipeline stages implemented and tested, and the full pipeline runs today via
> `teri serve` + the REST/SSE API. The Web UI and the teri↔pebesen community loop both landed
> 2026-06-23. This file no longer carries a phase plan.

## The backlog lives in two places — do not duplicate it here

| Document | Role |
|---|---|
| [`FEATURE-PARITY.md`](./FEATURE-PARITY.md) | **Authoritative** parity ledger vs MiroFish — every open `TASK-*` with file:line evidence. The source of truth for what remains. |
| [`SPRINT.md`](./SPRINT.md) | The **execution order** over that ledger — the final sprint (S0–S14), one PR per slice, ending with the toolchain upgrade. |
| [`RUNBOOK.md`](./RUNBOOK.md) §12 | The parity-**verification** surface (how each capability is checked). |

The historical `MIROFISH-PORT-PLAN.md` parity matrix in the meta workspace is **stale** —
superseded by `FEATURE-PARITY.md`. Do not extend it.

## What remains (summary — see `FEATURE-PARITY.md` for detail)

- **UI follow-ups** — live-verify, SSE adoption, branding/polish (TASK-UI-1..3).
- **Community seam** — sqlx-backed store, full-loop E2E (TASK-SEAM-2..3).
- **Simulation fidelity** — persona generation, action split, social-DB producer,
  provider-agnostic serve, shutdown hook, structured-output/ontology (TASK-SIM-1..6).
- **Autonomy (L2–L5)** — DECIDE orchestrator + calibration loop (TASK-AUTO-1..2).
- **Toolchain** — nightly + `rustc_codegen_gcc` + `wild` + `kache` + CUDA-nightly (`SPRINT.md` S14).

Tick items in `FEATURE-PARITY.md` as slices merge; this file only points the way.
