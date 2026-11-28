//! CopyObject, and the byte copy UploadPartCopy shares. A copy is asked of the actor twice: first it is checked and planned (which
//! stretches of which blobs to read), then the bytes are read, summed and written into a new blob by one job on a blocking thread,
//! outside the actor, and the copy is asked again with the blob, checked again and kept.

use aws_wire::ApiError;
use doze_fs::fs::{Fs, FsFile, RealFs};
use serde_json::{Map, Value, json};

use crate::blob::{Blobs, Staging};
use crate::checksum::{Algo, Done, Sums};
use crate::core::Core;
use crate::errors::{invalid_argument, invalid_request, no_such_bucket, precondition_failed};
use crate::events::Kind;
use crate::get::slices;
use crate::model::{Seg, Version, Versioning};
use crate::put::{add_checksum, check_key, meta_of, primary, tags_of};
use crate::req::{Answer, Out, Req, Slice, Staged};
use crate::timeutil::parse_http_date;

type File = <RealFs as Fs>::File;

/// The bytes a copy reads and writes at a time.
const PIECE: usize = 1024 * 1024;

/// The bucket, key and version a `CopySource` names.
pub(crate) fn parse_source(raw: &str) -> Option<(String, String, Option<String>)> {
    let trimmed = raw.strip_prefix('/').unwrap_or(raw);
    let (path, version) = match trimmed.split_once("?versionId=") {
        Some((p, v)) => (p, Some(v.to_owned())),
        None => (trimmed, None),
    };
    let decoded = aws_core::uri::decode_str(path)?;
    let (bucket, key) = decoded.split_once('/')?;
    (!bucket.is_empty() && !key.is_empty()).then(|| (bucket.to_owned(), key.to_owned(), version))
}

fn io_failed(e: impl std::fmt::Display) -> ApiError {
    ApiError::internal(format!("the object could not be copied: {e}"))
}

/// Copies `slices` into a new blob of `blobs`, summing with `want`, then flushes and names it: all of it in one job on a blocking
/// thread, outside the actor, a [`PIECE`] at a time through one buffer. Before, each piece was two blocking jobs (a read, a write)
/// and was summed on the event loop, inside the actor, which no other call of the region could reach until the copy was done.
///
/// # Errors
/// A source that cannot be read, or a disk that refuses; the staging file is removed.
pub(crate) async fn copy_blob(
    blobs: Blobs<RealFs>,
    slices: Vec<Slice>,
    want: Vec<Algo>,
) -> Result<Staged, ApiError> {
    tokio::task::spawn_blocking(move || {
        let mut staging = blobs.begin().map_err(io_failed)?;
        let done = match copy_into(&mut staging, &slices, &want) {
            Ok(done) => done,
            Err(e) => {
                blobs.discard(&staging);
                return Err(io_failed(e));
            }
        };
        let blob = blobs.publish(&staging).map_err(io_failed)?;
        Ok(Staged {
            blob,
            len: staging.len,
            done,
            source: None,
        })
    })
    .await
    .map_err(io_failed)?
}

fn copy_into(
    staging: &mut Staging<RealFs>,
    slices: &[Slice],
    want: &[Algo],
) -> std::io::Result<Done> {
    let mut sums = Sums::new(want);
    let mut buf = vec![0u8; PIECE];
    for s in slices {
        let (mut at, end) = (s.from, s.from + s.len);
        while at < end {
            let n = usize::try_from((end - at).min(PIECE as u64)).unwrap_or(PIECE);
            let piece = buf.get_mut(..n).unwrap_or_default();
            read_piece(&s.file, at, piece)?;
            sums.update(piece);
            staging.file.write_all_at(piece, staging.len)?;
            staging.len += n as u64;
            at += n as u64;
        }
    }
    Ok(sums.finish())
}

fn read_piece(file: &File, from: u64, buf: &mut [u8]) -> std::io::Result<()> {
    let mut got = 0;
    while got < buf.len() {
        let m = file.read_at(buf.get_mut(got..).unwrap_or_default(), from + got as u64)?;
        if m == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        got += m;
    }
    Ok(())
}

