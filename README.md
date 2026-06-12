# Teri

> **Rust-native Swarm Intelligence Prediction Engine**
> A ground-up rewrite of [MiroFish](https://github.com/666ghj/MiroFish) — designed for performance, type safety, and deployment simplicity.

**Name:** "Teri" (Indonesian: *ikan teri*) is the anchovy — one of the smallest fish in the sea, yet one of the most consequential. Anchovies move in vast, tightly coordinated schools: thousands of individuals following simple local rules, producing emergent behavior no single fish planned or directed. That is exactly what this engine does. Seed the world. Spawn the swarm. Watch emergence happen. It's also a nod to Indonesian waters, where *ikan teri* has fed communities and ecosystems for centuries, punching far above its size.

---

## What is Teri?

Teri turns seed materials (news articles, policy drafts, financial signals, novels, or live community knowledge) into a **high-fidelity parallel digital world** populated by thousands of independent agents. Each agent carries its own persona, long-term memory, and behavioural logic. The swarm self-organises, and you observe — or intervene — from a God's-eye view.

**Input** → seed file or community platform signal + natural-language prediction query
**Output** → structured prediction report + interactive living simulation world

### Key Features

- 🧠 **Multi-Provider LLM Support** — OpenAI, Anthropic, Google Gemini, local models (Ollama, LM Studio, vLLM, shimmy)
- ⚙️ **Concurrent agent simulation** — two-phase ticks with bounded tokio concurrency (`SIM_PARALLELISM`)
- 💾 **Persistent Memory** — Rust-native redb for fast agent long-term memory
- 🛡️ **Backend honesty guard** — preflight refuses stub/canned inference backends before any run
- 🎯 **Zero Vendor Lock-in** — adapter pattern for any LLM provider
- 📦 **Single Binary** — no Docker, no venv, just `cargo build --release`
- 🔌 **Community Platform Adapters** — ingest live community signal from Pebesen, Reddit, Zulip, Discourse; write predictions back via `CommunityFeedback`
- 🌐 **Streaming-ready** — SSE stream types are built (`api/streaming`); the HTTP server itself lands in the serve phase (see Status)

---

## Why Rust?

| Concern | Python (MiroFish) | Teri (Rust) |
| --- | --- | --- |
| Agent concurrency | GIL-limited threads | `tokio` bounded concurrency |
| Memory per agent | ~MB overhead | Controlled, stack-friendly |
| Deployment | Docker + venv | Single static binary |
| Type safety | Runtime errors | Compile-time guarantees |
| Async LLM calls | `asyncio` | `tokio` native |

---

## Quick Start

```bash
# CLI surface works without any secrets:
cargo run --release -- --help

# Secrets arrive via envctl injection (vault-held, child-env only):
#   env-ctl secret add teri-llm --provider openai --value-stdin   # one-time registration
#   env-ctl run --provider openai -- teri run ...                 # canonical invocation
# For local development only, a .env file (gitignored) is accepted:
cp .env.example .env

# Run a simulation (preflights the inference backend, then runs the pipeline*)
cargo run --release -- run \
  --seed ./examples/seed.txt \
  --query "How will this policy affect public sentiment in 30 days?" \
  --agents 200

# Start REST API server (*see Status — server wiring is the serve-phase milestone)
cargo run --release -- serve --addr 0.0.0.0:8080
```

**Never export real API keys in your shell profile.** The missing-key error message tells you the
sanctioned path.

---

## Pipeline

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

---

## Project Structure

```
teri/
├── Cargo.toml
├── .env.example
├── README.md
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
    └── api/             # DTOs + SSE stream types (server wiring = serve phase)
```

---

## Configuration

All configuration via envctl injection, environment variables, or a local `.env` (dev only). See
[`.env.example`](.env.example).

### LLM Provider Support

**Teri is completely LLM-provider agnostic.** Choose any provider via adapter pattern:

- **OpenAI** (GPT-4, GPT-4o) - `OpenAiAdapter`
- **Anthropic** (Claude) - `AnthropicAdapter`
- **Google** (Gemini) - `GeminiAdapter`
- **Local models** (Ollama, LM Studio, vLLM, shimmy) - `OpenAiAdapter` (OpenAI-compatible)
- **Custom providers** - Implement the `LlmClient` trait

No vendor lock-in. Swap providers without changing simulation code.

### Backend honesty guard

`run`/`serve` preflight the configured backend before doing anything: `GET /models` (identity) and
a 1-token completion probe. Backends that list no models or answer with canned stub text (e.g.
shimmy's SafeTensors placeholder "Full transformer inference coming soon!") are **refused** — a
swarm simulated on canned text is fabrication, not prediction. Serve a real GGUF model.

---

## Status

🚧 **Skeleton with real organs — 140+ tests green.**

| Layer | State |
| --- | --- |
| seed ingestion (pdf/md/txt/json/url) | implemented + tested |
| LLM adapters (retry/backoff, multi-provider) | implemented + tested |
| persona generation (minijinja) | implemented + tested |
| sim loop (two-phase ticks, God-events) | implemented + tested |
| report generation (+ streaming variant) | implemented + tested |
| memory store (redb) | implemented; write-back wiring pending |
| graph build orchestration | **placeholder — the P1 keystone** |
| pipeline composition (`run`) / HTTP server (`serve`) | preflight + explicit bail; wiring pending |

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
