# Clawft Customization

Use this reference when applying the supplied `clawft` mission prompt. Treat it as a repo-specific operator brief layered on top of the core skill.

## Mission Summary

The repository to target is:

- `https://github.com/weave-logic-ai/clawft.git`

The requested workflow is not conceptual. It is an **actual delivery mission** requiring clone, inspection, build, verification, hardening, integration, packaging, and evidence collection.

## Repo-Specific Rules

| Rule | Requirement |
| --- | --- |
| Identity drift | Verify canonical repo identity separately from product, crate, binary, UI, and deployment names |
| Asset baseline | Treat the stated baseline of **60 assets** as a claim to verify, not a guaranteed truth |
| Evidence | Capture commands, outputs, logs, exit codes, artifact paths, and checksums |
| Packaging honesty | Only claim support for platforms with validated artifacts |
| Failure visibility | Surface every failure with root cause, fix attempt, and final status |

## Required Team Structure

Use the default seven-team structure from `team-topology.md` with these exact role labels:

1. Orchestrator Agent
2. Repo Analysis Agent
3. Build Agent
4. Integration Agent
5. QA/E2E Agent
6. Packaging/Release Agent
7. Audit Agent

## Required Build Commands

Run these required commands in the listed order unless the repo itself proves that one is invalid or unavailable. If a command is blocked, report the blocker and evidence rather than silently skipping it.

```bash
cargo build --release --features "native,exochain,cluster,mesh,ecc,wasm-sandbox,containers"
cargo build --release --target wasm32-unknown-unknown -p clawft-wasm
scripts/build.sh native
scripts/build.sh native-debug
scripts/build.sh test
scripts/build.sh gate
scripts/build.sh all
```

Treat any additional first-party checks as **supplemental verification**.

## Phase Expectations

Preserve the following high-level phase story:

| Phase | Focus |
| --- | --- |
| 0 | Pre-flight, clone, identity report, asset inventory |
| 1 | Environment discovery and prerequisite verification |
| 2 | Codebase analysis and feature traceability |
| 3 | Canonical builds |
| 4 | Failure triage and minimal repair |
| 5 | Connectivity, integration, and runtime validation |
| 6 | E2E verification |
| 7 | Cross-platform packaging strategy |
| 8 | Release candidate gate |
| 9 | Final deliverables |

## Must-Appear Deliverables

Ensure the final output includes at least these sections.

1. Executive summary
2. Exact repository identity and commit SHA
3. Environment report
4. Asset inventory and 60-baseline reconciliation
5. Build report
6. Failure and fix ledger
7. E2E verification report
8. Platform truth matrix
9. Packaging report
10. Artifact manifest with paths, sizes, and checksums
11. Final go/no-go recommendation

## Practical Use

When executing this repo-specific mission:

1. Save the user prompt as `operator-brief.md`.
2. Run `inspect_repo_surface.py` immediately after clone.
3. Generate the coordination files and wrapper workspace with `bootstrap_wrapper_workspace.py`.
4. Build the repository before relying on any wrapper shell.
5. Use the platform truth matrix language exactly: `native`, `wrapped`, `browser-delivered`, or `unsupported`.

If the repository cannot prove a desktop, browser, or mobile path, state that directly instead of inferring support.
