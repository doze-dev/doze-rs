//! Multipart uploads: start, upload and copy parts, abort. Each part is its own blob; completing the upload gives the object those
//! blobs as its segments, so no byte is copied.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::checksum::{Algo, b64};
use crate::copy::{copy_into, parse_source};
use crate::core::Core;
use crate::errors::{invalid_argument, invalid_range, no_such_bucket, no_such_upload};
use crate::get::{Span, parse_range, slices};
use crate::model::{CKind, PartRec, Upload};
use crate::put::{check_key, meta_of, tags_of};
use crate::req::{Answer, Out, Req};

/// The most parts an upload may have.
pub(crate) const MAX_PART: i64 = 10_000;

fn id_text() -> String {
    const CHARS: &[u8; 64] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ._";
    let mut b = [0u8; 64];
    // WHY: with no randomness the id is a run of one letter; the key and a sequence keep two uploads of one key apart.
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

pub(crate) fn upload_row(bucket: &str, key: &str, id: &str) -> String {
    format!("{bucket}\0{key}\0{id}")
}

pub(crate) fn part_row(id: &str, number: u32) -> String {
    format!("{id}\0{number:05}")
}

impl Core {
    /// The upload `id` of `key`, or `NoSuchUpload`.
    pub(crate) fn upload(&self, bucket: &str, key: &str, id: &str) -> Result<&Upload, ApiError> {
        self.require(bucket)?;
        self.uploads
            .get(&upload_row(bucket, key, id))
            .ok_or_else(|| no_such_upload(id))
    }

    /// Removes an upload record (by row) and its parts, and dooms the parts' blobs.
    pub(crate) fn remove_upload(&mut self, row: &str) {
        let Some(u) = self.uploads.remove(row) else {
            return;
        };
        self.upload_locks.remove(row);
        let prefix = format!("{}\0", u.id);
        let parts: Vec<(String, String)> = self
            .parts
            .with_prefix(&prefix)
            .map(|(k, p)| (k.to_owned(), p.blob.clone()))
            .collect();
        for (k, blob) in parts {
            self.parts.remove(&k);
            self.doomed.push(blob);
        }
    }

    pub(crate) fn create_upload(&mut self, req: &Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket(), req.key());
        check_key(key)?;
        self.require(bucket)?;
        // Checked now so the refusal comes at the start; the bucket's default retention is applied when the upload completes.
        let asked = crate::lock::requested(req, self.now_ms())?;
        self.new_lock(bucket, asked)?;
        let algo = req.s("ChecksumAlgorithm").and_then(Algo::parse);
        let kind = algo.map(|a| match req.s("ChecksumType") {
            Some("FULL_OBJECT") => CKind::FullObject,
            Some("COMPOSITE") => CKind::Composite,
            _ if a == Algo::Crc64Nvme => CKind::FullObject,
            _ => CKind::Composite,
        });
        let id = id_text();
        let mut meta = meta_of(req);
        let (sse, kms) = self.default_sse(bucket);
        meta.sse.get_or_insert(sse.clone());
        if meta.sse_kms_key.is_none() {
            meta.sse_kms_key = kms;
        }
        let upload = Upload {
            key: key.to_owned(),
            id: id.clone(),
            initiated_ms: self.now_ms(),
            meta,
            algo,
            kind,
            tags: req.s("Tagging").map(tags_of).unwrap_or_default(),
        };
        let row = upload_row(bucket, key, &id);
        if asked != crate::model::Lock::default() {
            self.upload_locks.insert(row.clone(), asked);
        }
        self.uploads.insert(row, upload);
        let mut out = Map::new();
        out.insert("Bucket".into(), json!(bucket));
        out.insert("Key".into(), json!(key));
        out.insert("UploadId".into(), json!(id));
        out.insert("ServerSideEncryption".into(), json!(sse));
        if let (Some(a), Some(k)) = (algo, kind) {
            out.insert("ChecksumAlgorithm".into(), json!(a.name()));
            let kind = if k == CKind::FullObject {
                "FULL_OBJECT"
            } else {
                "COMPOSITE"
            };
            out.insert("ChecksumType".into(), json!(kind));
        }
        Out::json(Value::Object(out))
    }

    /// The part number of a request, checked.
    fn part_number(req: &Req<'_>) -> Result<u32, ApiError> {
        req.int("PartNumber")
            .filter(|n| (1..=MAX_PART).contains(n))
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| {
                invalid_argument("Part number must be an integer between 1 and 10000, inclusive")
            })
    }

    pub(crate) fn upload_part(&mut self, mut req: Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket().to_owned(), req.key().to_owned());
        let id = req.s("UploadId").unwrap_or_default().to_owned();
        let number = Self::part_number(&req)?;
        self.upload(&bucket, &key, &id)?;
        let staged = req
            .staged
            .take()
            .ok_or_else(|| ApiError::internal("the part body was not received"))?;
        let blob = self
            .blobs
            .publish(&staged.staging)
            .map_err(|e| ApiError::internal(format!("the part could not be stored: {e}")))?;
        let etag = aws_core::hex::encode(&staged.done.md5);
        let sums: Vec<(Algo, String)> = staged
            .done
            .sums
            .iter()
            .map(|(a, raw)| (*a, b64(raw)))
            .collect();
        let echo = declared_in(&req);
        let out = part_output(
            &etag,
            &sums,
            &echo,
            self.uploads.get(&upload_row(&bucket, &key, &id)),
        );
        self.set_part(
            &id,
            number,
            PartRec {
                blob,
                size: staged.staging.len,
                etag,
                modified_ms: self.now_ms(),
                sums,
            },
        );
        Out::json(out)
    }

    /// Stores a part, and dooms the blob of the one it replaces.
    fn set_part(&mut self, id: &str, number: u32, part: PartRec) {
        let row = part_row(id, number);
        if let Some(old) = self.parts.get(&row) {
            self.doomed.push(old.blob.clone());
        }
        self.parts.insert(row, part);
    }

    pub(crate) async fn upload_part_copy(&mut self, req: &Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket().to_owned(), req.key().to_owned());
        let id = req.s("UploadId").unwrap_or_default().to_owned();
        let number = Self::part_number(req)?;
        self.upload(&bucket, &key, &id)?;
        let (sb, sk, sv) = req.s("CopySource").and_then(parse_source).ok_or_else(|| {
            invalid_argument(
                "Copy Source must mention the source bucket and key: sourcebucket/sourcekey",
            )
        })?;
        if self.buckets.get(&sb).is_none() {
            return Err(no_such_bucket(&sb));
        }
        let src = self.readable(&sb, &sk, sv.as_deref())?;
        let (from, len) = match req.s("CopySourceRange").map(|r| parse_range(r, src.size)) {
            None | Some(Span::Whole) => (0, src.size),
            Some(Span::Bytes(a, b)) => (a, b - a + 1),
            Some(Span::Unsatisfiable) => {
                return Err(invalid_range(
                    src.size,
                    req.s("CopySourceRange").unwrap_or_default(),
                ));
            }
        };
        let mut staging = self
            .blobs
            .begin()
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let parts = slices(self, &src.segs, from, len)?;
        let want = [Algo::Crc64Nvme];
        let done = match copy_into(&mut staging, parts, &want).await {
            Ok(d) => d,
            Err(e) => {
                self.blobs.discard(&staging);
                return Err(e);
            }
        };
        let blob = self
            .blobs
            .publish(&staging)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let etag = aws_core::hex::encode(&done.md5);
        let modified = self.now_ms();
        let sums = done.sums.iter().map(|(a, raw)| (*a, b64(raw))).collect();
        self.set_part(
            &id,
            number,
            PartRec {
                blob,
                size: staging.len,
                etag: etag.clone(),
                modified_ms: modified,
                sums,
            },
        );
        Out::json(
            json!({"CopyPartResult": {"ETag": format!("\"{etag}\""), "LastModified": modified / 1000}}),
        )
    }

    pub(crate) fn abort_upload(&mut self, req: &Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket(), req.key());
        let id = req.s("UploadId").unwrap_or_default();
        self.upload(bucket, key, id)?;
        self.remove_upload(&upload_row(bucket, key, id));
        Out::empty()
    }
}

/// The checksum algorithms a part request declared, which the answer echoes.
fn declared_in(req: &Req<'_>) -> Vec<Algo> {
    let mut v: Vec<Algo> = req
        .s("ChecksumAlgorithm")
        .and_then(Algo::parse)
        .into_iter()
        .collect();
    for a in Algo::ALL {
        if req.s(a.member()).is_some() && !v.contains(&a) {
            v.push(a);
        }
    }
    v
}

fn part_output(
    etag: &str,
    sums: &[(Algo, String)],
    echo: &[Algo],
    upload: Option<&Upload>,
) -> Value {
    let mut out = Map::new();
    out.insert("ETag".into(), json!(format!("\"{etag}\"")));
    let wanted = echo.iter().copied().chain(upload.and_then(|u| u.algo));
    for a in wanted {
        if let Some((_, v)) = sums.iter().find(|(x, _)| *x == a) {
            out.insert(a.member().into(), json!(v));
        }
    }
    if let Some(sse) = upload.and_then(|u| u.meta.sse.as_ref()) {
        out.insert("ServerSideEncryption".into(), json!(sse));
    }
    Value::Object(out)
}
