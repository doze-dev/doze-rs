//! `RestoreTableFromBackup` and `RestoreTableToPointInTime`: a new table, made from a backup's table record and items, or from the
//! source table's current settings and its items as they were at a time (`pitr.rs`). As the model says, the new table has no stream,
//! time to live, tags or point-in-time recovery of its own; indexes come along unless an override leaves them out, and the billing
//! mode, throughput and encryption can be overridden.
//!
//! AWS restores in the background (the table is `CREATING`, with `RestoreInProgress`, for minutes or hours); here the items are in
//! when the call answers, and the table becomes `ACTIVE` after the create window (`--dynamodb-create-ms`, none by default). The answer
//! still says `CREATING` and `RestoreInProgress: true`, as AWS's does. The messages are `(memory)`.

use std::collections::BTreeMap;

use aws_expr::Item;
use aws_wire::ApiError;
use serde_json::{Value as Json, json};

use crate::backup::table_missing;
use crate::core::{Core, Cx};
use crate::describe::{Shape, Sizes, description, seconds};
use crate::disk::{TxnOp, def};
use crate::extras::{Extra, Restored, restored_key};
use crate::keys::{self, Space};
use crate::meta::{Index, Pitr, Status, Table, Ttl};
use crate::req::{bad_param, bool_of, int_of, set, str_of, table_arn, time_ms_of, validation};
use crate::stage;
use doze_store::StoreError;

/// What a restore starts from.
struct Source {
    table: Table,
    items: Items,
    restored: Restored,
}

/// Where a restore's items come from.
enum Items {
    /// The backup with this id: read inside the write transaction.
    Backup([u8; 16]),
    /// The items as they were at a time, from the recovery log.
    Given(Vec<Item>),
}

impl Core {
    pub(crate) fn restore_from_backup(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let arn = str_of(&cx.body, "BackupArn").unwrap_or_default();
        let b = match self.extras.get(&crate::extras::backup_key(arn)) {
            Some(Extra::Backup(b)) => (**b).clone(),
            _ => {
                return Err(ApiError::modeled(
                    "BackupNotFoundException",
                    format!("Backup not found: {arn}"),
                ));
            }
        };
        self.target_free(cx)?;
        let source = Source {
            table: b.table.clone(),
            items: Items::Backup(b.id),
            restored: Restored {
                backup_arn: Some(b.arn.clone()),
                source_arn: b.table_arn.clone(),
                at_ms: b.created_ms,
            },
        };
        self.restore(cx, source)
    }

    pub(crate) fn restore_to_point_in_time(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let body = &cx.body;
        let name = match (
            str_of(body, "SourceTableName"),
            str_of(body, "SourceTableArn"),
        ) {
            (Some(n), None) => n.to_owned(),
            (None, Some(_)) => cx.table_name("SourceTableArn")?,
            (Some(_), Some(_)) => {
                return Err(validation(
                    "Only one of SourceTableArn or SourceTableName can be specified",
                ));
            }
            (None, None) => {
                return Err(validation(
                    "Either SourceTableArn or SourceTableName must be specified",
                ));
            }
        };
        let t = self
            .tables
            .get(&name)
            .cloned()
            .ok_or_else(|| table_missing(&name))?;
        if !t.pitr.enabled {
            return Err(ApiError::modeled(
                "PointInTimeRecoveryUnavailableException",
                format!("Point in time recovery is not enabled for table '{name}'"),
            ));
        }
        let at = restore_time(&t, body, cx.now_ms)?;
        self.target_free(cx)?;
        let items = self.items_at(&t, at)?;
        let source = Source {
            restored: Restored {
                backup_arn: None,
                source_arn: table_arn(cx.region(), cx.account(), &t.name),
                at_ms: at,
            },
            table: t,
            items: Items::Given(items),
        };
        self.restore(cx, source)
    }

    fn target_free(&self, cx: &Cx<'_>) -> Result<(), ApiError> {
        let target = str_of(&cx.body, "TargetTableName").unwrap_or_default();
        crate::schema::check_table_name(target)?;
        if self.tables.get(target).is_some() {
            return Err(ApiError::modeled(
                "TableAlreadyExistsException",
                format!("Table already exists: {target}"),
            ));
        }
        Ok(())
    }

