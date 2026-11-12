//! CreateTable, DescribeTable, DeleteTable and ListTables.
//!
//! Wording: the messages in `conformance/snapshots/test_dynamodb.json` (recorded 2026-11-09) are AWS's own; every other message in this file is
//! `(memory)`: from the developer guide and from use, and unchecked until a recording reaches it.

use std::collections::BTreeSet;

use aws_wire::ApiError;
use serde_json::{Value as Json, json};

use crate::core::{Core, Cx};
use crate::describe::{Shape, Sizes, description};
use crate::keys::{self, Space};
use crate::meta::{Index, Pitr, Status, Stream, Table, Ttl};
use crate::req::{array_of, bad_param, bool_of, int_of, str_of, table_not_found, validation};
use crate::schema::{
    MAX_GSI, MAX_LSI, MAX_PROJECTED, billing, check_table_name, definitions, index, key_schema,
    protection,
};
use crate::stream::label_of;

const PITR_DEFAULT_DAYS: i64 = 35;

impl Core {
    pub(crate) fn create_table(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let body = &cx.body;
        let name = str_of(body, "TableName").unwrap_or_default();
        check_table_name(name)?;
        refuse_unbuilt(body)?;
        let table = self.table_from(cx, name)?;
        if let Some(existing) = self.tables.get(name) {
            return Err(ApiError::modeled(
                "ResourceInUseException",
                if existing.status == Status::Deleting {
                    format!("Table is being deleted: {name}")
                } else {
                    format!("Table already exists: {name}")
                },
            ));
        }
        let shown = description(
            &table,
            cx.region(),
            cx.account(),
            Shape::Create,
            &Sizes::default(),
        );
        self.tables.insert(name.to_owned(), table);
        Ok(json!({ "TableDescription": shown }))
    }

    /// The table a `CreateTable` request describes, checked.
    fn table_from(&self, cx: &Cx<'_>, name: &str) -> Result<Table, ApiError> {
        let body = &cx.body;
        let defs = definitions(body)?;
        let (hash, range) = key_schema(array_of(body, "KeySchema"), &defs, true)?;
        let bill = billing(body, cx.now_ms)?;
        let until = cx.now_ms + self.windows.create_ms;
        let mut table = Table {
            id: random_id()?,
            name: name.to_owned(),
            created_ms: cx.now_ms,
            status: Status::Creating,
            status_until_ms: until,
            hash,
            range,
            billing: bill,
            gsis: Vec::new(),
            lsis: Vec::new(),
            next_index: 1,
            stream: None,
            ttl: Ttl::default(),
            protect: protection(body).unwrap_or(false),
            infrequent: str_of(body, "TableClass") == Some("STANDARD_INFREQUENT_ACCESS"),
            sse: sse_of(body),
            tags: tags_of(body)?,
            pitr: Pitr {
                enabled: false,
                days: PITR_DEFAULT_DAYS,
                enabled_ms: 0,
            },
            on_demand: on_demand_of(body, "OnDemandThroughput"),
            warm: warm_of(body),
        };
        self.add_indexes(&mut table, body, &defs, until)?;
        if let Some(spec) = body.get("StreamSpecification") {
            table.stream = new_stream(spec, cx.now_ms)?;
        }
        check_definitions(&table, &defs)?;
        Ok(table)
    }

    fn add_indexes(
        &self,
        table: &mut Table,
        body: &Json,
        defs: &std::collections::BTreeMap<String, char>,
        until: i64,
    ) -> Result<(), ApiError> {
        let mut names = BTreeSet::new();
        let ppr = table.billing.pay_per_request;
        let mut projected = 0;
        for e in array_of(body, "LocalSecondaryIndexes") {
            let table_hash = table.hash.clone();
            if table.range.is_none() {
                return Err(bad_param(
                    "Table KeySchema does not have a range key, which is required when specifying a LocalSecondaryIndex",
                ));
            }
            let ix = index(e, table.next_index, Some(&table_hash), defs, ppr, 0)?;
            table.next_index += 1;
            projected += projected_count(&ix);
            take_name(&mut names, &ix)?;
            table.lsis.push(ix);
        }
        for e in array_of(body, "GlobalSecondaryIndexes") {
            let mut ix = index(e, table.next_index, None, defs, ppr, 0)?;
            ix.status_until_ms = until;
            table.next_index += 1;
            projected += projected_count(&ix);
            take_name(&mut names, &ix)?;
            table.gsis.push(ix);
        }
        if table.lsis.len() > MAX_LSI {
            return Err(validation(format!(
                "Cannot have more than {MAX_LSI} local secondary indexes per table"
            )));
        }
        if table.gsis.len() > MAX_GSI {
            return Err(ApiError::modeled(
                "LimitExceededException",
                format!("Cannot have more than {MAX_GSI} global secondary indexes per table"),
            ));
        }
        if projected > MAX_PROJECTED {
            return Err(bad_param(&format!(
                "Total number of attributes specified in all Secondary Indexes must not exceed {MAX_PROJECTED}"
            )));
        }
        Ok(())
    }

    pub(crate) fn describe_table(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("TableName")?;
        let t = self
            .tables
            .get(&name)
            .cloned()
            .ok_or_else(|| table_not_found(&name))?;
        let sizes = self.sizes(&t)?;
        Ok(json!({ "Table": description(&t, cx.region(), cx.account(), Shape::Describe, &sizes) }))
    }

