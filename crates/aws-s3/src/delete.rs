//! Deleting objects: a plain delete adds a delete marker in a versioned bucket and removes the object in an unversioned one; a delete
//! that names a version removes that version for good.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::errors::malformed_xml;
use crate::events::Kind;
use crate::model::{Meta, Version, Versioning};
use crate::put::new_version_id;
use crate::req::{Answer, Out, Req};

/// What removing one key did.
#[derive(Debug, Default)]
pub(crate) struct Removed {
    /// The version removed, or the delete marker added.
    pub(crate) version_id: Option<String>,
    /// The thing removed or added is a delete marker.
    pub(crate) marker: bool,
    /// A delete marker was added (not a version removed).
    pub(crate) created: bool,
    /// Size of the object removed, for the event.
    pub(crate) size: u64,
    /// Something was there to remove, or a marker was added.
    pub(crate) happened: bool,
}

/// The most keys one `DeleteObjects` may name.
const MAX_BATCH: usize = 1000;

impl Core {
    /// Deletes `key` (or its version `id`) in `bucket`. A version that Object Lock holds is not removed (`AccessDenied`), unless it is
    /// GOVERNANCE retention and the caller asks to `bypass` it; adding a delete marker is never stopped.
    pub(crate) fn remove_key(
        &mut self,
        bucket: &str,
        key: &str,
        id: Option<&str>,
        bypass: bool,
    ) -> Result<Removed, ApiError> {
        let state = self.require(bucket)?.versioning;
        if let Some(id) = id {
            return Ok(match self.find(bucket, key, id)? {
                Some(v) => {
                    self.guard_removal(&v, bypass)?;
                    self.drop_row(bucket, &v);
                    Removed {
                        version_id: Some(id.to_owned()),
                        marker: v.delete_marker,
                        created: false,
                        size: v.size,
                        happened: true,
                    }
                }
                None => Removed::default(),
            });
        }
        let existing = self.versions_of(bucket, key)?;
        let size = existing.first().map_or(0, |v| v.size);
        match state {
            Versioning::Off => {
                for v in &existing {
                    self.guard_removal(v, bypass)?;
                }
                for v in &existing {
                    self.drop_row(bucket, v);
                }
                Ok(Removed {
                    size,
                    happened: !existing.is_empty(),
                    ..Removed::default()
                })
            }
            Versioning::Enabled => self.add_marker(bucket, key, new_version_id(), size),
            Versioning::Suspended => {
                for v in existing.iter().filter(|v| v.version_id == "null") {
                    self.guard_removal(v, bypass)?;
                }
                for v in existing.iter().filter(|v| v.version_id == "null") {
                    self.drop_row(bucket, v);
                }
                self.add_marker(bucket, key, "null".to_owned(), size)
            }
        }
    }

    fn add_marker(
        &mut self,
        bucket: &str,
        key: &str,
        id: String,
        size: u64,
    ) -> Result<Removed, ApiError> {
        let seq = self.next_seq(bucket);
        let marker = Version {
            key: key.to_owned(),
            version_id: id.clone(),
            seq,
            delete_marker: true,
            modified_ms: self.now_ms(),
            size: 0,
            etag: String::new(),
            meta: Meta::default(),
            checksum: None,
            segs: Vec::new(),
            parts: Vec::new(),
            tags: Vec::new(),
            lock: crate::model::Lock::default(),
            acl: None,
        };
        self.put_row(bucket, &marker)?;
        Ok(Removed {
            version_id: Some(id),
            marker: true,
            created: true,
            size,
            happened: true,
        })
    }

    pub(crate) fn delete_object(&mut self, req: &Req<'_>) -> Answer {
        let (bucket, key) = (req.bucket(), req.key());
        let bypass = req.flag("BypassGovernanceRetention") == Some(true);
        let removed = self.remove_key(bucket, key, req.s("VersionId"), bypass)?;
        self.note_removed(req, key, &removed);
        let mut out = Map::new();
        if removed.marker {
            out.insert("DeleteMarker".into(), json!(true));
        }
        if let Some(id) = &removed.version_id {
            out.insert("VersionId".into(), json!(id));
        }
        Out::json(Value::Object(out))
    }

    pub(crate) fn delete_objects(&mut self, req: &Req<'_>) -> Answer {
        let bucket = req.bucket();
        self.require(bucket)?;
        let delete = req.get("Delete").ok_or_else(malformed_xml)?;
        let objects = delete
            .get("Objects")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if objects.is_empty() || objects.len() > MAX_BATCH {
            return Err(malformed_xml());
        }
        let quiet = delete
            .get("Quiet")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let bypass = req.flag("BypassGovernanceRetention") == Some(true);
        let mut done = Vec::new();
        let mut refused = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for o in &objects {
            let key = o.get("Key").and_then(Value::as_str).unwrap_or_default();
            let id = o.get("VersionId").and_then(Value::as_str);
            if !seen.insert((key.to_owned(), id.map(str::to_owned))) {
                continue;
            }
            match self.remove_key(bucket, key, id, bypass) {
                Ok(r) => {
                    self.note_removed(req, key, &r);
                    done.push(deleted_entry(key, id, &r));
                }
                // A key Object Lock holds is reported, and the others go on.
                Err(e) if e.code == "AccessDenied" => {
                    let mut entry = Map::new();
                    entry.insert("Key".into(), json!(key));
                    if let Some(id) = id {
                        entry.insert("VersionId".into(), json!(id));
                    }
                    entry.insert("Code".into(), json!(e.code));
                    entry.insert("Message".into(), json!(e.message));
                    refused.push(Value::Object(entry));
                }
                Err(e) => return Err(e),
            }
        }
        let mut out = Map::new();
        if !quiet {
            out.insert("Deleted".into(), json!(done));
        }
        if !refused.is_empty() {
            out.insert("Errors".into(), json!(refused));
        }
        Out::json(Value::Object(out))
    }
}

impl Core {
    /// Notes the event for a removal: a delete marker added, or an object (version) removed.
    fn note_removed(&mut self, req: &Req<'_>, key: &str, r: &Removed) {
        if !r.happened {
            return;
        }
        let name = if r.created {
            "ObjectRemoved:DeleteMarkerCreated"
        } else {
            "ObjectRemoved:Delete"
        };
        self.note_raw(
            req,
            Kind::Object(name),
            key,
            r.size,
            r.version_id.as_deref(),
        );
    }
}

fn deleted_entry(key: &str, asked: Option<&str>, r: &Removed) -> Value {
    let mut e = Map::new();
    e.insert("Key".into(), json!(key));
    if let Some(id) = asked {
        e.insert("VersionId".into(), json!(id));
        if r.marker {
            e.insert("DeleteMarker".into(), json!(true));
        }
    } else if r.created
        && let Some(id) = &r.version_id
    {
        e.insert("DeleteMarker".into(), json!(true));
        e.insert("DeleteMarkerVersionId".into(), json!(id));
    }
    Value::Object(e)
}
