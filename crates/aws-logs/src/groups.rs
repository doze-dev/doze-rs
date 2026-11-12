//! Log groups: create, delete, describe, list, retention, tags, the KMS key and deletion protection.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Group, stream_key};
use crate::req::{Put, Req, invalid, name_of_identifier, no_group};

/// Tags one resource may have.
const MAX_TAGS: usize = 50;
/// The retention periods, in days, that AWS accepts (the list it printed when asked for 2 days).
pub(crate) const RETENTION: [i64; 22] = [
    1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365, 400, 545, 731, 1096, 1827, 2192, 2557, 2922,
    3288, 3653,
];

impl Core {
    pub(crate) fn create_group(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("logGroupName").to_owned();
        if self.groups.get(&name).is_some() {
            // Recorded on AWS (2026-11-09), with no full stop.
            return Err(ApiError::modeled(
                "ResourceAlreadyExistsException",
                "The specified log group already exists",
            ));
        }
        if name.starts_with("aws/") {
            // (memory)
            return Err(invalid("Log group names can't start with 'aws/'"));
        }
        let tags: std::collections::BTreeMap<String, String> = r.map("tags").into_iter().collect();
        if tags.len() > MAX_TAGS {
            return Err(too_many_tags());
        }
        let group = Group {
            name: name.clone(),
            created_ms: self.now_ms(),
            retention_days: None,
            kms_key: r
                .opt("kmsKeyId")
                .filter(|k| !k.is_empty())
                .map(str::to_owned),
            class: r.opt("logGroupClass").unwrap_or("STANDARD").to_owned(),
            deletion_protection: r.flag("deletionProtectionEnabled").unwrap_or(false),
            tags,
            metric_filters: Vec::new(),
            subscriptions: Vec::new(),
        };
        self.groups.insert(name, group);
        Ok(json!({}))
    }

    pub(crate) fn delete_group(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let group = self.group(r)?;
        if group.deletion_protection {
            // (memory)
            return Err(ApiError::modeled(
                "OperationAbortedException",
                "Cannot delete the log group: deletion protection is enabled.",
            ));
        }
        let name = group.name.clone();
        let doomed: Vec<(String, u32)> = self
            .streams
            .iter()
            .filter(|(_, s)| s.group == name)
            .map(|(k, s)| (k.to_owned(), s.no))
            .collect();
        for (key, no) in doomed {
            self.streams.remove(&key);
            self.events
                .delete_range(&format!("{no:08x}"), &format!("{:08x}", no + 1));
        }
        self.groups.remove(&name);
        self.swept.remove(&name);
        Ok(json!({}))
    }

    pub(crate) fn describe_groups(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let prefix = r.opt("logGroupNamePrefix");
        let pattern = r.opt("logGroupNamePattern").filter(|p| !p.is_empty());
        if prefix.is_some() && pattern.is_some() {
            // (memory)
            return Err(invalid(
                "LogGroup name prefix and LogGroup name pattern are mutually exclusive parameters.",
            ));
        }
        let ids: Vec<String> = r
            .strings("logGroupIdentifiers")
            .iter()
            .map(|i| name_of_identifier(i))
            .collect();
        let class = r.opt("logGroupClass");
        let all: Vec<&Group> = self
            .groups
            .iter()
            .map(|(_, g)| g)
            .filter(|g| prefix.is_none_or(|p| g.name.starts_with(p)))
            .filter(|g| pattern.is_none_or(|p| g.name.contains(p)))
            .filter(|g| ids.is_empty() || ids.contains(&g.name))
            .filter(|g| class.is_none_or(|c| g.class == c))
            .collect();
        let (page, token) = paged(&all, r, 50, |g| g.name.as_str());
        let shown: Vec<Value> = page
            .iter()
            .map(|g| self.group_json(g, r.region(), r.account()))
            .collect();
        let mut out = json!({ "logGroups": shown });
        if let Some(t) = token {
            out.put("nextToken", json!(t));
        }
        Ok(out)
    }

    fn group_json(&self, g: &Group, region: &str, account: &str) -> Value {
        let mut v = json!({
            "logGroupName": g.name,
            "creationTime": g.created_ms,
            "metricFilterCount": g.metric_filters.len(),
            "arn": format!("{}:*", g.arn(region, account)),
            "logGroupArn": g.arn(region, account),
            "storedBytes": self
                .streams
                .with_prefix(&stream_key(&g.name, ""))
                .map(|(_, s)| s.bytes)
                .sum::<i64>(),
            "logGroupClass": g.class,
            "deletionProtectionEnabled": g.deletion_protection,
            "bearerTokenAuthenticationEnabled": false,
        });
        if let Some(days) = g.retention_days {
            v.put("retentionInDays", json!(days));
        }
        if let Some(k) = &g.kms_key {
            v.put("kmsKeyId", json!(k));
        }
        v
    }

