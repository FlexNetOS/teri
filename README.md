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

---

## Quick Start

### Prerequisites

1. **LLM API Key** — one of:
   ```bash
   # Direct (manual):
   export LLM_API_KEY=sk-...

   # Or via envctl (recommended):
   # Ensure your LLM provider is registered in envctl's vault
   ```

2. **Optional backend config**:
   ```bash
   export LLM_BASE_URL=https://api.openai.com/v1  # default
   export LLM_MODEL=gpt-4o                          # default
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
```

### Start REST API Server

```bash
envctl run -- teri serve --addr 0.0.0.0:8080
```

---

## Configuration

All configuration via environment variables (no config files required):

| Variable | Default | Description |
|----------|---------|-------------|
| `LLM_BASE_URL` | `https://api.openai.com/v1` | LLM API endpoint |
| `LLM_API_KEY` | *(required)* | API key for the LLM backend |
| `LLM_MODEL` | `gpt-4o` | Model name for completions |
| `EMBED_MODEL` | `text-embedding-3-small` | Embedding model |
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
│   ├── lib.rs            # Module declarations + preflight_check_backend
│   ├── config.rs         # Lazy Config loading (FIX-1.2: envctl seam)
│   ├── error.rs          # Error types (includes TeriError::ConfigMissing)
│   ├── agent/            # Agent pool, personas, memory structures
│   ├── api/              # HTTP server scaffold + SSE streaming
│   ├── graph/            # Knowledge graph (petgraph) structure
│   ├── llm.rs            # LlmClient trait + 3 adapter stubs (OpenAI, Anthropic, Gemini)
│   ├── memory/           # Persistent memory (redb) structures
│   ├── report/           # Report generation scaffold
│   ├── seed/             # Seed file parsing structure
│   └── sim/              # Simulation loop scaffold
```

---

## LLM Provider Support

Teri uses an adapter pattern — the core simulation logic never depends on a specific provider:

- **OpenAI** (GPT-4, GPT-4o) - `OpenAiAdapter`
- **Anthropic** (Claude 3.5 Sonnet, Opus, Haiku) - `AnthropicAdapter`
- **Google** (Gemini 1.5 Pro, Flash) - `GeminiAdapter`
- **Local models** (Ollama, LM Studio, vLLM, shimmy) - via `OpenAiAdapter` (OpenAI-compatible endpoint)

---

## Security

### GGUF/Stub Backend Guard (FIX-1.3)

Before any simulation runs, Teri preflight-checks the backend:
- Health probe (`/health` endpoint) if available
- Sentinel completion request as fallback
- Detects stub/canned-text backends and refuses to proceed (prevents meaningless simulations on deterministic cached responses)

### Environment Safety

API keys are **never** written to config files. They flow through environment variables only, with optional auto-injection via envctl's secrets engine.

---

## Status

🚧 **Pre-alpha — core infrastructure built.**
Module interfaces defined, config/CLI/adapter layers wired. Pipeline and persistence implementation pending.

---

## Development

```bash
# Check compilation (requires LLM_API_KEY for Config::load verification)
cargo check

# Run tests
cargo test

# Build release binary
cargo build --release
```

---

## Acknowledgements

MiroFish by [BaiFu / 666ghj](https://github.com/666ghj) is the original reference implementation.
Simulation design draws on [OASIS](https://github.com/camel-ai/oasis) from the CAMEL-AI team.

---

## License

MIT
