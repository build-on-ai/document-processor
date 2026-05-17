# tools/

External Python harnesses that exercise the packaged Tauri app from outside the Rust process. **Not** Rust unit tests (those live in `src-tauri/src/parser.rs`'s `#[cfg(test)]` module) and **not** part of `npm run build` or `cargo test`.

| File | What it does |
|---|---|
| `upload_tester.py` | Walks every upload / import path the app exposes (file picker, drag&drop, watch folder, ZIP, CSV-of-URLs, HTTP, cloud-storage stubs, CLI) and emits a PASS / FAIL / MANUAL / SKIP report. Long form of "did our upload story regress?". |
| `test_upload.py` | Minimal smoke that drops a synthetic PDF / DOCX / TXT into the watch folder and confirms the app produces an entry under `dane/przetworzone/`. Quick "is the happy path alive". |

Run either as a regular Python 3.10+ script — no extra requirements:

```bash
python3 tools/upload_tester.py /path/to/release-binary-dir
python3 tools/test_upload.py    /path/to/release-binary-dir
```

The CI workflow does not run these (they need a built app + a real filesystem to drive). Treat them as maintainer-side regression scripts.
