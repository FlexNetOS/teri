<div align="center">

# Teri

简洁通用的群体智能引擎，预测万物
</br>
<em>A Simple and Universal Swarm Intelligence Engine, Predicting Anything</em>

[English](./README.md) | [中文文档](./README-ZH.md)

</div>

> **Teri** is a Rust-native rewrite of [MiroFish](https://github.com/666ghj/MiroFish)
> (AGPL-3.0 upstream). Teri is itself licensed **AGPL-3.0-or-later**, keeping faith with
> MiroFish's copyleft intent. It is the *upgrade*, not a downgrade: every MiroFish capability is a
> requirement here. "Teri" (Indonesian *ikan teri*, the anchovy) is one of the smallest fish in
> the sea, yet moves in vast, tightly coordinated schools — emergent behavior no single fish
> planned. That is exactly what this engine does: seed the world, spawn the swarm, watch
> emergence happen.

## ⚡ Overview

**Teri** is a next-generation AI prediction engine powered by multi-agent technology. By
extracting seed information from the real world (such as breaking news, policy drafts, or
financial signals), it automatically constructs a high-fidelity parallel digital world. Within
this space, thousands of intelligent agents with independent personalities, long-term memory, and
behavioral logic freely interact and undergo social evolution. You can inject variables
dynamically from a "God's-eye view" to precisely deduce future trajectories — **rehearse the
future in a digital sandbox, and win decisions after countless simulations**.

> You only need to: upload seed materials (data analysis reports or interesting novel stories) and
> describe your prediction requirements in natural language.</br>
> Teri will return: a detailed prediction report, and a deeply interactive high-fidelity digital
> world.

### Our Vision

Teri is dedicated to creating a swarm-intelligence mirror that maps reality. By capturing the
collective emergence triggered by individual interactions, we break through the limitations of
traditional prediction:

- **At the Macro Level**: a rehearsal laboratory for decision-makers, allowing policies and public
  relations to be tested at zero risk.
- **At the Micro Level**: a creative sandbox for individual users — whether deducing novel endings
  or exploring imaginative scenarios, everything can be fun, playful, and accessible.

From serious predictions to playful simulations, we let every "what if" see its outcome, making it
possible to predict anything.

## 🔄 Workflow

1. **Graph Building** — Seed extraction & individual/collective memory injection & GraphRAG
   construction.
2. **Environment Setup** — Entity-relation extraction & persona generation & agent configuration
   injection (simulation parameters).
3. **Simulation** — Dual-platform parallel simulation & auto-parsed prediction requirements &
   dynamic temporal-memory updates.
4. **Report Generation** — A ReportAgent with a rich toolset interacts deeply with the
   post-simulation environment.
5. **Deep Interaction** — Chat with any agent in the simulated world & converse with the
   ReportAgent.

## ✨ Why Teri (the upgrade over MiroFish)

| Concern | MiroFish (Python) | Teri (Rust) |
| --- | --- | --- |
| Backend runtime | Rust nightly via Yazelix Nix shell | Single static binary (`cargo build --release`) |
| Agent concurrency | GIL-limited threads | `tokio` bounded concurrency + `rayon` CPU parallelism |
| Temporal memory / graph | External **Zep Cloud** (`ZEP_API_KEY`) | **Native, in-process** temporal graph memory (petgraph + redb) — no external service, no extra key |
| Type safety | Runtime errors | Compile-time guarantees |
| Secrets | `.env` API keys on disk | child-process injection by contract; `.env` for local dev in this workspace |
| Backend honesty | — | Preflight guard refuses stub/canned inference backends before any run |

## 🚀 Quick Start

Teri has two surfaces: the **engine** (Rust — CLI `teri run` + REST/SSE server `teri serve`) and
the **web UI** (Vue 3 SPA — the 5-step prediction studio).

### Prerequisites

| Tool | Version | Description | Check |
| --- | --- | --- | --- |
| **Rust** | nightly (via Yazelix Nix shell) | Engine runtime | `nix develop /home/flexnetos/FlexNetOS/src/yazelix#ci -c cargo --version` |
| **Bun** | workspace-managed | Web UI package manager/runtime | `meta exec --include teri -- bun --version` |
| **LLM endpoint** | OpenAI-compatible | Any OpenAI-SDK-format LLM API or local backend | — |

### 1. Configure secrets

Teri never expects raw `export LLM_API_KEY` in your shell profile. The long-term contract is
`agent-env.toml` plus child-process secret injection. In this workspace, the immediately working
local path is a gitignored `.env` for development plus the Yazelix Nix shell for Rust commands.

```bash
cp .env.example .env   # local dev only

# LLM API (any OpenAI-SDK-compatible endpoint)
#   LLM_API_KEY=...                # optional for keyless local backends
#   LLM_BASE_URL=http://127.0.0.1:11435/v1
#   LLM_MODEL_NAME=Qwen/Qwen3-4B
```

> Teri has **no Zep Cloud dependency** — temporal graph memory is reimplemented natively in-process.
> There is no `ZEP_API_KEY`.

### 2. Run a simulation (CLI)

```bash
# Working local frontdoor in this workspace:
nix develop /home/flexnetos/FlexNetOS/src/yazelix#ci -c bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri && cargo run --release -- run \
  --seed ./examples/seed.txt \
  --query "How will this policy affect public sentiment in 30 days?"'

# The CLI surface works without any secrets:
nix develop /home/flexnetos/FlexNetOS/src/yazelix#ci -c bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri && cargo run --release -- --help'
```

The `run` path preflights the inference backend, then composes seed → graph → agents → sim →
report.

### 3. Start the API server + Web UI

```bash
# Engine (REST + SSE), preflights the backend before binding:
nix develop /home/flexnetos/FlexNetOS/src/yazelix#ci -c bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri && cargo run --release -- serve --addr 0.0.0.0:5001'

# Web UI (separate dev server):
meta exec --include teri -- bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri/frontend && bun install && bun run dev'

# Vendored pebesen UI (SvelteKit):
meta exec --include teri -- bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri/pebesen/frontend && bun install && bun run dev'
```

**Service URLs:**
- Web UI: `http://localhost:8374`
- Engine API: `http://localhost:5001`

## 🖥️ Web UI

The Teri studio is a Vue 3 single-page app (vue-router, vue-i18n, d3, axios) that drives the full
workflow as a guided wizard:

- **Step 1 — Graph Build** — upload seed, watch the knowledge graph build (d3 `GraphPanel`).
- **Step 2 — Environment Setup** — entity/relation review, persona + agent-config generation.
- **Step 3 — Simulation** — live dual-platform tick stream (SSE), God's-eye variable injection.
- **Step 4 — Report** — the generated prediction report.
- **Step 5 — Interaction** — chat with any agent in the world, or with the ReportAgent.
- **History Database** — browse and reopen prior runs.
- **i18n** — English / 中文 language switcher.

## ⚙️ Configuration

All configuration is via environment variables (no config files required):

| Variable | Default | Description |
| --- | --- | --- |
| `LLM_BASE_URL` | `http://127.0.0.1:11435/v1` | OpenAI-compatible LLM API endpoint |
| `LLM_API_KEY` | *(optional for keyless local)* | API key for hosted LLM backends |
| `LLM_MODEL_NAME` / `LLM_MODEL` | `Qwen/Qwen3-4B` | Completion model (`LLM_MODEL_NAME` wins) |
| `LLM_MAX_TOKENS` | `2048` | Generation cap/default for OpenAI-compatible calls; set `0` to use backend defaults |
| `LLM_MAX_CONCURRENT_REQUESTS` | `1` for local inferrs, otherwise unset | In-flight OpenAI-compatible request cap; prevents local CUDA queue timeouts |
| `EMBED_MODEL` | `all-MiniLM-L6-v2` | Embedding model |
| `DEFAULT_AGENT_COUNT` | `100` | Default agents per simulation |
| `SIM_MAX_TICKS` | `50` | Maximum ticks per run |
| `RUST_LOG` | `teri=debug,tower_http=info` | Logging level |

### inferrs verification and benchmarks

Use these repo-local gates before claiming the local GPU backend is healthy:

```bash
scripts/verify-inferrs-no-downgrade.sh
scripts/verify-inferrs-runtime.sh
scripts/benchmark-inferrs-local.sh
CODEX_SMOKE=1 scripts/verify-cloud-codex-coexistence.sh
```

The no-downgrade gate proves the defaults still target local inferrs
(`Qwen/Qwen3-4B`, `127.0.0.1:11435`), the Rust toolchain is nightly, the NVIDIA
610/CUDA 13.3/cuda-oxide stack is present, and `FlexNetOS/inferrs` builds with
CUDA. The benchmark script writes fresh per-model logs plus a `summary.md` under
`/tmp/teri-inferrs-benchmarks/<timestamp>/`.

## 🏗️ Architecture

The engine is a typed five-stage pipeline: `seed → graph → agent → sim → report`, exposed through a
CLI (`teri run`) and a REST/SSE server (`teri serve`). See [`ARCHITECTURE.md`](./ARCHITECTURE.md)
for module contracts and [`RUNBOOK.md`](./RUNBOOK.md) for the authoritative parity-verification
surface.

## Source Wires

Issue 86 adds a checked-in source-wire surface so external evidence, lineage, backend, browser,
observability, and optimizer references are inspectable inside the repo instead of living only in
issue text.

- Docs: [`docs/source-wires/README.md`](./docs/source-wires/README.md)
- Registry: `src/source_wires.rs`
- CLI:
  - `cargo run -- wires list`
  - `cargo run -- wires show brain_in_the_fish`
  - `cargo run -- wires validate`

## 🛡️ Backend honesty guard

`run` and `serve` both preflight the configured backend fail-closed: `GET /models` (identity) and a
1-token completion probe. Backends that list no models or answer with canned stub text are
**refused** — a swarm simulated on canned text is fabrication, not prediction. The guard is never
weakened to make a run proceed.

## 📊 Status

Teri is a broad agentic scenario engine, not an oracle. It can simulate and forecast scenarios that
fit its seed data, ontology, persona, action, memory, and report model. It does not prove causal
truth. Prediction/report `confidence` is **synthesized** metadata by default; an **opt-in
per-community calibration loop** (the autonomy LEARN layer, `src/autonomy/calibration.rs`) now
adjusts it toward calibrated for communities where actioned/accurate outcomes have been recorded —
using the same `(0.5 + accuracy).clamp(0.5, 1.5)` heuristic as the pebesen receiver. Until outcomes
are recorded for a community its weight is neutral (1.0), so confidence is unchanged.

Parity against MiroFish is tracked in [`RUNBOOK.md`](./RUNBOOK.md) §12 and the feature-parity
ledger. The **Web UI** is the principal in-progress surface relative to MiroFish.

## 📄 Acknowledgments

Teri is a Rust rewrite of **[MiroFish](https://github.com/666ghj/MiroFish)** by BaiFu / 666ghj
(AGPL-3.0), and is itself licensed AGPL-3.0-or-later in keeping with that copyleft intent. Its
simulation design draws on **[OASIS (Open Agent Social Interaction Simulations)](https://github.com/camel-ai/oasis)**
from the CAMEL-AI team. Our thanks to both.

## License

Teri is licensed under the **GNU Affero General Public License v3.0 or later**
([AGPL-3.0-or-later](LICENSE)). Copyright © 2026 FlexNetOS.

The AGPL's network-use clause is the point: if you run a modified teri as a network service
(`teri serve`), you must offer your modified source to its users. Improvements flow back to the
commons rather than disappearing into a closed product — the same reciprocity MiroFish intended.
