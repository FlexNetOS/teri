# Teri

> **Rust-native Swarm Intelligence Prediction Engine**
> A ground-up rewrite of [MiroFish](https://github.com/666ghj/MiroFish) — designed for performance, type safety, and deployment simplicity.

**Name:** "Teri" (Indonesian: *ikan teri*) is the anchovy — one of the smallest fish in the sea, yet one of the most consequential. Anchovies move in vast, tightly coordinated schools: thousands of individuals following simple local rules, producing emergent behavior no single fish planned or directed. That is exactly what this engine does. Seed the world. Spawn the swarm. Watch emergence happen.

---

## What is Teri?

Teri turns seed materials (news articles, policy drafts, financial signals, novels, or live community knowledge) into a **high-fidelity parallel digital world** populated by independent agents. Each agent carries its own persona, long-term memory, and behavioral logic. The swarm self-organizes, and you observe — or intervene — from a God's-eye view.

**Input** → seed file or community platform signal + natural-language prediction query
**Output** → structured prediction report + interactive living simulation world

### Key Features

- 🧠 **Multi-Provider LLM Support** - OpenAI, Anthropic, Google Gemini, local models (Ollama, LM Studio, shimmy) via adapter pattern
- 🦀 **Rust-native** - Fast, type-safe, zero-GIL overhead for future parallelism
- 💾 **Persistent Memory** - redb-backed agent long-term memory
- 🔌 **Zero Vendor Lock-in** - Adapter pattern for any OpenAI-compatible LLM provider
- 📦 **Single Binary** - `cargo build --release`, no Docker required
- ⚡ **Envctl Auto-Injection** - Secrets auto-injected via `envctl run -- teri ...`
- 🧠 **Multi-Provider LLM Support** — OpenAI, Anthropic, Google Gemini, local models (Ollama, LM Studio, vLLM, shimmy)
- ⚙️ **Concurrent agent simulation** — two-phase ticks with bounded tokio concurrency (`SIM_PARALLELISM`)
- 💾 **Persistent Memory** — Rust-native redb for fast agent long-term memory
- 🛡️ **Backend honesty guard** — preflight refuses stub/canned inference backends before any run
- 🎯 **Zero Vendor Lock-in** — adapter pattern for any LLM provider
- 📦 **Single Binary** — no Docker, no venv, just `cargo build --release`
- 🔌 **Community Platform Adapters** — ingest live community signal from Pebesen, Reddit, Zulip, Discourse; write predictions back via `CommunityFeedback`
- 🌐 **Streaming-ready** — SSE stream types are built (`api/streaming`); the HTTP server itself lands in the serve phase (see Status)

---

## Quick Start

### Prerequisites
| Concern | Python (MiroFish) | Teri (Rust) |
| --- | --- | --- |
| Agent concurrency | GIL-limited threads | `tokio` bounded concurrency |
| Memory per agent | ~MB overhead | Controlled, stack-friendly |
| Deployment | Docker + venv | Single static binary |
| Type safety | Runtime errors | Compile-time guarantees |
| Async LLM calls | `asyncio` | `tokio` native |

1. **LLM API Key** — required for hosted providers, optional for keyless local backends:
   ```bash
   # Direct (manual):
   export LLM_API_KEY=sk-...

   # Or via envctl (recommended):
   # Ensure your LLM provider is registered in envctl's vault
   ```

2. **Optional backend config**:
   ```bash
   export LLM_BASE_URL=http://127.0.0.1:11435/v1  # default shimmy/OpenAI-compatible endpoint
   export LLM_MODEL=OpenThinker3-7B                # default model name
   ```

### Run a Simulation

```bash
# With envctl (auto-inject secrets):
envctl run -- teri run \
  --seed ./examples/seed.txt \
  --query "How will this policy affect public sentiment in 30 days?"

# Or directly:
LLM_API_KEY=sk-... cargo run --release -- run \
  --seed ./examples/seed.txt \
  --query "How will this policy affect public sentiment in 30 days?"

# CLI surface works without any secrets:
cargo run --release -- --help

# Secrets arrive via envctl injection (vault-held, child-env only):
#   env-ctl secret add teri-llm --provider openai --value-stdin   # one-time registration
#   env-ctl run --provider openai -- teri run ...                 # canonical invocation
# For local development only, a .env file (gitignored) is accepted:
cp .env.example .env

# Run a simulation (preflights the inference backend, then runs the pipeline)
cargo run --release -- run \
  --seed ./examples/seed.txt \
  --query "How will this policy affect public sentiment in 30 days?"
```

### Start REST API Server

```bash
envctl run -- teri serve --addr 0.0.0.0:8080
# Start REST API server
cargo run --release -- serve --addr 0.0.0.0:8080
```

**Never export real API keys in your shell profile.** The missing-key error message tells you the
sanctioned path.

---

## Configuration

All configuration via environment variables (no config files required):

| Variable | Default | Description |
|----------|---------|-------------|
| `LLM_BASE_URL` | `http://127.0.0.1:11435/v1` | OpenAI-compatible LLM API endpoint |
| `LLM_API_KEY` | *(optional for keyless local)* | API key for hosted LLM backends |
| `LLM_MODEL_NAME` / `LLM_MODEL` | `OpenThinker3-7B` | Model name for completions (`LLM_MODEL_NAME` wins) |
| `EMBED_MODEL` | `all-MiniLM-L6-v2` | Embedding model |
| `DEFAULT_AGENT_COUNT` | `100` | Default number of agents per simulation |
| `SIM_MAX_TICKS` | `50` | Maximum ticks per simulation run |
| `RUST_LOG` | `teri=debug,tower_http=info` | Logging level |

---

## Local Inference with Shimmy

For local/offline inference, Teri supports shimmy (the FlexNetOS local LLM server) as an OpenAI-compatible endpoint:

```bash
export LLM_BASE_URL=http://localhost:8080/v1

# Teri will preflight the backend and refuse to run if shimmy reports stub mode.
envctl run -- teri run --seed ./examples/seed.txt --query "Test query"
```
Seed File or Community Platform
   │
   ▼
[seed]  ── parse & normalise ──► SeedDocument
   │
   ▼
[graph] ── entity/relation extraction ──► KnowledgeGraph (petgraph)
   │                                         │
   ▼                                         ▼
[agent] ── persona gen + memory init ──► AgentPool (N agents)
   │
   ▼
[sim]   ── tick loop (tokio, bounded) ──► SimulationState
   │           ▲
   │           └── God's-eye variable injection
   ▼
[report] ── ReportAgent synthesis ──► PredictionReport + InteractiveWorld
   │
   ├──► [api]   ── REST / SSE ──► Client (CLI or frontend)
   │
   └──► [feedback] ── CommunityFeedback ──► Source platform (optional write-back)
```

See `agent-env.toml` for teri's required secrets configuration.

---

## Project Structure

```
teri/
├── Cargo.toml
├── README.md
├── agent-env.toml        # envctl auto-injection manifest
├── src/
│   ├── main.rs           # CLI entry point (clap, arg-parse before config)
│   ├── lib.rs            # Module declarations + public re-exports
│   ├── preflight.rs      # Backend honesty guard (verify_backend; wired into run + serve)
│   ├── config.rs         # Lazy Config loading (FIX-1.2: envctl seam)
│   ├── error.rs          # Error types (includes TeriError::ConfigMissing)
│   ├── agent/            # Agent pool, personas, memory structures
│   ├── api/              # HTTP server routes + SSE stream infrastructure
│   ├── graph/            # Knowledge graph (petgraph) structure
│   ├── llm.rs            # LlmClient trait + OpenAI, Anthropic, Gemini adapters
│   ├── memory/           # Persistent memory (redb) structures
│   ├── report/           # Report generation scaffold
│   ├── seed/             # Seed file parsing structure
│   └── sim/              # Simulation loop
```

---

## LLM Provider Support
├── ARCHITECTURE.md
└── src/
    ├── main.rs          # CLI entry point (clap; parses before config — help is keyless)
    ├── lib.rs           # Module declarations
    ├── preflight.rs     # Inference-backend identity + stub refusal
    ├── seed/            # Seed ingestion & normalisation
    │   └── community/   # CommunityAdapter + CommunityFeedback traits + platform adapters
    ├── graph/           # Knowledge graph (petgraph + LLM extraction)
    ├── agent/           # Agent pool, personas, memory
    ├── sim/             # Simulation engine (two-phase tick loop, tokio)
    ├── report/          # Report generation & world interaction
    ├── memory/          # Persistent memory (redb)
    └── api/             # DTOs, REST routes, and SSE stream types
```

---

## Configuration

All configuration via envctl injection, environment variables, or a local `.env` (dev only). See
[`.env.example`](.env.example).

Teri uses an adapter pattern — the core simulation logic never depends on a specific provider:

- **OpenAI** (GPT-4, GPT-4o) - `OpenAiAdapter`
- **Anthropic** (Claude 3.5 Sonnet, Opus, Haiku) - `AnthropicAdapter`
- **Google** (Gemini 1.5 Pro, Flash) - `GeminiAdapter`
- **Local models** (Ollama, LM Studio, vLLM, shimmy) - via `OpenAiAdapter` (OpenAI-compatible endpoint)

---

## Security

### GGUF/Stub Backend Guard (FIX-1.3)

Before any simulation runs, Teri preflight-checks the backend:
- Model identity probe (`/models`) before work starts
- 1-token chat completion honesty probe
- Detects stub/canned-text backends and refuses to proceed (prevents meaningless simulations on deterministic cached responses)
- **Anthropic** (Claude) - `AnthropicAdapter`
- **Google** (Gemini) - `GeminiAdapter`
- **Local models** (Ollama, LM Studio, vLLM, shimmy) - `OpenAiAdapter` (OpenAI-compatible)
- **Custom providers** - Implement the `LlmClient` trait

### Environment Safety

API keys are **never** written to config files. They flow through environment variables only, with optional auto-injection via envctl's secrets engine.

### Backend honesty guard

`run`/`serve` preflight the configured backend before doing anything: `GET /models` (identity) and
a 1-token completion probe. Backends that list no models or answer with canned stub text (e.g.
shimmy's SafeTensors placeholder "Full transformer inference coming soon!") are **refused** — a
swarm simulated on canned text is fabrication, not prediction. Serve a real GGUF model.

---

## Status

**Pre-alpha, source-wired.**
The one-shot `teri run` path preflights the backend and composes seed -> graph -> agents -> sim -> report through `pipeline::run_pipeline`. The `teri serve` path preflights before binding and exposes the REST graph, simulation, and report surfaces.

Teri is a broad agentic scenario engine, not an oracle. It can simulate and forecast scenarios that fit its seed-data, ontology, persona, action, memory, and report model. It does not prove causal truth, and report `confidence` is synthesized report metadata rather than calibrated probability unless a separate evaluation/calibration loop is added.

---

## Development

```bash
# Check compilation
cargo check

# Run tests
cargo test

# Build release binary
cargo build --release
```
**Large Rust test suite; run `cargo test` for the current count.**

| Layer | State |
| --- | --- |
| seed ingestion (pdf/md/txt/json/url) | implemented + tested |
| LLM adapters (retry/backoff, multi-provider) | implemented + tested |
| persona generation (minijinja) | implemented + tested |
| sim loop (two-phase ticks, God-events) | implemented + tested |
| report generation (+ streaming variant) | implemented + tested |
| memory store (redb) | implemented; graph-memory write-back wired, agent LTM/vector sim write-back pending |
| graph build orchestration | implemented + tested |
| pipeline composition (`run`) / HTTP server (`serve`) | implemented; server provider path is narrower than the run path |

The phased plan (P1 wire-the-spine → P2 parity-core → P3 serve → P4 scale) lives in the meta
workspace: `MIROFISH-PORT-PLAN.md`.

---

## Acknowledgements

MiroFish by [BaiFu / 666ghj](https://github.com/666ghj) is the original reference implementation
(AGPL-3.0; teri is an MIT-licensed independent rewrite — parity by spec, never by code copy).
Simulation design draws on [OASIS](https://github.com/camel-ai/oasis) from the CAMEL-AI team.

---

## License

MIT
