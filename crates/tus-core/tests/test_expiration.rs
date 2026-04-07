mod helpers;
use helpers::*;

use std::time::Duration;
use tus_core::config::Config;

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

    let patch = h.handle(patch_req(&id, 0, bytes::Bytes::from_static(b"data"))).await;
    assert_eq!(patch.status.as_u16(), 410);
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