    /// Makes the target table and files the items in it.
    fn restore(&mut self, cx: &Cx<'_>, source: Source) -> Result<Json, ApiError> {
        let target = str_of(&cx.body, "TargetTableName").unwrap_or_default();
        let t = self.restored_table(cx, &source.table, target)?;
        // The items and their index entries are written by the writer, in the call's own transaction, not read and staged here on
        // the event loop (100,000 items held it for 185 to 345 ms). The new table has no stream, recovery log or time to live, so an
        // item is its row and its index entries.
        let (filled, items) = (t.clone(), source.items);
        self.pending
            .ops
            .push(TxnOp(Box::new(move |txn| fill(txn, &filled, items))));
        let mut shown = description(
            &t,
            cx.region(),
            cx.account(),
            Shape::Create,
            &Sizes::default(),
        );
        // Recorded 2026-11-19: a restore's answer carries neither `DeletionProtectionEnabled` nor the throughput's
        // `NumberOfDecreasesToday` (both restores, from a backup and to a point in time).
        if let Json::Object(m) = &mut shown {
            m.remove("DeletionProtectionEnabled");
        }
        if let Some(Json::Object(p)) = shown.get_mut("ProvisionedThroughput") {
            p.remove("NumberOfDecreasesToday");
        }
        set(
            &mut shown,
            "RestoreSummary",
            summary(&source.restored, true),
        );
        self.tables.insert(target.to_owned(), t);
        self.extras
            .insert(restored_key(target), Extra::Restored(source.restored));
        Ok(json!({ "TableDescription": shown }))
    }

    fn restored_table(&self, cx: &Cx<'_>, source: &Table, target: &str) -> Result<Table, ApiError> {
        let body = &cx.body;
        let until = cx.now_ms + self.windows.create_ms;
        let mut t = source.clone();
        t.id =
            aws_seal::random_bytes::<16>().ok_or_else(|| ApiError::internal("no random bytes"))?;
        target.clone_into(&mut t.name);
        t.created_ms = cx.now_ms;
        t.status = Status::Creating;
        t.status_until_ms = until;
        t.stream = None;
        t.ttl = Ttl::default();
        t.tags = BTreeMap::new();
        t.protect = false;
        t.pitr = Pitr {
            enabled: false,
            days: 35,
            enabled_ms: 0,
        };
        t.gsis = kept(
            &source.gsis,
            body.get("GlobalSecondaryIndexOverride"),
            "global",
        )?;
        t.lsis = kept(
            &source.lsis,
            body.get("LocalSecondaryIndexOverride"),
            "local",
        )?;
        for ix in &mut t.gsis {
            ix.status = Status::Creating;
            ix.status_until_ms = until;
        }
        overrides(&mut t, body, cx.now_ms)?;
        Ok(t)
    }

    /// The `RestoreSummary` a describe of a restored table carries.
    pub(crate) fn restore_summary(&self, table: &str) -> Option<Json> {
        match self.extras.get(&restored_key(table)) {
            Some(Extra::Restored(r)) => Some(summary(r, false)),
            _ => None,
        }
    }
}

fn summary(r: &Restored, in_progress: bool) -> Json {
    let mut out = json!({
        "SourceTableArn": r.source_arn,
        "RestoreDateTime": seconds(r.at_ms),
        "RestoreInProgress": in_progress,
    });
    if let Some(b) = &r.backup_arn {
        set(&mut out, "SourceBackupArn", json!(b));
    }
    out
}

/// The time a point-in-time restore asks for, held to the window recovery can serve.
fn restore_time(t: &Table, body: &Json, now_ms: i64) -> Result<i64, ApiError> {
    let latest = bool_of(body, "UseLatestRestorableTime").unwrap_or(false);
    let at = match (latest, time_ms_of(body, "RestoreDateTime")) {
        (true, Some(_)) => {
            return Err(validation(
                "Only one of RestoreDateTime or UseLatestRestorableTime can be specified",
            ));
        }
        (true, None) => return Ok(now_ms),
        (false, Some(at)) => at,
        (false, None) => {
            return Err(validation(
                "Either RestoreDateTime or UseLatestRestorableTime must be specified",
            ));
        }
    };
    let earliest = crate::pitr::earliest_ms(t, now_ms);
    if at < earliest || at > now_ms {
        // Recorded 2026-11-19.
        return Err(ApiError::modeled(
            "InvalidRestoreTimeException",
            format!(
                "RestoreDateTime '{}' must be within EarliestRestorableDateTime '{}' and LatestRestorableDateTime '{}' for table '{}'",
                iso(at),
                iso(earliest),
                iso(now_ms),
                t.name
            ),
        ));
    }
    Ok(at)
}

/// A time as a restore refusal shows it: whole seconds, `2001-09-09T01:46:40Z`.
fn iso(ms: i64) -> String {
    aws_core::time::format_iso8601(ms.div_euclid(1000)).replace(".000Z", "Z")
}

