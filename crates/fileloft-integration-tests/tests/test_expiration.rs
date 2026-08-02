mod helpers;
use helpers::*;

use fileloft_core::config::Config;
use fileloft_core::handler::TusRequest;
use std::time::Duration;

fn get_req(id: &str) -> TusRequest {
    TusRequest {
        method: http::Method::GET,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id.to_string()),
        headers: tus_headers(),
        body: None,
    }
}

#[tokio::test]
async fn expired_upload_returns_410_on_head() {
    let mut config = Config::default();
    config.extensions.expiration = true;
    config.extensions.expiration_ttl = Some(Duration::from_millis(1));
    let h = make_handler_with_config(config);

    let post = h.handle(post_req(100)).await;
    assert_eq!(post.status.as_u16(), 201);
    let id = id_from_response(&post);

    // Wait for the TTL to elapse
    tokio::time::sleep(Duration::from_millis(50)).await;

    let head = h.handle(head_req(&id)).await;
    assert_eq!(head.status.as_u16(), 410);
}

#[tokio::test]
async fn expired_upload_returns_410_on_patch() {
    let mut config = Config::default();
    config.extensions.expiration = true;
    config.extensions.expiration_ttl = Some(Duration::from_millis(1));
    let h = make_handler_with_config(config);

    let post = h.handle(post_req(100)).await;
    let id = id_from_response(&post);

    tokio::time::sleep(Duration::from_millis(50)).await;

    let patch = h
        .handle(patch_req(&id, 0, bytes::Bytes::from_static(b"data")))
        .await;
    assert_eq!(patch.status.as_u16(), 410);
}

#[tokio::test]
async fn expired_upload_returns_410_on_get() {
    let mut config = Config::default();
    config.enable_download = true;
    config.extensions.expiration = true;
    config.extensions.expiration_ttl = Some(Duration::from_millis(1));
    let h = make_handler_with_config(config);

    // Create and complete in a single request; POST does not enforce expiry,
    // so the upload finishes even with a 1ms TTL.
    let data = b"secret";
    let post = h
        .handle(post_req_with_body(
            data.len() as u64,
            bytes::Bytes::copy_from_slice(data),
        ))
        .await;
    assert_eq!(post.status.as_u16(), 201);
    let id = id_from_response(&post);

    tokio::time::sleep(Duration::from_millis(50)).await;

    let resp = h.handle(get_req(&id)).await;
    assert_eq!(
        resp.status.as_u16(),
        410,
        "expired upload must not be downloadable"
    );
}

#[tokio::test]
async fn non_expired_upload_is_downloadable() {
    let mut config = Config::default();
    config.enable_download = true;
    config.extensions.expiration = true;
    config.extensions.expiration_ttl = Some(Duration::from_secs(3600));
    let h = make_handler_with_config(config);

    let data = b"hello";
    let post = h
        .handle(post_req_with_body(
            data.len() as u64,
            bytes::Bytes::copy_from_slice(data),
        ))
        .await;
    assert_eq!(post.status.as_u16(), 201);
    let id = id_from_response(&post);

    let resp = h.handle(get_req(&id)).await;
    assert_eq!(resp.status.as_u16(), 200);
}

#[tokio::test]
async fn non_expired_upload_is_accessible() {
    let mut config = Config::default();
    config.extensions.expiration = true;
    config.extensions.expiration_ttl = Some(Duration::from_secs(3600));
    let h = make_handler_with_config(config);

    let post = h.handle(post_req(100)).await;
    let id = id_from_response(&post);

    let head = h.handle(head_req(&id)).await;
    assert_eq!(head.status.as_u16(), 204);
}
