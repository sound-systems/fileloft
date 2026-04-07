#![allow(dead_code)] // helpers are shared across test crates; not every file uses every helper

/// Shared test helpers used across integration test files.
use std::sync::Arc;

use bytes::Bytes;
use fileloft_core::{
    config::Config,
    handler::{TusHandler, TusRequest, TusResponse},
    proto::*,
};
use fileloft_store_memory::{MemoryLocker, MemoryStore};
use http::{HeaderMap, Method};

pub type TestHandler = TusHandler<MemoryStore, MemoryLocker>;

pub fn make_handler() -> Arc<TestHandler> {
    make_handler_with_config(Config::default())
}

pub fn make_handler_with_config(config: Config) -> Arc<TestHandler> {
    let store = MemoryStore::new();
    let locker = MemoryLocker::new();
    Arc::new(TusHandler::new(store, Some(locker), config))
}

pub fn tus_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(HDR_TUS_RESUMABLE, TUS_VERSION.parse().unwrap());
    h
}

pub fn options_req() -> TusRequest {
    TusRequest {
        method: Method::OPTIONS,
        uri: "/files/".parse().unwrap(),
        upload_id: None,
        headers: HeaderMap::new(),
        body: None,
    }
}

pub fn post_req(upload_length: u64) -> TusRequest {
    let mut headers = tus_headers();
    headers.insert(
        HDR_UPLOAD_LENGTH,
        upload_length.to_string().parse().unwrap(),
    );
    headers.insert("host", "localhost".parse().unwrap());
    TusRequest {
        method: Method::POST,
        uri: "/files/".parse().unwrap(),
        upload_id: None,
        headers,
        body: None,
    }
}

pub fn post_req_with_body(upload_length: u64, data: Bytes) -> TusRequest {
    let mut headers = tus_headers();
    headers.insert(
        HDR_UPLOAD_LENGTH,
        upload_length.to_string().parse().unwrap(),
    );
    headers.insert(HDR_CONTENT_TYPE, CONTENT_TYPE_OCTET_STREAM.parse().unwrap());
    headers.insert("host", "localhost".parse().unwrap());
    TusRequest {
        method: Method::POST,
        uri: "/files/".parse().unwrap(),
        upload_id: None,
        headers,
        body: Some(Box::new(std::io::Cursor::new(data))),
    }
}

pub fn head_req(id: &str) -> TusRequest {
    TusRequest {
        method: Method::HEAD,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id.to_string()),
        headers: tus_headers(),
        body: None,
    }
}

pub fn patch_req(id: &str, offset: u64, data: Bytes) -> TusRequest {
    let mut headers = tus_headers();
    headers.insert(HDR_UPLOAD_OFFSET, offset.to_string().parse().unwrap());
    headers.insert(HDR_CONTENT_TYPE, CONTENT_TYPE_OCTET_STREAM.parse().unwrap());
    TusRequest {
        method: Method::PATCH,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id.to_string()),
        headers,
        body: Some(Box::new(std::io::Cursor::new(data))),
    }
}

pub fn delete_req(id: &str) -> TusRequest {
    TusRequest {
        method: Method::DELETE,
        uri: format!("/files/{id}").parse().unwrap(),
        upload_id: Some(id.to_string()),
        headers: tus_headers(),
        body: None,
    }
}

/// Extract the upload ID from a Location header value like "http://localhost/files/{id}"
pub fn id_from_response(resp: &TusResponse) -> String {
    let location = resp
        .headers
        .get(HDR_LOCATION)
        .expect("no Location header")
        .to_str()
        .unwrap();
    location.rsplit('/').next().unwrap().to_string()
}

pub fn get_offset(resp: &TusResponse) -> u64 {
    resp.headers
        .get(HDR_UPLOAD_OFFSET)
        .expect("no Upload-Offset header")
        .to_str()
        .unwrap()
        .parse()
        .unwrap()
}
