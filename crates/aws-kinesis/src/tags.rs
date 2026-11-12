//! Tags on a stream: `AddTagsToStream`, `RemoveTagsFromStream`, `ListTagsForStream`, and the ARN forms `TagResource`,
//! `UntagResource`, `ListTagsForResource`.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::req::{Put, Req, invalid};

/// Tags one stream may carry.
const MAX_TAGS: usize = 50;

impl Core {
    pub(crate) fn add_tags(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new = r.map("Tags");
        let s = self.stream_mut(r)?;
        let added = new.iter().filter(|(k, _)| !s.tags.contains_key(k)).count();
        if s.tags.len() + added > MAX_TAGS {
            // (memory)
            return Err(invalid(format!(
                "A stream can have at most {MAX_TAGS} tags."
            )));
        }
        s.tags.extend(new);
        Ok(json!({}))
    }

    pub(crate) fn remove_tags(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let keys = r.strings("TagKeys");
        let s = self.stream_mut(r)?;
        for key in keys {
            s.tags.remove(&key);
        }
        Ok(json!({}))
    }

    pub(crate) fn list_tags(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let s = self.stream(r)?;
        let after = r.opt("ExclusiveStartTagKey");
        let limit = r
            .int("Limit")
            .map_or(50, |n| usize::try_from(n).unwrap_or(50));
        let rest: Vec<_> = s
            .tags
            .iter()
            .filter(|(k, _)| after.is_none_or(|a| k.as_str() > a))
            .collect();
        let tags: Vec<Value> = rest
            .iter()
            .take(limit)
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect();
        let mut out = json!({"Tags": tags});
        // `ListTagsForStream` says whether there are more; `ListTagsForResource` has no such member.
        if r.opt("ResourceARN").is_none() {
            out.put("HasMoreTags", json!(rest.len() > limit));
        }
        Ok(out)
    }
}
