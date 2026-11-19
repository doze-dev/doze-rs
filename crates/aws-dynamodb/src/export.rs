//! `ExportTableToPointInTime`, `DescribeExport`, `ListExports`: a table as it was at a time (full) or its changes over a window
//! (incremental), read from the recovery log (`pitr.rs`), written to the local S3 in the layout of the developer guide's "DynamoDB
//! table export output format": under `<prefix>/AWSDynamoDB/<export id>/` an empty `_started`, `manifest-summary.json`,
//! `manifest-files.json` (JSON lines: `itemCount`, `md5Checksum`, `etag`, `dataFileS3Key`), each manifest's `.md5`, and the data
//! under `data/` (an incremental export's data under `<prefix>/AWSDynamoDB/data/`), gzip-compressed JSON lines (`{"Item": ...}`, or
//! `{"Metadata", "Keys", "NewImage", "OldImage"}` for an incremental one) or Ion text (`$ion_1_0 {Item:{...}}`).
//!
//! AWS writes one data file per partition; a table here has one partition, so an export has one data file. The table is read in the
//! actor when the call is made; the files are written outside it (`work.rs`) before the call answers, so the answer says
//! `IN_PROGRESS` (as AWS's does) and a describe right after says `COMPLETED`, or `FAILED` with the reason (no such bucket). What the
//! `.md5` files hold (the base64 MD5 of their manifest), the keys being whole S3 keys (the prefix included), and the messages are
//! `(memory)`.

use aws_expr::{Item, Number, Value, item_to_json};
use aws_wire::ApiError;
use aws_wire::service::Caller;
use serde_json::{Value as Json, json};

use crate::backup::table_missing;
use crate::core::{Core, Cx};
use crate::describe::seconds;
use crate::extras::{Extra, Task, export_key, task_id};
use crate::meta::Table;
use crate::req::{int_of, set, str_of, validation};
use crate::work::{Failure, Work};

/// A client token holds for 8 hours (the model).
const TOKEN_MS: i64 = 8 * 60 * 60 * 1000;
/// An incremental export's window: at least 15 minutes, at most 24 hours `(memory)`.
const INCREMENTAL_MIN_MS: i64 = 15 * 60 * 1000;
const INCREMENTAL_MAX_MS: i64 = 24 * 60 * 60 * 1000;
const LIST_MAX: usize = 25;

/// What is written outside the actor.
#[derive(Debug)]
pub(crate) struct Job {
    pub(crate) arn: String,
    pub(crate) caller: Caller,
    pub(crate) bucket: String,
    /// `<prefix>/` or nothing.
    pub(crate) base: String,
    pub(crate) id: String,
    pub(crate) incremental: bool,
    pub(crate) ion: bool,
    /// The data, gzip-compressed, and how many items it holds.
    pub(crate) data: Vec<u8>,
    pub(crate) items: u64,
    /// The summary manifest without `endTime` and `manifestFilesS3Key`, which the writing fills in.
    pub(crate) summary: Json,
}

/// What the writing did.
#[derive(Debug)]
pub(crate) struct Written {
    pub(crate) manifest: String,
}

fn not_found(arn: &str) -> ApiError {
    // Recorded 2026-11-19: AWS says "Export not found" twice.
    ApiError::modeled(
        "ExportNotFoundException",
        format!("Export not found: Export not found: {arn}"),
    )
}

