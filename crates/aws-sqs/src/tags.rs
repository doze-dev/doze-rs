//! Queue tags and their limits. The messages are those AWS gave in `test_tag_limits` (recorded 2026-11-09).

use std::collections::BTreeMap;

use aws_model_sqs::keys::{
    ListQueueTagsRequest as List, ListQueueTagsResult as Listed, TagQueueRequest as Tag,
    UntagQueueRequest as Untag,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::{Core, empty, invalid_parameter, missing, reply, request};

/// The most tags a queue may carry.
const MAX_TAGS: usize = 50;

/// Checks tags a caller wants to add to a queue that has `existing`, and returns them. `queue` names the queue in the
/// message for too many.
pub(crate) fn check<'a>(
    queue: &str,
    existing: &BTreeMap<String, String>,
    new: impl Iterator<Item = (&'a str, &'a str)>,
) -> Result<Vec<(String, String)>, ApiError> {
    let new: Vec<(String, String)> = new.map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    for (key, value) in &new {
        if !(1..=128).contains(&key.chars().count()) {
            return Err(invalid_parameter(
                "Tag keys must be between 1 and 128 characters in length.",
            ));
        }
        if value.chars().count() > 256 {
            return Err(invalid_parameter(
                "Tag values must be between 0 and 256 characters in length.",
            ));
        }
        if key.to_ascii_lowercase().starts_with("aws:") {
            return Err(invalid_parameter(
                "Caller is an end user and not allowed to mutate system tags",
            ));
        }
    }
    let added = new
        .iter()
        .filter(|(k, _)| !existing.contains_key(k))
        .count();
    if existing.len() + added > MAX_TAGS {
        return Err(invalid_parameter(format!(
            "Too many tags added for queue {queue}."
        )));
    }
    Ok(new)
}

impl Core {
    pub(crate) fn tag_queue(&mut self, call: &Call) -> Reply {
        let v = request::<Tag>(call)?;
        let queue_url = v.get(Tag::QUEUE_URL).ok_or_else(|| missing("QueueUrl"))?;
        let tags = v.get(Tag::TAGS).ok_or_else(|| missing("Tags"))?;
        let queue = self.queue_mut(call, queue_url)?;
        let checked = check(&queue.name, &queue.tags, tags)?;
        queue.tags.extend(checked);
        empty()
    }

    pub(crate) fn untag_queue(&mut self, call: &Call) -> Reply {
        let v = request::<Untag>(call)?;
        let queue_url = v.get(Untag::QUEUE_URL).ok_or_else(|| missing("QueueUrl"))?;
        let keys: Vec<&str> = v
            .get(Untag::TAG_KEYS)
            .ok_or_else(|| missing("TagKeys"))?
            .collect();
        let queue = self.queue_mut(call, queue_url)?;
        for key in keys {
            queue.tags.remove(key);
        }
        empty()
    }

    pub(crate) fn list_queue_tags(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let queue_url = v.get(List::QUEUE_URL).ok_or_else(|| missing("QueueUrl"))?;
        let queue = self.queue_mut(call, queue_url)?;
        let tags: Vec<(String, String)> = queue
            .tags
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        reply::<Listed>(|o| {
            if !tags.is_empty() {
                o.put_map(Listed::TAGS, |m| {
                    for (k, v) in &tags {
                        m.insert(k, v);
                    }
                });
            }
        })
    }
}
