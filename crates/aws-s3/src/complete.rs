//! Completing a multipart upload: the listed parts are checked against what was uploaded, and become the object's segments.

use aws_wire::ApiError;
use md5::{Digest, Md5};
use serde_json::{Map, Value, json};

use crate::checksum::{Algo, b64, combine, composite};
use crate::core::Core;
use crate::events::Kind;
use crate::errors::malformed_xml;
use crate::model::{CKind, Checksum, PartInfo, PartRec, Seg, Upload, Version};
use crate::multipart::{part_row, upload_row};
use crate::put::add_checksum;
use crate::req::{Answer, Out, Req};

/// The smallest part of an upload that is not its last: 5 MiB.
const MIN_PART: u64 = 5 * 1024 * 1024;

fn invalid_part() -> ApiError {
    ApiError::new(
        400,
        "InvalidPart",
        "One or more of the specified parts could not be found.  The part may not have been uploaded, or the specified entity tag may not match the part's entity tag.",
    )
}

/// One part as the request lists it.
struct Listed {
    number: u32,
    etag: String,
}

fn listed(req: &Req<'_>) -> Result<Vec<Listed>, ApiError> {
    let parts = req
        .get("MultipartUpload")
        .and_then(|m| m.get("Parts"))
        .and_then(Value::as_array)
        .filter(|p| !p.is_empty())
        .ok_or_else(malformed_xml)?;
    let mut out = Vec::new();
    for p in parts {
        let number = p.get("PartNumber").and_then(Value::as_i64).and_then(|n| u32::try_from(n).ok());
        let etag = p.get("ETag").and_then(Value::as_str);
        let (Some(number), Some(etag)) = (number, etag) else {
            return Err(malformed_xml());
        };
        out.push(Listed {
            number,
            etag: etag.trim_matches('"').to_owned(),
        });
    }
    Ok(out)
}

impl Core {
    /// The stored parts that match the list, in order.
    fn matched_parts(&self, id: &str, list: &[Listed]) -> Result<Vec<(u32, PartRec)>, ApiError> {
        if list.windows(2).any(|w| w[0].number >= w[1].number) {
            return Err(ApiError::new(
                400,
                "InvalidPartOrder",
                "The list of parts was not in ascending order. The parts list must be specified in order by part number.",
            ));
        }
        let mut out = Vec::new();
        for (i, l) in list.iter().enumerate() {
            let part = self
                .parts
                .get(&part_row(id, l.number))
                .filter(|p| p.etag == l.etag)
                .ok_or_else(invalid_part)?;
            if i + 1 < list.len() && part.size < MIN_PART {
                return Err(ApiError::new(
                    400,
                    "EntityTooSmall",
                    "Your proposed upload is smaller than the minimum allowed size",
                )
                .with("ETag", &part.etag)
                .with("MinSizeAllowed", MIN_PART.to_string())
                .with("PartNumber", l.number.to_string())
                .with("ProposedSize", part.size.to_string()));
            }
            out.push((l.number, part.clone()));
        }
        Ok(out)
    }

    pub(crate) fn complete_upload(&mut self, req: &Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket().to_owned(), req.key().to_owned());
        let id = req.s("UploadId").unwrap_or_default().to_owned();
        let upload = self.upload(&bucket, &key, &id)?.clone();
        let list = listed(req)?;
        let chosen = self.matched_parts(&id, &list)?;
        let size: u64 = chosen.iter().map(|(_, p)| p.size).sum();
        let v = Version {
            key: key.clone(),
            version_id: String::new(),
            seq: 0,
            delete_marker: false,
            modified_ms: self.now_ms(),
            size,
            etag: multipart_etag(&chosen),
            meta: upload.meta.clone(),
            checksum: whole_checksum(&upload, &chosen),
            segs: chosen.iter().map(|(_, p)| Seg { blob: p.blob.clone(), len: p.size }).collect(),
            parts: chosen
                .iter()
                .map(|(n, p)| PartInfo {
                    number: *n,
                    size: p.size,
                    etag: p.etag.clone(),
                    checksum: upload.algo.and_then(|a| p.sums.iter().find(|(x, _)| *x == a)).map(|(_, v)| v.clone()),
                })
                .collect(),
            tags: upload.tags.clone(),
        };
        let used: Vec<String> = chosen.iter().map(|(_, p)| p.blob.clone()).collect();
        let stored = self.install(&bucket, v)?;
        self.finish_upload(&upload_row(&bucket, &key, &id), &used);
        self.note(req, Kind::Object("ObjectCreated:CompleteMultipartUpload"), &stored.key, stored.size, &stored.etag, Some(&stored.version_id));
        Out::json(complete_output(&bucket, &key, req.call.caller.endpoint.as_str(), &stored, self.versioned(&bucket)))
    }

    /// Removes the upload and its parts, keeping the blobs the object now owns.
    fn finish_upload(&mut self, row: &str, keep: &[String]) {
        self.remove_upload(row);
        self.doomed.retain(|b| !keep.contains(b));
    }

    pub(crate) fn versioned(&self, bucket: &str) -> bool {
        self.buckets.get(bucket).is_some_and(|b| b.versioning != crate::model::Versioning::Off)
    }
}

/// The ETag of a multipart object: the MD5 of the parts' MD5s, then `-` and the part count.
fn multipart_etag(parts: &[(u32, PartRec)]) -> String {
    let mut h = Md5::new();
    for (_, p) in parts {
        h.update(aws_core::hex::decode(&p.etag).unwrap_or_default());
    }
    format!("{}-{}", aws_core::hex::encode(&h.finalize()), parts.len())
}

/// The object's checksum: composite of the parts' checksums when the upload asked for that, else the checksum of all the bytes,
/// worked out from the parts' (CRC64NVME when the upload named no algorithm).
fn whole_checksum(upload: &Upload, parts: &[(u32, PartRec)]) -> Option<Checksum> {
    let algo = upload.algo.unwrap_or(Algo::Crc64Nvme);
    let digests: Option<Vec<Vec<u8>>> = parts
        .iter()
        .map(|(_, p)| p.sums.iter().find(|(a, _)| *a == algo).and_then(|(_, v)| aws_core::base64::decode(v)))
        .collect();
    let digests = digests?;
    if upload.kind == Some(CKind::Composite) {
        let value = format!("{}-{}", b64(&composite(algo, &digests)), parts.len());
        return Some(Checksum { algo, value, kind: CKind::Composite });
    }
    let mut acc = digests.first()?.clone();
    for ((_, p), d) in parts.iter().zip(&digests).skip(1) {
        acc = combine(algo, &acc, d, p.size)?;
    }
    Some(Checksum { algo, value: b64(&acc), kind: CKind::FullObject })
}

fn complete_output(bucket: &str, key: &str, endpoint: &str, v: &Version, versioned: bool) -> Value {
    let mut out = Map::new();
    out.insert("Location".into(), json!(format!("{endpoint}/{bucket}/{key}")));
    out.insert("Bucket".into(), json!(bucket));
    out.insert("Key".into(), json!(key));
    out.insert("ETag".into(), json!(format!("\"{}\"", v.etag)));
    add_checksum(&mut out, v.checksum.as_ref());
    if let Some(sse) = &v.meta.sse {
        out.insert("ServerSideEncryption".into(), json!(sse));
    }
    if versioned {
        out.insert("VersionId".into(), json!(v.version_id));
    }
    Value::Object(out)
}
