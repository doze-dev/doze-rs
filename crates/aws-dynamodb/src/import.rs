//! `ImportTable`, `DescribeImport`, `ListImports`: a new table filled from every object under a prefix of a bucket in the local S3,
//! in the developer guide's three formats ("Amazon S3 import formats for DynamoDB"): DynamoDB JSON lines (`{"Item": ...}`, one item
//! a line), Ion (`$ion_1_0 {Item:{...}}` lines, or one list of such structs), or CSV (the first line the header unless `HeaderList`
//! gives it, RFC 4180 quoting, the key attributes of the table and its indexes typed by their definitions and every other column a
//! string, an empty column left out). Objects may be gzip-compressed (`GZIP`) or not (`NONE`).
//!
//! The table is made `CREATING` when the call is made; the objects are read outside the actor (`work.rs`), and the items written in
//! one go when they are in, so a describe right after the call says `COMPLETED` (the answer itself says `IN_PROGRESS`, as AWS's does)
//! and the table is `ACTIVE`. An item that cannot be read or does not fit the key schema is counted in `ErrorCount` and left out, as
//! AWS does; AWS also writes each to a CloudWatch Logs group, which is not done here. When nothing can be read at all (no such
//! bucket, no objects) the import is `FAILED` and the table is removed. `ZSTD` is refused when asked for: doze-aws links no zstd
//! decoder. Every message is `(memory)`.

use aws_expr::Item;
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value as Json, json};

use crate::core::{Core, Cx};
use crate::describe::seconds;
use crate::extras::{Extra, Task, import_key, task_id};
use crate::meta::{Status, Table};
use crate::req::{array_of, set, str_of};
use crate::work::{Failure, Work};

/// The most one object may inflate to: a guard against a compressed bomb on a laptop.
const MAX_OBJECT_BYTES: u64 = 1 << 30;

/// What is read outside the actor.
#[derive(Debug)]
pub(crate) struct Job {
    pub(crate) arn: String,
    pub(crate) caller: Caller,
    pub(crate) bucket: String,
    pub(crate) prefix: String,
    pub(crate) format: Format,
    pub(crate) gzip: bool,
    /// The attributes a CSV column is typed by: the key attributes of the table and its indexes.
    pub(crate) keys: Vec<(String, char)>,
}

#[derive(Clone, Debug)]
pub(crate) enum Format {
    Json,
    Ion,
    Csv {
        delimiter: char,
        header: Option<Vec<String>>,
    },
}

/// What the reading found.
#[derive(Debug, Default)]
pub(crate) struct Read {
    pub(crate) items: Vec<Item>,
    pub(crate) errors: u64,
    pub(crate) bytes: u64,
}

fn not_found(arn: &str) -> ApiError {
    // Recorded 2026-11-19.
    ApiError::modeled(
        "ImportNotFoundException",
        format!("Import not found: Import with arn: {arn} doesn't exist"),
    )
}

