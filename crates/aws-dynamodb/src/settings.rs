//! Time to live, continuous backups (point-in-time recovery) and tags.

use std::collections::BTreeMap;

use aws_expr::Value;
use aws_wire::ApiError;
use serde_json::{Value as Json, json};

use crate::core::{Core, Cx};
use crate::describe::seconds;
use crate::keys::{self, Space};
use crate::meta::Table;
use crate::req::{array_of, bool_of, int_of, str_of, table_not_found, validation};
use crate::stage::{index_prefix, pk_bytes};

const TAG_LIMIT: usize = 50;
const HOUR_MS: i64 = 3_600_000;

/// The tag rules: at most 50, keys of 1 to 128 characters, values of up to 256, nothing starting `aws:`. The model gives the limits; the
/// wording is `(memory)`.
pub(crate) fn check_tags(tags: &BTreeMap<String, String>) -> Result<(), ApiError> {
    if tags.len() > TAG_LIMIT {
        return Err(ApiError::modeled(
            "LimitExceededException",
            format!("Too many tags: a table can have at most {TAG_LIMIT} tags"),
        ));
    }
    for (k, v) in tags {
        if k.starts_with("aws:") {
            return Err(validation(
                "Tag keys starting with 'aws:' are reserved for internal use",
            ));
        }
        if k.is_empty() || k.chars().count() > 128 {
            return Err(validation("Tag keys must be between 1 and 128 characters"));
        }
        if v.chars().count() > 256 {
            return Err(validation("Tag values must be at most 256 characters"));
        }
    }
    Ok(())
}

impl Core {
    pub(crate) fn describe_ttl(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("TableName")?;
        let t = self
            .tables
            .get(&name)
            .ok_or_else(|| table_not_found(&name))?;
        let desc = if t.ttl.enabled {
            json!({"TimeToLiveStatus": "ENABLED", "AttributeName": t.ttl.attribute})
        } else {
            json!({"TimeToLiveStatus": "DISABLED"})
        };
        Ok(json!({ "TimeToLiveDescription": desc }))
    }

    pub(crate) fn update_ttl(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("TableName")?;
        let t = self
            .tables
            .get(&name)
            .cloned()
            .ok_or_else(|| table_not_found(&name))?;
        let spec = cx
            .body
            .get("TimeToLiveSpecification")
            .cloned()
            .unwrap_or_default();
        let enable = bool_of(&spec, "Enabled").unwrap_or(false);
        let attribute = str_of(&spec, "AttributeName")
            .unwrap_or_default()
            .to_owned();
        if t.ttl.enabled == enable {
            return Err(validation(format!(
                "TimeToLive is already {}",
                if enable { "enabled" } else { "disabled" }
            )));
        }
        if t.ttl.changed_ms > 0 && cx.now_ms - t.ttl.changed_ms < HOUR_MS {
            return Err(validation(
                "Time to live has been modified multiple times within a fixed interval",
            ));
        }
        let mut next = t.clone();
        next.ttl.enabled = enable;
        next.ttl.changed_ms = cx.now_ms;
        if enable {
            next.ttl.attribute.clone_from(&attribute);
            self.backfill_ttl(&next)?;
        } else {
            let mut lo = t.id.to_vec();
            lo.extend_from_slice(&0_u32.to_be_bytes());
            self.pending
                .ranges
                .push((Space::Index, lo.clone(), keys::prefix_end(&lo)));
        }
        if let Some(live) = self.tables.get_mut(&name) {
            *live = next;
        }
        Ok(json!({"TimeToLiveSpecification": {"Enabled": enable, "AttributeName": attribute}}))
    }

