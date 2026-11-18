//! On-demand backups: `CreateBackup`, `DescribeBackup`, `ListBackups`, `DeleteBackup`. A backup is a copy of the table's items in the
//! file (the backup space, under the backup's id) and of its table record, so a restore makes the table as it was, indexes and all,
//! and deleting the table leaves the backup. Deleting a table with point-in-time recovery on makes a `SYSTEM` backup of it, kept 35
//! days, as the model says.
//!
//! AWS makes a backup in the background (`CREATING`, then `AVAILABLE` within minutes); here it is whole when the call answers, so
//! only that answer says `CREATING`. The messages are `(memory)` until a recording reaches them.

use aws_expr::Item;
use aws_wire::ApiError;
use serde_json::{Value as Json, json};

use crate::core::{Core, Cx};
use crate::describe::{key_schema, projection, seconds};
use crate::disk::Flow;
use crate::extras::{Backup, Extra, backup_key, task_id};
use crate::keys::{self, Space};
use crate::meta::Table;
use crate::req::{bad_param, int_of, str_of, table_arn, time_ms_of};

/// How long a `SYSTEM` backup is kept (the model).
const SYSTEM_DAYS_MS: i64 = 35 * 24 * 60 * 60 * 1000;
const LIST_MAX: usize = 100;

fn not_found(arn: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "BackupNotFoundException",
        format!("Backup not found: {arn}"),
    )
}

pub(crate) fn table_missing(name: &str) -> ApiError {
    // (memory), the form `DescribeContinuousBackups` answers.
    ApiError::modeled("TableNotFoundException", format!("Table not found: {name}"))
}

/// The backup's key in the backup space: its id, then the item's primary key.
fn item_prefix(id: &[u8; 16]) -> Vec<u8> {
    id.to_vec()
}

impl Core {
    pub(crate) fn create_backup(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("TableName")?;
        let t = self
            .tables
            .get(&name)
            .cloned()
            .ok_or_else(|| table_missing(&name))?;
        if t.status != crate::meta::Status::Active {
            // (memory)
            return Err(ApiError::modeled(
                "TableInUseException",
                format!("Table is not in a valid state to take a backup: {name}"),
            ));
        }
        let backup_name = str_of(&cx.body, "BackupName").unwrap_or_default();
        let b = self.backup_of(
            &t,
            (cx.region(), cx.account()),
            backup_name,
            false,
            cx.now_ms,
        )?;
        let mut details = details(&b);
        // The answer is made as the backup starts; the backup is whole before the next call.
        crate::req::set(&mut details, "BackupStatus", json!("CREATING"));
        Ok(json!({ "BackupDetails": details }))
    }

