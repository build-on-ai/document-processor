# Contributing to document-processor

Thanks for your interest! This document covers the parts of contributing that are specific to **document-processor** (Rust + Tauri 2 + Svelte 5). For the legal side see [`CLA.md`](CLA.md); for the security posture see [`SECURITY.md`](SECURITY.md).

## Quick start

```bash
# System deps (Ubuntu/Debian — see README for Windows)
sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev

# Rust toolchain (any 1.70+ works; CI pins to stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Frontend deps
npm ci

# Dev loop (Tauri shell + Svelte HMR)
npm run tauri dev

# Or via the launcher
./run.sh dev
```

`./run.sh install-deps` runs the apt + npm install in one go.

## Contributor License Agreement (CLA)

Before your PR can be merged, you must sign the [document-processor CLA](CLA.md). The [CLA Assistant bot](https://cla-assistant.io/) will prompt you on your first PR — one click, one time, all your future contributions to this repo are covered.

**Why a CLA?** document-processor is dual-licensed (AGPLv3 + commercial). To offer commercial licenses, the project must hold the rights to all contributed code. The CLA does **not** transfer ownership of your code — you retain copyright; you simply grant the maintainer the right to license the project (including your contributions) under multiple licenses.

## Repo layout

```
src/                — Svelte 5 frontend (App.svelte, main.js, styles.css + app.css)
src-tauri/          — Rust backend
  src/main.rs       — Tauri commands + AppState wiring
  src/parser.rs     — PDF/DOCX/TXT parsing + image context extraction
  src/db.rs         — SQLite persistence (rusqlite, bundled) + hybrid search queries
  src/search.rs     — Hybrid FTS5 + embedding search (local Ollama, circuit breaker)
  tauri.conf.json   — Window/bundle/CSP config
  capabilities/default.json — Tauri 2 capability grants for the main window
  Cargo.toml        — Rust deps (pinned)
  Cargo.lock        — tracked (desktop app, deterministic builds)
package.json        — Vite + Svelte + @tauri-apps/* deps
vite.config.js      — port 1420 dev server (Tauri expects 1420)
run.sh / uruchom.sh — launchers (English / Polish wrappers)
.env.example        — env vars the launcher / Tauri honour
```

## What to contribute

Welcomed:

- **Parsers for new formats** — propose in an issue first so we can pick the right crate (DOC/RTF/ODT all have several options with different tradeoffs).
- **Image-context improvements** — better OCR, smarter neighbour-text capture.
- **Watch-folder behaviour** — today the "watch folder" is a remembered path that the user re-scans manually (`scan_folder` / `scan_folder_force` in `src/main.rs`); a real filesystem watcher (with debouncing, batching, recursion controls) would be a new feature — propose it in an issue first.
- **UI fixes** — accessibility, keyboard navigation, dark/light theming.
- **Tests** — Rust unit tests in `src-tauri/src/parser.rs` (see the existing test module), or end-to-end harnesses driving Tauri commands.

Cautious about:

- **New heavy dependencies** — every Rust crate is a supply-chain attack surface; every npm dep ships with `npm audit` baggage. Justify in the issue.
- **Filesystem walking outside the user's selection or watch folder** — SECURITY.md is explicit that the app does not roam the disk.
- **CSP relaxation** — `tauri.conf.json` controls the renderer's CSP. Tightening is welcome; loosening needs a strong reason in the PR description.

## Local checks before opening a PR

```bash
# Rust: compile + lint + test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test  --manifest-path src-tauri/Cargo.toml

# Frontend: type/syntax + production build
npm ci
npm run build

# Dependency audits
npm audit --audit-level=high
cargo audit            # cargo install cargo-audit once
```

CI (`.github/workflows/ci.yml`) currently runs a **subset** of this: the Rust build + tests (with the frontend built first, since Tauri's build script needs `dist/`) and the frontend build + `npm audit`. `cargo fmt`, `cargo clippy` and `cargo audit` are **not** enforced in CI yet (see the comment in `ci.yml` — pending a dedicated style-pass PR), so run them locally anyway to keep the tree ready for when they're flipped on.

## Style

- **Rust:** rustfmt defaults, clippy clean at `--deny warnings`. Comments explain *why*; code explains *what*.
- **Svelte / JS:** match the Svelte 5 runes (`$state`, `$derived`) already in `src/App.svelte`. No new global state stores without an issue.
- **No private data in commits** — no real document samples, no machine paths under `/home/$user/`, no API tokens. `.gitignore` already excludes `dane/`, `sekrety/`, `node_modules/`, `src-tauri/target/`.

## Polish ↔ English

This repo predates the open-sourcing and carries a few Polish identifiers (`uruchom.sh`, and the `przetworzone/` and `download/` directories that `parser.rs` creates under the app data dir; `dane/` survives only as a `.gitignore` entry — nothing creates it anymore). New contributions should be English-first; if you rename one of the legacy Polish identifiers, do it as a standalone refactor PR (not bundled with a feature change) so the diff is easy to review.

## Reporting bugs / security issues

- **Bugs:** open an issue with a reproducer (input file format + size, OS, Rust version, the line from the app's stdout/stderr that mentions the failure).
- **Security:** do **not** open a public issue. Email the maintainer via [github.com/build-on-ai](https://github.com/build-on-ai). We aim to respond within 7 days. SECURITY.md lists the kinds of issues that take priority.

## PR checklist

- [ ] CLA signed (CLA Assistant will check automatically)
- [ ] Branch from `main`, rebased on the latest `main`
- [ ] `cargo fmt --check` / `cargo clippy -- -D warnings` / `cargo test` all pass
- [ ] `npm run build` succeeds with no new warnings
- [ ] `npm audit --audit-level=high` clean (or the new advisory has a written rationale in the PR description)
- [ ] PR description explains *what* and *why*
- [ ] No private data in commits

## License

By contributing, you agree that your contributions will be licensed under the project's dual license (AGPLv3 + commercial), as described in the [CLA](CLA.md).

---

Questions? Open a [Discussion](https://github.com/build-on-ai/document-processor/discussions) or file an issue.
