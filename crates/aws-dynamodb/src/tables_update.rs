//! UpdateTable: billing, global secondary indexes, the stream, encryption, table class and deletion protection.

use aws_wire::ApiError;
use serde_json::{Value as Json, json};

use crate::core::{Core, Cx};
use crate::describe::{Shape, Sizes, description};
use crate::meta::{Status, Table};
use crate::req::{array_of, bad_param, bool_of, str_of, table_not_found, validation};
use crate::schema::{MAX_GSI, billing, definitions, index, throughput};
use crate::stream::stream_arn;
use crate::tables::{new_stream, on_demand_of, warm_of};

/// Members of UpdateTable that name things doze-aws does not build.
const UNBUILT: [&str; 4] = [
    "ReplicaUpdates",
    "MultiRegionConsistency",
    "GlobalTableWitnessUpdates",
    "VectorIndexUpdates",
];
/// Members that count as asking for a change.
const CHANGES: [&str; 9] = [
    "BillingMode",
    "ProvisionedThroughput",
    "GlobalSecondaryIndexUpdates",
    "StreamSpecification",
    "SSESpecification",
    "TableClass",
    "DeletionProtectionEnabled",
    "OnDemandThroughput",
    "WarmThroughput",
];

impl Core {
    pub(crate) fn update_table(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let body = &cx.body;
        let name = cx.table_name("TableName")?;
        let mut t = self
            .tables
            .get(&name)
            .cloned()
            .ok_or_else(|| table_not_found(&name))?;
        if let Some(m) = UNBUILT.iter().find(|m| body.get(**m).is_some()) {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                format!("{m} of UpdateTable is not built in doze-aws."),
            ));
        }
        if !CHANGES.iter().any(|m| body.get(*m).is_some()) {
            return Err(validation(
                "At least one of ProvisionedThroughput, BillingMode, UpdateStreamEnabled, GlobalSecondaryIndexUpdates or SSESpecification or ReplicaUpdates is required",
            ));
        }
        if t.status != Status::Active {
            return Err(ApiError::modeled(
                "ResourceInUseException",
                format!(
                    "Table is being {}: {name}",
                    match t.status {
                        Status::Creating => "created",
                        Status::Deleting => "deleted",
                        _ => "updated",
                    }
                ),
            ));
        }
        let mut changing = self.update_billing(&mut t, cx)?;
        changing |= self.update_indexes(&mut t, cx)?;
        changing |= update_stream(&mut t, body, cx)?;
        update_settings(&mut t, body);
        if changing {
            t.status = Status::Updating;
            t.status_until_ms = cx.now_ms + self.windows.update_ms;
        }
        let shown = description(
            &t,
            cx.region(),
            cx.account(),
            Shape::Describe,
            &Sizes::default(),
        );
        self.tables.insert(name, t);
        Ok(json!({ "TableDescription": shown }))
    }

    /// `BillingMode` and `ProvisionedThroughput`. Returns whether anything changed.
    fn update_billing(&self, t: &mut Table, cx: &Cx<'_>) -> Result<bool, ApiError> {
        let body = &cx.body;
        let given = throughput(body.get("ProvisionedThroughput"));
        match (str_of(body, "BillingMode"), given) {
            (None, None) => Ok(false),
            (Some("PAY_PER_REQUEST"), _) if t.billing.pay_per_request && given.is_none() => {
                Ok(false)
            }
            (Some("PAY_PER_REQUEST"), Some(_)) => Err(bad_param(
                "Neither ReadCapacityUnits nor WriteCapacityUnits can be specified when BillingMode is PAY_PER_REQUEST",
            )),
            (Some("PAY_PER_REQUEST"), None) => {
                t.billing = billing(body, cx.now_ms)?;
                Ok(true)
            }
            (None | Some("PROVISIONED"), Some((read, write))) => {
                if !t.billing.pay_per_request
                    && (t.billing.read_units, t.billing.write_units) == (read, write)
                {
                    return Err(validation(format!(
                        "The provisioned throughput for the table will not change. The requested value equals the current value. Current ReadCapacityUnits provisioned for the table: {read}. Requested ReadCapacityUnits: {read}. Current WriteCapacityUnits provisioned for the table: {write}. Requested WriteCapacityUnits: {write}. Refer to the Amazon DynamoDB Developer Guide for current limits and how to request higher limits."
                    )));
                }
                if t.billing.pay_per_request && str_of(body, "BillingMode").is_none() {
                    return Err(bad_param(
                        "Cannot specify ProvisionedThroughput on a table with BillingMode PAY_PER_REQUEST",
                    ));
                }
                t.billing = billing(body, cx.now_ms)?;
                Ok(true)
            }
            (Some("PROVISIONED"), None) => Err(bad_param(
                "ProvisionedThroughput must be specified when BillingMode is PROVISIONED",
            )),
            _ => Ok(false),
        }
    }

    /// `GlobalSecondaryIndexUpdates`: create, update (throughput) or delete one index. Returns whether anything changed.
    fn update_indexes(&mut self, t: &mut Table, cx: &Cx<'_>) -> Result<bool, ApiError> {
        let updates = array_of(&cx.body, "GlobalSecondaryIndexUpdates");
        if updates.is_empty() {
            return Ok(false);
        }
        let mut defs = t.attribute_definitions();
        defs.extend(definitions(&cx.body)?);
        let mut structural = 0;
        for u in updates {
            if let Some(c) = u.get("Create") {
                structural += 1;
                self.create_index(t, c, &defs, cx)?;
            } else if let Some(d) = u.get("Delete") {
                structural += 1;
                self.delete_index(t, str_of(d, "IndexName").unwrap_or_default())?;
            } else if let Some(up) = u.get("Update") {
                update_index_throughput(t, up)?;
            }
        }
        if structural > 1 {
            return Err(bad_param(
                "Only one global secondary index update is allowed per UpdateTable call",
            ));
        }
        // Every definition the request gave must belong to a key of the table or of an index.
        let after = t.attribute_definitions();
        if definitions(&cx.body)?
            .keys()
            .any(|k| !after.contains_key(k))
        {
            return Err(bad_param(
                "Number of attributes in KeySchema does not exactly match number of attributes defined in AttributeDefinitions",
            ));
        }
        Ok(true)
    }

    fn create_index(
        &mut self,
        t: &mut Table,
        spec: &Json,
        defs: &std::collections::BTreeMap<String, char>,
        cx: &Cx<'_>,
    ) -> Result<(), ApiError> {
        if t.gsis.len() >= MAX_GSI {
            return Err(ApiError::modeled(
                "LimitExceededException",
                format!("Cannot have more than {MAX_GSI} global secondary indexes per table"),
            ));
        }
        let mut ix = index(
            spec,
            t.next_index,
            None,
            defs,
            t.billing.pay_per_request,
            cx.now_ms,
        )?;
        if t.index(&ix.name).is_some() {
            return Err(bad_param(&format!("Duplicate index name: {}", ix.name)));
        }
        ix.status_until_ms = cx.now_ms + self.windows.update_ms;
        t.next_index += 1;
        self.backfill_index(t, &ix)?;
        t.gsis.push(ix);
        Ok(())
    }

    fn delete_index(&mut self, t: &mut Table, name: &str) -> Result<(), ApiError> {
        let Some(at) = t.gsis.iter().position(|i| i.name == name) else {
            return Err(ApiError::modeled(
                "ResourceNotFoundException",
                format!("Requested resource not found: Index: {name} not found"),
            ));
        };
        let ix = t.gsis.remove(at);
        self.drop_index(t, &ix);
        Ok(())
    }
}

