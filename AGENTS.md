# Repository Guidelines

## Project Structure & Module Organization
- `src-tauri/` holds the Tauri Rust app (commands, state, SQLite, worktrees) and `tauri.conf.json`.
- `backend/` is the Rust backend crate (agent orchestration, config, migrations). See `backend/ARCHITECTURE.md`.
- `gui/` contains static HTML/CSS/JS assets consumed by the Tauri webview.
- `docs/plans/` stores implementation plans and manual test checklists.

## Build, Test, and Development Commands
Run from the repository root unless noted.
- `cd gui && python3 -m http.server 8000` (or `npx serve gui -l 8000`) — serve the GUI locally for dev.
- `cd src-tauri && cargo tauri dev` — run the desktop app in development mode.
- `cd src-tauri && cargo build` — build the Tauri binary.
- `cd src-tauri && cargo fmt` — format Rust code (repo root is not a Cargo workspace).
- `cargo tauri build` — build production bundles (outputs in `src-tauri/target/release/bundle/`).
- `cargo build --release -p phantom_harness` — build the release binary via the workspace crate.
- `scripts/dmg/build-dmg.sh` — build the custom macOS DMG after a release build.
- `cd src-tauri && cargo test` — run Rust unit tests (see `src-tauri/src/namegen.rs`, `src-tauri/src/worktree.rs`).
- `CLAUDE_SMOKE=1 cargo test -p phantom_harness_backend --test claude_smoke -- --nocapture` — run the Claude backend smoke test.
- Rust toolchain is pinned in `src-tauri/rust-toolchain.toml` (currently `1.92.0`).

## Coding Style & Naming Conventions
- Rust: edition 2021; use `cargo fmt` (4-space indent, rustfmt defaults).
- Frontend (HTML/CSS/JS): follow existing 2-space indentation in `gui/`.
- Branch names: use prefixes like `feat/`, `fix/`, `chore/`, `test/`, `docs/`, `refactor/`, `perf/` (see `src-tauri/src/worktree.rs`).

## Testing Guidelines
- Primary tests are Rust unit tests in `src-tauri/src/*`.
- Test names follow `test_*` functions under `#[cfg(test)]` modules.
- Add manual UI verification notes to `docs/plans/*` when UX is touched.

## Commit & Pull Request Guidelines
- Commits follow Conventional Commits: `type(scope): summary` (examples: `feat(ui): …`, `fix(acp): …`, `docs: …`).
- PRs should include: a concise description, linked issue/ticket if applicable, and screenshots or GIFs for UI changes (from `gui/`).

## Configuration & Data Notes
- Agent defaults live in `backend/config/agents.toml`.
- SQLite migrations are in `backend/migrations/`.
- Tauri permissions and schemas are generated under `src-tauri/gen/`.

## Philosophy

This codebase will outlive you. Every shortcut becomes someone else's burden. Every hack compounds into technical debt that slows the whole team down.

You are not just writing code. You are shaping the future of this project. The patterns you establish will be copied. The corners you cut will be cut again.

Fight entropy. Leave the codebase better than you found it.

## Napkin (Persistent Session Notes)

- At the start of every session: read `.claude/napkin.md` silently (do not announce it).
- While working: update `.claude/napkin.md` continuously when you learn something worth not forgetting (errors, surprises, user corrections, patterns that work or don't).
- Log your own mistakes too, not just user corrections.
