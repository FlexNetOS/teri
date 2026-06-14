# Teri Development TODO

> **Status:** Pre-alpha — core infrastructure built, pipeline implementation pending
> **Last Updated:** 2026-06-12 (FIX-1 hygiene refresh)

This checklist tracks end-to-end development of Teri. Tasks are updated against actual code state.

---

## Phase 0: Project Foundation ✅ COMPLETE

All structural items from Phase 0 were verified as implemented at the time of last audit (2026-03-09).

### Directory Structure
- [x] Create `src/` directory with module subdirectories
- [x] Implement config loader (environment-driven)
- [x] Implement error types and Result type aliases

### What exists (verified 2026-06-12):
| Module | Status | Notes |
|--------|--------|-------|
| `agent/` | scaffolding | persona/memory structures exist, behavior logic pending |
| `api/` | partial | SSE streaming stub exists (`src/api/streaming.rs`) |
| `graph/` | scaffolding | knowledge graph structure exists |
| `memory/` | scaffolding | redb-backed persistence layer scaffolded |
| `config` | **complete** | Lazy config loading with envctl injection support (FIX-1) |
| `llm` | **partial** | LlmClient struct + 3 adapters defined (OpenAI, Anthropic, Gemini), impl pending |
| `report/` | scaffolding | report output format scaffolded |
| `seed/` | scaffolding | seed file parsing scaffolded |
| `sim/` | scaffolding | simulation loop structure exists |

---

## Phase 1: Core Runtime ⚠️ IN PROGRESS

- [x] Arg-parse before config (FIX-1.1) — `--help` works keyless
- [x] Envctl auto-injection seam (FIX-1.2) — `agent-env.toml` + lazy Config::load()
- [x] GGUF/stub backend guard (FIX-1.3) — preflight check before simulation runs
- [ ] Implement LLM adapter backends (OpenAI/Anthropic/Gemini completion logic)
- [ ] Implement agent behavior logic (persona, memory update, decision-making)
- [ ] Implement simulation loop with parallel agent execution

---

## Phase 2: Pipeline & Persistence ⬚ NOT STARTED

- [ ] Implement seed processing pipeline
- [ ] Complete graph-based knowledge storage
- [ ] Implement report generation
- [ ] Wire SSE streaming for live simulation updates

---

## Phase 3: Production Ready ⬚ NOT STARTED

- [ ] Community platform adapters (Pebesen, Reddit, Zulip, Discourse)
- [ ] REST API server implementation
- [ ] Documentation and examples
- [ ] CI/CD pipeline setup
- [ ] Release tooling (Homebrew formula, etc.)

---

## Known Issues & Technical Debt

### D10 — Stale TODO items from prior audits
The original TODO listed ~50 checked tasks that were later found inaccurate. This audit corrected all Phase 0 items to reflect actual code state. Future updates must verify claims against live code before checking boxes.

### D12 — Upstream README overclaims
README.md originally claimed features not yet implemented (rayon parallelism, Docker deployment, community platform adapters). These have been reviewed against current code state.

---

## Backlog (prioritized)

1. **P0**: Implement actual LLM completion backends (OpenAI, Anthropic, Gemini adapters)
2. **P1**: Agent behavior logic — the core of "swarm intelligence"
3. **P1**: Simulation loop with parallel execution
4. **P2**: Pipeline and persistence layers
5. **P3**: Community platform adapters
6. **P3**: Production tooling (CI, docs, examples)
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
