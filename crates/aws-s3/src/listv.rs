//! ListObjectVersions: every version and delete marker, a key's newest first, paged by a key marker and a version marker.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::buckets::owner;
use crate::core::Core;
use crate::list::{Enc, FOLD_ROWS, MAX_KEYS, Query, fold, past};
use crate::model::{CKind, Version};
use crate::put::algo_name;
use crate::req::{Answer, Out, Req};

/// What a page of versions held.
#[derive(Default)]
pub(crate) struct Found {
    pub(crate) versions: Vec<Value>,
    pub(crate) markers: Vec<Value>,
    pub(crate) prefixes: Vec<String>,
    /// The last (key, version id) returned, when more follow.
    pub(crate) next: Option<(String, String)>,
    /// The rows read to make it.
    pub(crate) scanned: usize,
}

/// The row a listing starts at. With a version marker the key itself is read again, from its first row, to find the marker.
fn first_row(q: &Query<'_>, version_marker: Option<&str>) -> String {
    if version_marker.is_some() {
        format!("{}\0{}", q.bucket, q.after)
    } else if q.after.is_empty() || q.after < q.prefix {
        format!("{}\0{}", q.bucket, q.prefix)
    } else {
        format!("{}\0{}\u{1}", q.bucket, q.after)
    }
}

fn version_entry(v: &Version, latest: bool, enc: Enc, account: &str) -> Value {
    let mut o = Map::new();
    o.insert("Key".into(), json!(enc.text(&v.key)));
    o.insert("VersionId".into(), json!(v.version_id));
    o.insert("IsLatest".into(), json!(latest));
    o.insert("LastModified".into(), json!(v.modified_ms / 1000));
    o.insert("Owner".into(), owner(account));
    if !v.delete_marker {
        o.insert("ETag".into(), json!(format!("\"{}\"", v.etag)));
        o.insert("Size".into(), json!(v.size));
        o.insert(
            "StorageClass".into(),
            json!(v.meta.storage_class.as_deref().unwrap_or("STANDARD")),
        );
        if let Some(c) = &v.checksum {
            o.insert("ChecksumAlgorithm".into(), json!(algo_name(Some(c))));
            let kind = if c.kind == CKind::FullObject {
                "FULL_OBJECT"
            } else {
                "COMPOSITE"
            };
            o.insert("ChecksumType".into(), json!(kind));
        }
    }
    Value::Object(o)
}

impl Core {
    pub(crate) fn list_object_versions(&self, req: &Req<'_>) -> Answer {
        let bucket = req.bucket();
        self.require(bucket)?;
        let prefix = req.s("Prefix").unwrap_or_default();
        let delimiter = req.s("Delimiter").filter(|d| !d.is_empty());
        let max = req
            .int("MaxKeys")
            .map_or(MAX_KEYS, |m| usize::try_from(m).unwrap_or(0).min(MAX_KEYS));
        let (key_marker, version_marker) = (
            req.s("KeyMarker").unwrap_or_default(),
            req.s("VersionIdMarker"),
        );
        let q = Query {
            bucket,
            prefix,
            delimiter,
            after: key_marker,
            max,
        };
        let enc = Enc::of(req);
        let found = self.collect(&q, version_marker, enc, req.account())?;
        let mut out = Map::new();
        out.insert("Name".into(), json!(bucket));
        out.insert("Prefix".into(), json!(enc.text(prefix)));
        out.insert("KeyMarker".into(), json!(enc.text(key_marker)));
        out.insert(
            "VersionIdMarker".into(),
            json!(version_marker.unwrap_or_default()),
        );
        out.insert("MaxKeys".into(), json!(max));
        out.insert("IsTruncated".into(), json!(found.next.is_some()));
        if let Some(d) = delimiter {
            out.insert("Delimiter".into(), json!(enc.text(d)));
        }
        if req.s("EncodingType").is_some() {
            out.insert("EncodingType".into(), json!("url"));
        }
        if let Some((k, id)) = &found.next {
            out.insert("NextKeyMarker".into(), json!(enc.text(k)));
            out.insert("NextVersionIdMarker".into(), json!(id));
        }
        out.insert("Versions".into(), Value::Array(found.versions));
        out.insert("DeleteMarkers".into(), Value::Array(found.markers));
        let prefixes: Vec<Value> = found
            .prefixes
            .iter()
            .map(|p| json!({"Prefix": enc.text(p)}))
            .collect();
        out.insert("CommonPrefixes".into(), Value::Array(prefixes));
        Out::json(Value::Object(out))
    }

    pub(crate) fn collect(
        &self,
        q: &Query<'_>,
        version_marker: Option<&str>,
        enc: Enc,
        account: &str,
    ) -> Result<Found, ApiError> {
        let mut f = Found::default();
        let mut count = 0usize;
        let mut current: Option<String> = None;
        let mut skipping = version_marker.is_some();
        let mut bad = false;
        // Under a delimiter a fold's keys are not read: after a few rows of one fold the walk starts again past it, once per fold.
        let (mut in_fold, mut restart, mut restarted) = (0usize, None::<String>, None::<String>);
        let mut start = first_row(q, version_marker);
        loop {
            self.walk_from(q, &start, |key, bytes| {
                f.scanned += 1;
                if version_marker.is_some() && key < q.after {
                    return true;
                }
                if let Some(folded) = fold(q, key) {
                    if f.prefixes.last() == Some(&folded) {
                        in_fold += 1;
                        if in_fold >= FOLD_ROWS && restarted.as_ref() != Some(&folded) {
                            restarted = Some(folded.clone());
                            restart = Some(folded);
                            return false;
                        }
                        return true;
                    }
                    if count >= q.max {
                        return false;
                    }
                    count += 1;
                    in_fold = 0;
                    f.prefixes.push(folded);
                    return true;
                }
                let Ok(v) = crate::store::decode_version(bytes) else {
                    bad = true;
                    return false;
                };
                if skipping {
                    if v.key == q.after && Some(v.version_id.as_str()) == version_marker {
                        skipping = false;
                    }
                    return v.key == q.after || v.key.as_str() < q.after;
                }
                let latest = current.as_deref() != Some(key);
                if latest {
                    current = Some(key.to_owned());
                }
                if count >= q.max {
                    return false;
                }
                count += 1;
                f.next = Some((v.key.clone(), v.version_id.clone()));
                let entry = version_entry(&v, latest, enc, account);
                if v.delete_marker {
                    f.markers.push(entry)
                } else {
                    f.versions.push(entry)
                }
                true
            })?;
            match restart.take() {
                Some(fold) => start = past(q.bucket, &fold),
                None => break,
            }
        }
        if bad {
            return Err(ApiError::internal("a stored object could not be read"));
        }
        // `next` marks the last row returned; it only matters when the page stopped because it was full.
        if count < q.max || (f.next.is_some() && !self.versions_after(q, &f)?) {
            f.next = None;
        }
        Ok(f)
    }

    /// Whether any version follows the last one of a full page.
    fn versions_after(&self, q: &Query<'_>, f: &Found) -> Result<bool, ApiError> {
        let Some((k, id)) = &f.next else {
            return Ok(false);
        };
        let rows = self.versions_of(q.bucket, k)?;
        let later_version = rows
            .iter()
            .position(|v| &v.version_id == id)
            .is_some_and(|i| i + 1 < rows.len());
        if later_version {
            return Ok(true);
        }
        let probe = Query {
            after: k,
            max: 1,
            ..*q
        };
        let mut more = false;
        self.walk(&probe, |_, _| {
            more = true;
            false
        })?;
        Ok(more)
    }
}
