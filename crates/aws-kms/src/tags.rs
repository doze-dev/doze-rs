//! Tags on a key: `TagResource`, `UntagResource`, `ListResourceTags`.

use aws_model_kms::keys::{
    ListResourceTagsRequest as List, ListResourceTagsResponse as Listed, Tag,
    TagResourceRequest as Add, UntagResourceRequest as Remove,
};
use aws_schema::build::Object;
use aws_wire::ApiError;
use aws_wire::handler::{empty, reply, request};
use aws_wire::service::{Call, Reply};

use crate::core::Core;
use crate::find::not_found_key;
use crate::model::Key;

const MAX_TAGS: usize = 50;

impl Core {
    fn tagged(&mut self, call: &Call, given: &str) -> Result<&mut Key, ApiError> {
        let id = self.locate(call, given)?;
        self.keys
            .get_mut(&id)
            .ok_or_else(|| not_found_key(call, &id))
    }

    pub(crate) fn tag_resource(&mut self, call: &Call) -> Reply {
        let v = request::<Add>(call)?;
        let new: Vec<(String, String)> = v
            .get(Add::TAGS)
            .into_iter()
            .flatten()
            .map(|t| {
                (
                    t.get(Tag::TAG_KEY).unwrap_or_default().to_owned(),
                    t.get(Tag::TAG_VALUE).unwrap_or_default().to_owned(),
                )
            })
            .collect();
        let key = self.tagged(call, v.get(Add::KEY_ID).unwrap_or_default())?;
        let added = new
            .iter()
            .filter(|(k, _)| !key.tags.contains_key(k))
            .count();
        if key.tags.len() + added > MAX_TAGS {
            return Err(ApiError::modeled(
                "TagException",
                "A key can have at most 50 tags.",
            ));
        }
        if new
            .iter()
            .any(|(k, _)| k.to_ascii_lowercase().starts_with("aws:"))
        {
            return Err(ApiError::modeled(
                "TagException",
                "Tag keys that start with aws: are reserved.",
            ));
        }
        key.tags.extend(new);
        empty()
    }

    pub(crate) fn untag_resource(&mut self, call: &Call) -> Reply {
        let v = request::<Remove>(call)?;
        let names: Vec<&str> = v
            .get(Remove::TAG_KEYS)
            .map(Iterator::collect)
            .unwrap_or_default();
        let key = self.tagged(call, v.get(Remove::KEY_ID).unwrap_or_default())?;
        for n in names {
            key.tags.remove(n);
        }
        empty()
    }

    pub(crate) fn list_resource_tags(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let limit = v
            .get(List::LIMIT)
            .map_or(MAX_TAGS, |n| usize::try_from(n).unwrap_or(MAX_TAGS));
        let after = v.get(List::MARKER).unwrap_or_default();
        let key = self.key(call, v.get(List::KEY_ID).unwrap_or_default())?;
        let mut rows: Vec<(&String, &String)> = key
            .tags
            .iter()
            .filter(|(k, _)| k.as_str() > after)
            .collect();
        let more = rows.len() > limit;
        rows.truncate(limit);
        let marker = more
            .then(|| rows.last().map(|(k, _)| (*k).clone()))
            .flatten();
        reply::<Listed>(|o| {
            o.put_list(Listed::TAGS, |l| {
                for (k, val) in &rows {
                    l.push(|t: &mut Object<'_, Tag>| {
                        t.put(Tag::TAG_KEY, k);
                        t.put(Tag::TAG_VALUE, val);
                    });
                }
            });
            o.put_bool(Listed::TRUNCATED, more);
            if let Some(m) = &marker {
                o.put(Listed::NEXT_MARKER, m);
            }
        })
    }
}
