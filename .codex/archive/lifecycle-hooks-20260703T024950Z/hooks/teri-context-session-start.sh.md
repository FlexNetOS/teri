#!/usr/bin/env bash
set -euo pipefail

root="$(rtk git rev-parse --show-toplevel 2>/dev/null || printf '%s\n' "$PWD")"
cd "$root"

# Refresh the always-current research navigation map (gitignored, regenerated each session)
# and capture its one-line summary so the context below can point the agent at the live map.
TERI_RESEARCH_MAP_SUMMARY="$(bash "$root/scripts/gen-research-map.sh" 2>/dev/null || true)"
export TERI_RESEARCH_MAP_SUMMARY

rtk python3 - <<'PY'
import json
import os
from pathlib import Path

root = Path.cwd()

docs = [root / "README.md", root / "RUNBOOK.md", root / "CLAUDE.md"]
stale_patterns = [
    "Pipeline not yet implemented",
    "preflights, then bails",
    "Skeleton with real organs",
    "hardcoded OpenAI",
    "1629 tests",
]

hits = []
for path in docs:
    if not path.exists():
        continue
    text = path.read_text(encoding="utf-8", errors="replace")
    for pattern in stale_patterns:
        if pattern in text:
            hits.append(f"{path.name}: contains stale phrase {pattern!r}")

main = (root / "src/main.rs").read_text(encoding="utf-8", errors="replace")
pipeline = (root / "src/pipeline.rs").read_text(encoding="utf-8", errors="replace")
api = (root / "src/api/mod.rs").read_text(encoding="utf-8", errors="replace")

run_is_wired = "build_provider_llm" in main and "run_pipeline" in main
pipeline_exists = "pub async fn run_pipeline" in pipeline
server_provider_polymorphic = (
    "build_provider_llm" in api
    and "SimulationRunner<ProviderAdapter>" in api
)

lines = [
    "Teri truth guardrails:",
    "- Verify current source before claiming capability: src/main.rs, src/pipeline.rs, src/api/mod.rs, src/sim/mod.rs, src/preflight.rs, README.md, RUNBOOK.md, CLAUDE.md.",
    "- Strong claim: teri can run broad agentic scenario simulations over seed material that can be represented as text, graph facts, personas, actions, memory, and report synthesis.",
    "- Counterargument: teri cannot literally simulate or predict anything; seed quality, ontology extraction, prompts, LLM backend, finite actions, and missing domain solvers bound the result.",
    "- Do not treat simulation output as causal proof. Report confidence is synthesized report metadata, not a calibrated probability unless a calibration/evaluation loop is added.",
    "- Name missing specialized solvers when relevant: physics, epidemiology, markets, weather, supply chains, adversarial security, mechanistic control systems, and other domain-specific engines.",
]

if run_is_wired and pipeline_exists:
    lines.append("- Source truth: teri run is wired to provider-selected run_pipeline in src/main.rs and src/pipeline.rs.")
else:
    lines.append("- Warning: re-check run wiring before documenting teri run; source truth did not match the expected run_pipeline call.")

if server_provider_polymorphic:
    lines.append("- Source truth: serve/API builds a provider-polymorphic ApiState (SimulationRunner<ProviderAdapter>); run uses provider selection via build_provider_llm.")

if hits:
    lines.append("- Stale doc markers found: " + "; ".join(hits))
else:
    lines.append("- No known stale teri-run/provider/test-count phrases found in README.md, RUNBOOK.md, or CLAUDE.md.")

rmap = os.environ.get("TERI_RESEARCH_MAP_SUMMARY", "").strip()
if rmap:
    lines.append("- " + rmap + " — consult it for the current module/route/entry-point map.")

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "SessionStart",
        "additionalContext": "\n".join(lines),
        "watchPaths": [
            "src/main.rs",
            "src/pipeline.rs",
            "src/api/mod.rs",
            "src/sim/mod.rs",
            "src/preflight.rs",
            "README.md",
            "RUNBOOK.md",
            "CLAUDE.md"
        ]
    }
}))
PY
