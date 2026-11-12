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

/// The mode of an object's retention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Mode {
    /// Can be shortened, removed or overridden by a caller who asks to bypass it (and is allowed to).
    Governance,
    /// Cannot be shortened, removed or overridden by anyone until it expires.
    Compliance,
}

impl Mode {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Governance => "GOVERNANCE",
            Self::Compliance => "COMPLIANCE",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "GOVERNANCE" => Some(Self::Governance),
            "COMPLIANCE" => Some(Self::Compliance),
            _ => None,
        }
    }
}

/// An object version's retention: a mode and the instant it lasts until.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Retention {
    pub(crate) mode: Mode,
    pub(crate) until_ms: i64,
}

/// What Object Lock holds on an object version.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Lock {
    pub(crate) retention: Option<Retention>,
    /// `None` until a legal hold was ever set (AWS answers a read of it with an error); `Some(false)` is `OFF`.
    pub(crate) legal_hold: Option<bool>,
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
    pub(crate) lock: Lock,
}

/// A version row as record version 1 wrote it, before Object Lock: read, and given no lock.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct VersionV1 {
    key: String,
    version_id: String,
    seq: u64,
    delete_marker: bool,
    modified_ms: i64,
    size: u64,
    etag: String,
    meta: Meta,
    checksum: Option<Checksum>,
    segs: Vec<Seg>,
    parts: Vec<PartInfo>,
    tags: Vec<(String, String)>,
}

impl From<VersionV1> for Version {
    fn from(v: VersionV1) -> Self {
        Self {
            key: v.key,
            version_id: v.version_id,
            seq: v.seq,
            delete_marker: v.delete_marker,
            modified_ms: v.modified_ms,
            size: v.size,
            etag: v.etag,
            meta: v.meta,
            checksum: v.checksum,
            segs: v.segs,
            parts: v.parts,
            tags: v.tags,
            lock: Lock::default(),
        }
    }
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
