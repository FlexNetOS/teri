# Contributing to Pebesen

Thank you for your interest in contributing! This document covers the essential setup and workflow for contributors.

## Development Setup

### Prerequisites

- **Rust 1.85+** (2024 edition)
- **Node.js 22+** (latest LTS)
- **pnpm 9+**
- **Docker & Docker Compose**
- **Python 3.8+** (for pre-commit hooks)
- **uv** (recommended Python package manager)

### Quick Start

One-command setup that handles everything:

```bash
git clone https://github.com/SHA888/pebesen.git
cd pebesen
make setup
make dev
```

### Manual Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/SHA888/pebesen.git
   cd pebesen
   ```

2. Install dependencies and set up environment:
   ```bash
   make setup
   ```

3. Configure environment:
   ```bash
   cp .env.example .env
   # Edit .env with your settings
   ```

4. Start development environment:
   ```bash
   make dev
   ```

### Environment Configuration

Copy `.env.example` to `.env` and configure:

Required variables:
- `DATABASE_URL` - PostgreSQL connection
- `REDIS_URL` - Redis connection
- `MEILISEARCH_URL` - Search service URL
- `MEILISEARCH_MASTER_KEY` - Search service key
- `JWT_SECRET` - At least 64 characters

See `.env.example` for all available options.

## Development Workflow

### Available Commands

```bash
make help           # Show all available commands

# Setup & Dependencies
make setup         # Install all dependencies and set up environment
make deps          # Update all dependencies
make check-security # Run security audits

# Development
make dev           # Start full development environment
make dev-backend   # Backend only
make dev-frontend  # Frontend only
make build         # Build all components

# Database
make db-up         # Start database services
make db-down       # Stop database services
make db-reset      # Reset database (WARNING: destroys data)
make migrate       # Run database migrations

# Quality Assurance
make test          # Run all tests
make lint          # Run all linters
make format        # Format all code
make check         # Run all checks (format + lint + test)

# Utilities
make clean         # Clean build artifacts
make reindex       # Rebuild search index
make logs          # Show development logs
```

### Code Quality

All code is automatically checked before commits:

- **Rust**: `cargo fmt`, `cargo clippy`, `cargo test`
- **Frontend**: `prettier`, `eslint`, `svelte-check`
- **Security**: `cargo audit`, `pnpm audit`

Pre-commit hooks ensure all code meets quality standards.

### Branch Naming

- `feature/description` - New features
- `fix/description` - Bug fixes
- `docs/description` - Documentation updates
- `refactor/description` - Code refactoring

### Pull Request Process

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/your-feature`
3. Make your changes
4. Ensure code quality: `make check`
5. Commit your changes (pre-commit hooks will run)
6. Push to your fork
7. Submit a pull request

### Code Style

- **Rust**: `cargo fmt` and `cargo clippy` must pass
- **TypeScript/Svelte**: ESLint and Prettier must pass
- Use meaningful commit messages
- Document public APIs

## Architecture

### Rust Workspace Structure

```
crates/
├── api/          # HTTP API handlers and routing
├── core/         # Domain models and shared types
├── db/           # Database queries and migrations
├── search/       # Search indexing and queries
├── notifications/# Notification system
└── bin/          # Binary applications (main server, reindex tool)
```

### Frontend Structure

```
frontend/src/
├── lib/
│   ├── assets/   # Static assets
│   └── index.ts  # App entry point
├── routes/       # SvelteKit routes
├── stores/       # Svelte stores for state
└── components/   # Reusable components
```

### Database

- **PostgreSQL**: Primary data store
- **Redis**: Session state and pub/sub
- **Meilisearch**: Full-text search
- **Migrations**: Located in `migrations/` directory

## Testing

### Running Tests

```bash
# All tests
make test

# Rust only
cargo test --all-features

# Frontend only
cd frontend && pnpm test
```

### Test Coverage

- Aim for >80% coverage on new code
- Write unit tests for business logic
- Write integration tests for API endpoints

## Debugging

### Backend

```bash
# Debug build
cargo build

# Run with logs
RUST_LOG=debug cargo run

# Database queries
RUST_LOG=sqlx=debug cargo run
```

### Frontend

```bash
# Development mode
make dev-frontend

# Type checking
cd frontend && pnpm check
```

### Database

```bash
# Check service status
docker compose ps

# View logs
make logs

# Connect to database
docker exec -it pebesen_postgres psql -U pebesen -d pebesen
```

## Common Issues

### Port Conflicts

Default ports used by development services:
- PostgreSQL: internal only (no host port)
- Redis: 6380
- Meilisearch: 7701
- Frontend: 5173
- Backend: 3000

If you encounter port conflicts, stop other services or modify the ports.

### Database Connection

Ensure database services are running:
```bash
make db-up
```

Reset database if needed:
```bash
make db-reset
```

### Pre-commit Hooks

If pre-commit hooks fail:
1. Check the error messages
2. Run `make format` to fix formatting issues
3. Run `make lint` to check for linting errors
4. Run `make test` to ensure tests pass

## Getting Help

- Check existing issues on GitHub
- Ask questions in discussions
- Review architecture documentation in `ARCHITECTURE.md`
- Check the development commands with `make help`

## Security

- Report security vulnerabilities privately
- Follow secure coding practices
- Run security audits: `make check-security`
- Keep dependencies updated: `make deps`

## License

By contributing, you agree that your contributions will be licensed under the AGPL-3.0 license.
