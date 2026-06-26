# Pebesen

![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)
![Status: Pre-alpha](https://img.shields.io/badge/Status-Pre--alpha-orange.svg)
![Rust](https://img.shields.io/badge/Rust-2024%20edition-orange.svg)
![TypeScript](https://img.shields.io/badge/TypeScript-5.9+-blue.svg)

> Structured conversations. Aligned incentives. Built for the users most platforms ignore.

**📜 Licensed under AGPL-3.0 — Free for self-hosting and community use. Commercial license available for hosted deployments.**

---

## What This Is

Pebesen is an open-source messaging platform and community knowledge infrastructure built around two compounding bets:

1. **Most messaging tools are optimized for the sender. This one is optimized for the reader.**
2. **The people who create community value should share in it.**

It enforces topic discipline at the input layer, maintains a single user identity across all communities, accumulates contribution value as a first-class data primitive, and treats knowledge preservation as structural — not incidental.

---

## Why It Exists

Existing platforms made deliberate tradeoffs that served their primary use cases well. Those same tradeoffs create structural gaps:

| Design Tradeoff | Optimized For | Gap Created |
|---|---|---|
| Workspace-scoped identity | Enterprise security isolation | Users belonging to many communities |
| Casual, low-friction UX | Fast consumer adoption | Long-lived structured knowledge |
| Federation-first architecture | Decentralization | Non-technical users facing setup complexity |
| Real-time presence and engagement | Synchronous collaboration | Async-first, deep-work users |
| Advertiser-funded revenue | Scale and liquidity | Community ownership and contributor alignment |

The last row is the one that compounds all the others. When a platform's revenue comes from advertisers rather than the community itself, every product decision eventually optimizes for engagement over knowledge quality. The community creates value. The platform extracts it. The contributors get nothing. Moderation becomes a cost to minimize rather than labor to compensate.

Pebesen makes different tradeoffs, optimized for a different outcome.

---

## Core Architectural Decisions

Four structural decisions that compound:

1. **Single identity, multi-community** — one account joins unlimited spaces
2. **Mandatory topic tagging** — every message belongs to a Stream and a Topic, enforced at input
3. **Per-topic unread state** — read tracking at topic granularity, not channel granularity
4. **Contribution as a first-class primitive** — every meaningful action accumulates attribution data from day one, before any revenue exists to distribute

The fourth decision is what separates this from prior attempts at community platforms. It is not a feature added after product-market fit. It is in the schema from the first migration.

---

## Business Model

**Locked before Phase 0.2. Not deferred.**

Four revenue streams in order of implementation priority:

### 1. Hosted SaaS — Space-Based Tiers
Self-hosting is always free under AGPL. Hosted deployments are commercial.

Pricing is per-space, not per-seat. Per-seat pricing is structurally incompatible with lurker-first design — it punishes communities for having readers.

| Tier | Target | Pricing Signal |
|---|---|---|
| Community | Open source projects, small research groups | Free (hosted, capped) |
| Standard | Active professional communities | Flat monthly per space |
| Scale | Large or multi-space organizations | Volume flat fee |

### 2. Contributor Revenue Sharing
A percentage of each space's hosting revenue is redistributed to top contributors and active moderators within that space. Computed from the `contributions` primitive. Paid monthly.

This is not a feature. It is the mechanism that converts community ownership from rhetoric into an economic fact. It changes the moderator relationship from unpaid volunteer to aligned partner.

### 3. Verified Expertise Subscriptions
Professionals (doctors, lawyers, engineers, researchers) pay a monthly subscription to obtain a domain-verified credential badge within relevant spaces. Verification is self-sovereign — the platform provides the infrastructure, not the judgment.

Verified contributors get access to professional-tier subbranches. Communities get higher-signal expert participation. Professionals get reputation infrastructure with real-world career value.

### 4. B2B Community Intelligence API
Structured, privacy-respecting community insight data sold to researchers, brands, consultancies, and policy teams. Not raw data scraping — aggregated, anonymized signals extracted from the structured topic/stream hierarchy.

---

## Who This Is For

### Primary Segments

**Open source and technical communities**
- Distributed contributors across global timezones
- Need structured, long-lived topic history newcomers can navigate independently
- Size: 5–500 active contributors, long-lived projects

**Research and academic groups**
- Labs, cohorts, reading groups, conference communities
- Need permanent, searchable, topic-organized history
- Fragmented across multiple tools with no single coherent knowledge store

### Secondary Segments (Phase 2+)

**Context multiplexers** — consultants, DevRel, open source contributors active in 10+ communities simultaneously

**Neurodivergent users** — ADHD/autism users harmed at the model level, not the surface level

**Longitudinal participants** — community veterans who hold institutional memory with no platform support

**Knowledge consumers (lurkers)** — the ~75% of any community who read without posting

---

## Core Thesis

> A messaging platform that enforces structure at input produces knowledge as a side effect.
> Knowledge that accumulates structurally is irreplaceable.
> Contributors who share in the value they create do not leave.
> Irreplaceable tools with aligned incentives do not churn.

The retention model is not engagement loops. It is accumulated, structured, irretrievable-elsewhere knowledge combined with economic alignment between the platform and its contributors.

---

## What It Is Not

- Not a feature-for-feature alternative to any existing platform
- Not optimized for casual, ephemeral conversation
- Not an AI chat product — AI accelerates the model, it does not replace it
- Not a project management tool
- Not a platform that extracts community value without returning any of it

---

## Validation Status

- **Stage 0**: Confirmed active migration intent across the messaging platform landscape. Documented workaround products proving unmet demand. Studied failure modes of prior entrants.
- **Stage 1**: Primary segment: open source and technical communities. Secondary: context multiplexers and neurodivergent users.
- **Stage 2**: Qualitative patterns confirmed: newcomer onboarding friction, knowledge retrieval difficulty, multi-workspace fatigue, communication model mismatches for neurodivergent users.
- **Stage 3**: Quantitative proxies — measurable spikes in alternative platform searches 2025–2026, active community migration driven by privacy, pricing, and policy changes.
- **Stage 4**: Behavior proxies monetizing. Forum-style threading adopted industry-wide.
- **Stage 5**: Structural defensibility confirmed. Seat-based pricing structurally incompatible with lurker-first design. Contributor alignment structurally incompatible with advertiser-funded model.

---

## Tech Stack

| Layer | Choice | Rationale |
|---|---|---|
| Backend | Rust (Axum + Tokio) | Performance, safety, single binary deployment |
| Real-time | WebSocket (tokio-tungstenite) | Native async, no runtime overhead |
| Database | PostgreSQL + pgvector | Relational integrity + semantic search from day one |
| Cache / PubSub | Redis | Session state, fan-out, ephemeral presence |
| Search | Meilisearch | Self-hostable, fast, typo-tolerant |
| Frontend | TypeScript + SvelteKit (pnpm) | Lightweight, fast, component-scoped CSS |
| Self-host | Docker Compose | Single-command deployment |
| Package managers | pnpm (TS), uv (Python tools), Cargo (Rust) | Per-language best-in-class |

---

## Quick Start

### Prerequisites

- Rust 1.85+ (2024 edition)
- Node.js 22+ (latest LTS)
- pnpm 9+
- Docker & Docker Compose
- Python 3.8+ (for pre-commit hooks)
- uv

### Setup

```bash
git clone https://github.com/SHA888/pebesen
cd pebesen
make setup
make dev
```

```bash
make help          # Show all available commands
make dev           # Start full development environment
make test          # Run all tests
make lint          # Run all linters
make check         # Run all checks (format + lint + test)
make db-reset      # Reset database
```

---

## Project Status

**Phase**: Pre-Phase Complete ✅ — Phase 0 In Progress 🚧

See [`TODO.md`](./TODO.md) for phased build plan.
See [`ARCHITECTURE.md`](./ARCHITECTURE.md) for system design.

---

## License

Licensed under **GNU Affero General Public License v3.0** (AGPL-3.0).

- ✅ Free for self-hosting and community use
- ✅ Source code available with network copyleft provisions
- 💼 Commercial license available for hosted deployments — contact for terms

See [LICENSE](./LICENSE) for full text.