    pub(crate) fn delete_table(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let name = cx.table_name("TableName")?;
        let t = self
            .tables
            .get(&name)
            .cloned()
            .ok_or_else(|| table_not_found(&name))?;
        if t.protect {
            return Err(validation(format!(
                "Resource cannot be deleted as it is currently protected against deletion. Disable deletion protection first. Table: {name}"
            )));
        }
        let until = cx.now_ms + self.windows.delete_ms;
        let mut shown = t.clone();
        if let Some(live) = self.tables.get_mut(&name)
            && live.status != Status::Deleting
        {
            live.status = Status::Deleting;
            live.status_until_ms = until;
            shown.status = Status::Deleting;
        }
        Ok(
            json!({ "TableDescription": description(&shown, cx.region(), cx.account(), Shape::Delete, &Sizes::default()) }),
        )
    }

    pub(crate) fn list_tables(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let limit = int_of(&cx.body, "Limit")
            .and_then(|l| usize::try_from(l).ok())
            .unwrap_or(100);
        let after = str_of(&cx.body, "ExclusiveStartTableName");
        let mut names: Vec<&str> = self
            .tables
            .iter()
            .map(|(n, _)| n)
            .filter(|n| after.is_none_or(|a| *n > a))
            .collect();
        let more = names.len() > limit;
        names.truncate(limit);
        let mut out = json!({ "TableNames": names });
        if more && let Some(last) = names.last() {
            crate::req::set(&mut out, "LastEvaluatedTableName", json!(last));
        }
        Ok(out)
    }

    /// How many items and bytes a table and its indexes hold, counted from the file.
    pub(crate) fn sizes(&self, t: &Table) -> Result<Sizes, ApiError> {
        let count = |space: Space, prefix: &[u8]| -> Result<(i64, i64), ApiError> {
            let (mut n, mut bytes) = (0_i64, 0_i64);
            self.disk
                .scan(space, prefix, &keys::prefix_end(prefix), false, |k, v| {
                    n += 1;
                    bytes += i64::try_from(k.len() + v.len()).unwrap_or(0);
                    crate::disk::Flow::Continue
                })
                .map_err(|e| ApiError::internal(e.to_string()))?;
            Ok((n, bytes))
        };
        let mut sizes = Sizes {
            table: count(Space::Items, &t.id)?,
            indexes: Vec::new(),
        };
        for ix in t.gsis.iter().chain(&t.lsis) {
            sizes.indexes.push((
                ix.id,
                count(Space::Index, &crate::stage::index_prefix(t, ix))?,
            ));
        }
        Ok(sizes)
    }
}

fn projected_count(ix: &Index) -> usize {
    match &ix.projection {
        crate::meta::Projection::Include(n) => n.len(),
        _ => 0,
    }
}

fn take_name(names: &mut BTreeSet<String>, ix: &Index) -> Result<(), ApiError> {
    if names.insert(ix.name.clone()) {
        Ok(())
    } else {
        Err(bad_param(&format!("Duplicate index name: {}", ix.name)))
    }
}

/// Every attribute definition is a key attribute of the table or an index, and every key attribute is defined.
pub(crate) fn check_definitions(
    t: &Table,
    defs: &std::collections::BTreeMap<String, char>,
) -> Result<(), ApiError> {
    if t.attribute_definitions().len() != defs.len() {
        return Err(bad_param(
            "Number of attributes in KeySchema does not exactly match number of attributes defined in AttributeDefinitions",
        ));
    }
    Ok(())
}

/// Parameters that name something doze-aws does not build.
fn refuse_unbuilt(body: &Json) -> Result<(), ApiError> {
    for member in ["ResourcePolicy", "GlobalTableSettingsReplicationMode"] {
        if body.get(member).is_some() {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                format!("{member} of CreateTable is not built in doze-aws."),
            ));
        }
    }
    Ok(())
}

fn random_id() -> Result<[u8; 16], ApiError> {
    aws_seal::random_bytes::<16>().ok_or_else(|| ApiError::internal("no random bytes"))
}

/// A new stream from a `StreamSpecification`.
pub(crate) fn new_stream(spec: &Json, now_ms: i64) -> Result<Option<Stream>, ApiError> {
    if !bool_of(spec, "StreamEnabled").unwrap_or(false) {
        return Ok(None);
    }
    let view = str_of(spec, "StreamViewType")
        .ok_or_else(|| bad_param("StreamViewType must be specified when StreamEnabled is true"))?;
    Ok(Some(Stream {
        id: random_id()?,
        label: label_of(now_ms),
        view: view.to_owned(),
        enabled: true,
        last_seq: 0,
    }))
}

fn sse_of(body: &Json) -> Option<(String, Option<String>)> {
    let s = body.get("SSESpecification")?;
    if !bool_of(s, "Enabled").unwrap_or(false) {
        return None;
    }
    Some((
        str_of(s, "SSEType").unwrap_or("KMS").to_owned(),
        str_of(s, "KMSMasterKeyId").map(str::to_owned),
    ))
}

fn tags_of(body: &Json) -> Result<std::collections::BTreeMap<String, String>, ApiError> {
    let mut out = std::collections::BTreeMap::new();
    for t in array_of(body, "Tags") {
        out.insert(
            str_of(t, "Key").unwrap_or_default().to_owned(),
            str_of(t, "Value").unwrap_or_default().to_owned(),
        );
    }
    crate::settings::check_tags(&out)?;
    Ok(out)
}

pub(crate) fn on_demand_of(body: &Json, member: &str) -> Option<(i64, i64)> {
    let o = body.get(member)?;
    Some((
        int_of(o, "MaxReadRequestUnits")?,
        int_of(o, "MaxWriteRequestUnits")?,
    ))
}

pub(crate) fn warm_of(body: &Json) -> Option<(i64, i64)> {
    let o = body.get("WarmThroughput")?;
    Some((
        int_of(o, "ReadUnitsPerSecond")?,
        int_of(o, "WriteUnitsPerSecond")?,
    ))
}
