mod helpers;
use helpers::*;

use bytes::Bytes;
use fileloft_core::{
    config::Config,
    proto::{HDR_LOCATION, HDR_UPLOAD_LENGTH},
};

#[tokio::test]
async fn post_creates_upload_and_returns_201() {
    let h = make_handler();
    let resp = h.handle(post_req(512)).await;
    assert_eq!(resp.status.as_u16(), 201);
}

#[tokio::test]
async fn post_returns_location_header() {
    let h = make_handler();
    let resp = h.handle(post_req(512)).await;
    assert!(
        resp.headers.contains_key(HDR_LOCATION),
        "Location header missing"
    );
    let location = resp.headers.get(HDR_LOCATION).unwrap().to_str().unwrap();
    assert!(location.contains("/files/"), "unexpected location: {location}");
}

#[tokio::test]
async fn post_returns_zero_offset() {
    let h = make_handler();
    let resp = h.handle(post_req(512)).await;
    assert_eq!(get_offset(&resp), 0);
}

#[tokio::test]
async fn post_missing_tus_resumable_returns_412() {
    let h = make_handler();
    let req = fileloft_core::handler::TusRequest {
        method: http::Method::POST,
        uri: "/files/".parse().unwrap(),
        upload_id: None,
        headers: http::HeaderMap::new(),
        body: None,
    };
    let resp = h.handle(req).await;
    assert_eq!(resp.status.as_u16(), 412);
}

#[tokio::test]
async fn post_missing_upload_length_returns_400() {
    let h = make_handler();
    let mut headers = tus_headers();
    headers.insert("host", "localhost".parse().unwrap());
    let req = fileloft_core::handler::TusRequest {
        method: http::Method::POST,
        uri: "/files/".parse().unwrap(),
        upload_id: None,
        headers,
        body: None,
    };
    let resp = h.handle(req).await;
    assert_eq!(resp.status.as_u16(), 400);
}

#[tokio::test]
async fn post_exceeds_max_size_returns_413() {
    let config = Config {
        max_size: 100,
        ..Default::default()
    };
    let h = make_handler_with_config(config);

    let resp = h.handle(post_req(200)).await;
    assert_eq!(resp.status.as_u16(), 413);
}

#[tokio::test]
async fn post_with_zero_length_creates_complete_upload() {
    let h = make_handler();
    let resp = h.handle(post_req(0)).await;
    assert_eq!(resp.status.as_u16(), 201);

    // HEAD should show offset == 0 == length, i.e. complete
    let id = id_from_response(&resp);
    let head = h.handle(head_req(&id)).await;
    assert_eq!(get_offset(&head), 0);
    let length: u64 = head
        .headers
        .get(HDR_UPLOAD_LENGTH)
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(length, 0);
}

#[tokio::test]
async fn creation_with_upload_writes_initial_chunk() {
    let h = make_handler();
    let data = Bytes::from_static(b"hello world");
    let resp = h.handle(post_req_with_body(11, data)).await;
    assert_eq!(resp.status.as_u16(), 201);
    assert_eq!(get_offset(&resp), 11);
}

#[tokio::test]
async fn post_with_metadata_header() {
    let h = make_handler();
    // filename dGVzdC50eHQ= = "test.txt" in base64
    let mut headers = tus_headers();
    headers.insert(
        fileloft_core::proto::HDR_UPLOAD_LENGTH,
        "10".parse().unwrap(),
    );
    headers.insert(
        fileloft_core::proto::HDR_UPLOAD_METADATA,
        "filename dGVzdC50eHQ=".parse().unwrap(),
    );
    headers.insert("host", "localhost".parse().unwrap());
    let req = fileloft_core::handler::TusRequest {
        method: http::Method::POST,
        uri: "/files/".parse().unwrap(),
        upload_id: None,
        headers,
        body: None,
    };
    let resp = h.handle(req).await;
    assert_eq!(resp.status.as_u16(), 201);
    let id = id_from_response(&resp);
    // Verify metadata is accessible via HEAD
    let head = h.handle(head_req(&id)).await;
    assert_eq!(head.status.as_u16(), 204);
    let meta = head
        .headers
        .get(fileloft_core::proto::HDR_UPLOAD_METADATA)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(meta.contains("filename"), "metadata header: {meta}");
}
