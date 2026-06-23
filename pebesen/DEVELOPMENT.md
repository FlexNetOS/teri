# Development Guide

This guide covers the complete development workflow for Pebesen, from setup to deployment.

## Table of Contents

- [Quick Start](#quick-start)
- [Development Environment](#development-environment)
- [Project Structure](#project-structure)
- [Development Workflow](#development-workflow)
- [Testing](#testing)
- [Database Management](#database-management)
- [Code Quality](#code-quality)
- [Observability and Health Metrics](#observability-and-health-metrics)
- [Debugging](#debugging)
- [Deployment](#deployment)

## Quick Start

```bash
# Clone and set up everything
git clone https://github.com/SHA888/pebesen
cd pebesen
make setup

# Start development environment
make dev
```

## Development Environment

### Prerequisites

- **Rust 1.85+** (2024 edition)
- **Node.js 22+** (latest LTS)
- **pnpm 9+**
- **Docker & Docker Compose**
- **Python 3.8+** (for pre-commit hooks)
- **uv** (recommended Python package manager)
- **cargo-audit** — installed by `make setup`, or: `cargo install cargo-audit`

### Installation Commands

```bash
# Rust
rustup update stable
rustup component add rustfmt clippy

# pnpm
npm install -g pnpm

# Python tools
uv tool install pre-commit
```

### Environment Variables

Copy `.env.example` to `.env` and configure:

```bash
cp .env.example .env
```

Required variables:
- `DATABASE_URL` — PostgreSQL connection string
- `REDIS_URL` — Redis connection string
- `MEILISEARCH_URL` — Search service URL
- `MEILISEARCH_MASTER_KEY` — Search service master key
- `JWT_SECRET` — JWT signing secret (min 64 chars)
- `CONTRIBUTOR_PAYOUT_FRACTION` — default `0.20`
- `INTELLIGENCE_API_ENABLED` — default `false`

## Project Structure

```
pebesen/
├── crates/
│   ├── pebesen-api/          # Axum handlers, middleware, WebSocket
│   ├── pebesen-core/         # Domain types, business logic, no I/O
│   ├── pebesen-db/           # sqlx queries, migrations
│   ├── pebesen-search/       # Meilisearch + pgvector
│   ├── pebesen-notifications/# Email, digest scheduler
│   ├── pebesen-intelligence/ # B2B intelligence API (Phase 2 stub)
│   └── pebesen-bin/          # CLI binaries: reindex, reembed, export, mcp
├── frontend/
│   ├── src/
│   │   ├── lib/
│   │   ├── routes/
│   │   ├── stores/
│   │   └── components/
│   └── static/
├── migrations/
├── docs/
│   ├── self-hosting.md       # Phase 2
│   ├── contributor-payouts.md# Phase 1 — legibility required before Phase 2
│   ├── cold-start.md         # Internal — first 10 communities plan
│   └── upgrade.md            # Phase 2
├── docker-compose.yml
├── Makefile
├── Cargo.toml
└── pnpm-workspace.yaml
```

## Development Workflow

### Daily Development

```bash
make dev           # Start everything
make dev-backend   # Backend only
make dev-frontend  # Frontend only
make logs          # View logs
```

### Making Changes

1. Create feature branch: `git checkout -b feature/your-feature`
2. Make changes
3. Run quality checks: `make check`
4. Commit — pre-commit hooks run automatically
5. Push and open PR

### Database Changes

```bash
# New migration
echo "-- SQL here" > migrations/NNNN_description.sql

# Run migrations
make migrate

# Reset (dev only)
make db-reset
```

## Testing

```bash
make test                        # All tests
cargo test --all-features        # Rust only
cd frontend && pnpm test         # Frontend only
cargo test --all-features --watch# Watch mode
```

### Test Organization

- **Unit tests**: individual functions and modules
- **Integration tests**: API endpoints and DB interactions
- **Frontend tests**: component testing

### Coverage

```bash
# Rust
cargo install tarpaulin
cargo tarpaulin --all-features --workspace

# Frontend
cd frontend && pnpm test --coverage
```

## Database Management

### Services

- **PostgreSQL + pgvector**: port 5434 (internal)
- **Redis**: port 6380
- **Meilisearch**: port 7701

### Commands

```bash
make db-up        # Start services
make db-down      # Stop services
make db-reset     # Reset (WARNING: destroys data)
make migrate      # Run migrations
make refresh-stats# Refresh contributor_stats materialized view

# Direct access
docker exec -it pebesen_postgres psql -U pebesen -d pebesen
docker exec -it pebesen_redis redis-cli
curl http://localhost:7701/health
```

### Migration Guidelines

1. Numbered prefix, descriptive name
2. Write reversible migrations where possible
3. Test on fresh database before committing
4. Update corresponding domain types in `pebesen-core`
5. Never rename or drop columns without a deprecation migration first

## Code Quality

### Pre-commit Hooks

- Rust: `cargo fmt`, `cargo clippy`, `cargo check`
- Frontend: `prettier`, `eslint`, `svelte-check`
- General: trailing whitespace, file endings, large files

### Manual Checks

```bash
make format        # Format all code
make lint          # Run all linters
make check-security# cargo audit + pnpm audit
make deps          # Update dependencies
```

### Code Review Gates (non-negotiable)

The following are blocking PR rejections regardless of other quality:

- Any gamification element in the UI (badges, XP, streaks, levels, leaderboards, progress bars)
- Contribution weight recorded for gatekeeping moderation actions (close-as-duplicate without comment, remove without reason)
- Access token stored in localStorage or any persistent browser storage
- Cross-space data leakage in any search or intelligence endpoint
- Payout computation that silently redistributes opted-out shares without space owner awareness

---

## Observability and Health Metrics

### What to Measure

Pebesen tracks community health, not platform vanity metrics. The distinction is architectural — the wrong metrics cause the wrong product decisions.

**Primary health signals** (instrument from Phase 0):

| Signal | Query anchor | Alert threshold |
|---|---|---|
| Engaged members (30d) | `contributions` WHERE created_at > now()-30d, distinct user_id per space | < 3 in any paid space |
| Knowledge depth | topics with ≥1 reply + domain_id NOT NULL | Declining week-over-week |
| Contributor retention (90d) | top-10 by weight in period T still active in T+90 | < 60% |
| Space self-sufficiency | spaces WHERE subscription_revenue ≥ estimated_hosting_cost | Track, no alert yet |

**Anti-metrics — never display on dashboards, never optimize for:**
- Monthly active visitors (SEO traffic inflates this meaninglessly — Quora lesson)
- Total registrations
- Total message count
- Raw DAU/MAU ratio without engagement qualifier

### Instrumentation

```bash
# Rust — tracing
RUST_LOG=info cargo run          # Production log level
RUST_LOG=debug cargo run         # Debug with query logs
RUST_LOG=sqlx=debug cargo run    # SQL query tracing
```

Add `tracing::instrument` on all contribution recording functions — these are the most business-critical code paths.

### Health Check Endpoints

```
GET /health          # Liveness: returns 200 if process is alive
GET /health/ready    # Readiness: checks DB + Redis + Meilisearch connectivity
GET /health/metrics  # Prometheus-compatible metrics (Phase 1)
```

---

## Debugging

### Backend

```bash
RUST_LOG=debug cargo run
RUST_LOG=sqlx=debug cargo run    # SQL query logging
cargo build                       # Debug build
RUST_LOG=debug cargo test your_test
```

### Frontend

```bash
cd frontend && pnpm dev --debug
cd frontend && pnpm check         # Type checking
cd frontend && pnpm build --analyze
```

### Database

```bash
docker compose ps
docker compose logs postgres
docker exec -it pebesen_postgres psql -U pebesen -d pebesen
# \dt   list tables
# \d contributions   describe contributions table
```

## Performance

```bash
# Rust profiling
cargo install cargo-flamegraph
cargo flamegraph --bin pebesen

# Frontend bundle
cd frontend && pnpm build --analyze
npx lighthouse http://localhost:5173
```

## Deployment

### Self-Hosted (AGPL — always free)

```bash
git clone https://github.com/SHA888/pebesen
cd pebesen
cp .env.example .env
# Edit .env
docker compose up -d
```

See `docs/self-hosting.md` for full documentation (Phase 2).

### Hosted (Commercial)

Same topology per tenant. Tenant isolation at space level. See ARCHITECTURE.md deployment topology section.

### Target: First Space Self-Sufficiency

The real Phase 0 gate is not a line count or a feature checklist. It is **the first space whose subscription revenue covers its hosting cost**. A community platform that reaches this at small scale is structurally more durable than one that requires growth to survive. Every deployment decision should be made with this constraint in mind — single binary, minimal RAM, fast cold start.

---

## Troubleshooting

### Common Issues

**Port conflicts:**
```bash
netstat -tulpn | grep :5173
netstat -tulpn | grep :3000
make db-down
```

**Database connection errors:**
```bash
make db-up
docker compose ps
make db-reset
```

**pgvector extension missing:**
```bash
# Ensure using pgvector/pgvector:pg16 image, not postgres:16-alpine
docker compose down -v
docker compose up -d
make migrate
```

**Frontend build errors:**
```bash
cd frontend && rm -rf .svelte-kit build dist
pnpm install
```

**Rust compilation errors:**
```bash
cargo clean
cargo update
```

---

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for full guidelines.

### Code Review Process

1. All changes must pass `make check`
2. PRs require at least one approval
3. Tests required for new features
4. Documentation updated for API changes
5. Code review gates above enforced without exception

### Release Process

1. Update version in `Cargo.toml` workspace + `package.json`
2. Update `CHANGELOG.md`
3. Create semver git tag
4. CI publishes to GHCR on tag
