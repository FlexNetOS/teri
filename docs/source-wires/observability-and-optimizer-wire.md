# Observability And Optimizer Wire

## FACT

- `splitrail` is a token-usage and cost tracker with an MCP server, CLI surfaces, optional cloud upload, and analyzers for several coding agents including Codex CLI.
- `openevolve` at commit `80945ed82886d5c4ff2f3d22436765d50cb61266` presents itself as an evolutionary coding agent with reproducibility claims and evaluator-driven optimization.

## INFERENCE

- `splitrail` is useful to Teri as an external operator aid, especially for future local usage observability or agent-cost research.
- `openevolve` is only safe as a research reference in this issue because its core value proposition is autonomous optimization, which is far outside Teri's default safety posture.

## POLICY

- `splitrail` stays observability-only here. No upload, no API token setup, no cloud dependency.
- `openevolve` stays research-only here. No autonomous mutation, no hidden optimizer path, no execution during normal validation.

## QUESTION

- If Teri later records usage telemetry, which fields can be safely persisted without exposing prompt contents or secrets?
- Which Teri evaluator seams would be eligible for a future bounded optimizer experiment, if any?
