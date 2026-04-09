# fileloft-e2e-uppy

Manual and automated end-to-end checks for **fileloft** with [Uppy](https://uppy.io/) + `@uppy/tus` and the filesystem store.

## Vendored Uppy (no CDN at runtime)

The web UI does **not** load Uppy from a CDN. `npm run build` bundles `@uppy/core`, `@uppy/dashboard`, and `@uppy/tus` into `static/vendor/`; the Axum app serves `/assets/uppy-e2e.js` and CSS from those files so tests are deterministic offline.

**Regenerate after changing `package.json` or `e2e-entry.mjs`:**

```bash
cd crates/fileloft-e2e-uppy
npm ci
npm run build
```

Commit `static/vendor/*` and `package-lock.json` so CI and other machines need only `cargo test` (no npm).

## Run the demo server (manual validation)

From the **repository root** (builds assets first):

```bash
make e2e-server
```

Or run Cargo directly (ensure assets are built if you changed JS):

```bash
cargo run -p fileloft-e2e-uppy
```

Listens on **0.0.0.0:3000**. Open [http://localhost:3000](http://localhost:3000) in a browser, add files in the Uppy Dashboard, and confirm uploads land under `./uploads/` (created next to the process working directory). That directory is listed in the root `.gitignore` when created at the repo root.

## Automated headless E2E tests

Tests live in `tests/e2e.rs`, are marked **`#[ignore]`**, and use [ChromeDriver](https://chromedriver.chromium.org/) (via [fantoccini](https://crates.io/crates/fantoccini)) to drive headless Chrome.

**Requirements**

- [Google Chrome](https://www.google.com/chrome/) installed.
- **ChromeDriver** on your `PATH`, with a **major version that matches Chrome** (e.g. Chrome 147 → ChromeDriver 147). If they differ, `SessionNotCreated` / version mismatch errors appear when the test opens a session.

Check both:

```bash
# macOS
/Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome --version
chromedriver --version
```

If you see *“ChromeDriver only supports Chrome version X”* but *“Current browser version is Y”*, either **upgrade/downgrade Chrome** or **install the matching ChromeDriver** for your installed Chrome’s major version — you only need them to agree, not a specific version.

**Option A:** Update Chrome until its major version matches `chromedriver --version`.

**Option B:** Replace ChromeDriver with the build that matches your Chrome major version ([Chrome for Testing](https://googlechromelabs.github.io/chrome-for-testing/) lists pairs).

**Recommended: Makefile (starts ChromeDriver for you)**

From the repository root:

```bash
make test-e2e
```

This runs `e2e-assets` (`npm ci` + `npm run build`), then executes `scripts/run-e2e-tests.sh`, which:

1. If nothing is listening on the WebDriver port (default **9515**), starts `chromedriver --port=9515` in the background and stops it when the test run finishes.
2. If something already listens on that port (e.g. you started ChromeDriver yourself), it **reuses** it and does not stop it afterward.
3. Runs `cargo test -p fileloft-e2e-uppy -- --ignored`.

**Manual: run ChromeDriver yourself**

```bash
chromedriver --port=9515
cargo test -p fileloft-e2e-uppy -- --ignored
```

**Override the WebDriver URL or port**

```bash
make test-e2e CHROMEDRIVER_PORT=4444
# or
WEBDRIVER_URL=http://127.0.0.1:4444 cargo test -p fileloft-e2e-uppy -- --ignored
```

### What the tests cover

- **Small file** — uploads `test_fixtures/hello.txt` and checks on-disk bytes, `UploadInfo`, and tus `HEAD` headers.
- **Chunked upload** — uploads `test_fixtures/logo.png` (~90 KiB), which is larger than the 64 KiB Tus chunk size in `e2e-entry.mjs`, so multiple PATCH requests exercise resumable chunking.

### Optional env (timeouts / WebDriver)

These are read by `tests/e2e.rs` (defaults are generous for slow CI):

| Variable | Role |
| --- | --- |
| `E2E_WEBDRIVER_PAGE_LOAD_SEC` | WebDriver page load timeout (default 300) |
| `E2E_WEBDRIVER_SCRIPT_SEC` | WebDriver script timeout (default 300) |
| `E2E_FILE_INPUT_TIMEOUT_SEC` | Wait for Uppy’s file input (default 120) |
| `E2E_UPLOAD_TIMEOUT_SEC` | Wait for upload completion marker (default 180) |

`cargo test` may print that a test *has been running for over 60 seconds*; that is libtest’s notice, not a hard limit.

### macOS: Gatekeeper on `chromedriver`

If the driver is quarantined:

```bash
xattr -d com.apple.quarantine "$(which chromedriver)"
```

Or allow it under **System Settings → Privacy & Security**.
