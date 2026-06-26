---
description: Audit or upgrade teri's "simulate and predict anything" claim with evidence and counterargument.
argument-hint: [QUESTION="claim, feature, or scenario to test"]
---

Audit Teri's simulation/prediction capability claim for:

QUESTION: $QUESTION

Codex command rule: run repo commands through `rtk` (`rtk rg`, `rtk cargo`, `rtk git`, `rtk bash -lc`, etc.).

Use this structure:

1. Source truth
   - Read `src/main.rs`, `src/pipeline.rs`, `src/api/mod.rs`, `src/sim/mod.rs`, `src/report/mod.rs`, and `src/preflight.rs`.
   - Identify what is actually wired, what is tested, and which runtime path is narrower than the docs imply.

2. Strongest true claim
   - State what Teri can simulate/predict today in positive terms.
   - Keep it bounded to seed materials, graph extraction, agent personas, finite social/action grammar, memory, intervention hooks, and report synthesis.

3. Counterargument
   - Explain why "simulate and predict anything" is not literally true.
   - Cover seed-data limits, ontology extraction loss, prompt/model sensitivity, missing calibrated probabilities, no causal proof, finite action compression, and absent specialized solvers for domains like physics, epidemiology, markets, weather, supply chains, and adversarial security.

4. Upgrade plan
   - If asked to fill a gap, propose concrete repo changes: tests, docs, adapters, solvers, calibration harnesses, benchmark fixtures, or provenance fields.
   - Do not paper over limits with wording. Either implement a capability or document the boundary.

5. Verification
   - Run `rtk rg` for stale docs, plus `rtk cargo check`/targeted tests or explain why they were not run.

Never present report `confidence` as calibrated probability unless a calibration/evaluation implementation is present and cited.
