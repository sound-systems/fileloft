mod helpers;
use helpers::*;


#[tokio::test]
async fn head_returns_offset_zero_on_new_upload() {
    let h = make_handler();
    let post = h.handle(post_req(1024)).await;
    assert_eq!(post.status.as_u16(), 201);
    let id = id_from_response(&post);

    let head = h.handle(head_req(&id)).await;
    assert_eq!(head.status.as_u16(), 204);
    assert_eq!(get_offset(&head), 0);
}

#[tokio::test]
async fn head_returns_404_for_unknown_id() {
    let h = make_handler();
    let head = h.handle(head_req("nonexistent-id")).await;
    assert_eq!(head.status.as_u16(), 404);
}

#[tokio::test]
async fn head_returns_correct_offset_after_partial_upload() {
    let h = make_handler();
    let post = h.handle(post_req(100)).await;
    let id = id_from_response(&post);

    // Upload 40 bytes
    let data = bytes::Bytes::from(vec![0u8; 40]);
    let patch = h.handle(patch_req(&id, 0, data)).await;
    assert_eq!(patch.status.as_u16(), 204);

    let head = h.handle(head_req(&id)).await;
    assert_eq!(get_offset(&head), 40);
}

#[tokio::test]
async fn head_requires_tus_resumable_header() {
    let h = make_handler();
    let post = h.handle(post_req(100)).await;
    let id = id_from_response(&post);

    let req = fileloft_core::handler::TusRequest {
        method: http::Method::HEAD,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id.clone()),
        headers: http::HeaderMap::new(), // no Tus-Resumable
        body: None,
    };
    let resp = h.handle(req).await;
    assert_eq!(resp.status.as_u16(), 412);
}
