//! GET download (when enabled).

use bytes::Bytes;
use fileloft_core::config::{Config, CorsConfig};
use fileloft_core::handler::{TusBody, TusRequest};
use fileloft_core::proto::*;
use http::Method;
use tokio::io::AsyncReadExt;

mod helpers;
use helpers::*;

fn make_download_handler() -> std::sync::Arc<TestHandler> {
    make_handler_with_config(Config {
        enable_download: true,
        cors: CorsConfig {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    })
}

fn get_req(id: &str) -> TusRequest {
    TusRequest {
        method: Method::GET,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id.to_string()),
        headers: tus_headers(),
        body: None,
    }
}

/// Read the body bytes regardless of TusBody variant.
async fn read_body(body: &mut TusBody) -> Vec<u8> {
    match body {
        TusBody::Bytes(b) => b.to_vec(),
        TusBody::Reader(r) => {
            let mut buf = Vec::new();
            r.read_to_end(&mut buf).await.expect("read download body");
            buf
        }
    }
}

/// Upload `data` to a fresh upload and return its ID. Caller must use a handler
/// whose Config has `enable_download: true` and creation enabled.
async fn create_complete_upload(h: &std::sync::Arc<TestHandler>, data: &[u8]) -> String {
    let post = h.handle(post_req(data.len() as u64)).await;
    assert_eq!(post.status.as_u16(), 201);
    let id = id_from_response(&post);

    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_OFFSET, "0".parse().unwrap());
    headers.insert(HDR_CONTENT_TYPE, CONTENT_TYPE_OCTET_STREAM.parse().unwrap());
    let patch = TusRequest {
        method: Method::PATCH,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id.clone()),
        headers,
        body: Some(Box::new(std::io::Cursor::new(Bytes::copy_from_slice(data)))),
    };
    let patched = h.handle(patch).await;
    assert_eq!(patched.status.as_u16(), 204);
    id
}

// ---------------------------------------------------------------------------
// Feature gate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_disabled_returns_405() {
    let h = make_handler();
    let post = h.handle(post_req(3)).await;
    assert_eq!(post.status.as_u16(), 201);
    let id = id_from_response(&post);

    let resp = h.handle(get_req(&id)).await;
    assert_eq!(resp.status.as_u16(), 405);
}

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_returns_upload_bytes_when_complete() {
    let h = make_download_handler();
    let id = create_complete_upload(&h, b"abc").await;

    let mut got = h.handle(get_req(&id)).await;
    assert_eq!(got.status.as_u16(), 200);
    assert_eq!(read_body(&mut got.body).await, b"abc");
}

#[tokio::test]
async fn get_response_has_correct_content_headers() {
    let h = make_download_handler();
    let data = b"hello world";
    let id = create_complete_upload(&h, data).await;

    let got = h.handle(get_req(&id)).await;
    assert_eq!(got.status.as_u16(), 200);

    let ct = got
        .headers
        .get(HDR_CONTENT_TYPE)
        .expect("missing Content-Type")
        .to_str()
        .unwrap();
    assert_eq!(ct, "application/octet-stream");

    let cl = got
        .headers
        .get(HDR_CONTENT_LENGTH)
        .expect("missing Content-Length")
        .to_str()
        .unwrap();
    assert_eq!(cl, data.len().to_string());
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_incomplete_upload_returns_400() {
    let h = make_download_handler();
    let post = h.handle(post_req(100)).await;
    assert_eq!(post.status.as_u16(), 201);
    let id = id_from_response(&post);

    let resp = h.handle(get_req(&id)).await;
    assert_eq!(
        resp.status.as_u16(),
        400,
        "incomplete upload GET should be 400"
    );
}

#[tokio::test]
async fn get_nonexistent_upload_returns_404() {
    let h = make_download_handler();
    let resp = h.handle(get_req("does-not-exist")).await;
    assert_eq!(resp.status.as_u16(), 404);
}

#[tokio::test]
async fn get_deleted_upload_returns_404() {
    let h = make_download_handler();
    let id = create_complete_upload(&h, b"temp").await;

    let del = h.handle(delete_req(&id)).await;
    assert_eq!(del.status.as_u16(), 204);

    let resp = h.handle(get_req(&id)).await;
    assert_eq!(resp.status.as_u16(), 404);
}

// ---------------------------------------------------------------------------
// Download with larger payload
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_large_upload() {
    let h = make_download_handler();
    let data: Vec<u8> = (0..8192).map(|i| (i % 256) as u8).collect();
    let id = create_complete_upload(&h, &data).await;

    let mut got = h.handle(get_req(&id)).await;
    assert_eq!(got.status.as_u16(), 200);
    assert_eq!(read_body(&mut got.body).await, data);
}
