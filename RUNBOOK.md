# Teri Runbook

> Operational runbook for **teri** — the Rust-native Swarm Intelligence Prediction Engine.
> Audience: operators and agents who need to build, configure, run, verify, and troubleshoot
> teri. Everything here is grounded in the current `main` source (not aspirational); the
> "Known gaps" section marks what is wired vs. pending so you never operate on a false premise.

Teri is a ground-up MIT rewrite of [MiroFish](https://github.com/666ghj/MiroFish) (AGPL-3.0) —
parity **by spec, never by code copy**. Where it helps an operator, this runbook maps teri's
surface back to the MiroFish concept it implements.

---

## 1. What teri does (the pipeline)

Teri turns **seed material** (news, policy drafts, financial signals, a URL, or live community
signal) plus a **natural-language prediction query** into a parallel digital world of autonomous
agents, then synthesizes a **prediction report**. The pipeline is five stages — identical in
spirit to MiroFish's, re-expressed as Rust modules:

| # | Stage | teri module | MiroFish equivalent | What it produces |
|---|-------|-------------|---------------------|------------------|
| 1 | **Seed** | `seed/` (+ `seed/community/`) | `FileParser` / `TextProcessor` | normalized `SeedDocument` (pdf/md/txt/json/url) |
| 2 | **Graph** | `graph/` | GraphRAG `GraphBuilderService` / `OntologyGenerator` | `KnowledgeGraph` (petgraph) of entities + relations |
| 3 | **Agents** | `agent/` | `OasisProfileGenerator` / `SimulationConfigGenerator` | `AgentPool` of N personas with long-term memory |
| 4 | **Sim** | `sim/` | OASIS execution engine (CAMEL-AI) | `SimulationState` from a two-phase tokio tick loop |
| 5 | **Report** | `report/` | ReACT `ReportAgent` | `PredictionReport` + interactive world |

Supporting subsystems: `llm.rs` (provider-agnostic `LlmClient` + OpenAI/Anthropic/Gemini
adapters with retry/backoff), `embedding.rs` (`EmbeddingClient`, semantic recall), `memory/`
(redb persistent store), `preflight.rs` (backend honesty guard), `server.rs` + `api/` (REST),
`config.rs` (env-only config), `task.rs` (async task management, mirroring MiroFish's
`TaskManager`).

### Current operational reality (read this before running)

**"Can teri simulate anything?" — yes, within its model boundary.** Teri can run broad
agentic scenario simulations for seed material that can be represented as text, graph facts,
personas, finite actions, memory, and report synthesis. It is not an oracle: seed quality,
ontology extraction, prompts, LLM/backend quality, finite action grammar, and missing
domain-specialized solvers bound what the result means. Simulation output is not causal proof,
and report `confidence` is synthesized report metadata rather than calibrated probability unless
a separate calibration/evaluation loop is added.

- **`teri run`** — **works today.** Preflights the backend (§6), selects the configured provider
  via `build_provider_llm`, and runs the one-shot `seed -> graph -> agents -> sim -> report`
  pipeline through `pipeline::run_pipeline`.
- **`teri serve`** — **works today.** Preflights the backend (§6), then boots the axum REST
  server (`/health` + the three `/api/*` blueprints) for graph, simulation, report, and
  deep-interaction workflows.
- **Both** `run` and `serve` run the **same fail-closed guard** (§6) before doing work — `serve`
  refuses to *boot* against a stub/unreachable backend.

---

## 2. Prerequisites

| Requirement | Notes |
|-------------|-------|
| **Rust toolchain** | stable; build with `cargo build --release` |
| **A real OpenAI-compatible LLM backend** | local (shimmy/ruvllm/Ollama/LM Studio/vLLM) or hosted (OpenAI/Anthropic/Gemini). **Stub/canned backends are refused** — see §6. |
| **An LLM API key** *(for hosted backends)* | injected via envctl or a dev `.env`; **never** exported in a shell profile. Local keyless backends need none. |
| **envctl** *(recommended)* | vault-held secret injection — `envctl run -- teri …` |
| **`OPENAI_API_KEY`/`ZEP_API_KEY`** | only when using those backends; the default **Native** graph backend is keyless. |

---

## 3. Build

```bash
cd ~/Desktop/meta/teri
cargo build --release            # binary at ./target/release/teri
./target/release/teri --help     # MUST exit 0 keyless — regression probe after any CLI change
```

CLI exit codes: usage errors = `2` (clap), runtime errors = `1`, success/help = `0`.
Config loads **only inside commands**, never before argument parsing — so `--help`/`--version`
work with no secrets present.

---

## 4. Configuration (environment variables only)

Teri keeps **no config files and no secrets on disk**. All knobs are env vars, read lazily when
a command needs them. Defaults below are the **actual code defaults** on `main`.

> ⚠️ The README's config table is stale (it lists `https://api.openai.com/v1` / `gpt-4o`). The
> real defaults point at a local shimmy endpoint, per the owner decision of 2026-06-17. Trust
> this table.

### LLM / embeddings
| Variable | Default | Purpose |
|----------|---------|---------|
| `LLM_BASE_URL` | `http://127.0.0.1:11435/v1` | OpenAI-compatible endpoint (shimmy's local bind) |
| `LLM_MODEL` / `LLM_MODEL_NAME` | `OpenThinker3-7B` | completion model (`LLM_MODEL_NAME` wins, then `LLM_MODEL`, then default) |
| `LLM_API_KEY` | *(unset; optional for keyless local)* | API key; arrives via envctl injection in production |
| `EMBED_MODEL` | `all-MiniLM-L6-v2` (384-dim) | embedding model for semantic recall |
| `LLM_MAX_RETRIES` | *(adapter default)* | retry budget for transient LLM errors |
| `LLM_TIMEOUT_SECS` | *(adapter default)* | per-request timeout |

### Graph backend
| Variable | Default | Purpose |
|----------|---------|---------|
| `GRAPH_BACKEND` | `native` | `native` (petgraph + redb vectors, **keyless**) or `zep` (selectable seam; requires `ZEP_API_KEY`) |
| `ZEP_API_KEY` | *(unset)* | **required only** when `GRAPH_BACKEND=zep`; ignored under `native` |
| `GRAPH_DB_PATH` | *(path)* | on-disk graph store directory (auto-created) |

### Simulation
| Variable | Default | Purpose |
|----------|---------|---------|
| `DEFAULT_AGENT_COUNT` | `100` | agents when `--agents` is omitted |
| `SIM_MAX_TICKS` | `50` | max ticks per run |
| `SIM_PARALLELISM` | *(bounded)* | concurrent agent actions per tick (tokio semaphore) |
| `OASIS_DEFAULT_MAX_ROUNDS` | *(set)* | max interaction rounds (OASIS parity) |
| `OASIS_SIMULATION_DATA_DIR` | *(set)* | where per-sim artifacts/profiles land |

### Report agent
| Variable | Default | Purpose |
|----------|---------|---------|
| `REPORT_AGENT_MAX_TOOL_CALLS` | `5` | ReACT tool-call budget |
| `REPORT_AGENT_MAX_REFLECTION_ROUNDS` | `2` | reflection rounds |
| `REPORT_AGENT_TEMPERATURE` | `0.5` | report LLM temperature |

### Server / persistence / misc
| Variable | Default | Purpose |
|----------|---------|---------|
| `FLASK_HOST` | `0.0.0.0` | serve bind host (MiroFish env contract) |
| `FLASK_PORT` | `5001` | serve bind port |
| `FLASK_DEBUG` | *(off)* | debug flag (parity) |
| `BIND_ADDR` | `0.0.0.0:8080` | general engine bind address |
| `MEMORY_DB_PATH` | *(path)* | redb memory store file (parent dir auto-created) |
| `UPLOAD_FOLDER` | *(path)* | uploaded-seed storage (MiroFish U-001) |
| `SECRET_KEY` | *(set)* | session secret (parity) |
| `RUST_LOG` | `teri=debug,tower_http=info` | log filter (see §8) |

---

## 5. Secrets contract (envctl)

Teri **never** documents or expects a raw `export LLM_API_KEY` workflow. The key arrives via
**envctl injection** — vault-held, child-env only:

```bash
# One-time vault registration:
env-ctl secret add teri-llm --provider <p> --value-stdin

# Canonical invocation (key injected into teri's child env only):
env-ctl run --provider <p> -- teri serve
env-ctl run --provider <p> -- teri run --seed ./seed.txt --query "..."
```

`agent-env.toml` declares teri's required secrets. For **local development only**, a gitignored
`.env` is accepted (`cp .env.example .env`). The missing-key error message points back at this
contract — keep it that way. Never write a key to a config file or shell profile.

---

## 6. Backend honesty guard (critical — do not weaken)

A swarm pointed at a stub/canned backend fabricates an entire simulation from cached text, so
teri refuses stub/unreachable backends **fail-closed** before doing work. The guard
(`src/preflight.rs::verify_backend`) is wired into **both** `teri run` (before the pipeline) and
`teri serve` (before binding the socket) — verified against the binary:

1. **Identity** — `GET {LLM_BASE_URL}/models`. An **unreachable** backend or one that **lists no
   models** is refused. (The probe model is the served one when `LLM_MODEL` isn't in the list —
   you'll see a `WARN … is not served by … probing '<served>' instead`.)
2. **Honesty** — a 1-token `POST {LLM_BASE_URL}/chat/completions` probe. If the reply matches a
   `STUB_MARKERS` phrase (`full transformer inference coming soon`, `safetensors`, `stub mode`,
   `not implemented`, `placeholder`, `no backend`, `canned text`, …), it is refused. These only
   match engines that ignore `max_tokens` and return a fixed placeholder — a real 1-token reply
   can't contain a multi-word marker, so false positives are structurally impossible.

Refusal messages (all exit 1, before any work):
| Condition | Message |
|-----------|---------|
| backend down / wrong URL | `inference backend unreachable at …/models: …` |
| reachable but no models | `backend at … lists no models …` |
| canned stub probe | `REFUSING stub inference backend at …: the probe returned canned text (matched "…")` |

**If you hit one:** serve a real GGUF model (e.g. `shimmy serve` with a GGUF registered) or
repoint `LLM_BASE_URL` at a real OpenAI-compatible API. **Never weaken the guard to make a run
proceed** — extend `STUB_MARKERS` when a new stub engine appears.

---

## 7. Running teri

### Serve the REST API (supported runtime today)
```bash
# Default bind 0.0.0.0:5001 (FLASK_HOST/FLASK_PORT):
env-ctl run --provider <p> -- teri serve

# Explicit bind (--addr supersedes the env contract):
env-ctl run --provider <p> -- teri serve --addr 0.0.0.0:8080

# Fully keyless local (Native graph backend + local shimmy/ruvllm):
GRAPH_BACKEND=native LLM_BASE_URL=http://127.0.0.1:11435/v1 \
  teri serve --addr 127.0.0.1:5610
```
Bind precedence: `--addr` → `FLASK_HOST`:`FLASK_PORT` → `0.0.0.0:5001`.

### Run a simulation (one-shot CLI)
```bash
env-ctl run --provider <p> -- teri run \
  --seed ./examples/seed.txt \
  --query "How will this policy affect public sentiment in 30 days?" \
  --agents 100
```
Flags: `-s/--seed <path|url>` (required), `-q/--query <text>` (required),
`-a/--agents <n>` (default `100`), and `--out <path>` when you want the rendered
`PipelineOutcome` JSON persisted. This validates config/backend, then runs the in-process
pipeline and renders the report verdict.

---

## 8. REST API surface

Mounted by `server.rs`. `/health` is uncorsed; everything under `/api/*` has
`CORS: permissive` (parity with MiroFish's `resources={r"/api/*": {"origins": "*"}}`).

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/health` | liveness probe (no CORS) |
| **graph blueprint** | `/api/graph/*` | knowledge-graph ops |
| GET | `/api/graph/project/list` | list projects |
| GET | `/api/graph/project/:project_id` | project detail |
| POST | `/api/graph/project/:project_id/reset` | reset a project |
| POST | `/api/graph/build` | start a graph build |
| GET | `/api/graph/task/:task_id` | build-task status |
| GET | `/api/graph/tasks` | list build tasks |
| GET | `/api/graph/data/:graph_id` | fetch graph data |
| **simulation blueprint** | `/api/simulation/*` | sim lifecycle |
| GET | `/api/simulation/entities/:graph_id` | entities (+ `/by-type/:t`, `/:uuid`) |
| POST | `/api/simulation/create` | create a simulation |
| GET | `/api/simulation/list` · `/history` | list / history |
| POST | `/api/simulation/prepare` · `/prepare/status` | profile prep + status |
| GET | `/api/simulation/:simulation_id` · `/:id/profiles` | detail / generated profiles |
| **report blueprint** | `/api/report/*` | report + analysis |
| GET | `/api/report/list` · `/by-simulation/:id` · `/check/:id` | discovery |
| GET | `/api/report/:report_id` · `/:id/progress` | report + progress |
| GET | `/api/report/:id/agent-log` · `/agent-log/stream` | ReACT log (+ SSE) |
| GET | `/api/report/:id/console-log` · `/console-log/stream` | console log (+ SSE) |
| GET | `/api/report/:id/download` | download report |

Smoke test:
```bash
curl -s http://127.0.0.1:5001/health
curl -s http://127.0.0.1:5001/api/graph/project/list
```

---

## 9. Local inference (shimmy / ruvllm)

Teri's default endpoint is a **local OpenAI-compatible server**. Two known-good options:

- **shimmy** — `shimmy serve` with a **GGUF** model registered (SafeTensors/stub mode is
  refused by the guard).
- **ruvllm** (CUDA, RTX 5090s) — serves chat + `/v1/embeddings`. Point teri at it:
  ```bash
  LLM_API_KEY=ruvllm LLM_BASE_URL=http://127.0.0.1:8090/v1 \
  LLM_MODEL=/tmp/llama3 EMBED_MODEL=/tmp/llama3 \
  GRAPH_BACKEND=native teri serve --addr 127.0.0.1:5610
  ```

Post-reboot CUDA verification of the ruvllm backend is scripted at
`/tmp/reboot-verify-ruvllm.sh` (drives driver → CUDA build → serve → chat → embeddings → teri
keyless against ruvllm).

---

## 10. Observability & verification

### Logging
`RUST_LOG` controls verbosity (default `teri=debug,tower_http=info`):
```bash
RUST_LOG=teri=info teri serve            # quieter
RUST_LOG=teri=debug,tower_http=debug teri serve   # request tracing
```

### Test suite
```bash
cargo test                                   # full suite — keep green (1600+ tests on main)
cargo fmt --all && cargo clippy --all-targets -- -D warnings
./target/debug/teri --help                   # keyless exit-0 probe
```

### Runtime verification of a server (this harness)
A server that binds a port + loads a multi-GB model is killed by the sandbox and by inline
backgrounding. Launch with the Bash tool's `run_in_background: true` **and**
`dangerouslyDisableSandbox: true`; poll `/health` with a plain `curl` in a separate call.

---

## 11. Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `teri: configuration unavailable — key may not be set` | `Config::load` hit `ConfigMissing` | inject via `env-ctl run -- teri …`, or set `LLM_API_KEY` / use a keyless local backend |
| `REFUSING stub inference backend at … (matched "…")` | guard's 1-token probe returned canned stub text (§6) | serve a real GGUF model or repoint `LLM_BASE_URL`; **don't** weaken the guard |
| `inference backend unreachable at …/models: …` | backend down or wrong `LLM_BASE_URL` (refused fail-closed) | start the backend (`shimmy serve` with a GGUF) or fix `LLM_BASE_URL` |
| `backend at … lists no models` | backend reachable but serves nothing | load a real model before running/serving |
| old placeholder error from `teri run` | stale binary/docs or an unexpected regression | rebuild from current source and inspect `src/main.rs::run_cmd` plus `src/pipeline.rs::run_pipeline` |
| `--help` needs a key / fails keyless | regression: config loaded before arg-parse | config must load **inside** the command; fix and re-probe `teri --help` |
| Server won't bind | port in use / blocking socket | choose a free `--addr`; `pkill -f 'teri serve'` |
| `ZEP_API_KEY` error under default backend | only Zep needs it | ensure `GRAPH_BACKEND=native` (the keyless default) |
| empty chat reply (curl 52) from local backend | model panic (e.g. flash-attn on a backend not compiled for it) | use a backend serving standard attention; for ruvllm see the flash-attn fix in PR #30 |

---

## 12. MiroFish parity & capability matrix (verification)

**Verdict:** teri **includes or upgrades essentially every MiroFish component.** Of MiroFish's
five stages and their services, all are implemented and covered by the Rust test suite; several
are re-architected to be *stronger* (native, keyless, single-binary). The remaining items are a
short, named list of wiring gaps (§13), none of which block one-shot CLI or REST simulation.

**Method:** the authoritative `666ghj/MiroFish` GitHub source (the ground truth DeepWiki is
generated from) was inventoried component-by-component, then each was located in current teri
source (`file:line`) and its tests counted. This **supersedes** the 2026-06-12 snapshot in
`~/Desktop/meta/MIROFISH-PORT-PLAN.md` (which predates the graph/ontology/sim-config/report/server
work and lists them as "missing" — they are now real).

**Legend:** ✅ at parity · ⬆️ upgraded beyond MiroFish · ⚠️ gap / partial (see §13).

### Stage 1 — Graph Building
| MiroFish component | teri | Evidence |
|---|---|---|
| Doc ingestion (`FileParser`: pdf/md/txt/markdown) | ⬆️ + **json + url** | `seed/mod.rs` ext-dispatch, pdfium, reqwest+scraper, multi-encoding |
| 500/50 chunking (`TextProcessor`) | ✅ | `text_processor::split_text`, defaults `config.rs` |
| Ontology gen (10 entity + 6–10 edge, Pydantic) | ✅ | `services/ontology.rs` (PascalCase/UPPER_SNAKE, Person/Org fallback, cap 10/10) |
| Entity/relation extraction | ✅ | `graph/mod.rs` 2-pass LLM extract→parse→insert |
| Graph store (`GraphBuilderService` → **Zep Cloud**) | ⬆️ **native petgraph** | `graph/mod.rs` indexes + BFS subgraph + temporal edges + JSON/bincode — no cloud dependency |
| Zep Cloud GraphRAG | ⚠️ **façade** | `graph_backend.rs` `ZepGraphBackend` delegates to native; no live Zep HTTP client (keyless by design) |

### Stage 2 — Environment Setup
| MiroFish component | teri | Evidence |
|---|---|---|
| Persona generation | ✅ | `agent/mod.rs` minijinja `persona_gen.jinja` |
| Persona attrs (age/gender/mbti/country/profession/interests) | ✅ | `SocialProfile` `agent/mod.rs` (all six) |
| influence / reaction-speed | ⚠️ on **config** not profile | `AgentActivityConfig` (`influence_weight`, `response_delay_min/max`) |
| individual vs institutional accounts | ⚠️ **behavioral** | `simulation_config.rs` `entity_type` branching, not a stored `account_type` flag |
| Sim-config gen (time→event→agents→platform; bias/reaction/influence) | ✅ | `simulation_config.rs` LLM stages + per-agent rule fallback |
| OASIS profile export (twitter CSV / reddit JSON) | ✅ | `oasis_profile_export.rs` (exact 5-col CSV header, reddit JSON) |

### Stage 3 — Simulation
| MiroFish component | teri | Evidence |
|---|---|---|
| OASIS engine (**CAMEL-AI pip, Python subprocess**) | ⬆️ **native in-process** `SimEngine` | `sim/mod.rs`; `simulation_runner.rs` runs `tokio::spawn`, no subprocess, no Python |
| Two-phase tick loop + bounded LLM concurrency | ✅ | `sim/mod.rs` (concurrent prepare → serial apply, `parallelism`=8) |
| Action set (CREATE_POST/comment/quote/like/dislike/follow/mute/search/DO_NOTHING) | ✅ | `SocialAction` + `SocialWorld::apply` (`social_world.rs`); mute/search/do-nothing = NoOp by design |
| Dual Twitter+Reddit platforms (parallel) | ✅ | per-platform loggers + `{platform}_simulation.db`, parallel runs |
| God's-eye variable injection | ⬆️ | `sim/mod.rs` `with_inject_fn` / `inject_variable` (beyond MiroFish's scheduled events) |
| Real-time graph memory write-back per round | ✅ | `graph_memory.rs` `GraphMemoryUpdater` → `extend_from_text`, wired in `simulation_runner.rs` monitor |
| Agent LTM / vector write-back from the sim loop | ⚠️ **unwired** | `memory::write_ltm/write_vec_text/semantic_recall` have no sim-path callers (store is real & tested) |

### Stage 4 — Report
| MiroFish component | teri | Evidence |
|---|---|---|
| ReACT ReportAgent (outline → per-section, ≥3 tool calls) | ✅ | `report/mod.rs` `ReportAgent` (`plan_outline`, `generate_section_react`) |
| Graph tools: InsightForge, panorama, quick-search, interview | ✅ | `zep_tools.rs` (`insight_forge`, `panorama_search`, `quick_search`, `interview_agents`) |
| Report streaming variant + key-event extraction | ✅ | `generate_stream`, `extract_key_events` |
| Persistence + logs (`agent_log.jsonl`, `console_log.txt`) | ✅ | `report/manager.rs` + `logger.rs` + `console_logger.rs` |

### Stage 5 — Deep Interaction
| MiroFish component | teri | Evidence |
|---|---|---|
| Agent interview (live IPC, single/batch/all/history) | ✅ ⬆️ **in-process IPC** | `api/simulation.rs` `/interview[/batch/all/history]`; `simulation_ipc.rs` mpsc/oneshot (vs MiroFish filesystem 2-process dirs) |
| In-character / report chat | ✅ | `api/report.rs` `/chat` → `ReportAgent::chat` |

### Cross-cutting
| MiroFish component | teri | Evidence |
|---|---|---|
| LLM orchestration (OpenAI-compatible) | ✅ + `max_tokens` sent | `llm.rs` OpenAI/Anthropic/Gemini adapters, retry/backoff, `strip_think`/JSON-fence |
| — runtime provider selection | ⚠️ **split by runtime path** | `teri run` uses `build_provider_llm`; `serve`/`ApiState` still use `build_llm` with `OpenAiAdapter` |
| — Anthropic/Gemini streaming | ⚠️ assume OpenAI SSE framing | `llm.rs` `stream()` (non-streaming paths fine) |
| Semantic search (Zep hybrid cross-encoder) | ⬆️ **native** embeddings + cosine | `embedding.rs` `EmbeddingClient` + `memory::query_vec_similarity` real cosine over redb vector store (keyless) |
| HTTP API (3 blueprints, ~60 routes) | ✅ | `server.rs` axum boot + `api/{graph,simulation,report}.rs`; `/graph/data` uses `edges` (not D3 `links`) ⚠️ |
| Live SSE log streaming | ⚠️ **placeholder** | `*-log/stream` are one-shot JSON; `api/streaming.rs` + `report/sink.rs` infra unwired |
| Data structures (project/state/run_state/config json, csv, jsonl, sqlite, report files) | ✅ | `models/project.rs`, `simulation_manager.rs`, `oasis_profile_export.rs`, action loggers |
| `TaskManager` async tasks | ✅ | `task.rs` (singleton, create/update/complete/fail) |
| i18n (7 locales: zh,en,es,fr,pt,ru,de) | ⚠️ **en/zh only** | `i18n/mod.rs` (`include_str!` en + zh) |
| Persistence (Zep Cloud + SQLite) | ⬆️ **redb** (embedded) | `memory/mod.rs` redb; optional `sqlite` feature for OASIS-shape DBs |
| Backend honesty guard | ⬆️ **teri-only** | `preflight.rs` (MiroFish has none) — see §6 |
| Dual-LLM "boost" (`LLM_BOOST_*`, Reddit) | ⚠️ single LLM | minor; not implemented |

### Deliberately **not** ported (replaced by the meta estate)
Flask backend shell, Vue 3 + D3 frontend, Zep Cloud (optional façade only), the CAMEL-AI/OASIS
pip engine (reimplemented natively), and the Python 2-process subprocess model. Per the port
plan, the front door is `prompt_hub` and visualization is `n8n`; `/api/graph/data` is the
D3-shape JSON seam. These are **architecture choices, not gaps**.

### Where teri **upgrades** MiroFish (net)
1. **Single static binary** — native in-process OASIS engine; no Python, no CAMEL-AI, no subprocess.
2. **Keyless knowledge graph** — native petgraph + redb; no Zep Cloud dependency (Zep is an optional façade).
3. **Real embeddings + cosine semantic recall** over an embedded redb vector store (vs cloud hybrid search).
4. **In-process mpsc/oneshot IPC** (vs filesystem 2-process dir IPC).
5. **json + url seed ingestion** (MiroFish: pdf/md/txt only); temporal graph edges; JSON+bincode.
6. **Backend honesty guard** — refuses stub/unreachable backends before any work (MiroFish has none).
7. **Type-safe, broad Rust test coverage**, fail-closed throughout.

---

## 13. Known gaps / not-yet-wired

Operate with these in mind (state of `main`, 2026-06). None block simulating via the one-shot
CLI or REST API (§1, §7, §8); they are the honest backlog from the §12 verification:

- **Server provider breadth** — `teri run` uses provider selection, but `serve`/`ApiState` are still
  concretely backed by `OpenAiAdapter` through `api/mod.rs::build_llm`. Anthropic/Gemini adapters
  compile and are available to the run pipeline; server-wide polymorphism remains a high-value
  follow-up.
- **Anthropic/Gemini streaming** — `stream()` parses OpenAI SSE framing; native event/JSON-array
  framing is a TODO (non-streaming completion works).
- **Live SSE endpoints** — `*-log/stream` return one-shot JSON; `api/streaming.rs` + `report/sink.rs`
  infra is ready but unwired to a `text/event-stream` route.
- **Agent LTM/vector write-back from the sim loop** — the redb store + cosine recall are real and
  tested, but no sim-path code calls `write_ltm`/`write_vec_text` (graph-memory write-back *is* wired).
- **i18n coverage** — `en`/`zh` only vs MiroFish's 7 locales.
- **`/api/graph/data` shape** — emits `edges`, not D3-conventional `links` (1-line affordance for
  a D3 consumer).
- **Persona detail** — influence/reaction live on the sim-config layer, not the profile; individual
  vs institutional is behavioral (entity-type branching), not a stored `account_type` flag.
- **Specialized solvers and calibration** — teri's agentic world model does not replace physics,
  epidemiology, markets, weather, supply-chain, adversarial-security, or other domain engines, and
  report confidence is not calibrated probability.

The phased build order (P1 wire-the-spine → P2 parity-core → P3 serve+estate → P4
scale+provenance) lives at `~/Desktop/meta/MIROFISH-PORT-PLAN.md`. That file's **parity matrix is
stale** (2026-06-12); §12 here is the current verification — extend the plan, don't re-derive it.

---

## 14. References

- **MiroFish** (reference impl, AGPL-3.0): https://github.com/666ghj/MiroFish — the authoritative
  source for the §12 parity verification (`backend/app/{services,api,models}`, README, config).
- **MiroFish DeepWiki**: https://deepwiki.com/666ghj/MiroFish — overview, architecture, GraphRAG,
  OASIS execution, Zep memory, ReportAgent, data structures (a derived view of the repo above).
- **MiroFish demo**: https://mirofish-demo.pages.dev (Vue SPA wizard: graph → env → sim →
  report → deep-interaction).
- **OASIS** (simulation design, CAMEL-AI): https://github.com/camel-ai/oasis — reimplemented
  natively in teri (`sim/`, `social_world.rs`), not depended on.
- **teri docs**: `README.md`, `ARCHITECTURE.md`, `CLAUDE.md`, `docs/adr/`, `agent-env.toml`.
- **Port plan**: `~/Desktop/meta/MIROFISH-PORT-PLAN.md` (build order valid; its parity matrix is
  superseded by §12).

---

*License: MIT. Teri is an independent rewrite of MiroFish — parity by spec, never by code copy.*
