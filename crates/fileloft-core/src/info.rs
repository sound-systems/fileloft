use std::collections::HashMap;

use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::TusError;

/// Newtype around String making upload IDs distinct in function signatures.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UploadId(pub String);

impl UploadId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for UploadId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for UploadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for UploadId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for UploadId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Key-value pairs from the `Upload-Metadata` header.
/// Values are base64-decoded strings; a key with no value has `None`.
///
/// Wire format: `"key1 base64val1,key2 base64val2,keyOnly"`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata(pub HashMap<String, Option<String>>);

impl Metadata {
    /// Parse the `Upload-Metadata` header value.
    pub fn parse(header: &str) -> Result<Self, TusError> {
        let mut map = HashMap::new();
        for pair in header.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let mut parts = pair.splitn(2, ' ');
            let key = parts.next().unwrap_or("").trim().to_string();
            if key.is_empty() {
                return Err(TusError::InvalidMetadata("empty key in metadata".into()));
            }
            let value = match parts.next() {
                None | Some("") => None,
                Some(b64) => {
                    let decoded = STANDARD
                        .decode(b64.trim())
                        .map_err(|e| TusError::InvalidMetadata(e.to_string()))?;
                    Some(String::from_utf8(decoded).map_err(|e| {
                        TusError::InvalidMetadata(format!("non-UTF8 value for key {key}: {e}"))
                    })?)
                }
            };
            map.insert(key, value);
        }
        Ok(Self(map))
    }

    /// Encode back to the `Upload-Metadata` wire format.
    pub fn encode(&self) -> String {
        self.0
            .iter()
            .map(|(k, v)| match v {
                None => k.clone(),
                Some(val) => format!("{k} {}", STANDARD.encode(val)),
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn get(&self, key: &str) -> Option<&Option<String>> {
        self.0.get(key)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Complete state of a single upload resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadInfo {
    pub id: UploadId,
    /// Declared total size in bytes. `None` when `Upload-Defer-Length: 1` was used.
    pub size: Option<u64>,
    /// Bytes successfully written so far.
    pub offset: u64,
    pub metadata: Metadata,
    /// True when created with `Upload-Defer-Length: 1` and size not yet set.
    pub size_is_deferred: bool,
    /// Set by the expiration extension.
    pub expires_at: Option<DateTime<Utc>>,
    /// True for partial uploads (concatenation extension).
    pub is_partial: bool,
    /// True for final assembled uploads (concatenation extension).
    pub is_final: bool,
    /// IDs of partial uploads used to build this final upload.
    pub partial_uploads: Vec<UploadId>,
    /// Opaque storage-backend-specific metadata (e.g. file path, S3 key).
    pub storage: HashMap<String, String>,
}

impl UploadInfo {
    pub fn new(id: UploadId, size: Option<u64>) -> Self {
        Self {
            id,
            size,
            offset: 0,
            metadata: Metadata::default(),
            size_is_deferred: size.is_none(),
            expires_at: None,
            is_partial: false,
            is_final: false,
            partial_uploads: Vec::new(),
            storage: HashMap::new(),
        }
    }

    /// Returns true when all bytes have been received.
    pub fn is_complete(&self) -> bool {
        match self.size {
            Some(s) => self.offset == s,
            None => false,
        }
    }
}

/// Fields a `pre_create` hook may override before the upload slot is created.
#[derive(Debug, Default)]
pub struct UploadInfoChanges {
    pub id: Option<UploadId>,
    pub metadata: Option<Metadata>,
    pub storage: Option<HashMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_parse_key_value() {
        let m = Metadata::parse("filename dGVzdC50eHQ=,type dGV4dC9wbGFpbg==").unwrap();
        assert_eq!(m.0["filename"], Some("test.txt".into()));
        assert_eq!(m.0["type"], Some("text/plain".into()));
    }

    #[test]
    fn metadata_parse_key_only() {
        let m = Metadata::parse("is_private").unwrap();
        assert_eq!(m.0["is_private"], None);
    }

    #[test]
    fn metadata_parse_mixed() {
        let m = Metadata::parse("filename dGVzdC50eHQ=,is_private,size MTAyNA==").unwrap();
        assert_eq!(m.0["filename"], Some("test.txt".into()));
        assert_eq!(m.0["is_private"], None);
        assert_eq!(m.0["size"], Some("1024".into()));
    }

    #[test]
    fn metadata_parse_empty_string() {
        let m = Metadata::parse("").unwrap();
        assert!(m.0.is_empty());
    }

    #[test]
    fn metadata_parse_invalid_base64() {
        assert!(Metadata::parse("key not!!valid_b64").is_err());
    }

    #[test]
    fn metadata_roundtrip() {
        let original = "filename dGVzdC50eHQ=";
        let m = Metadata::parse(original).unwrap();
        let encoded = m.encode();
        let m2 = Metadata::parse(&encoded).unwrap();
        assert_eq!(m.0["filename"], m2.0["filename"]);
    }

    #[test]
    fn upload_info_is_complete() {
        let mut info = UploadInfo::new(UploadId::new(), Some(100));
        assert!(!info.is_complete());
        info.offset = 100;
        assert!(info.is_complete());
    }

    #[test]
    fn upload_info_deferred_never_complete_until_size_set() {
        let info = UploadInfo::new(UploadId::new(), None);
        assert!(!info.is_complete());
    }
}
