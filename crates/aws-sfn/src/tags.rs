//! Tags on state machines and activities: a `[{key, value}]` list on the wire. An ARN nothing holds is `ResourceNotFound`.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::req::{Req, not_found};
use crate::util::tag_list;

/// AWS's quota of tags on one resource.
const MOST_TAGS: usize = 50;

impl Core {
    fn tags_mut(&mut self, arn: &str) -> Result<&mut BTreeMap<String, String>, ApiError> {
        let missing = || not_found(format!("Resource not found: '{arn}'")); // (memory)
        if arn.contains(":activity:") {
            return self
                .activities
                .get_mut(arn)
                .map(|a| &mut a.tags)
                .ok_or_else(missing);
        }
        self.machines
            .get_mut(arn)
            .map(|m| &mut m.tags)
            .ok_or_else(missing)
    }

    pub(crate) fn tag_resource(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new = r.tags();
        let tags = self.tags_mut(r.s("resourceArn"))?;
        let mut next = tags.clone();
        next.extend(new);
        if next.len() > MOST_TAGS {
            // (memory)
            return Err(ApiError::modeled(
                "TooManyTags",
                "Too many tags: the most a resource may have is 50",
            ));
        }
        *tags = next;
        Ok(json!({}))
    }

    pub(crate) fn untag_resource(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let keys: Vec<String> =
            r.v.get("tagKeys")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|k| k.as_str().map(str::to_owned))
                .collect();
        let tags = self.tags_mut(r.s("resourceArn"))?;
        for k in keys {
            tags.remove(&k);
        }
        Ok(json!({}))
    }

    pub(crate) fn list_tags(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let tags = self.tags_mut(r.s("resourceArn"))?;
        Ok(json!({"tags": tag_list(tags)}))
    }
}