    /// Copies the table's items and record into a new backup.
    pub(crate) fn backup_of(
        &mut self,
        t: &Table,
        (region, account): (&str, &str),
        name: &str,
        system: bool,
        now_ms: i64,
    ) -> Result<Backup, ApiError> {
        let id =
            aws_seal::random_bytes::<16>().ok_or_else(|| ApiError::internal("no random bytes"))?;
        let tail = task_id(now_ms).ok_or_else(|| ApiError::internal("no random bytes"))?;
        let table_arn = table_arn(region, account, &t.name);
        let mut copied = Vec::new();
        let (mut items, mut bytes) = (0_u64, 0_u64);
        self.disk
            .scan(
                Space::Items,
                &t.id,
                &keys::prefix_end(&t.id),
                false,
                |k, v| {
                    let mut key = item_prefix(&id);
                    key.extend_from_slice(k.get(16..).unwrap_or_default());
                    items += 1;
                    bytes += v.len() as u64;
                    copied.push((key, v.to_vec()));
                    Flow::Continue
                },
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        for (k, v) in copied {
            self.pending.put(Space::Backup, k, v);
        }
        let b = Backup {
            id,
            arn: format!("{table_arn}/backup/{tail}"),
            name: name.to_owned(),
            created_ms: now_ms,
            system,
            table: t.clone(),
            table_arn,
            items,
            bytes,
        };
        self.extras
            .insert(backup_key(&b.arn), Extra::Backup(Box::new(b.clone())));
        Ok(b)
    }

    fn backup(&self, arn: &str) -> Result<Backup, ApiError> {
        match self.extras.get(&backup_key(arn)) {
            Some(Extra::Backup(b)) => Ok((**b).clone()),
            _ => Err(not_found(arn)),
        }
    }

    /// The items a backup holds.
    pub(crate) fn backup_items(&self, b: &Backup) -> Result<Vec<Item>, ApiError> {
        let mut out = Vec::new();
        let mut bad = None;
        let prefix = item_prefix(&b.id);
        self.disk
            .scan(
                Space::Backup,
                &prefix,
                &keys::prefix_end(&prefix),
                false,
                |_, v| {
                    match crate::stage::decode_item(v) {
                        Ok(i) => out.push(i),
                        Err(e) => bad = Some(e),
                    }
                    Flow::Continue
                },
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
        bad.map_or(Ok(out), Err)
    }

    pub(crate) fn describe_backup(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let arn = str_of(&cx.body, "BackupArn").unwrap_or_default();
        let b = self.backup(arn)?;
        Ok(json!({ "BackupDescription": description(&b, cx.now_ms) }))
    }

    pub(crate) fn delete_backup(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let arn = str_of(&cx.body, "BackupArn").unwrap_or_default();
        let b = self.backup(arn)?;
        self.extras.remove(&backup_key(arn));
        let prefix = item_prefix(&b.id);
        self.pending
            .ranges
            .push((Space::Backup, prefix.clone(), keys::prefix_end(&prefix)));
        let mut shown = description(&b, cx.now_ms);
        if let Some(d) = shown.get_mut("BackupDetails") {
            crate::req::set(d, "BackupStatus", json!("DELETED"));
        }
        Ok(json!({ "BackupDescription": shown }))
    }

    pub(crate) fn list_backups(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let body = &cx.body;
        let table = match str_of(body, "TableName") {
            Some(_) => Some(cx.table_name("TableName")?),
            None => None,
        };
        let kind = str_of(body, "BackupType").unwrap_or("USER");
        let (lo, hi) = (
            time_ms_of(body, "TimeRangeLowerBound"),
            time_ms_of(body, "TimeRangeUpperBound"),
        );
        let limit = int_of(body, "Limit")
            .and_then(|l| usize::try_from(l).ok())
            .unwrap_or(LIST_MAX);
        let mut all: Vec<&Backup> = self
            .extras
            .with_prefix("backup/")
            .filter_map(|(_, e)| match e {
                Extra::Backup(b) => Some(&**b),
                _ => None,
            })
            .filter(|b| table.as_deref().is_none_or(|t| b.table.name == t))
            .filter(|b| match kind {
                "ALL" => true,
                "SYSTEM" => b.system,
                "USER" => !b.system,
                // AWS Backup's backups are not made here.
                _ => false,
            })
            .filter(|b| lo.is_none_or(|l| b.created_ms >= l) && hi.is_none_or(|h| b.created_ms < h))
            .collect();
        all.sort_by(|a, b| (a.created_ms, &a.arn).cmp(&(b.created_ms, &b.arn)));
        if let Some(start) = str_of(body, "ExclusiveStartBackupArn") {
            let Some(at) = all.iter().position(|b| b.arn == start) else {
                return Err(bad_param("Invalid ExclusiveStartBackupArn"));
            };
            all.drain(..=at);
        }
        let more = all.len() > limit;
        all.truncate(limit);
        let summaries: Vec<Json> = all.iter().map(|b| summary(b, cx.now_ms)).collect();
        let mut out = json!({ "BackupSummaries": summaries });
        if more && let Some(last) = all.last() {
            crate::req::set(&mut out, "LastEvaluatedBackupArn", json!(last.arn));
        }
        Ok(out)
    }

    /// The `SYSTEM` backup AWS keeps of a table with point-in-time recovery on when it is deleted.
    pub(crate) fn system_backup(&mut self, t: &Table, cx: &Cx<'_>) -> Result<(), ApiError> {
        if !t.pitr.enabled {
            return Ok(());
        }
        // (memory) for the name AWS gives it.
        let name = format!("{}$DeletedTableBackup", t.name);
        self.backup_of(t, (cx.region(), cx.account()), &name, true, cx.now_ms)
            .map(drop)
    }

    /// Removes the `SYSTEM` backups past their 35 days.
    pub(crate) fn expire_backups(&mut self, now_ms: i64) {
        let gone: Vec<(String, [u8; 16])> = self
            .extras
            .with_prefix("backup/")
            .filter_map(|(k, e)| match e {
                Extra::Backup(b) if b.system && b.created_ms + SYSTEM_DAYS_MS <= now_ms => {
                    Some((k.to_owned(), b.id))
                }
                _ => None,
            })
            .collect();
        for (key, id) in gone {
            self.extras.remove(&key);
            let prefix = item_prefix(&id);
            self.pending
                .ranges
                .push((Space::Backup, prefix.clone(), keys::prefix_end(&prefix)));
        }
    }
}

fn kind(b: &Backup) -> &'static str {
    if b.system { "SYSTEM" } else { "USER" }
}

fn details(b: &Backup) -> Json {
    let mut out = json!({
        "BackupArn": b.arn,
        "BackupName": b.name,
        "BackupSizeBytes": b.bytes,
        "BackupStatus": "AVAILABLE",
        "BackupType": kind(b),
        "BackupCreationDateTime": seconds(b.created_ms),
    });
    if b.system {
        crate::req::set(
            &mut out,
            "BackupExpiryDateTime",
            json!(seconds(b.created_ms + SYSTEM_DAYS_MS)),
        );
    }
    out
}

fn summary(b: &Backup, _now_ms: i64) -> Json {
    let mut out = details(b);
    for (k, v) in [
        ("TableName", json!(b.table.name)),
        ("TableId", json!(b.table.id_text())),
        ("TableArn", json!(b.table_arn)),
    ] {
        crate::req::set(&mut out, k, v);
    }
    out
}

fn throughput(t: &Table) -> Json {
    let (r, w) = if t.billing.pay_per_request {
        (0, 0)
    } else {
        (t.billing.read_units, t.billing.write_units)
    };
    json!({"ReadCapacityUnits": r, "WriteCapacityUnits": w})
}

/// `BackupDescription`: the backup, the table it was made of, and the table's features then.
fn description(b: &Backup, _now_ms: i64) -> Json {
    let t = &b.table;
    let mut source = json!({
        "TableName": t.name,
        "TableId": t.id_text(),
        "TableArn": b.table_arn,
        "TableSizeBytes": b.bytes,
        "KeySchema": key_schema(&t.hash, t.range.as_ref()),
        "TableCreationDateTime": seconds(t.created_ms),
        "ProvisionedThroughput": throughput(t),
        "ItemCount": b.items,
        "BillingMode": if t.billing.pay_per_request { "PAY_PER_REQUEST" } else { "PROVISIONED" },
    });
    if let Some((r, w)) = t.on_demand {
        crate::req::set(
            &mut source,
            "OnDemandThroughput",
            json!({"MaxReadRequestUnits": r, "MaxWriteRequestUnits": w}),
        );
    }
    json!({
        "BackupDetails": details(b),
        "SourceTableDetails": source,
        "SourceTableFeatureDetails": features(t),
    })
}

fn features(t: &Table) -> Json {
    let mut out = json!({});
    if !t.lsis.is_empty() {
        let lsis: Vec<Json> = t
            .lsis
            .iter()
            .map(|i| json!({"IndexName": i.name, "KeySchema": key_schema(&i.hash, i.range.as_ref()), "Projection": projection(&i.projection)}))
            .collect();
        crate::req::set(&mut out, "LocalSecondaryIndexes", json!(lsis));
    }
    if !t.gsis.is_empty() {
        let gsis: Vec<Json> = t
            .gsis
            .iter()
            .map(|i| {
                let (r, w) = i.provisioned.unwrap_or((0, 0));
                json!({
                    "IndexName": i.name,
                    "KeySchema": key_schema(&i.hash, i.range.as_ref()),
                    "Projection": projection(&i.projection),
                    "ProvisionedThroughput": {"ReadCapacityUnits": r, "WriteCapacityUnits": w},
                })
            })
            .collect();
        crate::req::set(&mut out, "GlobalSecondaryIndexes", json!(gsis));
    }
    if let Some(s) = t.stream.as_ref().filter(|s| s.enabled) {
        crate::req::set(
            &mut out,
            "StreamDescription",
            json!({"StreamEnabled": true, "StreamViewType": s.view}),
        );
    }
    let ttl = if t.ttl.enabled {
        json!({"TimeToLiveStatus": "ENABLED", "AttributeName": t.ttl.attribute})
    } else {
        json!({"TimeToLiveStatus": "DISABLED"})
    };
    crate::req::set(&mut out, "TimeToLiveDescription", ttl);
    if let Some((kind, key)) = &t.sse {
        let mut sse = json!({"Status": "ENABLED", "SSEType": kind});
        if let Some(k) = key {
            crate::req::set(&mut sse, "KMSMasterKeyArn", json!(k));
        }
        crate::req::set(&mut out, "SSEDescription", sse);
    }
    out
}
