# tools/

External Python harnesses that exercise the packaged Tauri app from outside the Rust process. **Not** Rust unit tests (those live in the `#[cfg(test)]` modules of `src-tauri/src/parser.rs` (17), `db.rs` (9) and `search.rs` (19) — 45 in total) and **not** part of `npm run build` or `cargo test`.

| File | What it does |
|---|---|
| `upload_tester.py` | Enumerates every upload / import path the app could expose (file picker, drag&drop, watch-folder re-scan, ZIP, CSV-of-URLs, HTTP, cloud-storage stubs, CLI) and emits a PASS / FAIL / MANUAL / SKIP report — but it does **not** drive the app or verify anything automatically: it generates sample files, step-by-step manual instructions and a JSON report for a human to work through. Long form of "did our upload story regress?". |
| `test_upload.py` | Minimal checklist generator: creates synthetic PDF / DOCX / TXT samples plus a test folder, then emits manual instructions + a JSON report. It does **not** verify processing itself — you set the folder as the watch folder, trigger a manual re-scan in the app, and check that output appears under the app data dir (Linux: `~/.local/share/com.buildonai.document-processor/przetworzone/`). Quick "is the happy path alive". |

Run either as a regular Python 3.10+ script — no extra requirements:

```bash
python3 tools/upload_tester.py /path/to/release-binary-dir
python3 tools/test_upload.py    /path/to/release-binary-dir
```

The CI workflow does not run these (they need a built app + a real filesystem to drive). Treat them as maintainer-side regression scripts.
