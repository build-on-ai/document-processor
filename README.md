# Document Processor

A modern desktop application for parsing and processing documents (PDF, DOCX, TXT) with image extraction and approximate text context.

Built with **Rust + Tauri + Svelte** for maximum performance and minimal footprint.

## Features

- **Multi-format parsing**: PDF, DOCX, TXT (`.doc` files are accepted but routed through the DOCX parser — pure legacy `.doc` binary support is not implemented; RTF is not supported)
- **Image extraction with approximate context**: Images are extracted together with nearby text, estimated from the image's order in the document — see [Image Context](#image-context) for exactly what is and isn't captured
- **Document classification**: Automatic detection of document types (umowa, pozew, ustawa, etc.)
- **Watch folder**: remembers a target folder for manual re-scan — new files are **not** picked up automatically; you re-run the scan yourself
- **SQLite database**: Fast search and organization
- **Hybrid search**: SQLite FTS5 lexical search, optionally fused with embeddings from a local Ollama instance; degrades to lexical-only when Ollama isn't running — see [Search](#search)
- **Modern UI**: Dark theme, responsive design
- **Linux desktop**: built and distributed for Linux (deb / AppImage); the Tauri toolchain compiles on Windows + macOS but no installers are produced by the default bundle config — adding `nsis` / `dmg` targets to `tauri.conf.json` is welcomed via a PR

## Installation

### Prerequisites

#### Linux (Ubuntu/Debian)
```bash
sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev
```

Verify the host before building — `bin/preflight` checks Node, Rust and
every Tauri system library, reports the complete list of what is missing
with a `fix:` line for each, and changes nothing:

```bash
bin/preflight        # exit 0 = safe to build, exit 1 = something missing
```

On newer Ubuntu `libappindicator3-dev` may be packaged as
`libayatana-appindicator3-dev`; preflight accepts either.

#### Windows (build-from-source only — no shipped installer)
The Tauri toolchain compiles on Windows but `tauri.conf.json` only
defines Linux bundle targets, so `npm run tauri build` produces a
debug executable rather than a packaged installer. To get an `.msi`
or `.exe`, add `nsis` / `msi` to `bundle.targets` in a fork.
Prerequisites if you want to try:

- Install [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)
- Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)

### Build from source

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
cd document-processor
npm install
npm run tauri build
```

### Development

```bash
npm run tauri dev
```

### Running the tests

```bash
npm install
npm run build          # required first — see below
cd src-tauri && cargo test
```

45 tests covering the database layer, the parsers and hybrid search
(FTS5 + embeddings, reciprocal rank fusion, the Ollama circuit breaker).

`npm run build` is only needed when you invoke cargo directly.
`tauri.conf.json` sets `beforeBuildCommand`, so `npm run tauri build`,
`npm run tauri dev` and `./run.sh` all build the frontend for you — the UI
is compiled into the binary, and a packaged install never asks the user
for a build step.

A bare `cargo test` skips that hook. `frontendDist` points at `../dist`
and `tauri::generate_context!` resolves it at compile time, so on a fresh
clone the run dies in the proc macro — `the frontendDist configuration is
set to "../dist" but this path doesn't exist` — before a single test runs.

## Usage

### GUI Application

1. Launch the application
2. Drag & drop documents or click to browse
3. Optionally set a "Watch Folder" to remember a folder for manual re-scan (it does not watch automatically — use "Scan again" to pick up new files)
4. View processed documents with extracted text and images

### Output Structure

Processed documents land under the OS-standard per-app data directory
(resolved via Tauri's path resolver — `$XDG_DATA_HOME/com.buildonai.document-processor`
on Linux, i.e. `~/.local/share/com.buildonai.document-processor` unless
`XDG_DATA_HOME` is set; the platform equivalent elsewhere):

