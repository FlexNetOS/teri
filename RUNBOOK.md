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

- **`teri serve`** — **works today.** Preflights the backend (§6), then boots the axum REST
  server (`/health` + the three `/api/*` blueprints). This is the supported runtime entrypoint.
- **`teri run`** — **preflights, then bails** with `Pipeline not yet implemented`. It validates
  config, runs the backend honesty guard (§6), creates the persistence dirs, logs the plan, and
  returns an error. The end-to-end `seed → … → report` composition is the P1 keystone still
  landing (see [Known gaps](#11-known-gaps--not-yet-wired)). Use `run` today to validate
  config/backend, not to get a report.
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

### Run a simulation (preflight-only today)
```bash
env-ctl run --provider <p> -- teri run \
  --seed ./examples/seed.txt \
  --query "How will this policy affect public sentiment in 30 days?" \
  --agents 100
```
Flags: `-s/--seed <path|url>` (required), `-q/--query <text>` (required),
`-a/--agents <n>` (default `100`). Today this validates config + backend then returns
`Pipeline not yet implemented` — see [Known gaps](#11-known-gaps--not-yet-wired).

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
| `Pipeline not yet implemented` | expected on `teri run` after the guard passes | use `teri serve`; full pipeline is the P1 keystone (pending) |
| `--help` needs a key / fails keyless | regression: config loaded before arg-parse | config must load **inside** the command; fix and re-probe `teri --help` |
| Server won't bind | port in use / blocking socket | choose a free `--addr`; `pkill -f 'teri serve'` |
| `ZEP_API_KEY` error under default backend | only Zep needs it | ensure `GRAPH_BACKEND=native` (the keyless default) |
| empty chat reply (curl 52) from local backend | model panic (e.g. flash-attn on a backend not compiled for it) | use a backend serving standard attention; for ruvllm see the flash-attn fix in PR #30 |

---

## 12. Known gaps / not-yet-wired

Operate with these in mind (state of `main`, 2026-06):

- **`teri run` end-to-end pipeline** — preflights then bails (`Pipeline not yet implemented`).
  The `seed → graph → agents → sim → report` composition is the **P1 keystone**.
- **`KnowledgeGraph::build` orchestration** — placeholder; the graph stage's keystone.
- **Memory write-back hooks** — redb store implemented; the simulation→memory write-back wiring
  is pending (MiroFish's `ZepGraphMemoryUpdater` "Episodes" path).
- **Zep backend** — selectable seam; today it delegates to the native surface (no live Zep HTTP
  client in the tree). `GRAPH_BACKEND=zep` does not yet talk to Zep Cloud.
- **README config table** — stale defaults; this runbook's §4 is authoritative.

The phased build order (P1 wire-the-spine → P2 parity-core → P3 serve+estate → P4
scale+provenance) lives at `~/Desktop/meta/MIROFISH-PORT-PLAN.md` — extend that plan, don't
re-derive scope.

---

## 13. References

- **MiroFish** (reference impl, AGPL-3.0): https://github.com/666ghj/MiroFish
- **MiroFish DeepWiki**: https://deepwiki.com/666ghj/MiroFish — overview, architecture,
  GraphRAG, OASIS execution, Zep memory, ReportAgent, data structures
- **MiroFish demo**: https://mirofish-demo.pages.dev (Vue SPA wizard: graph → env → sim →
  report → deep-interaction)
- **OASIS** (simulation design, CAMEL-AI): https://github.com/camel-ai/oasis
- **teri docs**: `README.md`, `ARCHITECTURE.md`, `CLAUDE.md`, `agent-env.toml`
- **Port plan**: `~/Desktop/meta/MIROFISH-PORT-PLAN.md`

---

*License: MIT. Teri is an independent rewrite of MiroFish — parity by spec, never by code copy.*
