//! A table as the API describes it. The three answers that carry a description differ in what they hold, as AWS's do (recorded):
//! `CreateTable` has no warm throughput, no billing-switch time and no `Backfilling`, `DescribeTable` has them, and `DeleteTable` has
//! neither the schema nor the creation time.

use serde_json::{Value as Json, json};

use crate::meta::{Index, Key, Projection, Status, Table};
use crate::req::{set, table_arn};
use crate::stream::stream_arn;

/// Which answer the description is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Create,
    Describe,
    Delete,
}

/// How much a table and its indexes hold, which AWS refreshes every six hours and this service counts when asked.
#[derive(Debug, Default)]
pub(crate) struct Sizes {
    pub(crate) table: (i64, i64),
    pub(crate) indexes: Vec<(u32, (i64, i64))>,
}

impl Sizes {
    fn of(&self, index: u32) -> (i64, i64) {
        self.indexes
            .iter()
            .find(|(i, _)| *i == index)
            .map_or((0, 0), |(_, s)| *s)
    }
}

/// What a description is made from.
struct View<'a> {
    t: &'a Table,
    region: &'a str,
    account: &'a str,
    shape: Shape,
    sizes: &'a Sizes,
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn seconds(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

fn key_schema(hash: &Key, range: Option<&Key>) -> Json {
    let mut out = vec![json!({"AttributeName": hash.name, "KeyType": "HASH"})];
    if let Some(r) = range {
        out.push(json!({"AttributeName": r.name, "KeyType": "RANGE"}));
    }
    Json::Array(out)
}

fn projection(p: &Projection) -> Json {
    match p {
        Projection::All => json!({"ProjectionType": "ALL"}),
        Projection::KeysOnly => json!({"ProjectionType": "KEYS_ONLY"}),
        Projection::Include(n) => json!({"ProjectionType": "INCLUDE", "NonKeyAttributes": n}),
    }
}

fn throughput(units: Option<(i64, i64)>) -> Json {
    let (r, w) = units.unwrap_or((0, 0));
    json!({"NumberOfDecreasesToday": 0, "ReadCapacityUnits": r, "WriteCapacityUnits": w})
}

/// Warm throughput: what is reported when none was asked for. `(memory)` for the provisioned figures; the on-demand ones are recorded.
fn warm(t: &Table) -> Json {
    let (r, w) = t.warm.unwrap_or(if t.billing.pay_per_request {
        (12_000, 4_000)
    } else {
        (3_000, 1_000)
    });
    json!({"ReadUnitsPerSecond": r, "WriteUnitsPerSecond": w, "Status": "ACTIVE"})
}

impl View<'_> {
    fn index(&self, ix: &Index, local: bool) -> Json {
        let (count, bytes) = self.sizes.of(ix.id);
        let arn = format!(
            "{}/index/{}",
            table_arn(self.region, self.account, &self.t.name),
            ix.name
        );
        let mut out = json!({
            "IndexName": ix.name,
            "KeySchema": key_schema(&ix.hash, ix.range.as_ref()),
            "Projection": projection(&ix.projection),
            "IndexSizeBytes": bytes,
            "ItemCount": count,
            "IndexArn": arn,
        });
        if !local {
            set(&mut out, "IndexStatus", json!(ix.status.name()));
            if ix.status == Status::Creating && self.shape != Shape::Create {
                set(&mut out, "Backfilling", json!(true));
            }
            set(
                &mut out,
                "ProvisionedThroughput",
                throughput(ix.provisioned),
            );
            if self.shape == Shape::Describe {
                set(&mut out, "WarmThroughput", warm(self.t));
            }
        }
        out
    }

    /// The schema, the indexes, the stream and the settings that follow the table's name.
    fn schema(&self, out: &mut Json) {
        let t = self.t;
        let defs: Vec<Json> = t
            .attribute_definitions()
            .iter()
            .map(|(n, ty)| json!({"AttributeName": n, "AttributeType": ty.to_string()}))
            .collect();
        set(out, "AttributeDefinitions", Json::Array(defs));
        set(out, "KeySchema", key_schema(&t.hash, t.range.as_ref()));
        set(out, "CreationDateTime", json!(seconds(t.created_ms)));
        if !t.lsis.is_empty() {
            let lsis = t.lsis.iter().map(|i| self.index(i, true)).collect();
            set(out, "LocalSecondaryIndexes", lsis);
        }
        if !t.gsis.is_empty() {
            let gsis = t.gsis.iter().map(|i| self.index(i, false)).collect();
            set(out, "GlobalSecondaryIndexes", gsis);
        }
        if let Some(s) = t.stream.as_ref().filter(|s| s.enabled) {
            let arn = stream_arn(self.region, self.account, &t.name, &s.label);
            set(
                out,
                "StreamSpecification",
                json!({"StreamEnabled": true, "StreamViewType": s.view}),
            );
            set(out, "LatestStreamLabel", json!(s.label));
            set(out, "LatestStreamArn", json!(arn));
        }
        if self.shape == Shape::Describe {
            set(out, "WarmThroughput", warm(t));
        }
        settings(out, t);
    }
}

/// The settings that do not change how the table behaves.
fn settings(out: &mut Json, t: &Table) {
    if let Some((r, w)) = t.on_demand {
        let throughput = json!({"MaxReadRequestUnits": r, "MaxWriteRequestUnits": w});
        set(out, "OnDemandThroughput", throughput);
    }
    if t.infrequent {
        let class = json!({"TableClass": "STANDARD_INFREQUENT_ACCESS", "LastUpdateDateTime": seconds(t.created_ms)});
        set(out, "TableClassSummary", class);
    }
    if let Some((kind, key)) = &t.sse {
        let mut sse = json!({"Status": "ENABLED", "SSEType": kind});
        if let Some(k) = key {
            set(&mut sse, "KMSMasterKeyArn", json!(k));
        }
        set(out, "SSEDescription", sse);
    }
}

/// The `TableDescription` for `t`.
pub(crate) fn description(
    t: &Table,
    region: &str,
    account: &str,
    shape: Shape,
    sizes: &Sizes,
) -> Json {
    let view = View {
        t,
        region,
        account,
        shape,
        sizes,
    };
    let units =
        (!t.billing.pay_per_request).then_some((t.billing.read_units, t.billing.write_units));
    let mut out = json!({
        "TableName": t.name,
        "TableStatus": t.status.name(),
        "ProvisionedThroughput": throughput(units),
        "TableSizeBytes": sizes.table.1,
        "ItemCount": sizes.table.0,
        "TableArn": table_arn(region, account, &t.name),
        "TableId": t.id_text(),
        "DeletionProtectionEnabled": t.protect,
    });
    if t.billing.pay_per_request {
        let mut summary = json!({"BillingMode": "PAY_PER_REQUEST"});
        if shape != Shape::Create {
            let since = json!(seconds(t.billing.pay_per_request_since_ms));
            set(&mut summary, "LastUpdateToPayPerRequestDateTime", since);
        }
        set(&mut out, "BillingModeSummary", summary);
    }
    if shape != Shape::Delete {
        view.schema(&mut out);
    }
    out
}
