# Teri — Architecture

> Real, as-built architecture (2026-06-23). Supersedes the prior draft that still referenced
> RocksDB and an unbuilt community layer. Teri is a Rust workspace: the **teri engine** (single
> binary) plus the **vendored pebesen** community platform (7 crates). All graphs below reflect code
> that exists in this repo unless explicitly marked `(planned)`.

## 1. System map (workspace)

```
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ teri workspace (Cargo, edition 2024, default-members = ["."])                           │
│                                                                                        │
│  ┌───────────────────────────── teri engine (bin + lib) ─────────────────────────────┐ │
│  │                                                                                    │ │
│  │  CLI  ──  src/main.rs  (clap; arg-parse BEFORE config; --help/-V keyless)          │ │
│  │   │                                                                                │ │
│  │   ├─ teri run    → preflight → pipeline::run_pipeline  (one-shot CLI)              │ │
│  │   └─ teri serve  → preflight → server::serve (axum REST + SSE)                     │ │
│  │                                                                                    │ │
│  │  config.rs   error.rs(TeriError)   preflight.rs(verify_backend)   logging.rs       │ │
│  │  llm.rs  ── LlmClient trait + OpenAiAdapter / AnthropicAdapter / GeminiAdapter     │ │
│  │  i18n/ (en, zh)     task.rs (TaskManager)     embedding.rs (cosine over redb)      │ │
│  │                                                                                    │ │
│  │  PIPELINE STAGES        SERVICES (src/services/)            STORES                 │ │
│  │  ┌─ seed/    ───────►   graph_builder  ontology            ┌─ memory/ (redb)       │ │
│  │  ├─ graph/ (petgraph)   entity_reader  oasis_profile_export │   • agent LTM         │ │
│  │  ├─ agent/  ───────►    simulation_config                  │   • vector embeddings │ │
│  │  ├─ sim/    ───────►    simulation_runner / _manager / _ipc │  ┌─ graph (in-proc)  │ │
│  │  ├─ report/ ───────►    graph_memory  agent_memory          │   • KnowledgeGraph    │ │
│  │  └─ pipeline.rs        zep_tools (ReportAgent toolset)      └─ fs artifacts         │ │
│  │                        graph_backend                           actions.jsonl,      │ │
│  │  api/  ── graph · simulation · report · streaming(SSE) · server.rs   reports/      │ │
│  │  seed/community/ (planned) ── CommunityAdapter / CommunityFeedback                 │ │
│  └────────────────────────────────────────────────────────────────────────────────┘ │
│                                            ▲  reads signal / writes predictions       │
│                                            │  (planned seam)                          │
│  ┌──────────────────────── pebesen (vendored, pebesen/crates/*) ────────────────────┐ │
│  │  api · core · db(sqlx/postgres) · search(meili) · notifications · intelligence    │ │
│  │  bin     +  frontend/ (svelte)   +  migrations/   +  docker-compose (pg/redis)    │ │
│  │  intelligence crate = prediction RECEIVER for teri (placeholder → to flesh out)   │ │
│  └────────────────────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

## 2. The five-stage pipeline (data flow, typed)

```
 seed file / URL / community signal           natural-language prediction query
        │                                                   │
        ▼                                                   │
 ┌─────────────┐  SeedDocument {raw_text, metadata}         │
 │  seed/      │  pdf · md · txt · json · url               │
 │  ingestor   │  text_processor: chunk + preprocess        │
 └─────┬───────┘                                            │
       ▼                                                    │
 ┌─────────────┐  KnowledgeGraph (petgraph<Entity,Relation>)│
 │  graph/     │  ontology gen (LLM) → 2-pass entity/relation extraction
 │  builder    │  temporal edges (valid_at / invalid_at / expired_at)
 └─────┬───────┘                                            │
       ▼                                                    │
 ┌─────────────┐  AgentPool (N personas)                    │
 │  agent/     │  OASIS persona gen (LLM) + agent config     │
 │  pool       │  short-term (VecDeque) + long-term (redb)   │
 └─────┬───────┘                                            │
       ▼                                                    ▼
 ┌─────────────────────────────────────────────────────────────┐
 │  sim/  SimEngine — two-phase tick loop                        │
 │   phase 1 (concurrent): each active agent observes feed       │
 │                         snapshot → LLM → action               │
 │   phase 2 (sequential): apply actions to SocialWorld          │
 │   • dual platform: Twitter + Reddit (per-agent platform)      │
 │   • time-of-day activation policy (seeded RNG)                │
 │   • God's-eye inject_fn(tick, &mut WorldState)  ◄── injection │
 │   • graph-memory write-back (activities → KnowledgeGraph)     │
 │   • agent-memory write-back (utterances → redb LTM + vectors) │
 │   ── actions.jsonl  (per-platform action stream)              │
 └─────┬────────────────────────────────────────────────────────┘
       ▼
 ┌─────────────┐  PredictionReport {summary, timeline, highlights, confidence}
 │  report/    │  ReportAgent (ReACT loop, per-section)
 │  agent      │  tools: insight_forge · panorama_search · quick_search
 │             │         · interview_agents · recall_agent_discussion
 └─────┬───────┘
       ▼
 ┌─────────────┐   deep interaction: chat with any agent · chat with ReportAgent
 │  api/ (axum)│   REST (59 routes) + SSE (agent-log · console-log · events · ticks)
 └─────────────┘   → CLI consumer  ·  Web UI (Vue, port 3000)
