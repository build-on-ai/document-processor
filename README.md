# Document Processor

A modern desktop application for parsing and processing documents (PDF, DOCX, TXT) with intelligent image context extraction.

Built with **Rust + Tauri + Svelte** for maximum performance and minimal footprint.

## Features

- **Multi-format parsing**: PDF, DOCX, TXT (`.doc` files are accepted but routed through the DOCX parser — pure legacy `.doc` binary support is not implemented; RTF is not supported)
- **Image extraction with context**: Images are extracted with surrounding text preserved
- **Document classification**: Automatic detection of document types (umowa, pozew, ustawa, etc.)
- **Watch folder**: Automatic processing of new documents
- **SQLite database**: Fast search and organization
- **Modern UI**: Dark theme, responsive design
- **Linux desktop**: built and distributed for Linux (deb / AppImage); the Tauri toolchain compiles on Windows + macOS but no installers are produced by the default bundle config — adding `nsis` / `dmg` targets to `tauri.conf.json` is welcomed via a PR

## Installation

### Prerequisites

#### Linux (Ubuntu/Debian)
```bash
sudo apt install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev
```

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

## Usage

### GUI Application

1. Launch the application
2. Drag & drop documents or click to browse
3. Set a "Watch Folder" for automatic processing
4. View processed documents with extracted text and images

### Output Structure

Processed documents land under the app's data directory:

```
<project-root>/dane/
├── documents.db                       # SQLite index of every processed file
└── przetworzone/<document-id>/        # one folder per document
    ├── document.md                    # Human-readable markdown
    ├── document.json                  # Structured data for AI
    ├── images/
    │   ├── img_001.png                # Extracted images
    │   ├── img_001.json               # Image metadata + context
    │   └── thumb_001.png              # Thumbnails
    └── original.<ext>                 # Original file copy
```

`<project-root>` is the directory the launcher selects on startup (the
working directory under development; `/opt/document-processor` on a
packaged install). `dane/` and `przetworzone/` are Polish legacy
identifiers (Polish for "data" and "processed") — see CONTRIBUTING for
the rename policy.

### Image Context

Each image includes:
- `context_before`: 200 characters of text before the image
- `context_after`: 200 characters after
- `position_marker`: Page and position reference
- `ocr_text`: Text extracted from image (if applicable)
- `ai_description`: AI-generated description (when available)

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
│   └── styles.css       # Global styles
├── src-tauri/
│   └── src/
│       ├── main.rs      # Tauri entry point
│       ├── parser.rs    # Document parsing logic
│       ├── db.rs        # SQLite database
│       └── watcher.rs   # Folder watching
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
