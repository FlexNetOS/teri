---
name: fullstack-repo-wrapper
description: >
  Build an evidence-driven wrapper app around a full-stack GitHub repository, then
  verify and package it across desktop, browser, and truthful mobile delivery
  surfaces. Use when the user wants a Rust, Node.js, WASM, Tauri, web, or
  mixed-stack repo cloned, analyzed, wrapped, tested, and shipped with multi-agent
  roles, hook points, subagent handoffs, and customizable operator prompts.
---

# Fullstack Repo Wrapper

Create a wrapper application and a verification workflow around an existing full-stack repository. Treat the repository as the source of truth. Do not invent capabilities the repo does not actually build, serve, or package.

## Use This Skill When

Use this skill when the user wants a GitHub repository turned into a runnable, testable application surface rather than merely inspected.

| Use case | Typical signals |
| --- | --- |
| Wrap a full-stack repo into desktop/web/mobile-compatible surfaces | The request mentions Tauri, Electron, PWA, Capacitor, desktop app, browser build, mobile wrapper, or cross-platform packaging |
| Orchestrate a serious build-and-verify mission | The request demands phases, evidence, checkpoints, logs, go/no-go gates, or release-candidate criteria |
| Work with mixed stacks | The repo contains Rust, Node.js, WASM, UI code, APIs, scripts, or containerized pieces |
| Apply a long operator brief | The user supplies a repository URL plus a detailed execution prompt that must be preserved and customized |

Do **not** use this skill when the user only wants a small patch, a README summary, or a purely conceptual architecture discussion with no real repository execution.

## Core Workflow

Follow this sequence unless the repository itself proves that a different order is required.

1. **Normalize the objective**.
   Record the repo URL, desired delivery surfaces, packaging truth constraints, and any attached operator brief. If the user provided a long prompt, save it as `operator-brief.md` inside the working directory.

2. **Clone and map repository identity**.
   Resolve the real remote URL, default branch, HEAD commit, workspace layout, naming drift, and build surfaces. Run `scripts/inspect_repo_surface.py` against the local clone before making wrapper decisions.

3. **Establish team topology, subagents, and hooks**.
   Read `references/team-topology.md`. Use the listed teams as the default operating model. If true subagent orchestration is available, instantiate those roles directly. Otherwise, emulate them with explicit handoff sections and serialized checkpoints. Register hook checkpoints before clone, after analysis, before build, after build, before E2E, before packaging, and at release gate.

4. **Choose the wrapper truthfully**.
   Read `references/runtime-and-wrapper-matrix.md`. Pick the wrapper surface that matches actual build artifacts, not wishful platform coverage. Prefer an existing native packaging path already present in the repo. Use the bundled Tauri/Vite wrapper when a desktop shell is justified. Use the bundled PWA shell when a browser-delivered or mobile-browser path is the strongest truthful option.

5. **Scaffold the workspace and wrapper**.
   Run `scripts/bootstrap_wrapper_workspace.py` to create the evidence folders, reports, coordination files, and template wrapper code. Pass the repo URL, product name, app id, mode, and optional custom prompt file.

6. **Build the repository first, then the wrapper**.
   Do not let the wrapper hide repository failures. Build the source repo using its native commands and scripts, collect logs and exit codes, then wire the wrapper to the verified outputs or runtime endpoints.

7. **Verify end-to-end behavior**.
   Read `references/verification-playbook.md`. Validate startup, routing, bridge behavior, wrapper launch, UI loading, backend connectivity, negative paths, and packaging honesty. The integrated product must behave correctly.

8. **Deliver facts, evidence, and platform truth**.
   Report what was proven, what remains blocked, what the wrapper actually delivers, and which artifacts exist with paths and checksums.

## Wrapper Selection Rules

Use this table before scaffolding any shell.

