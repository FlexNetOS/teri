# Teri — Autonomous Development Guidance

## Architecture Overview

Teri is a Rust-native swarm intelligence prediction engine. The core architecture has three layers:

1. **CLI layer** (main.rs) — clap-based CLI with arg-parse-before-config discipline (--help works keyless)
2. **Config layer** (config.rs) — lazy env-driven configuration with envctl auto-injection support via agent-env.toml
3. **Runtime layer** (lib.rs + modules) — LLM adapter abstraction, simulation engine, agent pool, persistence

### Key Design Decisions
- **LLM provider agnostic**: `LlmClient` trait + concrete adapters (OpenAI, Anthropic, Gemini)
- **Config = env vars only**: no config files, no secrets on disk. Keys flow via envctl when available.
- **Stub guard mandatory**: all simulation paths preflight-check backend; stub backends are refused

## Dev Commands

```bash
cargo check        # Fast compilation check
cargo test         # Run all tests
cargo clippy       # Linting
cargo build --release  # Release binary at ./target/release/teri
```

## Envctl Integration

For secret injection, use envctl:
```bash
envctl run -- teri run --seed ... --query ...
envctl run -- teri serve --addr ...
```

The `agent-env.toml` file declares teri's required secrets.

## Stub Backend Guard

Before running simulations, verify the backend is not stubby:
- Run with `--help` first (keyless) to confirm CLI works
- Check shimmy/inference endpoint reports non-stub mode via `/health` probe
- Look for "GGUF/stub backend detected" error if guard triggers

## Coding Conventions

- Error types use thiserror; prefer TeriError variants over anyhow
- All modules must be re-exported in lib.rs
- Config errors distinguish between Config (hard) and ConfigMissing (graceful degradation)
- Never write secrets to disk or config files