```
<app-data-dir>/
├── documents.db                       # SQLite index of every processed file
└── przetworzone/<document-id>/        # one folder per document
    ├── document.md                    # Human-readable markdown
    ├── document.json                  # Structured data for AI
    ├── images/
    │   ├── img_001.png                # Extracted images (PDF: img_NNN.png;
    │   │                              #   DOCX: original names from word/media/)
    │   └── thumb_img_001.png          # Thumbnails: thumb_<image filename>
    └── original.<ext>                 # Original file copy
```

There is no per-image metadata file on disk — image metadata (context,
position marker, dimensions, paths) lives in `document.json` and in the
`images` table of `documents.db`.

This resolves the same way whether you're running `cargo tauri dev` or
a packaged `.deb`/AppImage install — it does not depend on where the
executable lives. `przetworzone/` is a Polish legacy identifier
(Polish for "processed") — see CONTRIBUTING for the rename policy.

### Image Context

Image context is **approximate**: the position of each image in the text
is *estimated from the image's order in the document* (PDF: object
iteration order; DOCX: order within `word/media/`) — it is **not**
derived from the image's real layout position (see
`src-tauri/src/parser.rs`). Each image record includes:

- `context_before`: up to 200 characters of text before the estimated position
- `context_after`: up to 200 characters after the estimated position — **PDF only**; for DOCX it is always `None`
- `page`: **not tracked** — always `None`
- `position_marker`: an ordering marker (`obj_<n>` for PDF objects, `media_<n>` for DOCX media entries), not a page/position reference
- `ocr_text`, `ai_description`: reserved columns for a future OCR / AI-description pass — **not implemented**, always empty today. Kept in the schema so a later migration doesn't have to add them; don't build against them expecting real values.

## Search

The app has an instant, as-you-type search box: results appear in a
dropdown while you type (debounced), arrow keys + Enter navigate and
open a hit, and opening a result jumps to the matching fragment of the
document (`src/App.svelte`). Under the hood
(`src-tauri/src/search.rs`, `db.rs::search_hybrid`):

- **Lexical**: SQLite FTS5 over ~1000-character text chunks, ranked by
  `bm25()`, with exact-fragment highlighting.
- **Semantic (optional)**: cosine similarity over chunk embeddings from
  a **local Ollama** instance — `POST http://localhost:11434/api/embed`,
  model `nomic-embed-text` (768-dim). The lexical and semantic ranked
  lists are fused via reciprocal rank fusion (RRF).
- **Fail-open**: if Ollama is unreachable or the model isn't pulled, a
  circuit breaker (3 consecutive failures → 60 s cooldown) skips
  embedding calls and search degrades to lexical-only. A document that
  can't be embedded still imports successfully — it's indexed for
  lexical search either way.
- **Local only**: the embedding requests to `localhost:11434` are the
  only network traffic the app generates. No cloud calls.

## Integrations

### Claude Code skills (external)

Two companion Claude Code skills live outside this repo and call the
processor over the command line. They are **not** bundled here — install
them separately in your Claude Code configuration:

- `/parse` — parses a document and extracts text + images.
  ```
  /parse ~/Documents/contract.pdf
  ```
- `/document-upload-analyzer` — analyses document upload methods in a
  web application.
  ```
  /document-upload-analyzer
  ```

## Architecture

```
document-processor/
├── src/                  # Svelte frontend
│   ├── App.svelte       # Main component
│   ├── main.js          # Entry point
│   ├── styles.css       # Global styles (imported by main.js)
│   └── app.css          # Component styles (imported by App.svelte)
├── src-tauri/
│   └── src/
│       ├── main.rs      # Tauri entry point
│       ├── parser.rs    # Document parsing logic
│       ├── db.rs        # SQLite database
│       └── search.rs    # Hybrid FTS5 + embedding search
└── package.json
```

## Technologies

- **Backend**: Rust (lopdf, pdf-extract, image, rusqlite)
- **Frontend**: Svelte 5
- **Framework**: Tauri 2
- **Database**: SQLite (rusqlite with bundled)
- **Build**: Vite

## License

Dual-licensed:

- **AGPLv3** for open source, personal, and internal use — see [LICENSE](LICENSE).
- **Commercial license** for SaaS, embedded use, or proprietary modifications — see [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md).
