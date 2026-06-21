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

## ITERATE cycle-3 addendum (2026-06-14, LIGHTWEIGHT find)
- **Gate worked, twice:** opus parity gate caught 2 no-downgrade defects in the social-taxonomy port by reading source dispatch + `to_episode_text` — TREND dropped (survives `FILTERED_ACTIONS`, IS recorded) and Like/Dislike post-vs-comment narrowed (separate render paths + ID namespaces). FAIL→fix→re-verify→PASS (parity.md l104-171, commit dabcb77). Keep this source-reading strength.
- **Lesson applied (strengthen-only, in-scope):** added a consolidation/omission-discriminant rule to `rust-port-porter` agent def + `rust-port-translate` SKILL — prove downstream-indistinguishability before COLLAPSING/OMITTING a variant, else preserve the discriminant. Cuts collapse-then-bounce retries across the remaining ~45 units. LESSONS.md row appended.

## ITERATE cycle-4 addendum (2026-06-14, LIGHTWEIGHT find)
- **reuse-Y over-classified at DISCOVER:** all 6 verified BACKEND reuse-Y units reclassified under the differential gate (0/6 held) — U-006/U-008/U-009→extend-Y, U-013→port-fresh, U-004→`- [≠]`, U-048→extend-Y (merge-ledger.md l97-104; reuse-verify-{llm,seed,infra}.md; commit 7950c28). The "verify-only quick-win" framing was optimistic; reality was six small ports. GATE held (reuse-is-never-trusted caught every divergence — keep strength); only the plan was wrong.
- **Lesson applied (strengthen-only, in-scope, no gate relaxed):** marked reuse-Y PROVISIONAL (`reuse-Y?` = architect claim, not a verified state) in `rust-port-architect` agent def + `references/merge-ledger.md` (classification table + reuse rule) — plan reuse-Y as differential-verify-plus-probable-small-port, spot-check claims vs actual Y source before asserting; cite 6/6. LESSONS.md row appended. PR to harness_hub.

## ITERATE cycles 8–9 addendum (2026-06-14, MATERIAL owner-flagged find — `[≠]` bar)

**OWNER FLAG (verbatim):** "[≠] noted as optional sounds like a feature skip. If you do not add it to the rust port then is it still optional here? This is a bad-behavior flag to me."

### Gate-quality gap (the material one)
- **`[≠]` (intentional-divergence) was being used to SKIP portable features** by rationalizing "the dest's architecture won't use it" — and the parity gate **ACCEPTED** the rationalization. Two instances, both corrected to PORTED and both passing re-verification:
  - **U-018** (commit 9aa1dd0): `to_reddit_format`/`to_twitter_format`/`to_dict` (OASIS profile serializers) were `[≠]`'d "teri's native sim consumes `SocialProfile` directly, so OASIS export not needed." These are cheap serializers producing real observable output shapes (specific keys) MiroFish has → not porting them = teri has **less** capability. Corrected: PORTED (exact keys, conditional omission). The skip ALSO **hid a second downgrade** — bio+persona had been collapsed into one field, a narrowing the serializers exposed (loop_state.md l22-25 NO-DOWNGRADE CORRECTION).
  - **U-004** (commit 1d8e293): rotating-FILE logging `[≠]`'d "console-by-design" (merge-ledger.md l103). Persistent file logging is a real feature → corrected: PORTED (file-rotate 10MB×5, opt-in `TERI_LOG_DIR`).
- **Root gap:** the verifier def only required a `[≠]` be "explicit, with rationale + owner approval" (l43) — it never required *challenging whether the divergence is a disguised portable-feature skip*. "won't be used" passed as a rationale. That is the class of gap.

### What was applied (strengthen-never-weaken — this only HARDENS no-downgrade)
Tightened the `[≠]` bar precisely across the harness: `[≠]` is permitted ONLY when the source behavior is **(a)** genuinely INEXPRESSIBLE in the dest (substrate truly can't represent it → really a `- [!]`), **(b)** NON-CONTRACTUAL/unobservable (retry jitter; a filtered-before-recorded op; a GIL/console artifact), or **(c)** a strict SUPERSET (dest has MORE, loses nothing). NEVER as "won't be used" / "probably unused" / "consumes it directly so the export isn't needed" / "`serde` covers it" for a feature that produces a **distinct observable output** (a serialization shape, an export format, a file sink, a CLI flag, a distinct render path). **A portable feature is ported, not `[≠]`-skipped — when in doubt, PORT IT.** The **parity-verifier must CHALLENGE every `[≠]`** and **FAIL a disguised skip** (routes back to the porter to PORT it).
- Applied to: `agents/rust-port-parity-verifier.md` (gate CHALLENGES `[≠]`, FAILs disguised skip + verdict-rollup), `agents/rust-port-porter.md` + `skills/rust-port-translate/SKILL.md` (don't propose `[≠]` for a portable feature), `agents/rust-port-architect.md` (don't classify a portable feature as a divergence), `references/parity-ledger.md` (the canonical `[≠]`-bar definition + "when in doubt port"), `references/merge-ledger.md` + `references/runtime-constructs.md` (the bar on the merge/map-onto side). LESSONS.md row appended; harness_hub PR with change-history row, auto-merge armed.

### Legitimate-`[≠]` sanity check (the rule does NOT over-correct)
The three known-good `[≠]` remain valid under the precise definition — they are exactly cases (b)/(c), not skips:
- **retry jitter** (parity.md l205) — case (b): stochastic sleep duration, perturbs no output/error/side-effect → non-contractual. VALID `[≠]`.
- **REFRESH in `FILTERED_ACTIONS`** (parity.md l108, l157; S-TAX-020) — case (b): filtered out *before* it is ever recorded as an `AgentActivity`, produces no observable output. VALID `[≠]`. (Correctly distinguished from TREND, which survives the filter and IS recorded → was a DROP, now ported.)
- **`is_supported` json-superset** (parity.md l291) — case (c): teri's accepted-extension set is a strict superset of MiroFish's (identical behavior on the 4 shared extensions, adds json teri genuinely ingests). VALID `[≠]`.

### Queued (proposed-upgrades.md) — interim `[≠]` re-audit under the tightened bar
The pre-DONE left-behind sweep already re-validates `[≠]`, but the owner flagged it NOW. Queued a tracked task to re-challenge this run's ~20+ existing `[≠]` ledger rows under the precise bar before DONE — surfacing in particular the remaining U-018-adjacent `[≠]` and the Zep-mechanics `[≠]` (S-189/S-193/S-194 async-batching) as ones to re-challenge. Not done here (queued, not force-run); the three above are already confirmed legitimate.

### No-downgrade attestation
This change ONLY strengthens: it converts "`[≠]` needs a rationale" into "`[≠]` needs a rationale that survives a challenge proving inexpressible/non-contractual/superset, else the feature gets PORTED." No parity, symbol, merge, or DONE condition is loosened; the gate becomes *harder* to pass (a "won't be used" `[≠]` that used to pass now FAILs → port). Scope law honored (rust-port harness only).
