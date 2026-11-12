//! CopyObject, and the byte copy UploadPartCopy shares: the source's bytes are read in pieces on blocking threads into a new blob,
//! summed on the way.

use std::sync::Arc;

use aws_wire::ApiError;
use doze_fs::fs::{Fs, FsFile, RealFs};
use serde_json::{Map, Value, json};

use crate::blob::Staging;
use crate::checksum::{Algo, Done, Sums};
use crate::core::Core;
use crate::errors::{invalid_argument, invalid_request, no_such_bucket, precondition_failed};
use crate::get::{check_conditions, slices};
use crate::model::{Seg, Version, Versioning};
use crate::put::{add_checksum, check_key, meta_of, primary, tags_of};
use crate::req::{Answer, Out, Req, Slice};
use crate::timeutil::parse_http_date;

type File = <RealFs as Fs>::File;

const PIECE: usize = 256 * 1024;

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

/// Copies `slices` into `staging`, summing with `want`.
pub(crate) async fn copy_into(
    staging: &mut Staging<RealFs>,
    slices: Vec<Slice>,
    want: &[Algo],
) -> Result<Done, ApiError> {
    let mut sums = Sums::new(want);
    for s in slices {
        let (mut at, end) = (s.from, s.from + s.len);
        while at < end {
            let n = usize::try_from((end - at).min(PIECE as u64)).unwrap_or(PIECE);
            let (file, from) = (Arc::clone(&s.file), at);
            let piece = tokio::task::spawn_blocking(move || read_piece(&file, from, n))
                .await
                .map_err(io_failed)?
                .map_err(io_failed)?;
            sums.update(&piece);
            let (out, pos) = (staging.file(), staging.len);
            let data = piece.clone();
            tokio::task::spawn_blocking(move || out.write_all_at(&data, pos))
                .await
                .map_err(io_failed)?
                .map_err(io_failed)?;
            staging.len += piece.len() as u64;
            at += piece.len() as u64;
        }
    }
    Ok(sums.finish())
}

fn read_piece(file: &File, from: u64, n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    let mut got = 0;
    while got < n {
        let m = file.read_at(buf.get_mut(got..).unwrap_or_default(), from + got as u64)?;
        if m == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        got += m;
    }
    Ok(buf)
}

impl Core {
    /// The source version of a copy, with its conditions checked.
    fn copy_source(&self, req: &Req<'_>) -> Result<Version, ApiError> {
        let raw = req.s("CopySource").ok_or_else(|| invalid_argument("Copy Source must mention the source bucket and key: sourcebucket/sourcekey"))?;
        let (b, k, id) = parse_source(raw).ok_or_else(|| {
            invalid_argument("Copy Source must mention the source bucket and key: sourcebucket/sourcekey")
        })?;
        if self.buckets.get(&b).is_none() {
            return Err(no_such_bucket(&b));
        }
        let v = self.readable(&b, &k, id.as_deref())?;
        let at = |name: &str| req.s(name).and_then(parse_http_date);
        let modified = v.modified_ms / 1000;
        let etag_ok = |list: &str| list.split(',').any(|e| e.trim().trim_matches('"') == v.etag || e.trim() == "*");
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
        let _ = check_conditions;
        Ok(v)
    }

    pub(crate) async fn copy_object(&mut self, req: &Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket().to_owned(), req.key().to_owned());
        check_key(&key)?;
        let dest = self.require(&bucket)?.versioning;
        let src = self.copy_source(req)?;
        let replace = req.s("MetadataDirective") == Some("REPLACE");
        let same = src.key == key && req.s("CopySource").is_some_and(|c| c.trim_start_matches('/').starts_with(&format!("{bucket}/")));
        if same && !replace && dest != Versioning::Enabled && req.s("StorageClass").is_none() && req.s("ServerSideEncryption").is_none() && req.s("WebsiteRedirectLocation").is_none() {
            return Err(invalid_request(
                "This copy request is illegal because it is trying to copy an object to itself without changing the object's metadata, storage class, website redirect location or encryption attributes.",
            ));
        }
        let algos = [req.s("ChecksumAlgorithm").and_then(Algo::parse).or(src.checksum.as_ref().map(|c| c.algo)).unwrap_or(Algo::Crc64Nvme)];
        let mut staging = self.blobs.begin().map_err(io_failed)?;
        let sl = slices(self, &src.segs, 0, src.size)?;
        let done = match copy_into(&mut staging, sl, &algos).await {
            Ok(d) => d,
            Err(e) => {
                self.blobs.discard(&staging);
                return Err(e);
            }
        };
        let blob = self.blobs.publish(&staging).map_err(io_failed)?;
        let mut meta = if replace { meta_of(req) } else { src.meta.clone() };
        let (sse, kms) = self.default_sse(&bucket);
        if let Some(s) = req.s("ServerSideEncryption") {
            meta.sse = Some(s.to_owned());
        }
        meta.sse.get_or_insert(sse);
        if meta.sse_kms_key.is_none() {
            meta.sse_kms_key = kms;
        }
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
            size: staging.len,
            etag: aws_core::hex::encode(&done.md5),
            meta,
            checksum: primary(&done),
            segs: vec![Seg { blob, len: staging.len }],
            parts: Vec::new(),
            tags,
        };
        let stored = self.install(&bucket, v)?;
        Out::json(copy_output(&stored, &src, dest != Versioning::Off))
    }
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
