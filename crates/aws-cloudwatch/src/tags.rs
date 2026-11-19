//! Tags of alarms, dashboards and the stored documents, by ARN.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::req::{Req, validation};

/// Tags one resource may have.
const MAX_TAGS: usize = 50;

impl Core {
    /// The tags of the alarm or dashboard an ARN names.
    fn tags_of(&mut self, arn: &str) -> Result<&mut BTreeMap<String, String>, ApiError> {
        if let Some((_, name)) = arn.split_once(":alarm:") {
            return self
                .alarms
                .get_mut(name)
                .map(|a| &mut a.tags)
                .ok_or_else(not_found);
        }
        if let Some((_, name)) = arn.split_once(":dashboard/") {
            return self
                .docs
                .get_mut(&crate::model::doc_key("dashboard", name))
                .map(|d| &mut d.tags)
                .ok_or_else(not_found);
        }
        if let Some(key) = crate::stored::key_of_arn(arn) {
            return self
                .docs
                .get_mut(&key)
                .map(|d| &mut d.tags)
                .ok_or_else(not_found);
        }
        Err(not_found())
    }

    pub(crate) fn tag(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new = r.tags();
        let tags = self.tags_of(r.s("ResourceARN"))?;
        let added = new.iter().filter(|(k, _)| !tags.contains_key(k)).count();
        if tags.len() + added > MAX_TAGS {
            // (memory)
            return Err(validation(format!(
                "A resource can have at most {MAX_TAGS} tags."
            )));
        }
        tags.extend(new);
        Ok(json!({}))
    }

    pub(crate) fn untag(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let keys = r.strings("TagKeys");
        let tags = self.tags_of(r.s("ResourceARN"))?;
        for k in keys {
            tags.remove(&k);
        }
        Ok(json!({}))
    }

    pub(crate) fn list_tags(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let tags = self.tags_of(r.s("ResourceARN"))?;
        Ok(
            json!({"Tags": tags.iter().map(|(k, v)| json!({"Key": k, "Value": v})).collect::<Vec<_>>()}),
        )
    }
}

fn not_found() -> ApiError {
    // (memory)
    ApiError::modeled(
        "ResourceNotFoundException",
        "The specified resource was not found.",
    )
}