impl Core {
    pub(crate) fn import_table(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let body = &cx.body;
        if let Some(done) = self.repeated_import(body, cx.now_ms)? {
            return Ok(json!({ "ImportTableDescription": done }));
        }
        let params = body
            .get("TableCreationParameters")
            .cloned()
            .unwrap_or_default();
        let name = str_of(&params, "TableName").unwrap_or_default().to_owned();
        crate::schema::check_table_name(&name)?;
        if self.tables.get(&name).is_some() {
            return Err(ApiError::modeled(
                "ResourceInUseException",
                format!("Table already exists: {name}"),
            ));
        }
        let gzip = match str_of(body, "InputCompressionType").unwrap_or("NONE") {
            "GZIP" => true,
            "ZSTD" => {
                return Err(ApiError::new(
                    501,
                    "NotImplemented",
                    "InputCompressionType ZSTD is not built in doze-aws; compress the objects with GZIP, or none.",
                ));
            }
            _ => false,
        };
        let format = format_of(body);
        let made = Cx {
            call: cx.call,
            body: params.clone(),
            now_ms: cx.now_ms,
        };
        let mut t = self.table_from(&made, &name)?;
        // The table is being made until the items are in.
        t.status = Status::Creating;
        t.status_until_ms = i64::MAX;
        let id = task_id(cx.now_ms).ok_or_else(|| ApiError::internal("no random bytes"))?;
        let table_arn = crate::req::table_arn(cx.region(), cx.account(), &name);
        let arn = format!("{table_arn}/import/{id}");
        let source = body.get("S3BucketSource").cloned().unwrap_or_default();
        let shown = shown(cx, &t, &arn, &params);
        let keys = t.attribute_definitions().into_iter().collect();
        self.tables.insert(name, t);
        self.extras.insert(
            import_key(&arn),
            Extra::Task(Task {
                arn: arn.clone(),
                table_arn,
                started_ms: cx.now_ms,
                token: str_of(body, "ClientToken").map(|t| (t.to_owned(), fingerprint(body))),
                shown: shown.to_string(),
            }),
        );
        self.work.push(Work::Import(Box::new(Job {
            arn,
            caller: cx.call.caller.clone(),
            bucket: str_of(&source, "S3Bucket").unwrap_or_default().to_owned(),
            prefix: str_of(&source, "S3KeyPrefix")
                .unwrap_or_default()
                .to_owned(),
            format,
            gzip,
            keys,
        })));
        Ok(json!({ "ImportTableDescription": shown }))
    }

    fn repeated_import(&self, body: &Json, now_ms: i64) -> Result<Option<Json>, ApiError> {
        let Some(token) = str_of(body, "ClientToken") else {
            return Ok(None);
        };
        let fp = fingerprint(body);
        for (_, e) in self.extras.with_prefix("import/") {
            let Extra::Task(task) = e else { continue };
            let Some((t, f)) = &task.token else { continue };
            if t != token || task.started_ms + 8 * 60 * 60 * 1000 < now_ms {
                continue;
            }
            if *f != fp {
                return Err(ApiError::modeled(
                    "ImportConflictException",
                    format!(
                        "An import with client token {token} was made with different parameters"
                    ),
                ));
            }
            return Ok(serde_json::from_str(&task.shown).ok());
        }
        Ok(None)
    }

    /// The reading finished: the items go in and the table is ready, or the import failed and the table goes.
    pub(crate) fn imported(
        &mut self,
        arn: &str,
        result: Result<Read, Failure>,
        now_ms: i64,
    ) -> Result<(), ApiError> {
        let key = import_key(arn);
        let Some(Extra::Task(task)) = self.extras.get(&key).cloned() else {
            return Ok(());
        };
        let name = task
            .table_arn
            .rsplit_once("table/")
            .map_or("", |(_, n)| n)
            .to_owned();
        let mut shown: Json = serde_json::from_str(&task.shown).unwrap_or_else(|_| json!({}));
        set(&mut shown, "EndTime", json!(seconds(now_ms)));
        match (result, self.tables.get(&name).cloned()) {
            (Ok(read), Some(t)) => {
                let (imported, bad) = self.fill(&t, &read.items, now_ms)?;
                if let Some(live) = self.tables.get_mut(&name) {
                    live.status = Status::Creating;
                    live.status_until_ms = now_ms + self.windows.create_ms;
                    for ix in &mut live.gsis {
                        ix.status_until_ms = now_ms + self.windows.create_ms;
                    }
                }
                set(&mut shown, "ImportStatus", json!("COMPLETED"));
                set(&mut shown, "ProcessedSizeBytes", json!(read.bytes));
                set(
                    &mut shown,
                    "ProcessedItemCount",
                    json!(read.items.len() as u64 + read.errors),
                );
                set(&mut shown, "ImportedItemCount", json!(imported));
                set(&mut shown, "ErrorCount", json!(read.errors + bad));
            }
            (Err(f), _) => {
                self.drop_table(&name);
                set(&mut shown, "ImportStatus", json!("FAILED"));
                set(&mut shown, "FailureCode", json!(f.code));
                set(&mut shown, "FailureMessage", json!(f.message));
            }
            (Ok(_), None) => {
                set(&mut shown, "ImportStatus", json!("CANCELLED"));
            }
        }
        self.extras.insert(
            key,
            Extra::Task(Task {
                shown: shown.to_string(),
                ..task
            }),
        );
        Ok(())
    }