    /// Files the items that already have a time-to-live attribute.
    fn backfill_ttl(&mut self, t: &Table) -> Result<(), ApiError> {
        let mut entries = Vec::new();
        self.disk
            .scan(
                Space::Items,
                &t.id,
                &keys::prefix_end(&t.id),
                false,
                |k, v| {
                    if let Ok(item) = crate::stage::decode_item(v)
                        && let Some(Value::N(n)) = item.get(&t.ttl.attribute)
                        && let Some(secs) = n.floor_i64()
                        && let Ok(pk) = pk_bytes(t, &item)
                    {
                        let mut key = t.id.to_vec();
                        key.extend_from_slice(&0_u32.to_be_bytes());
                        aws_expr::Number::from_i64(secs).order_key(&mut key);
                        key.extend_from_slice(&pk);
                        entries.push((key, pk));
                    }
                    let _ = k;
                    crate::disk::Flow::Continue
                },
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (key, pk) in entries {
            self.pending.put(Space::Index, key, pk);
        }
        Ok(())
    }

    /// Files every item of a table in a new index; an item that cannot be indexed (a key of the wrong type) is left out of it.
    pub(crate) fn backfill_index(
        &mut self,
        t: &Table,
        ix: &crate::meta::Index,
    ) -> Result<(), ApiError> {
        let mut entries = Vec::new();
        self.disk
            .scan(
                Space::Items,
                &t.id,
                &keys::prefix_end(&t.id),
                false,
                |_, v| {
                    if let Ok(item) = crate::stage::decode_item(v)
                        && let Ok(pk) = pk_bytes(t, &item)
                        && let Ok(Some(key)) = crate::stage::index_entry(t, ix, &item, &pk)
                    {
                        entries.push((key, pk));
                    }
                    crate::disk::Flow::Continue
                },
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (key, pk) in entries {
            self.pending.put(Space::Index, key, pk);
        }
        Ok(())
    }

    /// Removes every entry of an index.
    pub(crate) fn drop_index(&mut self, t: &Table, ix: &crate::meta::Index) {
        let prefix = index_prefix(t, ix);
        self.pending
            .ranges
            .push((Space::Index, prefix.clone(), keys::prefix_end(&prefix)));
    }

    pub(crate) fn describe_backups(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("TableName")?;
        let t = self
            .tables
            .get(&name)
            .ok_or_else(|| backups_table_missing(&name))?;
        Ok(json!({ "ContinuousBackupsDescription": backups(t, cx.now_ms) }))
    }

    pub(crate) fn update_backups(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("TableName")?;
        if self.tables.get(&name).is_none() {
            return Err(backups_table_missing(&name));
        }
        let spec = cx
            .body
            .get("PointInTimeRecoverySpecification")
            .cloned()
            .unwrap_or_default();
        let enable = bool_of(&spec, "PointInTimeRecoveryEnabled").unwrap_or(false);
        let days = int_of(&spec, "RecoveryPeriodInDays");
        if let Some(t) = self.tables.get_mut(&name) {
            let was = t.pitr.enabled;
            if enable && !was {
                t.pitr.enabled_ms = cx.now_ms;
                t.pitr.days = days.unwrap_or(35);
            } else if let Some(d) = days {
                t.pitr.days = d;
            }
            t.pitr.enabled = enable;
        }
        let t = self.table(&name)?;
        Ok(json!({ "ContinuousBackupsDescription": backups(&t, cx.now_ms) }))
    }

    pub(crate) fn tag_resource(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("ResourceArn")?;
        let mut tags = self
            .tables
            .get(&name)
            .ok_or_else(crate::req::not_found)?
            .tags
            .clone();
        for t in array_of(&cx.body, "Tags") {
            tags.insert(
                str_of(t, "Key").unwrap_or_default().to_owned(),
                str_of(t, "Value").unwrap_or_default().to_owned(),
            );
        }
        check_tags(&tags)?;
        if let Some(t) = self.tables.get_mut(&name) {
            t.tags = tags;
        }
        Ok(Json::Null)
    }

    pub(crate) fn untag_resource(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("ResourceArn")?;
        if self.tables.get(&name).is_none() {
            return Err(crate::req::not_found());
        }
        if let Some(t) = self.tables.get_mut(&name) {
            for k in array_of(&cx.body, "TagKeys")
                .iter()
                .filter_map(Json::as_str)
            {
                t.tags.remove(k);
            }
        }
        Ok(Json::Null)
    }

    pub(crate) fn list_tags(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("ResourceArn")?;
        let t = self.tables.get(&name).ok_or_else(crate::req::not_found)?;
        let tags: Vec<Json> = t
            .tags
            .iter()
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect();
        Ok(json!({ "Tags": tags }))
    }
}

fn backups_table_missing(name: &str) -> ApiError {
    ApiError::modeled("TableNotFoundException", format!("Table not found: {name}"))
}

/// Continuous backups are always on; point-in-time recovery is the setting. `(memory)` for the restorable window: it begins when
/// recovery was enabled (or the period ago, if later) and ends at the present.
fn backups(t: &Table, now_ms: i64) -> Json {
    let pitr = if t.pitr.enabled {
        let earliest = t.pitr.enabled_ms.max(now_ms - t.pitr.days * 24 * HOUR_MS);
        json!({
            "PointInTimeRecoveryStatus": "ENABLED",
            "RecoveryPeriodInDays": t.pitr.days,
            "EarliestRestorableDateTime": seconds(earliest),
            "LatestRestorableDateTime": seconds(now_ms),
        })
    } else {
        json!({"PointInTimeRecoveryStatus": "DISABLED"})
    };
    json!({"ContinuousBackupsStatus": "ENABLED", "PointInTimeRecoveryDescription": pitr})
}
