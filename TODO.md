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
