mod helpers;
use helpers::*;

use fileloft_core::{
    error::TusError,
    handler::{NoLocker, TusHandler, TusRequest},
    info::{UploadId, UploadInfo},
    proto::HDR_UPLOAD_CONCAT,
    store::{SendDataStore, SendUpload},
};

#[tokio::test]
async fn rejects_malformed_upload_ids() {
    let h = make_handler();

    for id in ["../secret", "nested/id", "", ".", "..", "has space"] {
        let resp = h
            .handle(TusRequest {
                method: http::Method::HEAD,
                uri: "/files/malformed".parse().unwrap(),
                upload_id: Some(id.to_string()),
                headers: tus_headers(),
                body: None,
            })
            .await;
        assert_eq!(resp.status.as_u16(), 400, "id should be rejected: {id:?}");
    }
}

#[tokio::test]
async fn rejects_malformed_concat_upload_ids() {
    let mut config = fileloft_core::config::Config::default();
    config.extensions.concatenation = true;
    let h = make_handler_with_config(config);

    let mut headers = tus_headers();
    headers.insert(
        HDR_UPLOAD_CONCAT,
        "final;http://localhost/files/../secret".parse().unwrap(),
    );
    headers.insert("host", "localhost".parse().unwrap());

    let resp = h
        .handle(TusRequest {
            method: http::Method::POST,
            uri: "/files/".parse().unwrap(),
            upload_id: None,
            headers,
            body: None,
        })
        .await;

    assert_eq!(resp.status.as_u16(), 400);
}

#[tokio::test]
async fn internal_errors_do_not_leak_details() {
    let h = TusHandler::new(FailingStore, None::<NoLocker>, Default::default());
    let resp = h.handle(head_req("valid-id")).await;

    assert_eq!(resp.status.as_u16(), 500);
    assert_eq!(resp.bytes_slice(), Some(&b"internal server error"[..]));
}

struct FailingStore;

impl SendDataStore for FailingStore {
    type UploadType = FailingUpload;

    async fn create_upload(&self, _info: UploadInfo) -> Result<Self::UploadType, TusError> {
        Err(TusError::Internal("secret path /tmp/fileloft".into()))
    }

    async fn get_upload(&self, _id: &UploadId) -> Result<Self::UploadType, TusError> {
        Err(TusError::Internal("secret path /tmp/fileloft".into()))
    }
}

struct FailingUpload;

impl SendUpload for FailingUpload {
    async fn write_chunk(
        &mut self,
        _offset: u64,
        _reader: &mut (dyn tokio::io::AsyncRead + Unpin + Send),
    ) -> Result<u64, TusError> {
        Err(TusError::Internal("secret path /tmp/fileloft".into()))
    }

    async fn get_info(&self) -> Result<UploadInfo, TusError> {
        Err(TusError::Internal("secret path /tmp/fileloft".into()))
    }

    async fn finalize(&mut self) -> Result<(), TusError> {
        Err(TusError::Internal("secret path /tmp/fileloft".into()))
    }

    async fn delete(self) -> Result<(), TusError> {
        Err(TusError::Internal("secret path /tmp/fileloft".into()))
    }

    async fn declare_length(&mut self, _length: u64) -> Result<(), TusError> {
        Err(TusError::Internal("secret path /tmp/fileloft".into()))
    }

    async fn concatenate(&mut self, _partials: &[UploadInfo]) -> Result<(), TusError> {
        Err(TusError::Internal("secret path /tmp/fileloft".into()))
    }

    async fn read_content(&self) -> Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, TusError> {
        Err(TusError::Internal("secret path /tmp/fileloft".into()))
    }
}