    /// Files the items that fit the table; answers how many went in and how many did not.
    fn fill(&mut self, t: &Table, items: &[Item], now_ms: i64) -> Result<(u64, u64), ApiError> {
        let (mut ok, mut bad) = (0, 0);
        let mut seen = std::collections::HashMap::new();
        for item in items {
            match crate::stage::validate_item(t, item) {
                Ok(pk) => {
                    let old = seen.insert(pk, item.clone());
                    self.stage(t, now_ms, old.as_ref(), Some(item), false)?;
                    ok += 1;
                }
                Err(_) => bad += 1,
            }
        }
        Ok((ok, bad))
    }

    pub(crate) fn describe_import(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let arn = str_of(&cx.body, "ImportArn").unwrap_or_default();
        match self.extras.get(&import_key(arn)) {
            Some(Extra::Task(t)) => Ok(json!({
                "ImportTableDescription": serde_json::from_str::<Json>(&t.shown).unwrap_or_default()
            })),
            _ => Err(not_found(arn)),
        }
    }

    pub(crate) fn list_imports(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let (page, next) = self.tasks_page(cx, "import/", "PageSize")?;
        let summaries: Vec<Json> = page
            .iter()
            .map(|s| {
                let mut out = json!({});
                for k in [
                    "ImportArn",
                    "ImportStatus",
                    "TableArn",
                    "S3BucketSource",
                    "InputFormat",
                    "StartTime",
                    "EndTime",
                ] {
                    if let Some(v) = s.get(k) {
                        set(&mut out, k, v.clone());
                    }
                }
                out
            })
            .collect();
        let mut out = json!({ "ImportSummaryList": summaries });
        if let Some(n) = next {
            set(&mut out, "NextToken", json!(n));
        }
        Ok(out)
    }
}

fn fingerprint(body: &Json) -> String {
    let mut b = body.clone();
    if let Json::Object(m) = &mut b {
        m.remove("ClientToken");
    }
    b.to_string()
}

fn format_of(body: &Json) -> Format {
    match str_of(body, "InputFormat") {
        Some("ION") => Format::Ion,
        Some("CSV") => {
            let csv = body
                .get("InputFormatOptions")
                .and_then(|o| o.get("Csv"))
                .cloned()
                .unwrap_or_default();
            let header = csv.get("HeaderList").map(|_| {
                array_of(&csv, "HeaderList")
                    .iter()
                    .filter_map(Json::as_str)
                    .map(str::to_owned)
                    .collect()
            });
            Format::Csv {
                delimiter: str_of(&csv, "Delimiter")
                    .and_then(|d| d.chars().next())
                    .unwrap_or(','),
                header,
            }
        }
        _ => Format::Json,
    }
}

fn shown(cx: &Cx<'_>, t: &Table, arn: &str, params: &Json) -> Json {
    let body = &cx.body;
    let mut out = json!({
        "ImportArn": arn,
        "ImportStatus": "IN_PROGRESS",
        "TableArn": crate::req::table_arn(cx.region(), cx.account(), &t.name),
        "TableId": t.id_text(),
        "S3BucketSource": body.get("S3BucketSource"),
        "ErrorCount": 0,
        "InputFormat": str_of(body, "InputFormat"),
        "InputCompressionType": str_of(body, "InputCompressionType").unwrap_or("NONE"),
        "TableCreationParameters": params,
        "StartTime": seconds(cx.now_ms),
        "ProcessedSizeBytes": 0,
        "ProcessedItemCount": 0,
        "ImportedItemCount": 0,
    });
    for member in ["ClientToken", "InputFormatOptions"] {
        if let Some(v) = body.get(member) {
            set(&mut out, member, v.clone());
        }
    }
    out
}

mod source;

pub use source::check;
pub(crate) use source::read;
