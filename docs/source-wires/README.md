# Teri Source Wires

## Purpose

Issue 86 creates a checked-in source-wire surface for Teri so future work can start from inspected evidence instead of re-discovering the same repos, PRs, and research lanes.

## Definition of a wire

A wire is a durable contract between Teri and an external source. In this issue, a wire means a machine-readable registry entry, human-readable docs, explicit evidence paths and adoption gates, and validation that every selected source is represented exactly once.

It does not mean vendoring external code, silently adding dependencies, or claiming live integrations that have not been proven.

## Required Add wires

- `fabio-rovai/brain-in-the-fish`
- `MOZARTINOS/mirofish-guide`

## Selected optional wires

- `666ghj/MiroFish#325`
- `666ghj/BettaFish`
- `ericcurtin/inferrs`
- `FlexNetOS/cellm`
- `cryscan/web-rwkv`
- `cluaiz/cluaiz`
- `Piebald-AI/splitrail`
- `algorithmicsuperintelligence/openevolve@80945ed82886d5c4ff2f3d22436765d50cb61266`

## Deferred sources

See [deferred-sources.md](./deferred-sources.md). Deferred items are recorded so later agents do not silently widen scope or pretend an uninspected source was covered here.

## How to read FACT / INFERENCE / POLICY / QUESTION / GAP / BLOCKER

- `FACT`: directly supported by inspected repo files, local Teri code, or PR metadata gathered in this issue.
- `INFERENCE`: a reasoned conclusion from inspected evidence; useful, but not equivalent to direct proof.
- `POLICY`: a Teri-side safety rule or adoption rule we are imposing in this repo.
- `QUESTION`: unresolved point that needs stronger proof or a later implementation pass.
- `GAP`: known missing functionality or verification inside Teri.
- `BLOCKER`: something that would prevent a later adoption level from advancing.

## How future PRs adopt a source

1. Start from `teri wires show <id>`.
2. Re-check the listed evidence paths against current source state.
3. Confirm the target Teri surfaces and adoption gate still match reality.
4. Advance from `L0`/`L1`/`L2` only when code, tests, and local validation prove the new level.
5. Keep non-goals explicit so research references do not turn into hidden runtime behavior.

## Safety rules

- No external code vendoring, copying, or submodules in this issue.
- No network-dependent tests.
- No weakening of Teri's backend honesty guard.
- No default backend change to an unproven route.
- No telemetry upload, cloud token setup, or secret commits.
- No optimizer or autonomous-mutation path enters default runtime behavior from this issue.

## Registry and validation

- Registry: `src/source_wires.rs`
- CLI: `teri wires list`, `teri wires show <id>`, `teri wires validate`
- Validation rules: required wires must exist, selected optional wires must exist, non-deferred wires must carry adoption gates, and non-deferred wires must carry evidence paths.

`.context/` does not exist in this repo, so the issue-86 task CSVs live beside these docs instead of inventing a new context estate.
