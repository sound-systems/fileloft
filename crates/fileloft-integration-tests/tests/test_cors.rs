//! CORS header behaviour (P2).

use fileloft_core::config::{Config, CorsConfig};
use fileloft_core::proto::*;

mod helpers;
use helpers::*;

fn make_cors_handler() -> std::sync::Arc<TestHandler> {
    make_handler_with_config(Config {
        cors: CorsConfig {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    })
}

// ---------------------------------------------------------------------------
// Base headers (every response)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cors_disabled_omits_all_cors_headers() {
    let h = make_handler(); // default: cors.enabled = false
    let resp = h.handle(options_req()).await;
    assert!(resp.headers.get(HDR_ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
    assert!(resp.headers.get(HDR_ACCESS_CONTROL_ALLOW_METHODS).is_none());
    assert!(resp.headers.get(HDR_ACCESS_CONTROL_ALLOW_HEADERS).is_none());
    assert!(resp
        .headers
        .get(HDR_ACCESS_CONTROL_EXPOSE_HEADERS)
        .is_none());
    assert!(resp.headers.get(HDR_ACCESS_CONTROL_MAX_AGE).is_none());
}

#[tokio::test]
async fn cors_enabled_adds_allow_origin_star() {
    let h = make_cors_handler();
    let resp = h.handle(options_req()).await;
    let origin = resp
        .headers
        .get(HDR_ACCESS_CONTROL_ALLOW_ORIGIN)
        .expect("missing ACAO")
        .to_str()
        .unwrap();
    assert_eq!(origin, "*");
}

#[tokio::test]
async fn cors_custom_origin() {
    let h = make_handler_with_config(Config {
        cors: CorsConfig {
            enabled: true,
            allow_origin: "https://example.com".into(),
            ..Default::default()
        },
        ..Default::default()
    });
    let resp = h.handle(options_req()).await;
    let origin = resp
        .headers
        .get(HDR_ACCESS_CONTROL_ALLOW_ORIGIN)
        .expect("missing ACAO")
        .to_str()
        .unwrap();
    assert_eq!(origin, "https://example.com");
}

#[tokio::test]
async fn cors_credentials_header_when_enabled() {
    let h = make_handler_with_config(Config {
        cors: CorsConfig {
            enabled: true,
            allow_credentials: true,
            ..Default::default()
        },
        ..Default::default()
    });
    let resp = h.handle(options_req()).await;
    let creds = resp
        .headers
        .get(HDR_ACCESS_CONTROL_ALLOW_CREDENTIALS)
        .expect("missing credentials header")
        .to_str()
        .unwrap();
    assert_eq!(creds, "true");
}

#[tokio::test]
async fn cors_credentials_absent_when_false() {
    let h = make_cors_handler();
    let resp = h.handle(options_req()).await;
    assert!(
        resp.headers
            .get(HDR_ACCESS_CONTROL_ALLOW_CREDENTIALS)
            .is_none(),
        "credentials header should not be present when disabled"
    );
}

// ---------------------------------------------------------------------------
// OPTIONS preflight headers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn options_cors_allow_methods_excludes_get_when_no_download() {
    let h = make_cors_handler();
    let resp = h.handle(options_req()).await;
    let methods = resp
        .headers
        .get(HDR_ACCESS_CONTROL_ALLOW_METHODS)
        .expect("missing allow-methods")
        .to_str()
        .unwrap();
    assert!(
        !methods.contains("GET"),
        "GET should not be in methods: {methods}"
    );
    assert!(
        methods.contains("POST"),
        "POST should be in methods: {methods}"
    );
}

#[tokio::test]
async fn options_cors_allow_methods_includes_get_when_download_enabled() {
    let h = make_handler_with_config(Config {
        cors: CorsConfig {
            enabled: true,
            ..Default::default()
        },
        enable_download: true,
        ..Default::default()
    });
    let resp = h.handle(options_req()).await;
    let methods = resp
        .headers
        .get(HDR_ACCESS_CONTROL_ALLOW_METHODS)
        .expect("missing allow-methods")
        .to_str()
        .unwrap();
    assert!(
        methods.contains("GET"),
        "GET should be in methods: {methods}"
    );
}

#[tokio::test]
async fn options_cors_max_age_custom() {
    let h = make_handler_with_config(Config {
        cors: CorsConfig {
            enabled: true,
            max_age: 3600,
            ..Default::default()
        },
        ..Default::default()
    });
    let resp = h.handle(options_req()).await;
    let age = resp
        .headers
        .get(HDR_ACCESS_CONTROL_MAX_AGE)
        .expect("missing max-age")
        .to_str()
        .unwrap();
    assert_eq!(age, "3600");
}

#[tokio::test]
async fn options_cors_extra_allow_headers_merged() {
    let h = make_handler_with_config(Config {
        cors: CorsConfig {
            enabled: true,
            extra_allow_headers: vec!["X-My-Token".into(), "X-Request-Id".into()],
            ..Default::default()
        },
        ..Default::default()
    });
    let resp = h.handle(options_req()).await;
    let allow = resp
        .headers
        .get(HDR_ACCESS_CONTROL_ALLOW_HEADERS)
        .expect("missing allow-headers")
        .to_str()
        .unwrap();
    assert!(
        allow.contains("Tus-Resumable"),
        "missing default header: {allow}"
    );
    assert!(
        allow.contains("X-My-Token"),
        "missing extra header: {allow}"
    );
    assert!(
        allow.contains("X-Request-Id"),
        "missing extra header: {allow}"
    );
}

#[tokio::test]
async fn cors_extra_expose_headers_merged() {
    let h = make_handler_with_config(Config {
        cors: CorsConfig {
            enabled: true,
            extra_expose_headers: vec!["X-Upload-Location".into()],
            ..Default::default()
        },
        ..Default::default()
    });
    let resp = h.handle(post_req(10)).await;
    let expose = resp
        .headers
        .get(HDR_ACCESS_CONTROL_EXPOSE_HEADERS)
        .expect("missing expose-headers")
        .to_str()
        .unwrap();
    assert!(
        expose.contains("Upload-Offset"),
        "missing default expose: {expose}"
    );
    assert!(
        expose.contains("X-Upload-Location"),
        "missing extra expose: {expose}"
    );
}

// ---------------------------------------------------------------------------
// Expose headers appear on non-OPTIONS responses
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cors_expose_headers_on_post_response() {
    let h = make_cors_handler();
    let resp = h.handle(post_req(10)).await;
    assert_eq!(resp.status.as_u16(), 201);
    let expose = resp
        .headers
        .get(HDR_ACCESS_CONTROL_EXPOSE_HEADERS)
        .expect("missing expose-headers on POST response")
        .to_str()
        .unwrap();
    assert!(expose.contains("Location"), "missing Location: {expose}");
    assert!(
        expose.contains("Upload-Offset"),
        "missing Upload-Offset: {expose}"
    );
}
