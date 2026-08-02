mod helpers;
use helpers::*;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use fileloft_core::{config::Config, handler::TusRequest, hooks::HookEvent, proto::*};

fn make_concat_handler() -> std::sync::Arc<TestHandler> {
    let mut config = Config::default();
    config.extensions.concatenation = true;
    make_handler_with_config(config)
}

/// Build a final concatenation of the given partial IDs and return the response.
async fn post_final(
    h: &TestHandler,
    partial_ids: &[String],
) -> fileloft_core::handler::TusResponse {
    let urls: Vec<String> = partial_ids
        .iter()
        .map(|id| format!("http://localhost/files/{id}"))
        .collect();
    let concat_value = format!("final;{}", urls.join(" "));
    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_CONCAT, concat_value.parse().unwrap());
    headers.insert("host", "localhost".parse().unwrap());
    h.handle(TusRequest {
        method: http::Method::POST,
        uri: "/files/".parse().unwrap(),
        upload_id: None,
        headers,
        body: None,
    })
    .await
}

async fn create_partial(h: &TestHandler, data: &[u8]) -> String {
    // Create a partial upload
    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_LENGTH, data.len().to_string().parse().unwrap());
    headers.insert(HDR_UPLOAD_CONCAT, "partial".parse().unwrap());
    headers.insert("host", "localhost".parse().unwrap());
    let post = h
        .handle(TusRequest {
            method: http::Method::POST,
            uri: "/files/".parse().unwrap(),
            upload_id: None,
            headers,
            body: None,
        })
        .await;
    assert_eq!(post.status.as_u16(), 201, "partial create failed");
    let id = id_from_response(&post);

    // Upload the data
    let patch = h
        .handle(patch_req(&id, 0, bytes::Bytes::copy_from_slice(data)))
        .await;
    assert_eq!(patch.status.as_u16(), 204, "partial upload failed");

    id
}

#[tokio::test]
async fn concatenation_assembles_partials() {
    let h = make_concat_handler();

    let id1 = create_partial(&h, b"hello ").await;
    let id2 = create_partial(&h, b"world").await;

    // Create final upload
    let concat_value = format!("final;http://localhost/files/{id1} http://localhost/files/{id2}");
    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_CONCAT, concat_value.parse().unwrap());
    headers.insert("host", "localhost".parse().unwrap());
    let post_final = h
        .handle(TusRequest {
            method: http::Method::POST,
            uri: "/files/".parse().unwrap(),
            upload_id: None,
            headers,
            body: None,
        })
        .await;
    assert_eq!(
        post_final.status.as_u16(),
        201,
        "final concat failed: {}",
        String::from_utf8_lossy(post_final.bytes_slice().unwrap_or(&[]))
    );

    let final_id = id_from_response(&post_final);
    let head = h.handle(head_req(&final_id)).await;
    assert_eq!(head.status.as_u16(), 204);
    // Offset should equal total size (6 + 5 = 11)
    assert_eq!(get_offset(&head), 11);
}

#[tokio::test]
async fn patch_on_final_upload_returns_403() {
    let h = make_concat_handler();

    let id1 = create_partial(&h, b"abc").await;
    let id2 = create_partial(&h, b"def").await;

    let concat_value = format!("final;http://localhost/files/{id1} http://localhost/files/{id2}");
    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_CONCAT, concat_value.parse().unwrap());
    headers.insert("host", "localhost".parse().unwrap());
    let post_final = h
        .handle(TusRequest {
            method: http::Method::POST,
            uri: "/files/".parse().unwrap(),
            upload_id: None,
            headers,
            body: None,
        })
        .await;
    let final_id = id_from_response(&post_final);

    // PATCH on a final upload must be rejected
    let patch = h
        .handle(patch_req(&final_id, 0, bytes::Bytes::from_static(b"extra")))
        .await;
    assert_eq!(patch.status.as_u16(), 403);
}

#[tokio::test]
async fn final_concat_with_incomplete_partial_returns_400() {
    let h = make_concat_handler();

    // Create a partial upload but don't finish it
    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_LENGTH, "100".parse().unwrap());
    headers.insert(HDR_UPLOAD_CONCAT, "partial".parse().unwrap());
    headers.insert("host", "localhost".parse().unwrap());
    let partial = h
        .handle(TusRequest {
            method: http::Method::POST,
            uri: "/files/".parse().unwrap(),
            upload_id: None,
            headers,
            body: None,
        })
        .await;
    let partial_id = id_from_response(&partial);

    // Only upload 5 bytes (incomplete)
    h.handle(patch_req(
        &partial_id,
        0,
        bytes::Bytes::from_static(b"hello"),
    ))
    .await;

    // Attempt final concat with incomplete partial
    let concat_value = format!("final;http://localhost/files/{partial_id}");
    let mut headers2 = tus_headers();
    headers2.insert(HDR_UPLOAD_CONCAT, concat_value.parse().unwrap());
    headers2.insert("host", "localhost".parse().unwrap());
    let resp = h
        .handle(TusRequest {
            method: http::Method::POST,
            uri: "/files/".parse().unwrap(),
            upload_id: None,
            headers: headers2,
            body: None,
        })
        .await;
    assert_eq!(resp.status.as_u16(), 400);
}

fn cleanup_handler_with_channel() -> std::sync::Arc<TestHandler> {
    let mut config = Config::default();
    config.extensions.concatenation = true;
    config.extensions.cleanup_concat_partials = true;
    config.hooks.channel_capacity = 32;
    make_handler_with_config(config)
}

#[tokio::test]
async fn cleanup_partials_emits_terminated_events() {
    let h = cleanup_handler_with_channel();

    let id1 = create_partial(&h, b"hello ").await;
    let id2 = create_partial(&h, b"world").await;

    // Subscribe after the partials exist so we only observe the final POST's events.
    let mut rx = h.hook_receiver().expect("hook channel configured");

    let resp = post_final(&h, &[id1.clone(), id2.clone()]).await;
    assert_eq!(resp.status.as_u16(), 201);

    let mut terminated = HashSet::new();
    while let Ok(event) = rx.try_recv() {
        if let HookEvent::UploadTerminated { id } = event {
            terminated.insert(id.to_string());
        }
    }
    assert!(
        terminated.contains(&id1),
        "cleanup should emit UploadTerminated for partial 1"
    );
    assert!(
        terminated.contains(&id2),
        "cleanup should emit UploadTerminated for partial 2"
    );

    // The partials are actually gone.
    assert_eq!(h.handle(head_req(&id1)).await.status.as_u16(), 404);
    assert_eq!(h.handle(head_req(&id2)).await.status.as_u16(), 404);
}

#[tokio::test]
async fn cleanup_partials_runs_pre_terminate_hook() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_hook = Arc::clone(&seen);

    let mut config = Config::default();
    config.extensions.concatenation = true;
    config.extensions.cleanup_concat_partials = true;
    config.hooks.pre_terminate = Some(Arc::new(move |info| {
        seen_hook.lock().unwrap().push(info.id.to_string());
        Box::pin(async { Ok(()) })
    }));
    let h = make_handler_with_config(config);

    let id1 = create_partial(&h, b"aa").await;
    let id2 = create_partial(&h, b"bb").await;

    let resp = post_final(&h, &[id1.clone(), id2.clone()]).await;
    assert_eq!(resp.status.as_u16(), 201);

    let called = seen.lock().unwrap();
    assert!(
        called.contains(&id1) && called.contains(&id2),
        "pre_terminate must run for each cleaned-up partial, got {called:?}"
    );
}
