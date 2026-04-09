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

#[tokio::test]
async fn get_disabled_returns_405() {
    let h = make_handler();
    let post = h.handle(post_req(3)).await;
    assert_eq!(post.status.as_u16(), 201);
    let id = id_from_response(&post);

    let resp = h.handle(get_req(&id)).await;
    assert_eq!(resp.status.as_u16(), 405);
}

#[tokio::test]
async fn get_returns_upload_bytes_when_complete() {
    let h = make_download_handler();
    let post = h.handle(post_req(3)).await;
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
        body: Some(Box::new(std::io::Cursor::new(Bytes::from("abc")))),
    };
    let patched = h.handle(patch).await;
    assert_eq!(patched.status.as_u16(), 204);

    let mut got = h.handle(get_req(&id)).await;
    assert_eq!(got.status.as_u16(), 200);
    match &mut got.body {
        TusBody::Bytes(b) => assert_eq!(b.as_ref(), b"abc"),
        TusBody::Reader(r) => {
            let mut buf = Vec::new();
            r.read_to_end(&mut buf).await.expect("read download body");
            assert_eq!(buf, b"abc");
        }
    }
}
