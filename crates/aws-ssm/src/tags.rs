//! Tags on a parameter: `AddTagsToResource`, `RemoveTagsFromResource`, `ListTagsForResource`. Only the `Parameter` resource
//! type is built; the others (managed instances, documents, maintenance windows) belong to the refused half of SSM.

use aws_model_ssm::keys::{
    AddTagsToResourceRequest as Add, ListTagsForResourceRequest as List,
    ListTagsForResourceResult as Listed, RemoveTagsFromResourceRequest as Remove, Tag,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};

use crate::core::{Core, empty, reply, request};
use crate::model::Parameter;

const MAX_TAGS: usize = 50;

impl Core {
    /// The parameter a tag call names. `kind` is the `ResourceType`.
    fn tagged(&mut self, kind: &str, id: &str) -> Result<&mut Parameter, ApiError> {
        if kind != "Parameter" {
            return Err(ApiError::modeled(
                "InvalidResourceType",
                format!("The resource type {kind} is not built in doze-aws; only Parameter is."),
            ));
        }
        self.params.get_mut(id).ok_or_else(|| {
            ApiError::modeled(
                "InvalidResourceId",
                "The resource ID is not valid. Verify the ID and try again.",
            )
        })
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
        let p = self.tagged(
            v.get(Add::RESOURCE_TYPE).unwrap_or_default(),
            v.get(Add::RESOURCE_ID).unwrap_or_default(),
        )?;
        let added = new.iter().filter(|(k, _)| !p.tags.contains_key(k)).count();
        if p.tags.len() + added > MAX_TAGS {
            return Err(ApiError::modeled(
                "TooManyUpdates",
                "The number of tags on a parameter can't be more than 50.",
            ));
        }
        p.tags.extend(new);
        empty()
    }

    pub(crate) fn remove_tags(&mut self, call: &Call) -> Reply {
        let v = request::<Remove>(call)?;
        let keys: Vec<&str> = v
            .get(Remove::TAG_KEYS)
            .map(Iterator::collect)
            .unwrap_or_default();
        let p = self.tagged(
            v.get(Remove::RESOURCE_TYPE).unwrap_or_default(),
            v.get(Remove::RESOURCE_ID).unwrap_or_default(),
        )?;
        for key in keys {
            p.tags.remove(key);
        }
        empty()
    }

    pub(crate) fn list_tags(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let p = self.tagged(
            v.get(List::RESOURCE_TYPE).unwrap_or_default(),
            v.get(List::RESOURCE_ID).unwrap_or_default(),
        )?;
        let tags: Vec<(String, String)> =
            p.tags.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
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
