# Team Topology

Use this reference when the task requires disciplined orchestration rather than ad hoc execution.

## Default Team Map

| Team | Primary responsibility | Typical outputs |
| --- | --- | --- |
| **Orchestrator Agent** | Own sequencing, checkpoints, rollback decisions, and final go/no-go recommendation | Phase plan, checkpoint log, final recommendation |
| **Repo Analysis Agent** | Inspect workspace layout, dependencies, naming drift, crates, packages, scripts, and asset inventory | Repository identity matrix, workspace map, asset inventory |
| **Build Agent** | Prepare toolchains, run builds, capture logs, isolate compile failures | Environment table, build logs, rebuilt artifacts |
| **Integration Agent** | Wire native runtime, browser build, WASM, services, UI, and wrapper shell | Runtime topology, configuration notes, bridge checks |
| **QA/E2E Agent** | Validate integrated behavior, regressions, negative paths, and recovery | E2E matrix, bug notes, pass/fail evidence |
| **Packaging/Release Agent** | Produce packageable outputs and platform-specific instructions | Package bundles, install steps, truth matrix |
| **Audit Agent** | Maintain evidence, checksums, manifests, and change ledger | Artifact manifest, evidence index, fix ledger |

## Subagent Model

When subagents are available, instantiate one subagent per team. Keep them narrow. Each subagent should own a single responsibility and report back in a fixed schema.

When subagents are **not** available, emulate them with serialized sections named after the same teams. Preserve role ownership anyway. The operating model matters more than the implementation detail.

## Required Handoffs

Every major handoff should include the following fields.

| Field | Meaning |
| --- | --- |
| `phase` | Current execution phase |
| `owner` | Team or subagent producing the handoff |
| `inputs` | Verified upstream facts and artifacts |
| `actions_taken` | Concrete commands or checks already executed |
| `artifacts` | Paths to logs, reports, binaries, bundles, or screenshots |
| `risks` | Known blockers, ambiguities, or likely failure points |
| `next_owner` | Team expected to act next |
| `gate` | PASS, FAIL, or PARTIAL |

## Hook Points

Register the following hooks before execution starts.

| Hook name | Fire when | Minimum payload |
| --- | --- | --- |
| `preflight_start` | Before cloning or modifying the repo | Repo URL, operator brief path, requested surfaces |
| `post_clone_inventory` | After clone and workspace scan | Resolved remote, branch, commit, workspace map |
| `pre_build_gate` | Before the first real build | Toolchain status, planned commands, blockers |
| `post_build_gate` | After required build steps | Exit codes, logs, built artifacts, failed targets |
| `pre_integration_gate` | Before wiring the wrapper | Verified runtime assets or endpoints |
| `post_e2e_gate` | After E2E verification | Test matrix, failures, evidence links |
| `pre_package_gate` | Before packaging | Truth matrix draft, package candidates |
| `release_candidate_gate` | Before final recommendation | Full evidence index, manifest, known issues |
| `failure_triage` | Immediately after a failure | Root cause hypothesis, minimal fix plan, rollback note |

## Coordination Pattern

Use this order unless the repo proves a stronger one:

1. Orchestrator defines the phase and current gate.
2. Repo Analysis produces the identity and workspace map.
3. Build verifies the environment and compiles the repo.
4. Integration wires the wrapper only after real artifacts exist.
5. QA/E2E validates integrated behavior.
6. Packaging/Release prepares only proven surfaces.
7. Audit consolidates evidence after every phase.
8. Orchestrator issues the next go/no-go decision.

## Suggested Team Files

If the working directory is empty, create these coordination files:

| File | Purpose |
| --- | --- |
| `coordination/teams.yaml` | Team names, responsibilities, and owners |
| `coordination/subagents.yaml` | One record per subagent or emulated role |
| `coordination/hooks.yaml` | Hook registry and payload rules |
| `reports/checkpoints.md` | Phase-by-phase handoff ledger |

## Minimal YAML Shapes

Use these defaults if you need machine-readable coordination files.

```yaml
teams:
  - name: orchestrator
    responsibility: sequencing and gates
  - name: repo-analysis
    responsibility: identity, inventory, topology
```

```yaml
hooks:
  - name: preflight_start
    when: before clone
  - name: release_candidate_gate
    when: before final recommendation
```

Keep the files small. The purpose is to preserve structure, not to create ceremonial overhead.
