# Proposed upgrades — rust-port (MiroFish → teri DISCOVER, iteration 1)

Applied upgrades (low-risk, in-scope) are in the harness_hub PR. This file flags the **gate-adjacent**
change for explicit owner awareness, plus structural items deferred for owner sign-off. Scope law:
rust-port harness only.

## A. Gate-adjacent — APPLIED as strengthen-by-precision, flagged for owner awareness

**F5 — per-unit/per-path source-runnable classification (replaces the whole-source binary halt).**
- **What changed:** SKILL Phase 1 step 6. The DISCOVER source-runnable precondition (added v1.7.1) was a
  binary "source can't be executed → NEEDS-HUMAN, stop." It now classifies units as
  *locally-differentiable* (MUST be runnable — its parity gate needs the source) vs *external-service /
  map-onto-substrate* (verified by behavioral equivalence of the mapped dest/substrate path, never by
  running the source's external path).
- **Why this is strengthen-only, not a weakening:** every TRUE halt is preserved verbatim — toolchain
  can't run at all; a locally-differentiable unit has no runnable path; (merge) Y or a named substrate
  absent. The ONLY thing removed is a **demonstrably false** halt: an external-service path that is
  map-onto-substrate-verified was *never going to be run differentially against the source*, so halting
  the whole DISCOVER on it blocks correct work with no safety benefit. Precision up, true-positives
  unchanged.
- **Evidence + recurrence:** recurrence 2 of the applied "DISCOVER source-runnable precondition" lesson
  (CLAUDE.md v1.7.1). MiroFish is partly-runnable (loop_state 32-34, research 13-22): local utils/models
  run; Zep-Cloud/OASIS paths need creds and are map-onto-substrate. A literal binary gate would have
  false-halted this DISCOVER; the operator already (correctly, but undocumentedly) classified per-path.
- **Owner action:** confirm the refined NEEDS-HUMAN trigger wording. If you prefer the binary gate
  retained until a separate sign-off, revert the SKILL step-6 hunk only (the other four applied upgrades
  are independent of it).

## B. Structural / cross-harness — DEFERRED for owner sign-off (not applied)

1. **Promote `discover_progress:` incremental-commit + cold-resume to the packaged-harness standard
   (cross-harness).** F2 was applied to rust-port only (scope law). The same "long multi-step phase
   commits once → an interruption strands everything" class applies to any harness with a long DISCOVER
   (meta-plugin's organize pass, code-research's map+analyze fan-out). Propose adopting the
   per-deliverable-commit + progress-checklist pattern in `docs/packaged-harness-standard.md` so every
   loop harness gets resumable long phases. Cross-harness ⇒ propose, never force-apply.

2. **Promote the drop-resilience agent-spawn contract (incremental disk write + bounded pointer-return)
   to the standard (cross-harness).** F1 was applied to rust-port's DISCOVER agents + orchestrator. The
   same socket-drop-strands-the-phase failure can hit any harness whose agents emit large multi-file
   deliverables (code-research analysts, meta-plugin reporters). Propose a standard rule: "an agent
   producing a large/multi-file artifact writes it incrementally and returns a <400-word pointer-summary,
   never the full content." Cross-harness ⇒ propose.

3. **A reusable vendored-dependency exclusion helper script (`scripts/`).** F4 was documented in the
   inventory skill + symbol-map ref + cartographer. If the exclude-glob list is hand-written each run, a
   tiny `scripts/source-walk-excludes.txt` (or a one-line helper that emits the find/index prune args)
   would prevent drift between the three places it is now stated. Deferred because it is an optimization,
   not yet a recurrence — note it; bundle it on the second hand-written recurrence.
