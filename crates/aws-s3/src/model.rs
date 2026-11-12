//! What S3 keeps: buckets with their configuration, object versions (and delete markers), multipart uploads and their parts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::checksum::Algo;

/// A bucket's versioning state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Versioning {
    /// Never enabled: objects have the version id `null` and an overwrite replaces.
    #[default]
    Off,
    Enabled,
    /// Was enabled: new objects are the `null` version, and older versions stay.
    Suspended,
}

/// A bucket.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Bucket {
    pub(crate) created_ms: i64,
    pub(crate) versioning: Versioning,
    /// The next sequence number of a version in this bucket: a newer version has a larger one.
    pub(crate) next_seq: u64,
    /// Configuration stored as the request carried it (as JSON), by kind (`cors`, `policy`, `lifecycle`, ...), returned as sent.
    pub(crate) configs: BTreeMap<String, String>,
    /// Object Lock was enabled when the bucket was made.
    pub(crate) object_lock: bool,
}

/// The headers an object keeps and returns.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Meta {
    pub(crate) content_type: Option<String>,
    pub(crate) cache_control: Option<String>,
    pub(crate) content_disposition: Option<String>,
    pub(crate) content_encoding: Option<String>,
    pub(crate) content_language: Option<String>,
    /// The `Expires` header as sent.
    pub(crate) expires: Option<String>,
    pub(crate) website_redirect: Option<String>,
    pub(crate) storage_class: Option<String>,
    /// `AES256` or `aws:kms` and the key.
    pub(crate) sse: Option<String>,
    pub(crate) sse_kms_key: Option<String>,
    pub(crate) user: BTreeMap<String, String>,
}

/// One piece of an object's bytes: a blob, of `len` bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Seg {
    pub(crate) blob: String,
    pub(crate) len: u64,
}

/// How a stored checksum was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CKind {
    /// Of all the bytes.
    FullObject,
    /// Of the parts' checksums (multipart only).
    Composite,
}

/// An object's checksum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Checksum {
    pub(crate) algo: Algo,
    /// Base64, with `-N` for a composite.
    pub(crate) value: String,
    pub(crate) kind: CKind,
}

/// A part of a completed multipart object, kept for `partNumber` reads and `GetObjectAttributes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PartInfo {
    pub(crate) number: u32,
    pub(crate) size: u64,
    pub(crate) etag: String,
    pub(crate) checksum: Option<String>,
}

/// An object version, or a delete marker.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Version {
    pub(crate) key: String,
    /// `null` for an object made while versioning was off or suspended.
    pub(crate) version_id: String,
    pub(crate) seq: u64,
    pub(crate) delete_marker: bool,
    pub(crate) modified_ms: i64,
    pub(crate) size: u64,
    /// Without quotes.
    pub(crate) etag: String,
    pub(crate) meta: Meta,
    pub(crate) checksum: Option<Checksum>,
    pub(crate) segs: Vec<Seg>,
    pub(crate) parts: Vec<PartInfo>,
    pub(crate) tags: Vec<(String, String)>,
}

/// A multipart upload in progress.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Upload {
    pub(crate) key: String,
    pub(crate) id: String,
    pub(crate) initiated_ms: i64,
    pub(crate) meta: Meta,
    pub(crate) algo: Option<Algo>,
    pub(crate) kind: Option<CKind>,
    pub(crate) tags: Vec<(String, String)>,
}

/// One uploaded part.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PartRec {
    pub(crate) blob: String,
    pub(crate) size: u64,
    pub(crate) etag: String,
    pub(crate) modified_ms: i64,
    /// Base64 digests by algorithm: the one the upload declared, and CRC64NVME for the whole-object checksum.
    pub(crate) sums: Vec<(Algo, String)>,
}

impl Version {
    /// The tags as a count, for `x-amz-tagging-count`.
    pub(crate) fn tag_count(&self) -> usize {
        self.tags.len()
    }
}
