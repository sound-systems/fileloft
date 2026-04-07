mod helpers;
use helpers::*;

#[tokio::test]
async fn delete_returns_204() {
    let h = make_handler();
    let post = h.handle(post_req(100)).await;
    let id = id_from_response(&post);

    let del = h.handle(delete_req(&id)).await;
    assert_eq!(del.status.as_u16(), 204);
}

#[tokio::test]
async fn delete_makes_upload_unreachable() {
    let h = make_handler();
    let post = h.handle(post_req(100)).await;
    let id = id_from_response(&post);

    h.handle(delete_req(&id)).await;

    // Subsequent HEAD should return 404
    let head = h.handle(head_req(&id)).await;
    assert_eq!(head.status.as_u16(), 404);
}

#[tokio::test]
async fn delete_twice_returns_404() {
    let h = make_handler();
    let post = h.handle(post_req(100)).await;
    let id = id_from_response(&post);

    let del1 = h.handle(delete_req(&id)).await;
    assert_eq!(del1.status.as_u16(), 204);

    let del2 = h.handle(delete_req(&id)).await;
    assert_eq!(del2.status.as_u16(), 404);
}

#[tokio::test]
async fn delete_unknown_id_returns_404() {
    let h = make_handler();
    let del = h.handle(delete_req("does-not-exist")).await;
    assert_eq!(del.status.as_u16(), 404);
}

#[tokio::test]
async fn delete_disabled_returns_404() {
    let mut config = tus_core::config::Config::default();
    config.extensions.termination = false;
    let h = make_handler_with_config(config);

    let post = h.handle(post_req(100)).await;
    let id = id_from_response(&post);

    let del = h.handle(delete_req(&id)).await;
    // ExtensionNotEnabled → 404
    assert_eq!(del.status.as_u16(), 404);
}
