//! Tags on a parameter or a document: `AddTagsToResource`, `RemoveTagsFromResource`, `ListTagsForResource`. The other resource
//! types (managed instances, maintenance windows) belong to the refused half of SSM.

use aws_model_ssm::keys::{
    AddTagsToResourceRequest as Add, ListTagsForResourceRequest as List,
    ListTagsForResourceResult as Listed, RemoveTagsFromResourceRequest as Remove, Tag,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::Core;
use aws_wire::handler::{empty, reply, request};
use std::collections::BTreeMap;

const MAX_TAGS: usize = 50;

impl Core {
    /// The tags of the parameter or document a tag call names. `kind` is the `ResourceType`.
    fn tagged(&mut self, kind: &str, id: &str) -> Result<&mut BTreeMap<String, String>, ApiError> {
        // AWS's message is empty (recorded 2026-11-15).
        let missing = || ApiError::modeled("InvalidResourceId", "");
        match kind {
            "Parameter" => self
                .params
                .get_mut(id)
                .map(|p| &mut p.tags)
                .ok_or_else(missing),
            "Document" => self
                .documents
                .get_mut(id)
                .map(|d| &mut d.tags)
                .ok_or_else(missing),
            _ => Err(ApiError::modeled(
                "InvalidResourceType",
                format!(
                    "The resource type {kind} is not built in doze-aws; Parameter and Document are."
                ),
            )),
        }
    }

    pub(crate) fn add_tags(&mut self, call: &Call) -> Reply {
        let v = request::<Add>(call)?;
        let new: Vec<(String, String)> = v
            .get(Add::TAGS)
            .map(|tags| {
                tags.map(|t| {
                    (
                        t.get(Tag::KEY).unwrap_or_default().to_owned(),
                        t.get(Tag::VALUE).unwrap_or_default().to_owned(),
                    )
                })
                .collect()
            })
            .unwrap_or_default();
        let tags = self.tagged(
            v.get(Add::RESOURCE_TYPE).unwrap_or_default(),
            v.get(Add::RESOURCE_ID).unwrap_or_default(),
        )?;
        let added = new.iter().filter(|(k, _)| !tags.contains_key(k)).count();
        if tags.len() + added > MAX_TAGS {
            return Err(ApiError::modeled(
                "TooManyUpdates",
                "The number of tags on a parameter can't be more than 50.",
            ));
        }
        tags.extend(new);
        empty()
    }

    pub(crate) fn remove_tags(&mut self, call: &Call) -> Reply {
        let v = request::<Remove>(call)?;
        let keys: Vec<&str> = v
            .get(Remove::TAG_KEYS)
            .map(Iterator::collect)
            .unwrap_or_default();
        let tags = self.tagged(
            v.get(Remove::RESOURCE_TYPE).unwrap_or_default(),
            v.get(Remove::RESOURCE_ID).unwrap_or_default(),
        )?;
        for key in keys {
            tags.remove(key);
        }
        empty()
    }

    pub(crate) fn list_tags(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let tags = self.tagged(
            v.get(List::RESOURCE_TYPE).unwrap_or_default(),
            v.get(List::RESOURCE_ID).unwrap_or_default(),
        )?;
        let tags: Vec<(String, String)> =
            tags.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        reply::<Listed>(|o| {
            o.put_list(Listed::TAG_LIST, |l| {
                for (k, v) in &tags {
                    l.push(|t| {
                        t.put(Tag::KEY, k);
                        t.put(Tag::VALUE, v);
                    });
                }
            });
        })
    }
}
