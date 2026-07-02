# Inference Backend Wire

## FACT

- Teri's current docs and config already center an OpenAI-compatible backend surface and specifically document local `inferrs` verification scripts.
- `ericcurtin/inferrs` advertises OpenAI-compatible (`/v1/completions`, `/v1/chat/completions`, `/v1/models`), Anthropic-compatible (`/v1/messages`), and Ollama-compatible APIs.
- `cluaiz` exposes `cluaiz serve` as an OpenAI-compatible REST daemon and documents native orchestration, plugin, skill, and MCP flows, while also labeling itself alpha/research-stage.
- Teri's backend honesty guard requires a real `/models` identity check and a live token probe before `run` or `serve` proceeds.

## INFERENCE

- `inferrs` is the more directly aligned backend wire for Teri's current runtime because Teri already documents it and uses OpenAI-compatible defaults.
- `cluaiz` is more useful as a local-orchestration comparison source than as a default backend candidate right now.

## POLICY

- Do not downgrade Teri's current backend honesty guard for route flexibility.
- Do not treat alpha-stage `cluaiz` claims as production proof.
- Keep issue-86 validation keyless and offline where practical; no live model dependency is introduced by the registry.

## QUESTION

- Should Teri eventually expose a checked-in compatibility matrix by provider family, or only by verified local routes?
- Which `cluaiz` route concepts are actually relevant to Teri: OpenAI-compatible serving, MCP/plugin orchestration, or both?
