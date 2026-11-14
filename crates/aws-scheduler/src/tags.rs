//! Tags, which Scheduler keeps on schedule groups only (a schedule has none).

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::key;
use crate::req::{Req, invalid, no_group};

impl Core {
    /// The tags of the group the ARN names.
    fn tags_of(&mut self, r: &Req<'_>) -> Result<&mut BTreeMap<String, String>, ApiError> {
        let arn = r.s("ResourceArn");
        let prefix = format!(
            "arn:aws:scheduler:{}:{}:schedule-group/",
            r.region(),
            r.account()
        );
        let Some(name) = arn.strip_prefix(&prefix).filter(|n| !n.contains('/')) else {
            // (memory) Schedules carry no tags; an ARN that is not a group of this account and region is refused.
            return Err(invalid(format!(
                "Invalid ResourceArn {arn}: only schedule groups of this account and region can be tagged."
            )));
        };
        self.groups
            .get_mut(&key(r.account(), &[name]))
            .map(|g| &mut g.tags)
            .ok_or_else(|| no_group(name))
    }

    pub(crate) fn tag_resource(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new = r.tags();
        self.tags_of(r)?.extend(new);
        Ok(Value::Null)
    }

    pub(crate) fn untag_resource(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let keys: Vec<String> =
            r.v.get("TagKeys")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|k| k.as_str().map(str::to_owned))
                .collect();
        let tags = self.tags_of(r)?;
        for k in keys {
            tags.remove(&k);
        }
        Ok(Value::Null)
    }

    pub(crate) fn list_tags(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let tags = self.tags_of(r)?;
        let list: Vec<Value> = tags
            .iter()
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect();
        Ok(json!({"Tags": list}))
    }
}
