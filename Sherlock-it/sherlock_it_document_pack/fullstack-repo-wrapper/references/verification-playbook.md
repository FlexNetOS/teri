# Verification Playbook

Use this reference when the user expects a real delivery workflow with evidence rather than a conceptual plan.

## Phase Pattern

Run the task in explicit phases. After every phase, emit the same checkpoint structure.

| Checkpoint field | Requirement |
| --- | --- |
| Phase status | PASS, FAIL, or PARTIAL |
| What was executed | Exact commands, scripts, or checks |
| Artifacts produced | Files, bundles, screenshots, logs, reports |
| Remaining blockers | Concrete unresolved issues |
| Evidence references | Paths to logs, manifests, screenshots, or checksums |

## Recommended Phase Skeleton

| Phase | Goal |
| --- | --- |
| 0 | Pre-flight, clone, identity mapping, asset inventory |
| 1 | Environment discovery and prerequisite verification |
| 2 | Codebase analysis and feature traceability |
| 3 | Canonical builds and first-party scripts |
| 4 | Failure triage and minimal repair loop |
| 5 | Connectivity, integration, and runtime validation |
| 6 | End-to-end verification |
| 7 | Cross-platform packaging strategy |
| 8 | Release candidate gate |
| 9 | Final deliverables |

The user may rename phases, but keep the gate discipline.

## Evidence Standard

Create a predictable evidence tree inside the working directory.

| Path | Purpose |
| --- | --- |
| `evidence/logs/` | Raw command logs, stdout, stderr, exit codes |
| `evidence/screens/` | Screenshots or captured UI states when useful |
| `reports/` | Human-readable summaries and tables |
| `artifacts/` | Produced binaries, bundles, archives, and checksums |
| `reports/fix-ledger.md` | Minimal agent-introduced changes |
| `reports/artifact-manifest.md` | Final manifest with size and checksum |

For every important command, save:

1. the command string,
2. the exit code,
3. the log path,
4. a short interpretation.

## Failure Triage Loop

When a build or runtime step fails, use this loop.

1. Isolate the failing command.
2. Classify the failure: infrastructure, dependency, source, configuration, flaky test, or unsupported platform.
3. Attempt the smallest reversible fix.
4. Re-run the failing step.
5. Re-run the broader gate only after the local fix passes.
6. Record the change in `reports/fix-ledger.md`.

## End-to-End Standard

Passing unit tests is not enough. The integrated product must behave correctly.

For each E2E case, record the following fields.

| Field | Requirement |
| --- | --- |
| Test name | Clear and stable name |
| Prerequisite state | Repo commit, environment, services, wrapper mode |
| Steps | Exact user-level interactions |
| Expected outcome | Verifiable result |
| Actual outcome | What really happened |
| Evidence | Logs, screenshots, URLs, artifacts |
| Pass/fail | Final verdict |

## Platform Truth Matrix

Always publish a platform table like this.

| Platform | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Linux desktop | native / wrapped / browser-delivered / unsupported | Artifact or log reference | Constraints |
| macOS desktop | native / wrapped / browser-delivered / unsupported | Artifact or log reference | Constraints |
| Windows desktop | native / wrapped / browser-delivered / unsupported | Artifact or log reference | Constraints |
| Mobile | native / wrapped / browser-delivered / unsupported | Artifact or log reference | Constraints |

## Release Gate

Only recommend release when the evidence supports it.

| Verdict | Meaning |
| --- | --- |
| **READY FOR RELEASE** | Required builds, verification, and packaging evidence are complete |
| **READY WITH EXCEPTIONS** | Core path is proven, but documented exceptions remain |
| **NOT READY** | Critical build, runtime, or packaging gaps remain |

Separate facts from assumptions. If a capability is inferred but not executed, mark it as an assumption or unsupported path rather than a verified feature.