fn update_index_throughput(t: &mut Table, up: &Json) -> Result<(), ApiError> {
    let name = str_of(up, "IndexName").unwrap_or_default();
    let ppr = t.billing.pay_per_request;
    let Some(ix) = t.gsis.iter_mut().find(|i| i.name == name) else {
        return Err(ApiError::modeled(
            "ResourceNotFoundException",
            format!("Requested resource not found: Index: {name} not found"),
        ));
    };
    if let Some(units) = throughput(up.get("ProvisionedThroughput")) {
        if ppr {
            return Err(bad_param(
                "Cannot update ProvisionedThroughput of an index on a table with BillingMode PAY_PER_REQUEST",
            ));
        }
        ix.provisioned = Some(units);
    }
    Ok(())
}

/// `StreamSpecification`: enable or disable the table's stream. Returns whether anything changed.
fn update_stream(t: &mut Table, body: &Json, cx: &Cx<'_>) -> Result<bool, ApiError> {
    let Some(spec) = body.get("StreamSpecification") else {
        return Ok(false);
    };
    let enable = bool_of(spec, "StreamEnabled").unwrap_or(false);
    let current = t.stream.as_ref().filter(|s| s.enabled);
    match (enable, current) {
        (true, Some(s)) => Err(validation(format!(
            "Table already has an enabled stream: {}",
            stream_arn(cx.region(), cx.account(), &t.name, &s.label)
        ))),
        (false, None) => Err(validation("Table does not have an enabled stream")),
        (true, None) => {
            t.stream = new_stream(spec, cx.now_ms)?;
            Ok(true)
        }
        (false, Some(_)) => {
            // WHY: the records of a stream that was switched off are not kept for a day, as AWS keeps them; there is no reader for them.
            t.stream = None;
            Ok(true)
        }
    }
}

/// The settings that change nothing in how the table behaves.
fn update_settings(t: &mut Table, body: &Json) {
    if let Some(p) = bool_of(body, "DeletionProtectionEnabled") {
        t.protect = p;
    }
    if let Some(c) = str_of(body, "TableClass") {
        t.infrequent = c == "STANDARD_INFREQUENT_ACCESS";
    }
    if let Some(s) = body.get("SSESpecification") {
        t.sse = bool_of(s, "Enabled").unwrap_or(false).then(|| {
            (
                str_of(s, "SSEType").unwrap_or("KMS").to_owned(),
                str_of(s, "KMSMasterKeyId").map(str::to_owned),
            )
        });
    }
    if let Some(o) = on_demand_of(body, "OnDemandThroughput") {
        t.on_demand = Some(o);
    }
    if let Some(w) = warm_of(body) {
        t.warm = Some(w);
    }
}