/// What a copy is to read, as the actor's plan answers it: the stretches of the source in `body`, and in `json` the algorithm and
/// the source version's sequence number, which the copy's second asking checks is still the source's.
pub(crate) fn plan(slices: Vec<Slice>, algo: Algo, source: &Version) -> Answer {
    Ok(Out {
        json: json!({"Algo": algo.name(), "Seq": source.seq}),
        status: None,
        body: Some(slices),
    })
}

/// The copy's bytes, as its second asking is handed them, checked against the source as it is now: it must be the version that
/// was copied (it was replaced in between: AWS's words for a conflicting operation).
pub(crate) fn copied(req: &mut Req<'_>, source: &Version) -> Result<Staged, ApiError> {
    let staged = req
        .staged
        .take()
        .ok_or_else(|| ApiError::internal("the copy's bytes were not made"))?;
    if staged.source != Some(source.seq) {
        return Err(ApiError::new(
            409,
            "OperationAborted",
            "A conflicting conditional operation is currently in progress against this resource. Please try again.",
        ));
    }
    Ok(staged)
}

impl Core {
    /// The source version of a copy, with its conditions checked.
    fn copy_source(&self, req: &Req<'_>) -> Result<Version, ApiError> {
        let raw = req.s("CopySource").ok_or_else(|| {
            invalid_argument(
                "Copy Source must mention the source bucket and key: sourcebucket/sourcekey",
            )
        })?;
        let (b, k, id) = parse_source(raw).ok_or_else(|| {
            invalid_argument(
                "Copy Source must mention the source bucket and key: sourcebucket/sourcekey",
            )
        })?;
        if self.buckets.get(&b).is_none() {
            return Err(no_such_bucket(&b));
        }
        let v = self.readable(&b, &k, id.as_deref())?;
        crate::get::check_readable(&v)?;
        let at = |name: &str| req.s(name).and_then(parse_http_date);
        let modified = v.modified_ms / 1000;
        let etag_ok = |list: &str| {
            list.split(',')
                .any(|e| e.trim().trim_matches('"') == v.etag || e.trim() == "*")
        };
        if req.s("CopySourceIfMatch").is_some_and(|m| !etag_ok(m))
            || at("CopySourceIfUnmodifiedSince").is_some_and(|t| modified > t)
        {
            return Err(precondition_failed("x-amz-copy-source-If-Match"));
        }
        if req.s("CopySourceIfNoneMatch").is_some_and(etag_ok)
            || at("CopySourceIfModifiedSince").is_some_and(|t| modified <= t)
        {
            return Err(precondition_failed("x-amz-copy-source-If-None-Match"));
        }
        Ok(v)
    }

    /// The checks of a copy, made when it is planned and again when its bytes are handed in.
    fn check_copy(&self, req: &Req<'_>) -> Result<Checked, ApiError> {
        let bucket = req.bucket();
        check_key(req.key())?;
        let dest = self.require(bucket)?.versioning;
        let src = self.copy_source(req)?;
        let replace = req.s("MetadataDirective") == Some("REPLACE");
        check_not_onto_itself(req, &src, bucket, replace, dest)?;
        // The copy has the lock its own request names (or the bucket's default), never the source's.
        let lock = self.new_lock(bucket, crate::lock::requested(req, self.now_ms())?)?;
        // A copy has the ACL its own request names, never the source's (memory: as AWS documents for CopyObject).
        let acl = self.object_write_acl(req)?;
        self.guard_replace(bucket, req.key())?;
        let algo = req
            .s("ChecksumAlgorithm")
            .and_then(Algo::parse)
            .or(src.checksum.as_ref().map(|c| c.algo))
            .unwrap_or(Algo::Crc64Nvme);
        Ok(Checked {
            src,
            dest,
            replace,
            lock,
            acl,
            algo,
        })
    }

    /// What a `CopyObject` is to read: the whole source.
    pub(crate) fn plan_copy_object(&self, req: &Req<'_>) -> Answer {
        let c = self.check_copy(req)?;
        plan(slices(self, &c.src.segs, 0, c.src.size)?, c.algo, &c.src)
    }

