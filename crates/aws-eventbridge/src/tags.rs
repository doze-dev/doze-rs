//! Tags on buses and rules, by ARN.

use std::collections::BTreeMap;

use aws_wire::ApiError;
use serde_json::Value;

use crate::core::Core;
use crate::model::key;
use crate::names::Parts;
use crate::req::{Req, invalid, no_bus, no_rule, not_found};
use crate::util::{Out, tag_list};

impl Core {
    /// The tags of the bus or rule the ARN names.
    fn tags_of(
        &mut self,
        account: &str,
        arn: &str,
    ) -> Result<&mut BTreeMap<String, String>, ApiError> {
        let parts = Parts::of(arn).ok_or_else(|| invalid(format!("Invalid ARN: {arn}")))?;
        match parts.kind {
            "event-bus" => self
                .buses
                .get_mut(&key(account, &[parts.tail]))
                .map(|b| &mut b.tags)
                .ok_or_else(|| no_bus(parts.tail)),
            "rule" => {
                let (bus, name) = parts
                    .rule()
                    .ok_or_else(|| invalid(format!("Invalid ARN: {arn}")))?;
                self.rules
                    .get_mut(&key(account, &[bus, name]))
                    .map(|r| &mut r.tags)
                    .ok_or_else(|| no_rule(bus, name))
            }
            _ => Err(not_found(format!("Resource {arn} does not exist."))),
        }
    }

    pub(crate) fn tag_resource(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new = r.tags();
        self.tags_of(r.account(), r.s("ResourceARN"))?.extend(new);
        Ok(Value::Null)
    }

    pub(crate) fn untag_resource(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let tags = self.tags_of(r.account(), r.s("ResourceARN"))?;
        for k in r.list("TagKeys").iter().filter_map(Value::as_str) {
            tags.remove(k);
        }
        Ok(Value::Null)
    }

    pub(crate) fn list_tags(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let tags = self.tags_of(r.account(), r.s("ResourceARN"))?;
        Ok(Out::new().with("Tags", tag_list(tags)).done())
    }
}
