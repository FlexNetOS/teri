# Run evaluation — rust-port (MiroFish → teri), ITERATE cycles 1–2 (iteration 2)

**Evaluated:** 2026-06-14 by evolution-steward (`/harness:harness-evolution find`, LIGHTWEIGHT — HAND OFF, not DONE)
**Run:** port-and-merge, **target==dest==teri** (Python Flask/OASIS → existing Rust app)
**Phase reached:** ITERATE cycles 1–2 done (U-015 verified; GAP-1 valid_at + GAP-2 vec-sim enablers verified). 1/50 units verified.
**Source artifacts read:** loop_state.md, baseline.md (CORRECTION block), findings/parity.md (U-015 + cycle-2 verdicts), parity-ledger.md, merge-ledger.md. Commits on port/mirofish: 9836238 (repair), 064655c (U-015), 4cdfd0d (valid_at+vec_sim).

## Scorecard

### What worked (the no-downgrade machinery held)
- **The no-downgrade gate CAUGHT the false-green** — not at DISCOVER (where it should have), but the
  ITERATE-cycle-1 porter could not build c894de8, which surfaced the bad PR-#4 merge. The harness
  failed *closed* in the end: the bad tip blocked work rather than letting a regression through. Repair
  landed (9836238); TRUE baseline re-established at 156 green, then 171 by cycle 2.
- **The parity gate did real differential work, not existence-checking.** Cycle-2's cosine check ran an
  *independent* magnitude-vs-direction differential (query `[1,1,0]` vs P=`[10,0,0]` high-magnitude-wrong
  vs Q aligned) to prove genuine cosine, not raw dot (parity.md Claim B). Serde backward-compat was
  pinned (old JSON without `valid_at` deserializes). GAP-OQ3-EMBED was honestly left `- [!]` with a
  grep-proof that no fake/random embedder exists — no fake-pass. This is the gate behaving exactly as
  designed.
- **No silent drops.** Every U-015 symbol (17/17) covered `- [x]`/`- [≠]` or distributed-with-citation to
  U-012/U-013; chunking adjudicated to U-013 as `- [!]`, not dropped.

### Friction / gaps (fix targets)
- **HIGH (gate-quality) — FALSE-GREEN DISCOVER BASELINE (F-G1).** `baseline.md` asserted "Teri Build ✓
  PASS / 142 tests GREEN on develop @ c894de8" (line 14) but c894de8 (PR #4 merge) does **not compile**
  (duplicated `api_key` field in config.rs; dead `Config::from_env()` + dup block in main.rs — CORRECTION
  block line 6; loop_state 25-29; parity.md 14). The DISCOVER build-health gate emitted a GREEN verdict
  it had not executed against the real dest tip. The loop's entire reference baseline was a phantom for
  iterations 0–1; only the cycle-1 porter's build failure exposed it. A gate that reports green it did
  not run is the highest-value upgrade target this iteration.
- **MED (return-integrity) — CROSS-LOOP RELAY HIJACKED A SUB-AGENT'S RETURN (F-G2).** A concurrent
  envctl/forge-loop weave/relay heartbeat ("relay:resumed", a DriftSummary in a *different* worktree)
  reached the cycle-2 porter mid-run; the porter returned an ACK of *that* message instead of its
  work-summary. The work was correct on disk, so the orchestrator recovered by verifying via git — but it
  could not trust the return. Sub-agents have no rule to ignore foreign relay/weave/notification noise and
  to always return their own work-summary. Sibling of the iter-1 drop-resilience class (continuity noise
  interfering with an agent's deliverable).
- **LOW (port-in-place build friction) — WORKTREE [workspace] ARTIFACT (F-G3).** Building a teri worktree
  nested under `meta/.worktrees` required adding `[workspace]` to teri/Cargo.toml (teri is a meta-root
  member; cargo walks up and rejects the worktree as a non-member). Harmless to teri's standalone CI but a
  structural artifact that MUST be stripped before develop→main promotion (loop_state 28-29). The
  port-in-place (target==dest, dest is a meta-workspace member) setup has no documented worktree-build
  recipe → operator hand-added it.

### Coverage
- Intact. 1/50 units verified + 2 cross-cutting enablers (GAP-1/GAP-2) resolved, both unlocking named
  downstream units (U-017/U-021/U-024) that remain `- [ ]`. Nothing capped or silently deferred;
  GAP-OQ3-EMBED and GAP-U015-1 carried forward `- [!]` with owners.

### Human walls
- One genuine, correctly-handled stop: the non-compiling dest tip. That is *exactly* where the loop
  should fail-closed. The defect is that it should have been caught at DISCOVER (by an executed baseline
  build) rather than one full iteration later at the cycle-1 porter.

## Lessons mined (see LESSONS.md rows)
1. **F-G1 false-green baseline** → build-health-auditor (executed-evidence baseline) + SKILL Phase 1 step 6
   (fail-closed on a red dest tip). **APPLIED — strictly STRENGTHENING the no-downgrade baseline gate.**
2. **F-G2 cross-loop relay-noise hijack** → porter agent def + orchestrator agent-spawn contract
   (ignore foreign relay/weave/notification; always return your work-summary). **APPLIED.** Recurrence 2 of
   the continuity-noise class.
3. **F-G3 port-in-place worktree [workspace] build recipe** → merge-ledger §Port-in-place +
   loop_state note (worktree-only, strip-before-promote). **PROPOSED** (promotion-adjacent; touches the
   develop→main promotion step) — see proposed-upgrades.md.

## No-downgrade attestation
The baseline-gate edit only **strengthens**: it converts "confirm it builds" into "paste the executed
`cargo build` + `cargo test` evidence with real counts, and FAIL-CLOSED (NEEDS-HUMAN / recorded repair
task) on a non-compiling dest tip — never emit GREEN you did not run." No parity, symbol, merge, or DONE
condition is loosened. The relay-noise rule adds a return-integrity requirement; it relaxes nothing. F-G3
is proposed, not applied, because it is promotion-adjacent. Scope law honored (rust-port harness only).
