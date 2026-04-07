use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};

use base64::{engine::general_purpose::STANDARD, Engine};
use digest::DynDigest;
use tokio::io::{AsyncRead, ReadBuf};

use crate::error::TusError;
use crate::proto::SUPPORTED_CHECKSUM_ALGORITHMS;

/// Checksum algorithm supported by the tus checksum extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksumAlgorithm {
    Sha1,
    Sha256,
    Md5,
}

impl ChecksumAlgorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
            Self::Md5 => "md5",
        }
    }

    fn make_hasher(&self) -> Box<dyn DynDigest + Send> {
        match self {
            Self::Sha1 => Box::new(sha1::Sha1::default()),
            Self::Sha256 => Box::new(sha2::Sha256::default()),
            Self::Md5 => Box::new(md5::Md5::default()),
        }
    }
}

impl FromStr for ChecksumAlgorithm {
    type Err = TusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sha1" => Ok(Self::Sha1),
            "sha256" => Ok(Self::Sha256),
            "md5" => Ok(Self::Md5),
            other => Err(TusError::UnsupportedChecksumAlgorithm(other.to_string())),
        }
    }
}

/// Comma-separated list of algorithms to advertise in `Tus-Checksum-Algorithm`.
pub fn algorithms_header() -> String {
    SUPPORTED_CHECKSUM_ALGORITHMS.join(",")
}

/// Parse the `Upload-Checksum` header: `"<algorithm> <base64-hash>"`.
pub fn parse_checksum_header(value: &str) -> Result<(ChecksumAlgorithm, Vec<u8>), TusError> {
    let (alg_str, b64) = value.split_once(' ').ok_or_else(|| {
        TusError::InvalidMetadata("malformed Upload-Checksum header (expected '<alg> <base64>')".into())
    })?;
    let algorithm: ChecksumAlgorithm = alg_str.parse()?;
    let hash = STANDARD
        .decode(b64.trim())
        .map_err(|e| TusError::InvalidMetadata(format!("bad base64 in Upload-Checksum: {e}")))?;
    Ok((algorithm, hash))
}

/// Wraps any `AsyncRead`, feeding bytes through a hasher as they pass through.
/// Call `verify()` after all bytes have been read to check against the expected hash.
pub struct ChecksumReader<R> {
    inner: R,
    hasher: Box<dyn DynDigest + Send>,
    expected: Vec<u8>,
}

impl<R: AsyncRead + Unpin> ChecksumReader<R> {
    pub fn new(inner: R, algorithm: ChecksumAlgorithm, expected: Vec<u8>) -> Self {
        Self {
            inner,
            hasher: algorithm.make_hasher(),
            expected,
        }
    }

    /// Compare the accumulated hash against the expected value.
    /// Call this *after* all bytes have been read through this reader.
    pub fn verify(self) -> Result<(), TusError> {
        let computed = self.hasher.finalize();
        if computed.as_ref() == self.expected.as_slice() {
            Ok(())
        } else {
            Err(TusError::ChecksumMismatch)
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for ChecksumReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let me = self.get_mut();
        let before = buf.filled().len();
        let result = Pin::new(&mut me.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result {
            let filled = &buf.filled()[before..];
            if !filled.is_empty() {
                me.hasher.update(filled);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use base64::{engine::general_purpose::STANDARD, Engine};
    use sha1::{Digest, Sha1};
    use tokio::io::AsyncReadExt;

    use super::*;

    fn sha1_b64(data: &[u8]) -> Vec<u8> {
        let mut h = Sha1::new();
        Digest::update(&mut h, data);
        Digest::finalize(h).to_vec()
    }

    #[tokio::test]
    async fn checksum_reader_correct_hash() {
        let data = b"hello tus";
        let expected = sha1_b64(data);
        let cursor = Cursor::new(data.to_vec());
        let mut reader = ChecksumReader::new(cursor, ChecksumAlgorithm::Sha1, expected);
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(&buf, data);
        // Verify must succeed
        reader.verify().unwrap();
    }

    #[tokio::test]
    async fn checksum_reader_wrong_hash() {
        let data = b"hello tus";
        let wrong = vec![0u8; 20]; // wrong SHA1 (all zeros)
        let cursor = Cursor::new(data.to_vec());
        let mut reader = ChecksumReader::new(cursor, ChecksumAlgorithm::Sha1, wrong);
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();
        assert!(matches!(reader.verify(), Err(TusError::ChecksumMismatch)));
    }

    #[test]
    fn parse_checksum_header_sha1() {
        let data = b"test";
        let hash = sha1_b64(data);
        let b64 = STANDARD.encode(&hash);
        let header = format!("sha1 {b64}");
        let (alg, decoded) = parse_checksum_header(&header).unwrap();
        assert_eq!(alg, ChecksumAlgorithm::Sha1);
        assert_eq!(decoded, hash);
    }

    #[test]
    fn parse_checksum_header_unknown_algorithm() {
        let err = parse_checksum_header("crc32 AAAA").unwrap_err();
        assert!(matches!(err, TusError::UnsupportedChecksumAlgorithm(_)));
    }

    #[test]
    fn parse_checksum_header_bad_base64() {
        let err = parse_checksum_header("sha1 not_valid!!").unwrap_err();
        assert!(matches!(err, TusError::InvalidMetadata(_)));
    }

    #[test]
    fn parse_checksum_header_missing_space() {
        let err = parse_checksum_header("sha1").unwrap_err();
        assert!(matches!(err, TusError::InvalidMetadata(_)));
    }
}