```

## 3. teri ↔ pebesen seam (community signal loop, planned)

```
   ┌────────── pebesen (community platform) ──────────┐
   │  spaces · streams · topics · messages · users    │
   │   ▲ predictions in            signal out │        │
   │   │ (intelligence crate)                 ▼        │
   └───┼──────────────────────────────────────┼───────┘
       │                                       │
  CommunityFeedback                       CommunityAdapter
  push_topic_signals                      fetch_domains/contributors
  push_contributor_trajectories           fetch_signal/topics
  push_health_risks                       to_seed_document
       │                                       │
       ▼                                       ▼
   ┌──────────────────── teri engine ────────────────────┐
   │  CommunityAdapter signal ─► seed ─► …pipeline…       │
   │  …report ─► CommunityFeedback ─► pebesen.intelligence│
   │  (calibration: actioned predictions tune confidence) │
   └──────────────────────────────────────────────────────┘
```

## 4. Concurrency & trust

```
 tokio runtime (async I/O: LLM calls, axum, redb, IPC)
        └── rayon / buffer_unordered (CPU: parallel agent steps per tick)
 fail-closed preflight guard (verify_backend): GET /models + 1-token probe
        → refuses unreachable / no-model / canned-stub backends before ANY work
 secrets: envctl vault injection (child-env only); .env for local dev; never on disk
```

## 5. Crate / dependency choices (as built)

| Concern | Crate | Role |
|---|---|---|
| Async runtime | `tokio` | I/O, server, IPC |
| HTTP server | `axum` + `tower-http` | REST + SSE, CORS, tracing |
| LLM HTTP | `reqwest` | OpenAI/Anthropic/Gemini adapters |
| Knowledge graph | `petgraph` | in-process `KnowledgeGraph<Entity,Relation>` |
| Persistence | `redb` | agent LTM + vector embeddings (no RocksDB) |
| CPU parallelism | `rayon` / async `buffer_unordered` | per-tick agent steps |
| Templating | `minijinja` | persona / prompt templates |
| CLI | `clap` | derive, keyless help |
| Errors | `thiserror` (`TeriError`) / `anyhow` | lib / bin |
| Seed parsing | `pdfium-render`, `scraper`, `encoding_rs` | pdf / html / encodings |

## 6. Source wire registry

Issue 86 adds a checked-in external-source registry in `src/source_wires.rs`. It is intentionally
read-only and exists to answer which upstream repos or PRs matter to Teri, which Teri surfaces
they map to, which evidence was inspected, and which adoption gate blocks deeper integration.

The registry is surfaced through `teri wires list/show/validate` and is cross-linked from
`docs/source-wires/`.

See [`README.md`](./README.md) for usage, [`FEATURE-PARITY.md`](./FEATURE-PARITY.md) for the
MiroFish parity ledger, [`docs/source-wires/README.md`](./docs/source-wires/README.md) for the
issue-86 wire estate, and [`docs/USER-STORY.md`](./docs/USER-STORY.md) /
[`docs/AGENTIC-STORY.md`](./docs/AGENTIC-STORY.md) for the product + autonomy narratives.
