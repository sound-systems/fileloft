mod helpers;
use helpers::*;

use base64::{engine::general_purpose::STANDARD, Engine};
use bytes::Bytes;
use fileloft_core::{handler::TusRequest, proto::*};
use sha1::{Digest, Sha1};

#[tokio::test]
async fn patch_full_upload_in_one_chunk() {
    let h = make_handler();
    let data = Bytes::from_static(b"hello tus world");
    let size = data.len() as u64;

    let post = h.handle(post_req(size)).await;
    let id = id_from_response(&post);

    let patch = h.handle(patch_req(&id, 0, data)).await;
    assert_eq!(patch.status.as_u16(), 204);
    assert_eq!(get_offset(&patch), size);
}

#[tokio::test]
async fn patch_resumes_at_partial_offset() {
    let h = make_handler();
    let post = h.handle(post_req(10)).await;
    let id = id_from_response(&post);

    // First chunk: 5 bytes
    let chunk1 = h
        .handle(patch_req(&id, 0, Bytes::from_static(b"hello")))
        .await;
    assert_eq!(chunk1.status.as_u16(), 204);
    assert_eq!(get_offset(&chunk1), 5);

    // Second chunk: remaining 5 bytes
    let chunk2 = h
        .handle(patch_req(&id, 5, Bytes::from_static(b"world")))
        .await;
    assert_eq!(chunk2.status.as_u16(), 204);
    assert_eq!(get_offset(&chunk2), 10);
}

#[tokio::test]
async fn patch_offset_mismatch_returns_409() {
    let h = make_handler();
    let post = h.handle(post_req(100)).await;
    let id = id_from_response(&post);

    // Client claims offset 50 but server has 0
    let patch = h
        .handle(patch_req(&id, 50, Bytes::from_static(b"data")))
        .await;
    assert_eq!(patch.status.as_u16(), 409);
}

#[tokio::test]
async fn patch_missing_tus_resumable_returns_412() {
    let h = make_handler();
    let post = h.handle(post_req(10)).await;
    let id = id_from_response(&post);

    let mut headers = http::HeaderMap::new();
    headers.insert(HDR_UPLOAD_OFFSET, "0".parse().unwrap());
    headers.insert(HDR_CONTENT_TYPE, CONTENT_TYPE_OCTET_STREAM.parse().unwrap());
    let req = TusRequest {
        method: http::Method::PATCH,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id),
        headers,
        body: Some(Box::new(std::io::Cursor::new(b"data" as &[u8]))),
    };
    let resp = h.handle(req).await;
    assert_eq!(resp.status.as_u16(), 412);
}

#[tokio::test]
async fn patch_wrong_content_type_returns_415() {
    let h = make_handler();
    let post = h.handle(post_req(10)).await;
    let id = id_from_response(&post);

    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_OFFSET, "0".parse().unwrap());
    headers.insert(HDR_CONTENT_TYPE, "text/plain".parse().unwrap());
    let req = TusRequest {
        method: http::Method::PATCH,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id),
        headers,
        body: Some(Box::new(std::io::Cursor::new(b"data" as &[u8]))),
    };
    let resp = h.handle(req).await;
    assert_eq!(resp.status.as_u16(), 415);
}

#[tokio::test]
async fn patch_unknown_id_returns_404() {
    let h = make_handler();
    let patch = h
        .handle(patch_req("no-such-id", 0, Bytes::from_static(b"data")))
        .await;
    assert_eq!(patch.status.as_u16(), 404);
}

