# TODO — current state and the line forward

> **Honesty note (2026-06-12):** the previous 700+-line checklist had drifted stale in both
> directions (dozens of unchecked items were long done; several checked items overclaimed).
> It was replaced with this current-state snapshot. The authoritative phased plan lives in the
> meta workspace: **`MIROFISH-PORT-PLAN.md`** (parity matrix vs upstream MiroFish, vehicle
> verdict, P1–P4). Do not grow this file back into a parallel plan — extend the port plan.

## Done (verified by the 152-test suite + CLI drives)

- [x] Seed ingestion: pdf (pdfium) / md / txt / json / url (reqwest+scraper), normalisation.
- [x] LLM adapter layer: OpenAI-compatible (Ollama/LM Studio/vLLM/shimmy), Anthropic, Gemini;
      retry/backoff; provider-agnostic `LlmClient` trait.
- [x] Persona generation (minijinja templates) + agent pool.
- [x] Simulation loop: two-phase ticks, bounded tokio concurrency, God's-eye event injection.
- [x] Report generation (sync + streaming variants, minijinja, key-event extraction).
- [x] Persistent memory store (redb) — storage layer.
- [x] CLI hygiene: clap parses before config (keyless `--help`/`--version` exit 0; usage
      errors exit 2); missing-key error points at the envctl injection contract.
- [x] Inference-backend preflight (`src/preflight.rs`): `/models` identity + 1-token probe;
      refuses unreachable backends, empty model lists, and canned stub engines
      (shimmy SafeTensors placeholder detected by marker).

## P1 — wire the spine (the e2e milestone; see MIROFISH-PORT-PLAN.md)

- [ ] `KnowledgeGraph::build` orchestration: chunk → LLM extraction (prompt builders + parsers
      exist and are tested) → parse → insert. The single placeholder standing between the
      pieces and a real run.
- [ ] Compose `run`: seed → graph → personas → sim (wire MemoryStore + per-tick graph
      write-back) → report; emit report + verdict artifact.
- [ ] Send `max_tokens` explicitly on completions (shimmy defaults to 256 when omitted).
- [ ] Provider-selection logic from config (base_url/provider → adapter choice).

## P2 — parity core

- [ ] Ontology-generation pass (LLM-designed schema, upstream stage-1 parity).
- [ ] OASIS-grade persona/config generators (mbti/profession/influence/reaction-speed;
      individual vs institutional accounts); Twitter/Reddit platform presets incl. DO_NOTHING bias.
- [ ] ReACT report loop with graph tools (InsightForge analog over petgraph+redb).
- [ ] JSON-mode hardening (`response_format` is ignored by some local backends).
- [ ] Anthropic adapter: configurable base_url; fix stream parsers that assume OpenAI SSE framing.

## P3 — serve + estate integration

- [ ] axum server on the existing DTOs (`api/streaming` TickStream is ready); interview + chat
      endpoints; `/graph/data` D3-shape JSON.
- [ ] prompt_hub front-door dispatch (prediction request → handoff.task.v1 → teri run →
      witnessed delivery).
- [ ] `--out verdict.json` artifact parity with the CLI fork.

## P4 — scale + provenance

- [ ] Embeddings + hybrid memory search (shimmy lacks /v1/embeddings — bench in-process candle
      vs a shimmy fork route).
- [ ] Throughput: shimmy pool / batching for hundreds of persona agents.
- [ ] RVF/cognitum witness anchoring on sim runs (predictions with provenance).