/// The indexes an override keeps: all of them with none given, else the ones it names, each of which must exist.
fn kept(indexes: &[Index], wanted: Option<&Json>, kind: &str) -> Result<Vec<Index>, ApiError> {
    let Some(Json::Array(list)) = wanted else {
        return Ok(indexes.to_vec());
    };
    let mut out = Vec::new();
    for w in list {
        let name = str_of(w, "IndexName").unwrap_or_default();
        let Some(ix) = indexes.iter().find(|i| i.name == name) else {
            return Err(bad_param(&format!(
                "The {kind} secondary index {name} does not exist in the source table"
            )));
        };
        let mut ix = ix.clone();
        if let Some(p) = w.get("ProvisionedThroughput") {
            ix.provisioned = Some((
                int_of(p, "ReadCapacityUnits").unwrap_or(0),
                int_of(p, "WriteCapacityUnits").unwrap_or(0),
            ));
        }
        out.push(ix);
    }
    Ok(out)
}

/// The billing mode, throughput and encryption overrides.
fn overrides(t: &mut Table, body: &Json, now_ms: i64) -> Result<(), ApiError> {
    let units = body.get("ProvisionedThroughputOverride").map(|p| {
        (
            int_of(p, "ReadCapacityUnits").unwrap_or(0),
            int_of(p, "WriteCapacityUnits").unwrap_or(0),
        )
    });
    match str_of(body, "BillingModeOverride") {
        Some("PAY_PER_REQUEST") => {
            t.billing.pay_per_request = true;
            t.billing.read_units = 0;
            t.billing.write_units = 0;
            t.billing.pay_per_request_since_ms = now_ms;
            for ix in &mut t.gsis {
                ix.provisioned = None;
            }
        }
        Some(_) => {
            let (r, w) = units.ok_or_else(|| {
                validation("ProvisionedThroughputOverride must be specified when BillingModeOverride is PROVISIONED")
            })?;
            t.billing.pay_per_request = false;
            t.billing.read_units = r;
            t.billing.write_units = w;
        }
        None => {
            if let Some((r, w)) = units.filter(|_| !t.billing.pay_per_request) {
                t.billing.read_units = r;
                t.billing.write_units = w;
            }
        }
    }
    if let Some(o) = crate::tables::on_demand_of(body, "OnDemandThroughputOverride") {
        t.on_demand = Some(o);
    }
    if let Some(s) = body.get("SSESpecificationOverride") {
        t.sse = bool_of(s, "Enabled").unwrap_or(false).then(|| {
            (
                str_of(s, "SSEType").unwrap_or("KMS").to_owned(),
                str_of(s, "KMSMasterKeyId").map(str::to_owned),
            )
        });
    }
    Ok(())
}

/// Writes a restored table's items and their index entries inside the write transaction. The items were each written to the source
/// table, so their keys and index attributes are good for it, and a restored table's indexes are some of its; what fails here is a
/// corrupt row.
fn fill(txn: &redb::WriteTransaction, t: &Table, items: Items) -> Result<(), StoreError> {
    use redb::ReadableTable as _;
    let corrupt = |e: ApiError| StoreError::Corrupt(e.message);
    let mut rows = txn.open_table(def(Space::Items))?;
    let mut index = txn.open_table(def(Space::Index))?;
    let mut put = |item: &Item, encoded: Option<&[u8]>| -> Result<(), StoreError> {
        let pk = stage::pk_bytes(t, item).map_err(corrupt)?;
        let value = match encoded {
            Some(v) => v.to_vec(),
            None => stage::encode_item(item).map_err(corrupt)?,
        };
        rows.insert(stage::item_key(t, &pk).as_slice(), value.as_slice())?;
        for ix in t.gsis.iter().chain(&t.lsis) {
            if let Some(entry) = stage::index_entry(t, ix, item, &pk).map_err(corrupt)? {
                index.insert(entry.as_slice(), pk.as_slice())?;
            }
        }
        Ok(())
    };
    match items {
        Items::Given(items) => {
            for item in &items {
                put(item, None)?;
            }
        }
        Items::Backup(id) => {
            let backup = txn.open_table(def(Space::Backup))?;
            for row in backup.range::<&[u8]>(id.as_slice()..keys::prefix_end(&id).as_slice())? {
                let (k, v) = row?;
                // The id alone keys the backup's counts, not an item.
                if k.value().len() == id.len() {
                    continue;
                }
                let item = stage::decode_item(v.value()).map_err(corrupt)?;
                put(&item, Some(v.value()))?;
            }
        }
    }
    Ok(())
}
