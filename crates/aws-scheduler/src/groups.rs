//! Schedule groups: `default`, which every account has and nobody deletes, and the ones callers make. Deleting a group deletes its
//! schedules with it, as AWS does; AWS shows the group `DELETING` for a while first, here it is gone when the call answers.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{DEFAULT_GROUP, Group, group_arn, key};
use crate::page::{page, secs, with_token};
use crate::req::{Req, conflict, invalid, no_group};

impl Core {
    /// Makes the account's `default` group the first time the account is seen.
    pub(crate) fn ensure_default(&mut self, account: &str) {
        let k = key(account, &[DEFAULT_GROUP]);
        if self.groups.get(&k).is_none() {
            let now = self.now_ms();
            let g = Group {
                account: account.to_owned(),
                name: DEFAULT_GROUP.to_owned(),
                arn: group_arn(&self.region, account, DEFAULT_GROUP),
                tags: std::collections::BTreeMap::new(),
                created_ms: now,
                modified_ms: now,
            };
            self.groups.insert(k, g);
        }
    }

    pub(crate) fn create_group(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("Name");
        let k = key(r.account(), &[name]);
        if self.groups.get(&k).is_some() {
            // (memory)
            return Err(conflict(format!("Schedule group {name} already exists.")));
        }
        let now = self.now_ms();
        let arn = group_arn(r.region(), r.account(), name);
        self.groups.insert(
            k,
            Group {
                account: r.account().to_owned(),
                name: name.to_owned(),
                arn: arn.clone(),
                tags: r.tags().into_iter().collect(),
                created_ms: now,
                modified_ms: now,
            },
        );
        Ok(json!({"ScheduleGroupArn": arn}))
    }

    pub(crate) fn get_group(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("Name");
        let g = self
            .groups
            .get(&key(r.account(), &[name]))
            .ok_or_else(|| no_group(name))?;
        Ok(summary(g))
    }

    pub(crate) fn delete_group(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("Name");
        if name == DEFAULT_GROUP {
            // (memory)
            return Err(invalid("The default schedule group cannot be deleted."));
        }
        let k = key(r.account(), &[name]);
        if self.groups.remove(&k).is_none() {
            return Err(no_group(name));
        }
        let prefix = format!("{k}/");
        let doomed: Vec<String> = self
            .schedules
            .with_prefix(&prefix)
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in doomed {
            self.schedules.remove(&k);
        }
        Ok(Value::Null)
    }

    pub(crate) fn list_groups(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = key(r.account(), &[]) + "/";
        let name_prefix = r.s("NamePrefix");
        let items = self
            .groups
            .with_prefix(&prefix)
            .filter(|(_, g)| g.name.starts_with(name_prefix))
            .map(|(k, g)| (k.to_owned(), summary(g)));
        let (groups, token) = page(items, r)?;
        Ok(with_token(json!({"ScheduleGroups": groups}), token))
    }
}

/// A group as `GetScheduleGroup` and `ListScheduleGroups` show it.
fn summary(g: &Group) -> Value {
    json!({
        "Arn": g.arn,
        "Name": g.name,
        "State": "ACTIVE",
        "CreationDate": secs(g.created_ms),
        "LastModificationDate": secs(g.modified_ms),
    })
}
