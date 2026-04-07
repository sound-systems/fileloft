mod helpers;
use helpers::*;

use fileloft_core::proto::*;

#[tokio::test]
async fn options_returns_204() {
    let h = make_handler();
    let resp = h.handle(options_req()).await;
    assert_eq!(resp.status.as_u16(), 204);
}

#[tokio::test]
async fn options_advertises_tus_version() {
    let h = make_handler();
    let resp = h.handle(options_req()).await;
    let version = resp.headers.get(HDR_TUS_VERSION).unwrap().to_str().unwrap();
    assert_eq!(version, TUS_VERSION);
}

#[tokio::test]
async fn options_includes_default_extensions() {
    let h = make_handler();
    let resp = h.handle(options_req()).await;
    let exts = resp
        .headers
        .get(HDR_TUS_EXTENSION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(exts.contains(EXT_CREATION), "missing creation: {exts}");
    assert!(
        exts.contains(EXT_CREATION_WITH_UPLOAD),
        "missing creation-with-upload: {exts}"
    );
    assert!(exts.contains(EXT_CHECKSUM), "missing checksum: {exts}");
    assert!(
        exts.contains(EXT_TERMINATION),
        "missing termination: {exts}"
    );
}

#[tokio::test]
async fn options_includes_checksum_algorithms() {
    let h = make_handler();
    let resp = h.handle(options_req()).await;
    let algs = resp
        .headers
        .get(HDR_TUS_CHECKSUM_ALGORITHM)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(algs.contains("sha1"), "missing sha1: {algs}");
    assert!(algs.contains("sha256"), "missing sha256: {algs}");
    assert!(algs.contains("md5"), "missing md5: {algs}");
}

#[tokio::test]
async fn options_does_not_require_tus_resumable() {
    // OPTIONS is the only request exempt from the Tus-Resumable header requirement
    let h = make_handler();
    let req = options_req(); // no Tus-Resumable header
    let resp = h.handle(req).await;
    assert_eq!(resp.status.as_u16(), 204);
}
