//! Listing multipart uploads and the parts of one.

use serde_json::{Map, Value, json};

use crate::buckets::owner;
use crate::core::Core;
use crate::list::{Enc, MAX_KEYS};
use crate::req::{Answer, Out, Req};

impl Core {
    pub(crate) fn list_parts(&self, req: &Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket(), req.key());
        let id = req.s("UploadId").unwrap_or_default();
        let upload = self.upload(bucket, key, id)?;
        let marker = req.int("PartNumberMarker").unwrap_or(0);
        let max = req.int("MaxParts").map_or(MAX_KEYS, |m| usize::try_from(m).unwrap_or(0).min(MAX_KEYS));
        let part_prefix = format!("{id}\0");
        let all: Vec<(u32, &crate::model::PartRec)> = self
            .parts
            .with_prefix(&part_prefix)
            .filter_map(|(k, p)| Some((k.rsplit('\0').next()?.parse().ok()?, p)))
            .filter(|(n, _)| i64::from(*n) > marker)
            .collect();
        let page: Vec<_> = all.iter().take(max).collect();
        let truncated = all.len() > page.len();
        let parts: Vec<Value> = page
            .iter()
            .map(|(n, p)| {
                let mut o = Map::new();
                o.insert("PartNumber".into(), json!(n));
                o.insert("LastModified".into(), json!(p.modified_ms / 1000));
                o.insert("ETag".into(), json!(format!("\"{}\"", p.etag)));
                o.insert("Size".into(), json!(p.size));
                if let Some(a) = upload.algo
                    && let Some((_, v)) = p.sums.iter().find(|(x, _)| *x == a)
                {
                    o.insert(a.member().into(), json!(v));
                }
                Value::Object(o)
            })
            .collect();
        let mut out = Map::new();
        out.insert("Bucket".into(), json!(bucket));
        out.insert("Key".into(), json!(key));
        out.insert("UploadId".into(), json!(id));
        out.insert("PartNumberMarker".into(), json!(marker));
        let last = page.last().map_or(0, |(n, _)| *n);
        out.insert("NextPartNumberMarker".into(), json!(last));
        out.insert("MaxParts".into(), json!(max));
        out.insert("IsTruncated".into(), json!(truncated));
        out.insert("Parts".into(), Value::Array(parts));
        out.insert("StorageClass".into(), json!(upload.meta.storage_class.as_deref().unwrap_or("STANDARD")));
        out.insert("Owner".into(), owner(req.account()));
        out.insert("Initiator".into(), initiator(req.account()));
        if let Some(a) = upload.algo {
            out.insert("ChecksumAlgorithm".into(), json!(a.name()));
        }
        Out::json(Value::Object(out))
    }

    pub(crate) fn list_uploads(&self, req: &Req<'_>) -> Answer {
        let bucket = req.bucket();
        self.require(bucket)?;
        let prefix = req.s("Prefix").unwrap_or_default();
        let (km, um) = (req.s("KeyMarker").unwrap_or_default(), req.s("UploadIdMarker").unwrap_or_default());
        let max = req.int("MaxUploads").map_or(MAX_KEYS, |m| usize::try_from(m).unwrap_or(0).min(MAX_KEYS));
        let upload_prefix = format!("{bucket}\0");
        let mut all: Vec<&crate::model::Upload> = self
            .uploads
            .with_prefix(&upload_prefix)
            .map(|(_, u)| u)
            .filter(|u| u.key.starts_with(prefix))
            .filter(|u| km.is_empty() || u.key.as_str() > km || (u.key == km && u.id.as_str() > um))
            .collect();
        all.sort_by(|a, b| a.key.cmp(&b.key).then(a.initiated_ms.cmp(&b.initiated_ms)));
        let page: Vec<_> = all.iter().take(max).collect();
        let enc = Enc::of(req);
        let uploads: Vec<Value> = page
            .iter()
            .map(|u| {
                json!({
                    "UploadId": u.id, "Key": enc.text(&u.key), "Initiated": u.initiated_ms / 1000,
                    "StorageClass": u.meta.storage_class.as_deref().unwrap_or("STANDARD"),
                    "Owner": owner(req.account()), "Initiator": initiator(req.account()),
                })
            })
            .collect();
        let mut out = Map::new();
        out.insert("Bucket".into(), json!(bucket));
        out.insert("KeyMarker".into(), json!(enc.text(km)));
        out.insert("UploadIdMarker".into(), json!(um));
        out.insert("MaxUploads".into(), json!(max));
        out.insert("IsTruncated".into(), json!(all.len() > page.len()));
        if let Some(last) = page.last() {
            out.insert("NextKeyMarker".into(), json!(enc.text(&last.key)));
            out.insert("NextUploadIdMarker".into(), json!(last.id));
        }
        if !prefix.is_empty() {
            out.insert("Prefix".into(), json!(enc.text(prefix)));
        }
        if req.s("EncodingType").is_some() {
            out.insert("EncodingType".into(), json!("url"));
        }
        out.insert("Uploads".into(), Value::Array(uploads));
        Out::json(Value::Object(out))
    }
}

fn initiator(account: &str) -> Value {
    json!({"DisplayName": "doze", "ID": format!("arn:aws:iam::{account}:root")})
}
