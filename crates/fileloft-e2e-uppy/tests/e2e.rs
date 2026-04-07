//! Headless browser E2E: requires `chromedriver` (default `http://127.0.0.1:9515`).
//!
//! ```text
//! chromedriver --port=9515 &
//! cargo test -p fileloft-e2e-uppy -- --ignored
//! ```

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use fantoccini::{ClientBuilder, Locator};
use fileloft_core::info::UploadInfo;
use serde_json::json;

const EXPECTED_BYTES: &[u8] = b"Hello, tus!\n";

#[tokio::test]
#[ignore = "requires chromedriver (e.g. chromedriver --port=9515)"]
async fn uppy_upload_via_tus_then_verify_disk_and_head() {
    let _ = tracing_subscriber::fmt::try_init();

    let tmp = tempfile::tempdir().expect("tempdir");
    let (addr, _server) =
        fileloft_e2e_uppy::start_server(tmp.path().to_path_buf(), 0, Ipv4Addr::LOCALHOST.into())
            .await
            .expect("start server");

    let webdriver =
        std::env::var("WEBDRIVER_URL").unwrap_or_else(|_| "http://127.0.0.1:9515".to_string());

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
                "--window-size=1280,900"
            ]
        }),
    );

    let client = ClientBuilder::native()
        .capabilities(caps)
        .connect(&webdriver)
        .await
        .expect("connect WebDriver; start chromedriver first");

    let base = format!("http://127.0.0.1:{}", addr.port());
    client.goto(&base).await.expect("goto page");

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_fixtures/hello.txt");
    let fixture_str = fixture.to_string_lossy().to_string();

    let file_input = client
        .find(Locator::Css("input[type='file']"))
        .await
        .expect("file input");

    let script = r#"
        const el = arguments[0];
        el.style.display = 'block';
        el.style.visibility = 'visible';
    "#;
    let el_arg = serde_json::to_value(&file_input).expect("serialize WebElement");
    client
        .execute(script, vec![el_arg])
        .await
        .expect("make file input visible");

    file_input
        .send_keys(&fixture_str)
        .await
        .expect("attach file");

    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if client
            .find(Locator::Css("#upload-status.complete"))
            .await
            .is_ok()
        {
            break;
        }
        if Instant::now() > deadline {
            panic!("timeout waiting for #upload-status.complete");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    let status_el = client
        .find(Locator::Css("#upload-status"))
        .await
        .expect("status");
    let upload_url = status_el
        .attr("data-upload-url")
        .await
        .expect("attr")
        .expect("data-upload-url must be set after upload");
    let upload_id = upload_url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .expect("upload id segment");

    let data_path = tmp.path().join(upload_id);
    let got = std::fs::read(&data_path).expect("read final data file");
    assert_eq!(
        got.as_slice(),
        EXPECTED_BYTES,
        "on-disk data must match fixture"
    );

    let info_path = tmp.path().join(format!("{upload_id}.info"));
    let info_json = std::fs::read_to_string(&info_path).expect("read info");
    let info: UploadInfo = serde_json::from_str(&info_json).expect("parse UploadInfo");
    assert_eq!(info.size, Some(13));
    assert_eq!(info.offset, 13);
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

    let client_http = reqwest::Client::new();
    let head = client_http
        .head(format!("{base}/files/{upload_id}"))
        .header("Tus-Resumable", "1.0.0")
        .send()
        .await
        .expect("HEAD request");

    assert_eq!(head.status(), reqwest::StatusCode::OK);
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
    assert_eq!(off, "13");
    assert_eq!(len, "13");

    client.close().await.expect("close webdriver session");
}
