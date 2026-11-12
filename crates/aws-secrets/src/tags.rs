//! Tags on a secret: `TagResource`, `UntagResource`.

use aws_model_secrets_manager::keys::{
    Tag, TagResourceRequest as Add, UntagResourceRequest as Remove,
};
use aws_wire::handler::{empty, request};
use aws_wire::service::{Call, Reply};

use crate::core::{Core, invalid_parameter, not_found};

const MAX_TAGS: usize = 50;

impl Core {
    pub(crate) fn tag_resource(&mut self, call: &Call) -> Reply {
        let v = request::<Add>(call)?;
        let name = self.locate_live(v.get(Add::SECRET_ID).unwrap_or_default())?;
        let new: Vec<(String, String)> = v
            .get(Add::TAGS)
            .map(|t| {
                t.map(|t| {
                    (
                        t.get(Tag::KEY).unwrap_or_default().to_owned(),
                        t.get(Tag::VALUE).unwrap_or_default().to_owned(),
                    )
                })
                .collect()
            })
            .unwrap_or_default();
        let now = self.now_ms();
        let secret = self.secrets.get_mut(&name).ok_or_else(not_found)?;
        let added = new
            .iter()
            .filter(|(k, _)| !secret.tags.contains_key(k))
            .count();
        if secret.tags.len() + added > MAX_TAGS {
            return Err(invalid_parameter("The secret can have at most 50 tags."));
        }
        secret.tags.extend(new);
        secret.changed_ms = now;
        empty()
    }

    pub(crate) fn untag_resource(&mut self, call: &Call) -> Reply {
        let v = request::<Remove>(call)?;
        let name = self.locate_live(v.get(Remove::SECRET_ID).unwrap_or_default())?;
        let keys: Vec<&str> = v
            .get(Remove::TAG_KEYS)
            .map(Iterator::collect)
            .unwrap_or_default();
        let now = self.now_ms();
        let secret = self.secrets.get_mut(&name).ok_or_else(not_found)?;
        for key in keys {
            secret.tags.remove(key);
        }
        secret.changed_ms = now;
        empty()
    }
}