impl Core {
    pub(crate) fn export_table(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let body = &cx.body;
        if body.get("FilterSpecification").is_some() {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                "FilterSpecification of ExportTableToPointInTime is not built in doze-aws; export the whole table.",
            ));
        }
        prefix_long_enough(body)?;
        let name = cx.table_name("TableArn")?;
        let t = self
            .tables
            .get(&name)
            .cloned()
            .ok_or_else(|| table_missing(&name))?;
        let fingerprint = fingerprint(body);
        if let Some(done) = self.repeated(body, &fingerprint, cx.now_ms)? {
            return Ok(json!({ "ExportDescription": done }));
        }
        if !t.pitr.enabled {
            return Err(ApiError::modeled(
                "PointInTimeRecoveryUnavailableException",
                format!("Point in time recovery is not enabled for table '{name}'"),
            ));
        }
        let window = window(&t, body, cx.now_ms)?;
        let ion = str_of(body, "ExportFormat") == Some("ION");
        let (lines, items) = match window {
            Window::Full(at) => self.full_lines(&t, at, ion)?,
            Window::Incremental { from, to, new_only } => {
                self.incremental_lines(&t, from, to, new_only, ion)?
            }
        };
        let billed = lines.len() as u64;
        let id = task_id(cx.now_ms).ok_or_else(|| ApiError::internal("no random bytes"))?;
        let table_arn = crate::req::table_arn(cx.region(), cx.account(), &name);
        let arn = format!("{table_arn}/export/{id}");
        let shown = shown(cx, &t, &arn, &window, items, billed);
        let summary = summary(&shown, cx.now_ms);
        let data = gzip(&lines)?;
        self.extras.insert(
            export_key(&arn),
            Extra::Task(Task {
                arn: arn.clone(),
                table_arn,
                started_ms: cx.now_ms,
                token: str_of(body, "ClientToken").map(|t| (t.to_owned(), fingerprint)),
                shown: shown.to_string(),
            }),
        );
        let prefix = str_of(body, "S3Prefix")
            .unwrap_or_default()
            .trim_end_matches('/');
        self.work.push(Work::Export(Box::new(Job {
            arn,
            caller: cx.call.caller.clone(),
            bucket: str_of(body, "S3Bucket").unwrap_or_default().to_owned(),
            base: if prefix.is_empty() {
                String::new()
            } else {
                format!("{prefix}/")
            },
            id,
            incremental: matches!(window, Window::Incremental { .. }),
            ion,
            data,
            items,
            summary,
        })));
        Ok(json!({ "ExportDescription": shown }))
    }

    /// The export a repeated client token names, or the refusal when the request differs.
    fn repeated(
        &self,
        body: &Json,
        fingerprint: &str,
        now_ms: i64,
    ) -> Result<Option<Json>, ApiError> {
        let Some(token) = str_of(body, "ClientToken") else {
            return Ok(None);
        };
        for (_, e) in self.extras.with_prefix("export/") {
            let Extra::Task(task) = e else { continue };
            let Some((t, f)) = &task.token else { continue };
            if t != token || task.started_ms + TOKEN_MS < now_ms {
                continue;
            }
            if f != fingerprint {
                return Err(ApiError::modeled(
                    "ExportConflictException",
                    format!(
                        "An export with client token {token} was made with different parameters"
                    ),
                ));
            }
            return Ok(serde_json::from_str(&task.shown).ok());
        }
        Ok(None)
    }

    fn full_lines(&self, t: &Table, at: i64, ion: bool) -> Result<(Vec<String>, u64), ApiError> {
        let items = self.items_at(t, at)?;
        let n = items.len() as u64;
        let lines = items
            .iter()
            .map(|i| {
                if ion {
                    crate::ion::line("Item", &Value::M(i.clone()))
                } else {
                    json!({"Item": item_to_json(i)}).to_string()
                }
            })
            .collect();
        Ok((lines, n))
    }

    /// One line for each item that changed in the window: its keys, its image before (unless only new images are asked for) and
    /// after, and the time of its last change. An item made and deleted inside the window has no line.
    fn incremental_lines(
        &self,
        t: &Table,
        from: i64,
        to: i64,
        new_only: bool,
        ion: bool,
    ) -> Result<(Vec<String>, u64), ApiError> {
        let mut first: Vec<(Vec<u8>, Option<Item>, i64)> = Vec::new();
        let mut at: std::collections::HashMap<Vec<u8>, usize> = std::collections::HashMap::new();
        for c in self.changes(t, from, Some(to))? {
            match at.get(&c.pk) {
                Some(&i) => {
                    if let Some(e) = first.get_mut(i) {
                        e.2 = c.time_ms;
                    }
                }
                None => {
                    at.insert(c.pk.clone(), first.len());
                    first.push((c.pk, c.old, c.time_ms));
                }
            }
        }
        let after = self.before(t, to)?;
        let mut lines = Vec::new();
        for (pk, old, last) in first {
            let new = match after.get(&pk) {
                Some(then) => then.clone(),
                None => self.read_item(&crate::stage::item_key(t, &pk))?,
            };
            let Some(basis) = new.as_ref().or(old.as_ref()) else {
                continue;
            };
            let keys = crate::stage::key_of(t, basis);
            let micros = Value::N(Number::from_i64(last.saturating_mul(1000)));
            let mut record = Item::new();
            record.insert("Keys".into(), Value::M(keys));
            record.insert(
                "Metadata".into(),
                Value::M([("WriteTimestampMicros".to_owned(), micros)].into()),
            );
            if let Some(n) = new {
                record.insert("NewImage".into(), Value::M(n));
            }
            if let Some(o) = old.filter(|_| !new_only) {
                record.insert("OldImage".into(), Value::M(o));
            }
            lines.push(if ion {
                crate::ion::line("Record", &Value::M(record))
            } else {
                item_to_json(&record).to_string()
            });
        }
        let n = lines.len() as u64;
        Ok((lines, n))
    }

    /// The writing finished (or failed).
    pub(crate) fn exported(&mut self, arn: &str, result: Result<Written, Failure>, now_ms: i64) {
        let key = export_key(arn);
        let Some(Extra::Task(task)) = self.extras.get(&key).cloned() else {
            return;
        };
        let mut shown: Json = serde_json::from_str(&task.shown).unwrap_or_else(|_| json!({}));
        set(&mut shown, "EndTime", json!(seconds(now_ms)));
        match result {
            Ok(w) => {
                set(&mut shown, "ExportStatus", json!("COMPLETED"));
                set(&mut shown, "ExportManifest", json!(w.manifest));
            }
            Err(f) => {
                set(&mut shown, "ExportStatus", json!("FAILED"));
                set(&mut shown, "FailureCode", json!(f.code));
                set(&mut shown, "FailureMessage", json!(f.message));
                for gone in ["ItemCount", "BilledSizeBytes"] {
                    if let Json::Object(m) = &mut shown {
                        m.remove(gone);
                    }
                }
            }
        }
        self.extras.insert(
            key,
            Extra::Task(Task {
                shown: shown.to_string(),
                ..task
            }),
        );
    }

    pub(crate) fn describe_export(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let arn = str_of(&cx.body, "ExportArn").unwrap_or_default();
        match self.extras.get(&export_key(arn)) {
            Some(Extra::Task(t)) => Ok(json!({
                "ExportDescription": serde_json::from_str::<Json>(&t.shown).unwrap_or_default()
            })),
            _ => Err(not_found(arn)),
        }
    }

    pub(crate) fn list_exports(&mut self, cx: &Cx<'_>) -> Result<Json, ApiError> {
        let (page, next) = self.tasks_page(cx, "export/", "MaxResults")?;
        let summaries: Vec<Json> = page
            .iter()
            .map(|s| {
                let mut out = json!({
                    "ExportArn": s.get("ExportArn"),
                    "ExportStatus": s.get("ExportStatus"),
                });
                if let Some(kind) = s.get("ExportType") {
                    set(&mut out, "ExportType", kind.clone());
                }
                out
            })
            .collect();
        let mut out = json!({ "ExportSummaries": summaries });
        if let Some(n) = next {
            set(&mut out, "NextToken", json!(n));
        }
        Ok(out)
    }

    /// A page of export or import descriptions, oldest first, of one table when the request names one; and the token for the next.
    pub(crate) fn tasks_page(
        &self,
        cx: &Cx<'_>,
        prefix: &str,
        size_member: &str,
    ) -> Result<(Vec<Json>, Option<String>), ApiError> {
        let table = str_of(&cx.body, "TableArn");
        let size = int_of(&cx.body, size_member)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(LIST_MAX);
        let mut all: Vec<&Task> = self
            .extras
            .with_prefix(prefix)
            .filter_map(|(_, e)| match e {
                Extra::Task(t) => Some(t),
                _ => None,
            })
            .filter(|t| table.is_none_or(|a| t.table_arn == a))
            .collect();
        all.sort_by(|a, b| (a.started_ms, &a.arn).cmp(&(b.started_ms, &b.arn)));
        if let Some(token) = str_of(&cx.body, "NextToken") {
            let Some(at) = all.iter().position(|t| t.arn == token) else {
                return Err(validation(format!("Invalid NextToken: {token}")));
            };
            all.drain(..at);
        }
        let next = all.get(size).map(|t| t.arn.clone());
        all.truncate(size);
        let page = all
            .iter()
            .filter_map(|t| serde_json::from_str(&t.shown).ok())
            .collect();
        Ok((page, next))
    }
}

mod shape;
mod write;

use shape::{Window, fingerprint, shown, summary, window};
pub(crate) use write::{gzip, s3_failure, write};

/// Recorded 2026-11-19: an export's `S3Prefix` is at least 3 characters (the model says nothing of it).
fn prefix_long_enough(body: &Json) -> Result<(), ApiError> {
    match str_of(body, "S3Prefix").filter(|p| p.chars().count() < 3) {
        Some(p) => Err(crate::req::validation(format!(
            "Invalid Request: 1 validation error detected: Value '{p}' at 's3Prefix' failed to satisfy constraint: Member must have length greater than or equal to 3"
        ))),
        None => Ok(()),
    }
}
