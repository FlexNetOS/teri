# Proposed upgrades — rust-port (MiroFish → teri), ITERATE cycles 1–2 (iteration 2)

Applied upgrades (low-risk, in-scope) are in the harness_hub PR (change-history row + LESSONS rows):
the F-G1 executed-evidence baseline gate (strengthening) and the F-G2 relay-noise return rule. This file
flags the **promotion-adjacent** item deferred for owner sign-off, plus the still-open structural items
carried from iteration 1. Scope law: rust-port harness only.

## A. Promotion-adjacent — PROPOSED for owner sign-off (NOT applied)

**F-G3 — port-in-place worktree-build recipe (`[workspace]` aid + strip-before-promote).**
- **What:** when `rust_target == dest_repo` and the dest is a **meta-workspace member**, cargo walks up to
  the outermost workspace and rejects a nested worktree (under `meta/.worktrees`) as a non-member, so
  building the worktree requires adding a worktree-only `[workspace]` table to the dest `Cargo.toml`
  (loop_state l28-29 — operator hand-added it). It is harmless to the dest's standalone CI but is a
  structural artifact that **must be stripped before the develop→main promotion**.
- **Proposed edit:** a `references/merge-ledger.md` §Port-in-place worktree-build note — where to put the
  `[workspace]` table, that it is worktree-only, and an explicit strip-before-promote checklist item in
  the develop→main promotion step.
- **Why proposed, not applied:** it touches the **promotion** step (develop→main), so it is gate/promotion-
  adjacent; per the fail-closed policy a promotion-touching change is proposed, not auto-applied. It does
  not weaken anything — it adds a strip step — but owner should confirm the promotion-checklist wording.
- **Owner action:** approve the merge-ledger §Port-in-place note + the strip-before-promote checklist item.

## B. Structural / cross-harness — carried from iteration 1, still DEFERRED for owner sign-off

These were proposed at DISCOVER (iter 1) and remain open — re-listed so they aren't lost at the budget
boundary. Cross-harness ⇒ propose, never force-apply (scope law).

1. **Promote `discover_progress:` incremental-commit + cold-resume to the packaged-harness standard.**
   Applied to rust-port only; the "long multi-step phase commits once → an interruption strands
   everything" class also hits meta-plugin's organize pass and code-research's map+analyze fan-out.

2. **Promote the drop-resilience agent-spawn contract (incremental disk write + bounded pointer-return) to
   the standard.** The socket-drop-strands-the-phase failure can hit any harness whose agents emit large
   multi-file deliverables (code-research analysts, meta-plugin reporters). **This iteration strengthens
   the case:** F-G2 (relay-noise return-hijack) is the *sibling* class — both are "the agent's return
   payload got corrupted/lost by something other than the work itself." Propose adopting BOTH the
   incremental-write rule AND the ignore-foreign-relay-noise + always-return-work-summary rule as one
   standard "agent return-integrity contract" for every packaged harness.

3. **Reusable vendored-dependency exclusion helper script (`scripts/`).** Note it now; bundle it on the
   second hand-written recurrence of the exclude-glob list.

## C. Gate-adjacent note (already applied this iteration, owner awareness)

F-G1 strengthened the baseline gate (executed-evidence + red-tip-fail-closed) and was **applied** because
it is strictly STRENGTHENING the no-downgrade reference (it can only make the gate harder to pass — a green
that wasn't run, or a red tip, now stops the loop). No revert option is offered because there is no
weakening to roll back; if the owner wants the executed-build requirement relaxed, that would be a
gate *weakening* and is refused by policy.