| Repository signals | Preferred wrapper path | Why |
| --- | --- | --- |
| Existing desktop framework already present | Reuse the repo's native desktop path | Lowest risk and highest truthfulness |
| Web UI plus local or remote backend, desktop packaging requested | Tauri/Vite wrapper | Rust desktop shell plus web frontend is a strong default for mixed Rust and Node.js stacks |
| Web UI works and mobile compatibility is requested, but no native mobile app exists | PWA shell first | Truthful mobile-browser delivery beats pretending native support exists |
| WASM-only or browser-first repo | PWA shell or direct deployable web bundle | Avoid unnecessary native shells |
| Repo has no verified UI surface yet | Delay wrapper generation until a UI/runtime path is proven | A wrapper cannot compensate for an unverified product |

## Required Operating Rules

| Rule | Instruction |
| --- | --- |
| Evidence first | Capture commands, logs, exit codes, artifact paths, and checksums for every important step |
| Preserve identity drift | Map umbrella repo name, product name, crate names, package names, binaries, and deployment labels instead of silently normalizing them |
| Truthful platform claims | Mark every platform as native, wrapped, browser-delivered, or unsupported based only on verified artifacts |
| Minimal fixes | If the agent must patch the repo, make the smallest reversible change and keep a fix ledger |
| Repo before wrapper | Do not celebrate wrapper success if the underlying repository build is still broken |

## Bundled Resources

Read only what is relevant to the current job.

| Resource | Read when | Purpose |
| --- | --- | --- |
| `references/team-topology.md` | Setting up the multi-agent workflow | Team roles, hook names, subagent contracts, and handoff schema |
| `references/runtime-and-wrapper-matrix.md` | Deciding how to wrap the repo | Mapping repo signals to Tauri, PWA, and honest mobile paths |
| `references/verification-playbook.md` | Preparing evidence, gates, and final reporting | Phase gates, evidence standards, manifests, and go/no-go guidance |
| `references/clawft-customization.md` | Applying the attached `clawft` mission prompt | Repo-specific build order, feature flags, and reporting requirements |
| `scripts/inspect_repo_surface.py` | Right after cloning | Detect stack signals, package managers, UI surfaces, and recommended wrapper mode |
| `scripts/bootstrap_wrapper_workspace.py` | Once the approach is chosen | Create a reusable workspace, coordination files, and wrapper templates |
| `templates/tauri-vite-wrapper/` | Desktop or hybrid wrapping | Rust plus Node.js desktop shell starter |
| `templates/pwa-shell/` | Browser or mobile-browser delivery | Web/PWA wrapper starter |

## Default Output Contract

Unless the user specifies a different structure, deliver the final report in this order.

1. Repository identity and naming matrix
2. Environment and prerequisite status
3. Workspace and asset inventory
4. Build results with evidence references
5. Failure and fix ledger
6. Runtime topology and connection table
7. End-to-end verification results
8. Platform truth matrix
9. Packaging outputs and install/run instructions
10. Artifact manifest with paths, sizes, and checksums
11. Final recommendation: **READY FOR RELEASE**, **READY WITH EXCEPTIONS**, or **NOT READY**

## Example Invocation

A strong execution pattern looks like this:

1. Clone the repository and save the operator brief.
2. Run `python scripts/inspect_repo_surface.py /path/to/repo --json-out reports/repo-surface.json`.
3. Read the team, wrapper, and verification references.
4. Run `python scripts/bootstrap_wrapper_workspace.py --repo-url ... --repo-path ... --output-dir ... --product-name ... --app-id ... --mode hybrid --custom-prompt-file operator-brief.md`.
5. Build and verify the repo.
6. Point the generated wrapper at the verified assets or runtime endpoint.
7. Package only the surfaces that were actually proven.

## Important Constraint

If the environment does not expose true subagent primitives, still preserve the **multi-agent structure** by writing explicit role ownership, per-phase checkpoints, and handoff notes. The skill requires team discipline even when execution is serialized.
