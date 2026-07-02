# Repository Guidelines

## Project Structure & Module Organization
Teri is a Rust 2024 workspace with the main engine in `src/` and integration tests in `tests/`. The CLI entry point is `src/main.rs`; reusable engine modules live under `src/agent`, `src/api`, `src/graph`, `src/memory`, `src/report`, `src/services`, and related top-level modules. The Vue/Vite web UI is in `frontend/`. Example seeds and fixtures live in `examples/`, prompt/report templates in `templates/`, and documentation in `docs/`, `ARCHITECTURE.md`, and `RUNBOOK.md`. The vendored `pebesen/` workspace supports community-platform integration tests.

## Build, Test, and Development Commands
- `nix develop /home/flexnetos/FlexNetOS/src/yazelix#ci -c bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri && cargo build'` — build the default `teri` package from the workspace Rust toolchain.
- `nix develop /home/flexnetos/FlexNetOS/src/yazelix#ci -c bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri && cargo test'` — run Rust unit and integration tests for default workspace members.
- `nix develop /home/flexnetos/FlexNetOS/src/yazelix#ci -c bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri && cargo clippy --all-targets --all-features -- -D warnings'` — enforce Rust lints.
- `nix develop /home/flexnetos/FlexNetOS/src/yazelix#ci -c bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri && cargo fmt --all'` — format Rust code according to `.rustfmt.toml`.
- `nix develop /home/flexnetos/FlexNetOS/src/yazelix#ci -c bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri && cargo run -- --help'` — inspect the CLI; use the same wrapper for `cargo run --release -- run --seed examples/seed.txt --query "..."`.
- `meta exec --include teri -- bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri/frontend && bun install && bun run dev'` — start the Vue development server with the workspace-managed Bun toolchain.
- `meta exec --include teri -- bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri/frontend && bun run build'` — build the Vue web UI.
- `meta exec --include teri -- bash -lc 'cd /home/flexnetos/FlexNetOS/src/teri/pebesen/frontend && bun install && bun run dev'` — start the vendored SvelteKit dev server; do not use the stale `pnpm` examples in older docs.

## Coding Style & Naming Conventions
Use Rust 2024 idioms, four-space indentation, and the repository rustfmt settings (`max_width = 100`). Keep modules focused and name files after their domain (`pipeline.rs`, `preflight.rs`, `src/memory/*`). Prefer descriptive snake_case for Rust functions and modules, PascalCase for types, and SCREAMING_SNAKE_CASE for constants. Frontend code follows Vue single-file component conventions under `frontend/src`.

## Testing Guidelines
Place cross-module behavior tests in `tests/` with descriptive names ending in `_test.rs` or matching the feature under test. Prefer deterministic fixtures from `examples/` and avoid live external services unless the test explicitly validates integration behavior. Run Rust gates from the Yazelix Nix shell before submitting changes, and run frontend build checks with workspace `bun` when touching either frontend surface.

## Commit & Pull Request Guidelines
Git history uses concise Conventional Commit-style subjects such as `feat: ...`, `fix(i18n): ...`, `docs: ...`, and `chore(codex): ...`. Keep commits scoped to one change. PRs should explain what changed, why, validation performed, and any user-facing impact. Link related issues when available and include screenshots for UI changes.

## Security & Configuration Tips
Do not commit secrets, `.env`, generated databases, or build artifacts. Use `.env.example` as the documented local configuration shape. In this workspace, local Rust execution should be driven from the Yazelix Nix shell; keep child-process secret injection as the architectural contract recorded by `agent-env.toml`.
