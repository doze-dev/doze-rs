//! Tags on topics. The limits are AWS's published ones; the wording of the refusals is from memory.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::req::{Req, invalid_because, no_resource};

/// A resource carries at most 50 tags.
const MAX_TAGS: usize = 50;

fn limit() -> ApiError {
    // (memory)
    ApiError::modeled(
        "TagLimitExceededException",
        "Could not complete request: tag quota of per resource exceeded",
    )
}

/// Checks tags given for a new topic or for `TagResource`: no repeated key, none reserved, not more than the quota.
pub(crate) fn check_new(tags: &[(String, String)]) -> Result<(), ApiError> {
    let mut seen = BTreeMap::new();
    for (k, v) in tags {
        if k.to_ascii_lowercase().starts_with("aws:") {
            return Err(invalid_because(
                "Tags",
                "AWS internal tags cannot be modified or created",
            ));
        }
        if seen.insert(k.as_str(), v.as_str()).is_some() {
            return Err(invalid_because("Tags", "Duplicate tag keys"));
        }
    }
    if tags.len() > MAX_TAGS {
        return Err(limit());
    }
    Ok(())
}

impl Core {
    /// `TagResource`: a key already there takes the new value.
    pub(crate) fn tag_resource(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.tagged(r)?;
        let tags = r.tags();
        check_new(&tags)?;
        if let Some(t) = self.topics.get_mut(&key) {
            let mut next = t.tags.clone();
            next.extend(tags);
            if next.len() > MAX_TAGS {
                return Err(limit());
            }
            t.tags = next;
        }
        Ok(Value::Null)
    }

    /// `UntagResource`: a key that is not there is not an error.
    pub(crate) fn untag_resource(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.tagged(r)?;
        let keys = r.strings("TagKeys");
        if let Some(t) = self.topics.get_mut(&key) {
            for k in &keys {
                t.tags.remove(k);
            }
        }
        Ok(Value::Null)
    }

    /// `ListTagsForResource`.
    pub(crate) fn list_tags(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = self.tagged(r)?;
        let tags: Vec<Value> = self
            .topics
            .get(&key)
            .map(|t| {
                t.tags
                    .iter()
                    .map(|(k, v)| json!({"Key": k, "Value": v}))
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({"Tags": tags}))
    }

    /// The topic a `ResourceArn` names; anything else is a resource that does not exist.
    fn tagged(&self, r: &Req<'_>) -> Result<String, ApiError> {
        let arn = r.s("ResourceArn");
        self.topics
            .iter()
            .find(|(_, t)| t.arn == arn)
            .map(|(k, _)| k.to_owned())
            .ok_or_else(no_resource)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(n: usize) -> Vec<(String, String)> {
        (0..n).map(|i| (format!("k{i}"), "v".to_owned())).collect()
    }

    #[test]
    fn the_quota_repeats_and_reserved_keys_are_refused() {
        assert!(check_new(&tags(50)).is_ok());
        assert!(check_new(&tags(51)).unwrap_err().message.contains("quota"));
        let mut repeated = tags(2);
        repeated.push(("k0".into(), "other".into()));
        assert!(check_new(&repeated).is_err());
        assert!(check_new(&[("aws:x".into(), "v".into())]).is_err());
        assert!(check_new(&[("AWS:x".into(), "v".into())]).is_err());
    }
}