    pub(crate) fn list_groups(&self, r: &Req<'_>) -> Value {
        let class = r.opt("logGroupClass");
        let wanted: Vec<String> = r
            .opt("logGroupNamePattern")
            .map(|p| p.split('|').map(str::to_owned).collect())
            .unwrap_or_default();
        let matches = |name: &str| {
            wanted.is_empty()
                || wanted.iter().any(|w| match w.strip_prefix('^') {
                    Some(start) => name.starts_with(start),
                    None => name.contains(w.as_str()),
                })
        };
        let all: Vec<&Group> = self
            .groups
            .iter()
            .map(|(_, g)| g)
            .filter(|g| matches(&g.name) && class.is_none_or(|c| g.class == c))
            .collect();
        let (page, token) = paged(&all, r, 50, |g| g.name.as_str());
        let mut out = json!({
            "logGroups": page.iter().map(|g| json!({
                "logGroupName": g.name,
                "logGroupArn": g.arn(r.region(), r.account()),
                "logGroupClass": g.class,
            })).collect::<Vec<_>>(),
        });
        if let Some(t) = token {
            out.put("nextToken", json!(t));
        }
        out
    }

    pub(crate) fn put_retention(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let days = r.int("retentionInDays").unwrap_or(0);
        let g = self.group_mut(r)?;
        if !RETENTION.contains(&days) {
            // Recorded on AWS (2026-11-09).
            let list: Vec<String> = RETENTION.iter().map(ToString::to_string).collect();
            return Err(invalid(format!(
                "Invalid retention value. Valid values are: [{}]",
                list.join(", ")
            )));
        }
        g.retention_days = Some(days);
        Ok(json!({}))
    }

    pub(crate) fn delete_retention(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.group_mut(r)?.retention_days = None;
        Ok(json!({}))
    }

    pub(crate) fn associate_key(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let key = r.s("kmsKeyId").to_owned();
        self.group_mut(r)?.kms_key = Some(key);
        Ok(json!({}))
    }

    pub(crate) fn disassociate_key(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        self.group_mut(r)?.kms_key = None;
        Ok(json!({}))
    }

    pub(crate) fn set_protection(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let on = r.flag("deletionProtectionEnabled").unwrap_or(false);
        self.group_mut(r)?.deletion_protection = on;
        Ok(json!({}))
    }

    /// The group a tag call names: `logGroupName` (the older operations) or the log group in `resourceArn`.
    fn tagged(&mut self, r: &Req<'_>) -> Result<&mut Group, ApiError> {
        let name = match r.opt("resourceArn") {
            Some(arn) if arn.contains(":log-group:") => name_of_identifier(arn),
            Some(_) => return Err(no_group()),
            None => r.s("logGroupName").to_owned(),
        };
        self.groups.get_mut(&name).ok_or_else(no_group)
    }

    pub(crate) fn tag(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let new = r.map("tags");
        let g = self.tagged(r)?;
        let added = new.iter().filter(|(k, _)| !g.tags.contains_key(k)).count();
        if g.tags.len() + added > MAX_TAGS {
            return Err(too_many_tags());
        }
        g.tags.extend(new);
        Ok(json!({}))
    }

    pub(crate) fn untag(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut keys = r.strings("tagKeys");
        keys.extend(r.strings("tags"));
        let g = self.tagged(r)?;
        for k in keys {
            g.tags.remove(&k);
        }
        Ok(json!({}))
    }

    pub(crate) fn list_tags(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let g = self.tagged(r)?;
        Ok(json!({"tags": g.tags}))
    }
}

fn too_many_tags() -> ApiError {
    // (memory)
    ApiError::modeled(
        "TooManyTagsException",
        format!("A resource can have at most {MAX_TAGS} tags."),
    )
}

/// One page of `items` (sorted by `key`): after the request's `nextToken` (the key of the last item of the page before), at most
/// `limit` of them (default `default`). The token for the next page, if there is one.
pub(crate) fn paged<'a, T>(
    items: &[&'a T],
    r: &Req<'_>,
    default: usize,
    key: impl Fn(&T) -> &str,
) -> (Vec<&'a T>, Option<String>) {
    let limit = r
        .int("limit")
        .or_else(|| r.int("maxResults"))
        .map_or(default, |n| usize::try_from(n).unwrap_or(default));
    let after = r.opt("nextToken");
    let rest: Vec<&'a T> = items
        .iter()
        .copied()
        .filter(|i| after.is_none_or(|a| key(i) > a))
        .collect();
    let page: Vec<&'a T> = rest.iter().take(limit).copied().collect();
    let token = (rest.len() > limit)
        .then(|| page.last().map(|i| key(i).to_owned()))
        .flatten();
    (page, token)
}
