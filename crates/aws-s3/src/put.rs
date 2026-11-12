//! Writing an object: its headers, its place among the versions of its key, and PutObject.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::checksum::{Algo, Done, b64};
use crate::core::Core;
use crate::errors::{invalid_argument, key_too_long, precondition_failed};
use crate::events::Kind;
use crate::model::{CKind, Checksum, Meta, Seg, Version, Versioning};
use crate::req::{Answer, Out, Req};

/// The longest key S3 accepts, in bytes.
const MAX_KEY: usize = 1024;

/// The headers an object keeps, out of a request that carries them.
pub(crate) fn meta_of(req: &Req<'_>) -> Meta {
    let text = |name: &str| req.s(name).map(str::to_owned);
    let user: BTreeMap<String, String> = req
        .get("Metadata")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((k.to_ascii_lowercase(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    Meta {
        content_type: text("ContentType"),
        cache_control: text("CacheControl"),
        content_disposition: text("ContentDisposition"),
        content_encoding: text("ContentEncoding"),
        content_language: text("ContentLanguage"),
        expires: text("ExpiresString").or_else(|| text("Expires")),
        website_redirect: text("WebsiteRedirectLocation"),
        storage_class: text("StorageClass"),
        sse: text("ServerSideEncryption"),
        sse_kms_key: text("SSEKMSKeyId"),
        user,
    }
}

/// `k=v&k2=v2` (the `x-amz-tagging` header) as pairs.
pub(crate) fn tags_of(header: &str) -> Vec<(String, String)> {
    let decode = |s: &str| {
        let spaced = s.replace('+', " ");
        aws_core::uri::decode_str(&spaced).map_or(spaced.clone(), std::borrow::Cow::into_owned)
    };
    header
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

/// A new version id: 32 letters and digits.
pub(crate) fn new_version_id() -> String {
    const CHARS: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut b = [0u8; 32];
    // WHY: with no randomness the id is the zero byte's letter repeated; the sequence number in the row key keeps versions apart.
    if getrandom::fill(&mut b).is_err() {
        b.fill(0);
    }
    b.iter()
        .map(|x| {
            char::from(
                CHARS
                    .get(usize::from(*x) % CHARS.len())
                    .copied()
                    .unwrap_or(b'0'),
            )
        })
        .collect()
}

pub(crate) fn check_key(key: &str) -> Result<(), ApiError> {
    if key.len() > MAX_KEY {
        return Err(key_too_long());
    }
    if key.is_empty() || key.contains('\0') {
        return Err(invalid_argument("The specified key is not valid."));
    }
    Ok(())
}

impl Core {
    /// Makes `v` the newest version of its key in `bucket`, replacing the version it supersedes where the bucket's versioning says
    /// it replaces (the `null` version when versioning is off or suspended).
    pub(crate) fn install(&mut self, bucket: &str, mut v: Version) -> Result<Version, ApiError> {
        let state = self.require(bucket)?.versioning;
        let existing = self.versions_of(bucket, &v.key)?;
        v.seq = self.next_seq(bucket);
        if state == Versioning::Enabled {
            v.version_id = new_version_id();
        } else {
            "null".clone_into(&mut v.version_id);
            for old in existing.iter().filter(|o| o.version_id == "null") {
                self.drop_row(bucket, old);
            }
        }
        self.put_row(bucket, &v)?;
        Ok(v)
    }

    /// The server-side encryption a new object gets when the request does not say: the bucket's default, else `AES256`.
    pub(crate) fn default_sse(&self, bucket: &str) -> (String, Option<String>) {
        let found = self
            .buckets
            .get(bucket)
            .and_then(|b| b.configs.get("encryption"))
            .and_then(|c| serde_json::from_str::<Value>(c).ok())
            .and_then(|c| {
                let d = c
                    .get("Rules")?
                    .get(0)?
                    .get("ApplyServerSideEncryptionByDefault")?
                    .clone();
                Some((
                    d.get("SSEAlgorithm")?.as_str()?.to_owned(),
                    d.get("KMSMasterKeyID")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                ))
            });
        found.unwrap_or_else(|| ("AES256".to_owned(), None))
    }

    /// A conditional write: `If-None-Match: *` and `If-Match`.
    fn check_write_conditions(
        &self,
        req: &Req<'_>,
        current: Option<&Version>,
    ) -> Result<(), ApiError> {
        let live = current.filter(|v| !v.delete_marker);
        if req.s("IfNoneMatch") == Some("*") && live.is_some() {
            return Err(precondition_failed("If-None-Match"));
        }
        if let Some(want) = req.s("IfMatch")
            && live.is_none_or(|v| v.etag != want.trim_matches('"'))
        {
            return Err(precondition_failed("If-Match"));
        }
        Ok(())
    }

    pub(crate) async fn put_object(&mut self, mut req: Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket().to_owned(), req.key().to_owned());
        check_key(&key)?;
        self.require(&bucket)?;
        let current = self.latest(&bucket, &key)?;
        self.check_write_conditions(&req, current.as_ref())?;
        let lock = self.new_lock(&bucket, crate::lock::requested(&req, self.now_ms())?)?;
        self.guard_replace(&bucket, &key)?;
        let staged = req
            .staged
            .take()
            .ok_or_else(|| ApiError::internal("the object body was not received"))?;
        let blob = self
            .blobs
            .publish(&staged.staging)
            .map_err(|e| ApiError::internal(format!("the object could not be stored: {e}")))?;
        let mut meta = meta_of(&req);
        let (sse, kms) = self.default_sse(&bucket);
        meta.sse.get_or_insert(sse);
        if meta.sse_kms_key.is_none() {
            meta.sse_kms_key = kms;
        }
        let v = Version {
            key,
            version_id: String::new(),
            seq: 0,
            delete_marker: false,
            modified_ms: self.now_ms(),
            size: staged.staging.len,
            etag: aws_core::hex::encode(&staged.done.md5),
            meta,
            checksum: primary(&staged.done),
            segs: vec![Seg {
                blob,
                len: staged.staging.len,
            }],
            parts: Vec::new(),
            tags: req.s("Tagging").map(tags_of).unwrap_or_default(),
            lock,
        };
        let stored = self.install(&bucket, v)?;
        self.note(&req, Kind::Object("ObjectCreated:Put"), &stored);
        Out::json(self.put_output(&bucket, &stored))
    }

    fn put_output(&self, bucket: &str, v: &Version) -> Value {
        let mut out = Map::new();
        out.insert("ETag".into(), json!(format!("\"{}\"", v.etag)));
        add_checksum(&mut out, v.checksum.as_ref());
        if let Some(sse) = &v.meta.sse {
            out.insert("ServerSideEncryption".into(), json!(sse));
        }
        if let Some(k) = &v.meta.sse_kms_key {
            out.insert("SSEKMSKeyId".into(), json!(k));
        }
        if self
            .buckets
            .get(bucket)
            .is_some_and(|b| b.versioning != Versioning::Off)
        {
            out.insert("VersionId".into(), json!(v.version_id));
        }
        Value::Object(out)
    }
}

/// The checksum a body is stored with: the first it was summed with (the declared one, else CRC64NVME).
pub(crate) fn primary(done: &Done) -> Option<Checksum> {
    let (algo, raw) = done.sums.first()?;
    Some(Checksum {
        algo: *algo,
        value: b64(raw),
        kind: CKind::FullObject,
    })
}

/// `ChecksumCRC32: …` and `ChecksumType`, for the checksum an answer carries.
pub(crate) fn add_checksum(out: &mut Map<String, Value>, c: Option<&Checksum>) {
    if let Some(c) = c {
        out.insert(c.algo.member().into(), json!(c.value));
        let kind = match c.kind {
            CKind::FullObject => "FULL_OBJECT",
            CKind::Composite => "COMPOSITE",
        };
        out.insert("ChecksumType".into(), json!(kind));
    }
}

/// The algorithm of a checksum, for `ChecksumAlgorithm` in listings.
pub(crate) fn algo_name(c: Option<&Checksum>) -> Vec<&'static str> {
    c.map(|c| c.algo.name()).into_iter().collect()
}

/// Unused algorithms are listed here so the set stays whole when one is added.
#[allow(dead_code)]
const _ALL: [Algo; 5] = Algo::ALL;
