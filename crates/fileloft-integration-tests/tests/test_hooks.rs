mod helpers;
use helpers::*;

use fileloft_core::{config::Config, hooks::HookEvent, info::UploadInfoChanges};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn upload_created_event_fired_on_post() {
    let mut config = Config::default();
    config.hooks.channel_capacity = 16;
    let h = make_handler_with_config(config);

    let mut rx = h.hook_receiver().expect("hook channel configured");

    h.handle(post_req(100)).await;

    let event = rx.recv().await.unwrap();
    assert!(matches!(event, HookEvent::UploadCreated { .. }));
}

#[tokio::test]
async fn upload_finished_event_fired_on_complete() {
    let mut config = Config::default();
    config.hooks.channel_capacity = 16;
    let h = make_handler_with_config(config);

    let mut rx = h.hook_receiver().unwrap();

    let post = h.handle(post_req(5)).await;
    let id = id_from_response(&post);
    h.handle(patch_req(&id, 0, bytes::Bytes::from_static(b"hello")))
        .await;

    // Drain events: UploadCreated then UploadProgress then UploadFinished
    let mut got_finished = false;
    for _ in 0..10 {
        if let Ok(event) = rx.try_recv() {
            if matches!(event, HookEvent::UploadFinished { .. }) {
                got_finished = true;
                break;
            }
        }
    }
    assert!(got_finished, "UploadFinished event not received");
}

#[tokio::test]
async fn pre_create_hook_can_reject() {
    let mut config = Config::default();
    config.hooks.pre_create = Some(Arc::new(|_info| {
        Box::pin(async {
            Err(fileloft_core::TusError::HookRejected(
                "blocked by policy".into(),
            ))
        })
    }));
    let h = make_handler_with_config(config);

    let resp = h.handle(post_req(100)).await;
    assert_eq!(resp.status.as_u16(), 403);
}

#[tokio::test]
async fn pre_create_hook_can_modify_metadata() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls2 = Arc::clone(&calls);

    let mut config = Config::default();
    config.hooks.pre_create = Some(Arc::new(move |_info| {
        *calls2.lock().unwrap() += 1;
        Box::pin(async { Ok(UploadInfoChanges::default()) })
    }));
    let h = make_handler_with_config(config);

    let resp = h.handle(post_req(100)).await;
    assert_eq!(resp.status.as_u16(), 201);
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn pre_finish_hook_can_reject() {
    let mut config = Config::default();
    config.hooks.pre_finish = Some(Arc::new(|_info| {
        Box::pin(async {
            Err(fileloft_core::TusError::HookRejected(
                "post-processing failed".into(),
            ))
        })
    }));
    let h = make_handler_with_config(config);

    let post = h.handle(post_req(5)).await;
    let id = id_from_response(&post);

    let patch = h
        .handle(patch_req(&id, 0, bytes::Bytes::from_static(b"hello")))
        .await;
    assert_eq!(patch.status.as_u16(), 403);
}

#[tokio::test]
async fn upload_terminated_event_fired_on_delete() {
    let mut config = Config::default();
    config.hooks.channel_capacity = 16;
    let h = make_handler_with_config(config);

    let mut rx = h.hook_receiver().unwrap();

    let post = h.handle(post_req(100)).await;
    let id = id_from_response(&post);
    h.handle(delete_req(&id)).await;

    let mut got_terminated = false;
    for _ in 0..10 {
        if let Ok(event) = rx.try_recv() {
            if matches!(event, HookEvent::UploadTerminated { .. }) {
                got_terminated = true;
                break;
            }
        }
    }
    assert!(got_terminated, "UploadTerminated event not received");
}
