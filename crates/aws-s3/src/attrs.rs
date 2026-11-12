//! GetObjectAttributes: the parts of an object's description a caller picks.

use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::model::CKind;
use crate::req::{Answer, Out, Req};

impl Core {
    pub(crate) fn get_object_attributes(&self, req: &Req<'_>) -> Answer {
        let v = self.readable(req.bucket(), req.key(), req.s("VersionId"))?;
        let wanted: Vec<&str> = req
            .get("ObjectAttributes")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut out = Map::new();
        out.insert("LastModified".into(), json!(v.modified_ms / 1000));
        if v.version_id != "null" {
            out.insert("VersionId".into(), json!(v.version_id));
        }
        if wanted.contains(&"ETag") {
            out.insert("ETag".into(), json!(v.etag));
        }
        if wanted.contains(&"ObjectSize") {
            out.insert("ObjectSize".into(), json!(v.size));
        }
        if wanted.contains(&"StorageClass") {
            out.insert("StorageClass".into(), json!(v.meta.storage_class.as_deref().unwrap_or("STANDARD")));
        }
        if let (true, Some(c)) = (wanted.contains(&"Checksum"), &v.checksum) {
            let kind = if c.kind == CKind::FullObject { "FULL_OBJECT" } else { "COMPOSITE" };
            out.insert("Checksum".into(), json!({ c.algo.member(): c.value, "ChecksumType": kind }));
        }
        if wanted.contains(&"ObjectParts") && !v.parts.is_empty() {
            let parts: Vec<Value> = v
                .parts
                .iter()
                .map(|p| json!({"PartNumber": p.number, "Size": p.size}))
                .collect();
            out.insert(
                "ObjectParts".into(),
                json!({"TotalPartsCount": v.parts.len(), "PartNumberMarker": 0, "NextPartNumberMarker": v.parts.last().map_or(0, |p| p.number), "MaxParts": 1000, "IsTruncated": false, "Parts": parts}),
            );
        }
        Out::json(Value::Object(out))
    }
}
