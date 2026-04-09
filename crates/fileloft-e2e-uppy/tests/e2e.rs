//! Headless browser E2E: requires `chromedriver` (default `http://127.0.0.1:9515`).
//!
//! ```text
//! chromedriver --port=9515 &
//! cargo test -p fileloft-e2e-uppy -- --ignored
//! ```
//!
//! Optional env (defaults are generous for slow CI):
//! - `E2E_WEBDRIVER_PAGE_LOAD_SEC` / `E2E_WEBDRIVER_SCRIPT_SEC` — WebDriver page load & script timeouts (default 300).
//! - `E2E_FILE_INPUT_TIMEOUT_SEC` — wait for the hidden file input (default 120).
//! - `E2E_UPLOAD_TIMEOUT_SEC` — wait for `#upload-status.complete` (default 180).
//!
//! `cargo test` may print *"has been running for over 60 seconds"*; that is libtest's notice, not a
//! hard limit (unless an external harness enforces one).

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fantoccini::wd::TimeoutConfiguration;
use fantoccini::{Client, ClientBuilder, Locator};
use fileloft_core::info::UploadInfo;
use serde_json::json;
use tokio::task::JoinHandle;

// ── helpers ─────────────────────────────────────────────────────────────────

fn duration_from_env_secs(name: &str, default_secs: u64) -> Duration {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(default_secs))
}

async fn wait_for_http_ok(url: &str, overall: Duration) -> Result<(), String> {
    let deadline = Instant::now() + overall;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| e.to_string())?;
    while Instant::now() < deadline {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    Err(format!(
        "GET {url} did not return success within {:?}",
        overall
    ))
}

/// Uppy Dashboard injects a hidden `files[]` input after async plugin init.
const FILE_INPUT_SELECTORS: &[&str] = &[
    "input.uppy-Dashboard-input[name='files[]']",
    "input[name='files[]']",
    "#uppy-dashboard input[type='file']",
    "input[type='file']",
];

struct TestHarness {
    base: String,
    tmp: tempfile::TempDir,
    _server: JoinHandle<()>,
    client: Client,
}

impl TestHarness {
    async fn start() -> Self {
        let _ = tracing_subscriber::fmt::try_init();

        let tmp = tempfile::tempdir().expect("tempdir");
        let (addr, server) = fileloft_e2e_uppy::start_server(
            tmp.path().to_path_buf(),
            0,
            Ipv4Addr::LOCALHOST.into(),
        )
        .await
        .expect("start server");

        let webdriver =
            std::env::var("WEBDRIVER_URL").unwrap_or_else(|_| "http://127.0.0.1:9515".to_string());

        let base = format!("http://127.0.0.1:{}", addr.port());
        wait_for_http_ok(&base, Duration::from_secs(30))
            .await
            .expect("server should accept HTTP before WebDriver opens the page");

        let page_load = duration_from_env_secs("E2E_WEBDRIVER_PAGE_LOAD_SEC", 300);
        let wd_script_timeout = duration_from_env_secs("E2E_WEBDRIVER_SCRIPT_SEC", 300);

        let mut caps = serde_json::Map::new();
        caps.insert("browserName".to_string(), json!("chrome"));
        caps.insert(
            "goog:chromeOptions".to_string(),
            json!({
                "args": [
                    "--headless=new",
                    "--no-sandbox",
                    "--disable-dev-shm-usage",
                    "--disable-gpu",
                    "--window-size=1280,900",
                    "--remote-allow-origins=*"
                ]
            }),
        );

        let client = ClientBuilder::native()
            .capabilities(caps)
            .connect(&webdriver)
            .await
            .expect("connect WebDriver; start chromedriver first");

        client
            .update_timeouts(TimeoutConfiguration::new(
                Some(wd_script_timeout),
                Some(page_load),
                None,
            ))
            .await
            .expect("set WebDriver timeouts");

        client.goto(&base).await.expect("goto page");

        Self {
            base,
            tmp,
            _server: server,
            client,
        }
    }

