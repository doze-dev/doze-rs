//! Tags on objects and on buckets.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::errors::{malformed_xml, no_config, no_such_key};
use crate::events::Kind;
use crate::req::{Answer, Out, Req};

/// S3's limits on tags: ten per object, keys to 128 characters, values to 256.
const MAX_TAGS: usize = 10;
const MAX_KEY: usize = 128;
const MAX_VALUE: usize = 256;

fn invalid_tag(message: &str) -> ApiError {
    ApiError::new(400, "InvalidTag", message)
}

/// The pairs of a `Tagging` member, checked.
fn tag_set(tagging: &Value, limit: usize) -> Result<Vec<(String, String)>, ApiError> {
    let set = tagging
        .get("TagSet")
        .and_then(Value::as_array)
        .ok_or_else(malformed_xml)?;
    if set.len() > limit {
        return Err(invalid_tag("Object tags cannot be greater than 10"));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for t in set {
        let k = t
            .get("Key")
            .and_then(Value::as_str)
            .ok_or_else(malformed_xml)?;
        let v = t
            .get("Value")
            .and_then(Value::as_str)
            .ok_or_else(malformed_xml)?;
        if k.is_empty() || k.chars().count() > MAX_KEY {
            return Err(invalid_tag("The TagKey you have provided is invalid"));
        }
        if v.chars().count() > MAX_VALUE {
            return Err(invalid_tag("The TagValue you have provided is invalid"));
        }
        if out.iter().any(|(x, _)| x == k) {
            return Err(invalid_tag(
                "Cannot provide multiple Tags with the same key",
            ));
        }
        out.push((k.to_owned(), v.to_owned()));
    }
    Ok(out)
}

fn tags_json(tags: &[(String, String)]) -> Vec<Value> {
    tags.iter()
        .map(|(k, v)| json!({"Key": k, "Value": v}))
        .collect()
}

impl Core {
    pub(crate) fn get_tagging(&self, req: &Req<'_>, bucket_level: bool) -> Answer {
        if bucket_level {
            let b = self.require(req.bucket())?;
            let stored = b
                .configs
                .get("tagging")
                .ok_or_else(|| no_config("tagging"))?;
            return Out::json(serde_json::from_str(stored).unwrap_or(Value::Null));
        }
        let v = self.readable(req.bucket(), req.key(), req.s("VersionId"))?;
        let mut out = json!({"TagSet": tags_json(&v.tags)});
        if v.version_id != "null"
            && let Some(o) = out.as_object_mut()
        {
            o.insert("VersionId".into(), json!(v.version_id));
        }
        Out::json(out)
    }

    pub(crate) fn put_tagging(&mut self, req: &Req<'_>, bucket_level: bool) -> Answer {
        let tagging = req.get("Tagging").ok_or_else(malformed_xml)?;
        if bucket_level {
            let tags = tag_set(tagging, MAX_TAGS)?;
            self.require(req.bucket())?;
            let doc = json!({"TagSet": tags_json(&tags)}).to_string();
            if let Some(b) = self.buckets.get_mut(req.bucket()) {
                b.configs.insert("tagging".into(), doc);
            }
            return Out::no_content();
        }
        let tags = tag_set(tagging, MAX_TAGS)?;
        let mut v = self.readable(req.bucket(), req.key(), req.s("VersionId"))?;
        v.tags = tags;
        self.put_row(req.bucket(), &v)?;
        self.note(
            req,
            Kind::Object("ObjectTagging:Put"),
            &v.key,
            v.size,
            &v.etag,
            Some(&v.version_id),
        );
        let mut out = json!({});
        if v.version_id != "null"
            && let Some(o) = out.as_object_mut()
        {
            o.insert("VersionId".into(), json!(v.version_id));
        }
        Out::json(out)
    }

    pub(crate) fn delete_tagging(&mut self, req: &Req<'_>, bucket_level: bool) -> Answer {
        if bucket_level {
            self.require(req.bucket())?;
            if let Some(b) = self.buckets.get_mut(req.bucket()) {
                b.configs.remove("tagging");
            }
            return Out::empty();
        }
        let mut v = self
            .readable(req.bucket(), req.key(), req.s("VersionId"))
            .map_err(|e| {
                if e.code == "NoSuchKey" {
                    no_such_key(req.key())
                } else {
                    e
                }
            })?;
        v.tags.clear();
        self.put_row(req.bucket(), &v)?;
        self.note(
            req,
            Kind::Object("ObjectTagging:Delete"),
            &v.key,
            v.size,
            &v.etag,
            Some(&v.version_id),
        );
        Out::empty()
    }
}
