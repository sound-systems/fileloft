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

## Run the demo server

```bash
cargo run -p fileloft-e2e-uppy
```

Listens on **0.0.0.0:3000**. Uploads go under `./uploads/` (created if missing).

## Headless E2E test (chromedriver)

Requires [Chrome](https://www.google.com/chrome/) and a **matching** [ChromeDriver](https://chromedriver.chromium.org/) (same major version as Chrome). Default WebDriver URL: `http://127.0.0.1:9515`.

```bash
chromedriver --port=9515
cargo test -p fileloft-e2e-uppy -- --ignored
```

### macOS: “malware” / Gatekeeper on chromedriver

If the browser driver is quarantined:

```bash
xattr -d com.apple.quarantine "$(which chromedriver)"
```

Or allow it under **System Settings → Privacy & Security**.

### WebDriver URL

```bash
WEBDRIVER_URL=http://127.0.0.1:4444 cargo test -p fileloft-e2e-uppy -- --ignored
```

The test drives Uppy in headless Chrome, uploads `test_fixtures/hello.txt`, then asserts on-disk data, `UploadInfo` JSON, and tus `HEAD` headers.