    /// Attach a file to the Uppy Dashboard and wait for the upload to finish.
    /// Returns the upload ID extracted from the `data-upload-url` attribute.
    async fn upload_file(&self, fixture: &Path) -> String {
        let file_input_deadline = duration_from_env_secs("E2E_FILE_INPUT_TIMEOUT_SEC", 120);
        let upload_deadline = duration_from_env_secs("E2E_UPLOAD_TIMEOUT_SEC", 180);

        let deadline = Instant::now() + file_input_deadline;
        let file_input = 'found: loop {
            for sel in FILE_INPUT_SELECTORS {
                if let Ok(el) = self.client.find(Locator::Css(sel)).await {
                    break 'found el;
                }
            }
            if Instant::now() > deadline {
                let snippet = match self.client.source().await {
                    Ok(s) => s.chars().take(2500).collect::<String>(),
                    Err(e) => format!("(could not read page source: {e})"),
                };
                panic!("timeout waiting for Uppy file input. Page snippet:\n{snippet}");
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        };

        let script = r#"
            const el = arguments[0];
            el.style.display = 'block';
            el.style.visibility = 'visible';
        "#;
        let el_arg = serde_json::to_value(&file_input).expect("serialize WebElement");
        self.client
            .execute(script, vec![el_arg])
            .await
            .expect("make file input visible");

        file_input
            .send_keys(&fixture.to_string_lossy())
            .await
            .expect("attach file");

        let deadline = Instant::now() + upload_deadline;
        loop {
            if self
                .client
                .find(Locator::Css("#upload-status.complete"))
                .await
                .is_ok()
            {
                break;
            }
            if Instant::now() > deadline {
                let diag = self
                    .client
                    .execute(
                        r#"
                        const el = document.getElementById('upload-status');
                        const state = window.uppy ? JSON.stringify(window.uppy.getState(), null, 2) : '(window.uppy not found)';
                        return JSON.stringify({
                            statusEl: el ? { text: el.textContent, classes: el.className, data: el.dataset } : null,
                            uppyFileCount: window.uppy ? Object.keys(window.uppy.getState().files).length : -1,
                            uppyState: state.substring(0, 3000),
                            consoleLogs: window.__e2eLogs || [],
                        });
                        "#,
                        vec![],
                    )
                    .await
                    .ok();
                let source_snippet = self
                    .client
                    .source()
                    .await
                    .map(|s| s.chars().take(3000).collect::<String>())
                    .unwrap_or_else(|e| format!("(page source error: {e})"));
                panic!(
                    "timeout waiting for #upload-status.complete\n\
                     --- browser diagnostics ---\n{diag:?}\n\
                     --- page source (first 3000 chars) ---\n{source_snippet}"
                );
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        let status_el = self
            .client
            .find(Locator::Css("#upload-status"))
            .await
            .expect("status");
        let upload_url = status_el
            .attr("data-upload-url")
            .await
            .expect("attr")
            .expect("data-upload-url must be set after upload");
        upload_url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .expect("upload id segment")
            .to_string()
    }

    /// Reset the Uppy instance so a second file can be uploaded in the same session.
    #[allow(dead_code)]
    async fn reset_uppy(&self) {
        self.client
            .execute(
                r#"
                if (window.uppy) {
                    window.uppy.clear();
                }
                const el = document.getElementById('upload-status');
                if (el) {
                    el.classList.remove('complete');
                    el.textContent = '';
                    delete el.dataset.uploadUrl;
                }
                "#,
                vec![],
            )
            .await
            .expect("reset uppy state");
    }

    /// Verify the upload on disk and via HEAD.
    async fn verify_upload(&self, upload_id: &str, expected_bytes: &[u8], expected_name: &str) {
        let data_path = self.tmp.path().join(upload_id);
        let got = std::fs::read(&data_path).expect("read final data file");
        assert_eq!(
            got.len(),
            expected_bytes.len(),
            "on-disk file size must match fixture ({expected_name})"
        );
        assert_eq!(
            got.as_slice(),
            expected_bytes,
            "on-disk data must match fixture ({expected_name})"
        );

        let info_path = self.tmp.path().join(format!("{upload_id}.info"));
        let info_json = std::fs::read_to_string(&info_path).expect("read info");
        let info: UploadInfo = serde_json::from_str(&info_json).expect("parse UploadInfo");
        let expected_len = expected_bytes.len() as u64;
        assert_eq!(info.size, Some(expected_len));
        assert_eq!(info.offset, expected_len);

        let client_http = reqwest::Client::new();
        let head = client_http
            .head(format!("{}/files/{}", self.base, upload_id))
            .header("Tus-Resumable", "1.0.0")
            .send()
            .await
            .expect("HEAD request");

        assert!(
            head.status().is_success(),
            "HEAD should succeed, got {}",
            head.status()
        );
        let off = head
            .headers()
            .get("upload-offset")
            .and_then(|v| v.to_str().ok())
            .expect("Upload-Offset");
        let len = head
            .headers()
            .get("upload-length")
            .and_then(|v| v.to_str().ok())
            .expect("Upload-Length");
        let expected_str = expected_len.to_string();
        assert_eq!(off, expected_str);
        assert_eq!(len, expected_str);
    }

    async fn close(self) {
        self.client.close().await.expect("close webdriver session");
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires chromedriver (e.g. chromedriver --port=9515)"]
async fn uppy_upload_small_file() {
    let h = TestHarness::start().await;

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_fixtures/hello.txt");
    let expected = b"Hello, tus!\n";

    let upload_id = h.upload_file(&fixture).await;
    h.verify_upload(&upload_id, expected, "hello.txt").await;

    let info_path = h.tmp.path().join(format!("{upload_id}.info"));
    let info_json = std::fs::read_to_string(&info_path).expect("read info");
    let info: UploadInfo = serde_json::from_str(&info_json).expect("parse UploadInfo");
    let filename_ok = info
        .metadata
        .0
        .get("filename")
        .or_else(|| info.metadata.0.get("name"))
        .and_then(|o| o.as_ref())
        .map(|s| s.contains("hello"))
        .unwrap_or(false);
    assert!(
        filename_ok,
        "metadata should include filename or name for hello.txt: {:?}",
        info.metadata.0
    );

    h.close().await;
}

/// Upload a file larger than `chunkSize` (64 KiB in e2e-entry.mjs) to validate
/// that tus chunked transfers work end-to-end.  The logo is ~91 KB → 2 PATCH
/// requests (64 KB + remainder).
#[tokio::test]
#[ignore = "requires chromedriver (e.g. chromedriver --port=9515)"]
async fn uppy_upload_chunked_image() {
    let h = TestHarness::start().await;

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_fixtures/logo.png");
    let expected = std::fs::read(&fixture).expect("read logo.png fixture");
    assert!(
        expected.len() > 65_536,
        "fixture must be larger than chunkSize (64 KiB) to exercise chunking; got {} bytes",
        expected.len()
    );

    let upload_id = h.upload_file(&fixture).await;
    h.verify_upload(&upload_id, &expected, "logo.png").await;

    let info_path = h.tmp.path().join(format!("{upload_id}.info"));
    let info_json = std::fs::read_to_string(&info_path).expect("read info");
    let info: UploadInfo = serde_json::from_str(&info_json).expect("parse UploadInfo");
    let filename_ok = info
        .metadata
        .0
        .get("filename")
        .or_else(|| info.metadata.0.get("name"))
        .and_then(|o| o.as_ref())
        .map(|s| s.contains("logo"))
        .unwrap_or(false);
    assert!(
        filename_ok,
        "metadata should include filename or name for logo.png: {:?}",
        info.metadata.0
    );

    h.close().await;
}