    pub(crate) fn copy_object(&mut self, mut req: Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket().to_owned(), req.key().to_owned());
        let Checked {
            src,
            dest,
            replace,
            lock,
            acl,
            ..
        } = self.check_copy(&req)?;
        let Staged {
            blob, len, done, ..
        } = copied(&mut req, &src)?;
        let tags = if req.s("TaggingDirective") == Some("REPLACE") {
            req.s("Tagging").map(tags_of).unwrap_or_default()
        } else {
            src.tags.clone()
        };
        let v = Version {
            key,
            version_id: String::new(),
            seq: 0,
            delete_marker: false,
            modified_ms: self.now_ms(),
            size: len,
            etag: aws_core::hex::encode(&done.md5),
            meta: self.copied_meta(&req, &src, &bucket, replace),
            checksum: primary(&done),
            segs: vec![Seg { blob, len }],
            parts: Vec::new(),
            tags,
            lock,
            acl,
        };
        let stored = self.install(&bucket, v)?;
        self.note(&req, Kind::Object("ObjectCreated:Copy"), &stored);
        Out::json(copy_output(&stored, &src, dest != Versioning::Off))
    }

    /// The headers the copy keeps: the source's, or the request's when it replaces them. The storage class and the encryption are
    /// the request's whichever directive it gives (the model: `StorageClass` and `ServerSideEncryption` are their own members); a
    /// copy that names no encryption gets the destination bucket's default, not the source's (`ServerSideEncryption` in the model).
    fn copied_meta(
        &self,
        req: &Req<'_>,
        src: &Version,
        bucket: &str,
        replace: bool,
    ) -> crate::model::Meta {
        let mut meta = if replace {
            meta_of(req)
        } else {
            src.meta.clone()
        };
        if let Some(class) = req.s("StorageClass") {
            meta.storage_class = Some(class.to_owned());
        }
        let (default, default_key) = self.default_sse(bucket);
        let sse = req.s("ServerSideEncryption").unwrap_or(&default);
        // A key is the request's, else the default's when the encryption is the default's kind (KMS), else none.
        let named = req.s("SSEKMSKeyId").map(str::to_owned);
        let kms = sse.starts_with("aws:kms");
        meta.sse_kms_key = named.or(if kms { default_key } else { None });
        meta.sse = Some(sse.to_owned());
        meta
    }
}

/// A copy onto the same key that changes nothing is refused (unless versioning is on, where it makes a new version).
fn check_not_onto_itself(
    req: &Req<'_>,
    src: &Version,
    bucket: &str,
    replace: bool,
    dest: Versioning,
) -> Result<(), ApiError> {
    let same = src.key == req.key()
        && req
            .s("CopySource")
            .is_some_and(|c| c.trim_start_matches('/').starts_with(&format!("{bucket}/")));
    let changes = replace
        || dest == Versioning::Enabled
        || [
            "StorageClass",
            "ServerSideEncryption",
            "WebsiteRedirectLocation",
        ]
        .iter()
        .any(|m| req.s(m).is_some());
    if same && !changes {
        return Err(invalid_request(
            "This copy request is illegal because it is trying to copy an object to itself without changing the object's metadata, storage class, website redirect location or encryption attributes.",
        ));
    }
    Ok(())
}

fn copy_output(stored: &Version, src: &Version, versioned: bool) -> Value {
    let mut result = Map::new();
    result.insert("ETag".into(), json!(format!("\"{}\"", stored.etag)));
    result.insert("LastModified".into(), json!(stored.modified_ms / 1000));
    add_checksum(&mut result, stored.checksum.as_ref());
    let mut out = Map::new();
    out.insert("CopyObjectResult".into(), Value::Object(result));
    if let Some(sse) = &stored.meta.sse {
        out.insert("ServerSideEncryption".into(), json!(sse));
    }
    if versioned {
        out.insert("VersionId".into(), json!(stored.version_id));
    }
    if src.version_id != "null" {
        out.insert("CopySourceVersionId".into(), json!(src.version_id));
    }
    Value::Object(out)
}

/// A copy's checks, as they found it.
struct Checked {
    src: Version,
    dest: Versioning,
    replace: bool,
    lock: crate::model::Lock,
    acl: Option<Vec<crate::acl::Grant>>,
    algo: Algo,
}
