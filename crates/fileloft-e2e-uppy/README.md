# fileloft-e2e-uppy

Manual and automated end-to-end checks for **fileloft** with [Uppy](https://uppy.io/) + `@uppy/tus` and the filesystem store.

## Run the demo server

```bash
cargo run -p fileloft-e2e-uppy
```

Opens **http://0.0.0.0:3000** (or your machine’s IP on port 3000). Uploads are stored under `./uploads/` (created if missing).

## Headless E2E test (chromedriver)

Requires [Chrome](https://www.google.com/chrome/) and [chromedriver](https://chromedriver.chromium.org/) on your `PATH` (default WebDriver URL: `http://127.0.0.1:9515`).

```bash
chromedriver --port=9515
# other terminal:
cargo test -p fileloft-e2e-uppy -- --ignored
```

Override the WebDriver URL:

```bash
WEBDRIVER_URL=http://127.0.0.1:4444 cargo test -p fileloft-e2e-uppy -- --ignored
```

The test drives Uppy in headless Chrome, uploads `test_fixtures/hello.txt`, then asserts on-disk data, `UploadInfo` JSON, and tus `HEAD` headers.
