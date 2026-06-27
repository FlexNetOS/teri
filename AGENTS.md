# Repository Guidelines

## Project Structure & Module Organization
Teri is a Rust 2024 workspace with the main engine in `src/` and integration tests in `tests/`. The CLI entry point is `src/main.rs`; reusable engine modules live under `src/agent`, `src/api`, `src/graph`, `src/memory`, `src/report`, `src/services`, and related top-level modules. The Vue/Vite web UI is in `frontend/`. Example seeds and fixtures live in `examples/`, prompt/report templates in `templates/`, and documentation in `docs/`, `ARCHITECTURE.md`, and `RUNBOOK.md`. The vendored `pebesen/` workspace supports community-platform integration tests.

## Build, Test, and Development Commands
- `cargo build` — build the default `teri` package.
- `cargo test` — run Rust unit and integration tests for default workspace members.
- `cargo clippy --all-targets --all-features -- -D warnings` — enforce Rust lints.
- `cargo fmt` or `rustfmt` — format Rust code according to `.rustfmt.toml`.
- `cargo run -- --help` — inspect the CLI; use `cargo run --release -- run --seed examples/seed.txt --query "..."` for a local simulation.
- `cd frontend && bun install && bun run dev` — start the Vue development server.
- `cd frontend && bun run build` — build the web UI.

## Coding Style & Naming Conventions
Use Rust 2024 idioms, four-space indentation, and the repository rustfmt settings (`max_width = 100`). Keep modules focused and name files after their domain (`pipeline.rs`, `preflight.rs`, `src/memory/*`). Prefer descriptive snake_case for Rust functions and modules, PascalCase for types, and SCREAMING_SNAKE_CASE for constants. Frontend code follows Vue single-file component conventions under `frontend/src`.

## Testing Guidelines
Place cross-module behavior tests in `tests/` with descriptive names ending in `_test.rs` or matching the feature under test. Prefer deterministic fixtures from `examples/` and avoid live external services unless the test explicitly validates integration behavior. Run `cargo test` before submitting Rust changes; run frontend build checks when touching `frontend/`.

## Commit & Pull Request Guidelines
Git history uses concise Conventional Commit-style subjects such as `feat: ...`, `fix(i18n): ...`, `docs: ...`, and `chore(codex): ...`. Keep commits scoped to one change. PRs should explain what changed, why, validation performed, and any user-facing impact. Link related issues when available and include screenshots for UI changes.

## Security & Configuration Tips
Do not commit secrets, `.env`, generated databases, or build artifacts. Use `.env.example` as the documented local configuration shape and prefer envctl-based secret injection for normal development.
