---
description: Refresh teri docs against current source truth.
argument-hint: [SCOPE="docs or claims to verify"]
---

Refresh teri documentation against the current code before making any operational or capability claim.

Scope: $SCOPE

Codex command rule: run repo commands through `rtk` (`rtk rg`, `rtk cargo`, `rtk git`, `rtk bash -lc`, etc.).

Required source scan:

- `src/main.rs`: confirm how `teri run` and `teri serve` load config, preflight, select providers, and dispatch work.
- `src/pipeline.rs`: confirm whether the one-shot `run_pipeline` is wired and what stages it composes.
- `src/api/mod.rs`: distinguish `build_provider_llm` for `run` from concrete `OpenAiAdapter` use in `ApiState`/`build_llm`.
- `src/sim/mod.rs`, `src/report/mod.rs`, and `src/preflight.rs`: confirm simulation mechanics, report confidence semantics, and backend honesty behavior.
- `README.md`, `RUNBOOK.md`, and `CLAUDE.md`: remove stale runtime, provider, test-count, or status claims.

Search for stale phrases before finishing:

```bash
rtk rg -n "Pipeline not yet implemented|preflights, then bails|Skeleton with real organs|1629 tests|hardcoded OpenAI" README.md RUNBOOK.md CLAUDE.md
```

Rules:

- Prefer source-grounded descriptions over exact test counts unless the count was produced in this run.
- Do not call Teri an oracle, causal proof engine, or literally unbounded predictor.
- If a doc says "simulate anything" or "predict anything", qualify it with representability, seed quality, ontology extraction, finite action grammar, LLM/backend quality, and missing specialized solvers.
- Record the counterargument next to the strong claim, not hidden in a later caveat.
- Validate with the smallest meaningful `rtk cargo ...` Rust gate, then run broader gates when code changed.

Finish by reporting changed files, stale-phrase check, and validation commands.