#[tokio::test]
async fn patch_with_valid_sha1_checksum() {
    let h = make_handler();
    let data = b"checksum test data";
    let mut hasher = Sha1::new();
    hasher.update(data);
    let hash = hasher.finalize();
    let b64 = STANDARD.encode(hash);

    let post = h.handle(post_req(data.len() as u64)).await;
    let id = id_from_response(&post);

    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_OFFSET, "0".parse().unwrap());
    headers.insert(HDR_CONTENT_TYPE, CONTENT_TYPE_OCTET_STREAM.parse().unwrap());
    headers.insert(HDR_UPLOAD_CHECKSUM, format!("sha1 {b64}").parse().unwrap());
    let req = TusRequest {
        method: http::Method::PATCH,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id),
        headers,
        body: Some(Box::new(std::io::Cursor::new(data.to_vec()))),
    };
    let resp = h.handle(req).await;
    assert_eq!(
        resp.status.as_u16(),
        204,
        "body: {}",
        String::from_utf8_lossy(resp.bytes_slice().unwrap_or(&[]))
    );
}

#[tokio::test]
async fn patch_with_wrong_checksum_returns_460() {
    let h = make_handler();
    let data = b"checksum fail data";
    let wrong_b64 = STANDARD.encode([0u8; 20]); // all-zeros SHA1

    let post = h.handle(post_req(data.len() as u64)).await;
    let id = id_from_response(&post);

    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_OFFSET, "0".parse().unwrap());
    headers.insert(HDR_CONTENT_TYPE, CONTENT_TYPE_OCTET_STREAM.parse().unwrap());
    headers.insert(
        HDR_UPLOAD_CHECKSUM,
        format!("sha1 {wrong_b64}").parse().unwrap(),
    );
    let req = TusRequest {
        method: http::Method::PATCH,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id),
        headers,
        body: Some(Box::new(std::io::Cursor::new(data.to_vec()))),
    };
    let resp = h.handle(req).await;
    assert_eq!(resp.status.as_u16(), 460);
}

#[tokio::test]
async fn wrong_checksum_does_not_persist_chunk_or_advance_offset() {
    let h = make_handler();
    let data = b"checksum integrity data";
    let wrong_b64 = STANDARD.encode([0u8; 20]); // all-zeros SHA1

    let post = h.handle(post_req(data.len() as u64)).await;
    let id = id_from_response(&post);

    // A wrong checksum is rejected with 460.
    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_OFFSET, "0".parse().unwrap());
    headers.insert(HDR_CONTENT_TYPE, CONTENT_TYPE_OCTET_STREAM.parse().unwrap());
    headers.insert(
        HDR_UPLOAD_CHECKSUM,
        format!("sha1 {wrong_b64}").parse().unwrap(),
    );
    let req = TusRequest {
        method: http::Method::PATCH,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id.clone()),
        headers,
        body: Some(Box::new(std::io::Cursor::new(data.to_vec()))),
    };
    assert_eq!(h.handle(req).await.status.as_u16(), 460);

    // The rejected chunk must not have been persisted: the offset stays at 0.
    let head = h.handle(head_req(&id)).await;
    assert_eq!(
        get_offset(&head),
        0,
        "offset must not advance when the checksum fails"
    );

    // Re-sending the same chunk at offset 0 with the correct checksum succeeds
    // and completes the upload — proving the earlier failure left no partial data.
    let mut hasher = Sha1::new();
    hasher.update(data);
    let good_b64 = STANDARD.encode(hasher.finalize());
    let mut headers2 = tus_headers();
    headers2.insert(HDR_UPLOAD_OFFSET, "0".parse().unwrap());
    headers2.insert(HDR_CONTENT_TYPE, CONTENT_TYPE_OCTET_STREAM.parse().unwrap());
    headers2.insert(
        HDR_UPLOAD_CHECKSUM,
        format!("sha1 {good_b64}").parse().unwrap(),
    );
    let req2 = TusRequest {
        method: http::Method::PATCH,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id.clone()),
        headers: headers2,
        body: Some(Box::new(std::io::Cursor::new(data.to_vec()))),
    };
    let resp2 = h.handle(req2).await;
    assert_eq!(resp2.status.as_u16(), 204);
    assert_eq!(get_offset(&resp2), data.len() as u64);
}
