//! Extension toggle visibility in OPTIONS responses.

use std::time::Duration;

use fileloft_core::config::{Config, Extensions};
use fileloft_core::proto::*;

mod helpers;
use helpers::*;

fn make_ext_handler(ext: Extensions) -> std::sync::Arc<TestHandler> {
    make_handler_with_config(Config {
        extensions: ext,
        ..Default::default()
    })
}

/// Check that a comma-separated extension list contains (or does not contain) a specific extension.
fn ext_list_contains(list: &str, ext: &str) -> bool {
    list.split(',').any(|s| s.trim() == ext)
}

#[tokio::test]
async fn options_reflects_disabled_creation() {
    let ext = Extensions {
        creation: false,
        creation_with_upload: false,
        ..Default::default()
    };
    let h = make_ext_handler(ext);
    let resp = h.handle(options_req()).await;
    let exts = resp
        .headers
        .get(HDR_TUS_EXTENSION)
        .expect("missing Tus-Extension")
        .to_str()
        .unwrap();
    assert!(
        !ext_list_contains(exts, EXT_CREATION),
        "creation should be absent: {exts}"
    );
    assert!(
        !ext_list_contains(exts, EXT_CREATION_WITH_UPLOAD),
        "creation-with-upload should be absent: {exts}"
    );
}

#[tokio::test]
async fn options_reflects_disabled_checksum() {
    let ext = Extensions {
        checksum: false,
        ..Default::default()
    };
    let h = make_ext_handler(ext);
    let resp = h.handle(options_req()).await;
    let exts = resp
        .headers
        .get(HDR_TUS_EXTENSION)
        .expect("missing Tus-Extension")
        .to_str()
        .unwrap();
    assert!(
        !exts.contains(EXT_CHECKSUM),
        "checksum should be absent: {exts}"
    );
    assert!(
        resp.headers.get(HDR_TUS_CHECKSUM_ALGORITHM).is_none(),
        "checksum algorithm header should be absent when checksum is disabled"
    );
}

#[tokio::test]
async fn options_reflects_disabled_termination() {
    let ext = Extensions {
        termination: false,
        ..Default::default()
    };
    let h = make_ext_handler(ext);
    let resp = h.handle(options_req()).await;
    let exts = resp
        .headers
        .get(HDR_TUS_EXTENSION)
        .expect("missing Tus-Extension")
        .to_str()
        .unwrap();
    assert!(
        !exts.contains(EXT_TERMINATION),
        "termination should be absent: {exts}"
    );
}

#[tokio::test]
async fn options_reflects_enabled_concatenation() {
    let ext = Extensions {
        concatenation: true,
        ..Default::default()
    };
    let h = make_ext_handler(ext);
    let resp = h.handle(options_req()).await;
    let exts = resp
        .headers
        .get(HDR_TUS_EXTENSION)
        .expect("missing Tus-Extension")
        .to_str()
        .unwrap();
    assert!(
        exts.contains(EXT_CONCATENATION),
        "concatenation should be present: {exts}"
    );
}

#[tokio::test]
async fn options_reflects_enabled_expiration() {
    let ext = Extensions {
        expiration: true,
        expiration_ttl: Some(Duration::from_secs(3600)),
        ..Default::default()
    };
    let h = make_ext_handler(ext);
    let resp = h.handle(options_req()).await;
    let exts = resp
        .headers
        .get(HDR_TUS_EXTENSION)
        .expect("missing Tus-Extension")
        .to_str()
        .unwrap();
    assert!(
        exts.contains(EXT_EXPIRATION),
        "expiration should be present: {exts}"
    );
}

#[tokio::test]
async fn options_reflects_disabled_defer_length() {
    let ext = Extensions {
        creation_defer_length: false,
        ..Default::default()
    };
    let h = make_ext_handler(ext);
    let resp = h.handle(options_req()).await;
    let exts = resp
        .headers
        .get(HDR_TUS_EXTENSION)
        .expect("missing Tus-Extension")
        .to_str()
        .unwrap();
    assert!(
        !exts.contains(EXT_CREATION_DEFER_LENGTH),
        "creation-defer-length should be absent: {exts}"
    );
}

#[tokio::test]
async fn options_advertises_max_size_when_set() {
    let h = make_handler_with_config(Config {
        max_size: 1048576,
        ..Default::default()
    });
    let resp = h.handle(options_req()).await;
    let max = resp
        .headers
        .get(HDR_TUS_MAX_SIZE)
        .expect("missing Tus-Max-Size")
        .to_str()
        .unwrap();
    assert_eq!(max, "1048576");
}

#[tokio::test]
async fn options_omits_max_size_when_zero() {
    let h = make_handler(); // max_size = 0
    let resp = h.handle(options_req()).await;
    assert!(
        resp.headers.get(HDR_TUS_MAX_SIZE).is_none(),
        "Tus-Max-Size should be absent when max_size = 0"
    );
}
