# Run evaluation — rust-port (MiroFish → teri), DISCOVER, iteration 1

**Evaluated:** 2026-06-14 by evolution-steward (`/harness:harness-evolution find`)
**Run:** port-and-merge, **target==dest==teri** (Python Flask/OASIS → existing Rust app)
**Phase reached:** DISCOVER COMPLETE (ledger 50 units / 1087 symbols; architect+merge-ledger+research+
cross-repo-refs+baseline+y-regression all present). 0 ITERATE cycles yet.
**Source artifacts read:** loop_state.md, parity-ledger.md(45), symbol-map.md(40), merge-ledger.md,
research.md(40), baseline.md, findings/y-regression.md, worktree git log.

## Scorecard

### Friction (wasted cycles / retries / guesses)
- **HIGH — architect agent died mid-stream (socket close) at 402s / 29 tool-uses having written NOTHING
  to disk.** A re-spawn that wrote each deliverable incrementally and returned <400 words succeeded.
  Whole-phase strand from one connection drop. (F1)
- **HIGH — a PRIOR DISCOVER session was interrupted and left UNCOMMITTED** (parity-ledger harvest dated
  2026-06-13; the single DISCOVER commit `897ba5d` is 2026-06-14). Partial deliverables on disk but no
  loop_state/symbol-map/architect/HANDOFF → not resumable via the documented RESUME path; effectively
  re-done. Phase 1 commits ONCE (step 7), so any mid-DISCOVER drop strands everything. (F2)
- **MED — operator had to hand-annotate the target==dest collapse** in loop_state (lines 8-18) and the
  merge-ledger header, because the SKILL assumes rust_target ≠ dest_repo. The merge step/ledger partly
  duplicate the port step/parity-ledger with no documented handling. (F3)
- **MED — the cartographer had to manually scope out MiroFish's bundled `.venv`** and manually fall back
  to the Python `ast` module: symbol-map.md line 7 records "`git kb code index` not used (indexer not
  configured for this source tree)" and line 10 records "venv/node_modules/dist EXCLUDED". Both were
  ad-hoc operator judgment the skill does not prescribe. (F4)

### Gate quality
- No parity/merge gate ran yet (0 ITERATE cycles), so no missed-defect / false-block evidence at the
  parity grain. **One gate DID fire at DISCOVER and was too coarse:** the whole-source source-runnable
  precondition (SKILL Phase 1 step 6) is binary "source can't be executed → NEEDS-HUMAN, stop." MiroFish
  is **partly** runnable (local utils/models run; Zep-Cloud/OASIS paths need external creds and are
  map-onto-substrate, verified by behavioral equivalence of the mapped teri path — loop_state 32-34,
  research 13-22). A literal reading of the binary gate would have **false-halted DISCOVER**. The
  operator correctly classified per-path instead — undocumented. (F5) — this is **recurrence 2** of the
  already-applied "DISCOVER source-runnable precondition" lesson (CLAUDE.md change row v1.7.1).
- The DONE/parity/symbol gates themselves remain sound; nothing here weakens them. The venv-exclude and
  per-unit-runnable refinements only **tighten** coverage accuracy (more real source covered, fewer
  false halts) — strengthen-only, no-downgrade intact.

### Coverage
- Strong. 50 units / 1087 symbols harvested from REAL source only; venv correctly excluded (manually).
  6 cross-cutting GAP rows flagged `[!]` (no silent drop); SWEEP-4 dropped as `[≠]` intentional. The
  risk was the inverse of "left behind" — a naive harvest would have **over**-covered (15k+ venv .py
  files inventoried as source). Default exclusion makes that accuracy automatic, not operator-dependent.

### Human walls
- No genuine NEEDS-HUMAN wall hit. The one place the harness *would* have walled (binary source-runnable
  gate) was an **avoidable false wall** — closed by the F5 per-unit refinement.

## Lessons mined (see LESSONS.md rows)
1. **F4 venv/vendored-dependency default-exclude + non-git-kb AST fallback** (accuracy) → inventory skill,
   symbol-map ref, cartographer. APPLIED.
2. **F1 large/multi-file-deliverable agents need drop-resilience** (incremental disk writes + bounded
   pointer-summary return) → architect/cartographer/researcher defs + orchestrator agent-spawn contract.
   APPLIED.
3. **F2 incremental DISCOVER commits + a DISCOVER-progress checkpoint** so an interrupted DISCOVER resumes
   cold → SKILL Phase 1. APPLIED.
4. **F3 target==dest port-in-place configuration** (merge collapses to landed-in-dest + dest-not-regressed)
   → SKILL Phase 0/1 + merge-ledger ref. APPLIED.
5. **F5 per-unit/per-path source-runnable classification** (locally-differentiable vs external-service →
   substrate-path), not a whole-source binary gate → SKILL step 6. **Gate-adjacent: prose clarification
   APPLIED; the NEEDS-HUMAN-trigger wording change PROPOSED** for owner sign-off (fail-closed). Recurrence 2.

## No-downgrade attestation
Every applied edit only **strengthens** accuracy/coverage or adds resilience. None loosens a parity,
symbol, merge, or DONE condition. The F5 refinement keeps the hard NEEDS-HUMAN trigger for any unit that
genuinely requires running the source and has no runnable path; it only removes the false-halt on
external-service paths that are verified via the substrate. Strengthen-only, scope law honored
(rust-port harness only).
