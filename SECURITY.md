# Security

document-processor is a **local desktop application** (Tauri 2 +
Rust + Svelte) that parses PDF/DOCX/TXT files the user points it
at, extracts images with surrounding text, classifies document
types, and stores the result in a local SQLite database.

## Intended deployment

Single-user, on the operator's own machine. Input files are
chosen by the user. All processing and storage stay local — the
app does not upload parsed content anywhere by default.

## Trust boundaries

| Zone | Trust | Notes |
|---|---|---|
| Input files (PDF/DOCX/TXT; .doc routed through DOCX parser) | **Untrusted** | Any user-supplied file is parsed. Parser bugs can crash or, in the worst case, let an attacker exploit memory corruption in `pdf-extract` / `lopdf` / `image` crates. We rely on the Rust ecosystem and Tauri's sandbox for containment. |
| Local filesystem | Trusted within the paths the user opens | Tauri's `fs` plugin scopes access. The app does not walk the filesystem beyond what the user selects or the configured watch folder. |
| Network | None by default | No telemetry, no analytics, no automatic updates. If you add a plugin that phones home, that is explicitly your addition. |
| SQLite database | Trusted | Stored on the user's disk under the app's config dir. Treat it as personal data. |

## Deliberate trade-offs (not bugs)

### Parsers run on untrusted input

The whole point of the app is to parse files the user gives it.
Parsers can be surprised by malformed input. We use maintained
Rust crates (`pdf-extract`, `lopdf`, `image`) and rebuild on
upstream security releases, but a zero-day in one of these crates
affects this app.

**Mitigation when worried:** run inside a VM or container when
processing files from untrusted sources. Tauri's own webview
sandbox does not protect the Rust backend from memory corruption
in native parsers.

### Webview CSP is a baseline, not a lockdown

`src-tauri/tauri.conf.json` sets a baseline `default-src 'self'`
CSP that also allows Tauri's `ipc:` / `http://ipc.localhost` and
`asset:` / `http://asset.localhost` schemes (the Tauri 2 IPC and
asset protocols), `data:` URIs for images (icons + extracted
thumbnails), and `'unsafe-inline'` for stylesheets because Svelte
5 still emits some inline styles at runtime. This is a noticeable
improvement over the previous `csp: null` (no policy at all) but
**not** a hardened lockdown — `'unsafe-inline'` in `style-src`
leaves CSS-injection avenues if a future change ever lets
untrusted text reach a style context. If you tighten this in your
fork, remove `'unsafe-inline'` and verify the UI renders correctly
in `tauri build --release`.

### Watch folder opens a TOCTOU window

The watch-folder feature processes any file that lands in a
configured directory. If an attacker can write to that directory,
they can feed arbitrary input to the parsers. **Point the watch
folder at something only you write to.**

### No code signing on the built binaries

The `tauri build` output is not code-signed by default. A user
that downloads a release without signature verification trusts the
distribution channel (GitHub). Consider signing the release
binaries with your own key if you ship builds to other users.

## Reporting a vulnerability

Please report privately, not in public GitHub issues.

- **Email:** buildonai.tm@gmail.com
- **GitHub Security Advisory:** https://github.com/build-on-ai/document-processor/security/advisories/new

Include:

1. A clear description of the issue and its impact (crash vs
   memory corruption vs data leak vs escape).
2. A minimal reproducer — ideally the input file or a recipe for
   generating it.
3. Your preferred credit / disclosure terms.

Expect acknowledgement within a few business days. Fixes for
high-severity issues (memory corruption reachable from a crafted
document, information disclosure, escape from Tauri sandbox) are
prioritised.

## Hardening checklist for operators

If you process files that might come from untrusted sources:

- [ ] Run the app inside a VM, Flatpak, or `firejail` sandbox.
- [ ] Never set the watch folder to a directory writable by
      other users or services (e.g. a shared dropbox).
- [ ] Keep Rust toolchain and dependencies up to date
      (`cargo update` + `npm update`).
- [ ] Back up the SQLite database separately — it accumulates
      parsed content over time.
- [ ] If building releases for others, sign the binaries.
