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

use std::io::Write as _;
use std::sync::Arc;

use aws_expr::{Item, Number, Value, item_to_json};
use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::service::Caller;
use md5::Digest as _;
use serde_json::{Value as Json, json};

use crate::backup::table_missing;
use crate::core::{Core, Cx};
use crate::describe::seconds;
use crate::extras::{Extra, Task, export_key, task_id};
use crate::meta::Table;
use crate::req::{int_of, set, str_of, time_ms_of, validation};
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
    ApiError::modeled(
        "ExportNotFoundException",
        format!("Export not found: {arn}"),
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
        let prefix = str_of(body, "S3Prefix").unwrap_or_default().trim_end_matches('/');
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
    fn repeated(&self, body: &Json, fingerprint: &str, now_ms: i64) -> Result<Option<Json>, ApiError> {
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
                    format!("An export with client token {token} was made with different parameters"),
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
                json!({
                    "ExportArn": s.get("ExportArn"),
                    "ExportStatus": s.get("ExportStatus"),
                    "ExportType": s.get("ExportType"),
                })
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

/// What an export reads.
#[derive(Clone, Copy, Debug)]
enum Window {
    Full(i64),
    Incremental { from: i64, to: i64, new_only: bool },
}

/// The time (or window) an export asks for, held to what recovery can serve.
fn window(t: &Table, body: &Json, now_ms: i64) -> Result<Window, ApiError> {
    let earliest = crate::pitr::earliest_ms(t, now_ms);
    let outside = |what: &str| {
        ApiError::modeled(
            "InvalidExportTimeException",
            format!("{what} is outside of the point in time recovery window of the table"),
        )
    };
    let spec = body.get("IncrementalExportSpecification");
    match (str_of(body, "ExportType").unwrap_or("FULL_EXPORT"), spec) {
        ("INCREMENTAL_EXPORT", Some(spec)) => {
            let from = time_ms_of(spec, "ExportFromTime")
                .ok_or_else(|| validation("ExportFromTime must be specified for an incremental export"))?;
            let to = time_ms_of(spec, "ExportToTime").unwrap_or(now_ms);
            if from < earliest {
                return Err(outside("ExportFromTime"));
            }
            if to > now_ms {
                return Err(outside("ExportToTime"));
            }
            let span = to - from;
            if !(INCREMENTAL_MIN_MS..=INCREMENTAL_MAX_MS).contains(&span) {
                return Err(validation(
                    "The incremental export period must be between 15 minutes and 24 hours",
                ));
            }
            let new_only = str_of(spec, "ExportViewType") == Some("NEW_IMAGE");
            Ok(Window::Incremental { from, to, new_only })
        }
        ("INCREMENTAL_EXPORT", None) => Err(validation(
            "IncrementalExportSpecification must be specified for an incremental export",
        )),
        (_, Some(_)) => Err(validation(
            "IncrementalExportSpecification can only be specified for an incremental export",
        )),
        (_, None) => {
            let at = time_ms_of(body, "ExportTime").unwrap_or(now_ms);
            if at < earliest || at > now_ms {
                return Err(outside("ExportTime"));
            }
            Ok(Window::Full(at))
        }
    }
}

/// The request with its client token left out, as a repeat is compared.
fn fingerprint(body: &Json) -> String {
    let mut b = body.clone();
    if let Json::Object(m) = &mut b {
        m.remove("ClientToken");
    }
    b.to_string()
}

fn shown(cx: &Cx<'_>, t: &Table, arn: &str, window: &Window, items: u64, billed: u64) -> Json {
    let body = &cx.body;
    let mut out = json!({
        "ExportArn": arn,
        "ExportStatus": "IN_PROGRESS",
        "StartTime": seconds(cx.now_ms),
        "TableArn": crate::req::table_arn(cx.region(), cx.account(), &t.name),
        "TableId": t.id_text(),
        "S3Bucket": str_of(body, "S3Bucket"),
        "S3SseAlgorithm": str_of(body, "S3SseAlgorithm").unwrap_or("AES256"),
        "ExportFormat": str_of(body, "ExportFormat").unwrap_or("DYNAMODB_JSON"),
        "ItemCount": items,
        "BilledSizeBytes": billed,
    });
    for member in ["ClientToken", "S3BucketOwner", "S3Prefix", "S3SseKmsKeyId"] {
        if let Some(v) = str_of(body, member) {
            set(&mut out, member, json!(v));
        }
    }
    match window {
        Window::Full(at) => {
            set(&mut out, "ExportTime", json!(seconds(*at)));
            set(&mut out, "ExportType", json!("FULL_EXPORT"));
        }
        Window::Incremental { from, to, new_only } => {
            set(&mut out, "ExportType", json!("INCREMENTAL_EXPORT"));
            set(
                &mut out,
                "IncrementalExportSpecification",
                json!({
                    "ExportFromTime": seconds(*from),
                    "ExportToTime": seconds(*to),
                    "ExportViewType": if *new_only { "NEW_IMAGE" } else { "NEW_AND_OLD_IMAGES" },
                }),
            );
        }
    }
    out
}

/// `manifest-summary.json` as far as it is known before the files are written.
fn summary(shown: &Json, now_ms: i64) -> Json {
    let text = |k: &str| shown.get(k).cloned().unwrap_or(Json::Null);
    let iso = |ms: i64| aws_core::time::format_iso8601(ms.div_euclid(1000));
    let mut out = json!({
        "version": "2020-06-30",
        "exportArn": text("ExportArn"),
        "startTime": iso(now_ms),
        "tableArn": text("TableArn"),
        "tableId": text("TableId"),
        "s3Bucket": text("S3Bucket"),
        "s3Prefix": text("S3Prefix"),
        "s3SseAlgorithm": text("S3SseAlgorithm"),
        "s3SseKmsKeyId": text("S3SseKmsKeyId"),
        "billedSizeBytes": text("BilledSizeBytes"),
        "itemCount": text("ItemCount"),
        "outputFormat": text("ExportFormat"),
    });
    #[allow(clippy::cast_possible_truncation)]
    let ms_of = |v: Option<&Json>| v.and_then(Json::as_f64).map(|s| (s * 1000.0) as i64);
    if let Some(spec) = shown.get("IncrementalExportSpecification") {
        set(&mut out, "version", json!("2023-08-01"));
        set(&mut out, "exportType", json!("INCREMENTAL_EXPORT"));
        set(&mut out, "outputView", spec.get("ExportViewType").cloned().unwrap_or_default());
        if let (Some(f), Some(t)) = (ms_of(spec.get("ExportFromTime")), ms_of(spec.get("ExportToTime"))) {
            set(&mut out, "exportFromTime", json!(iso(f)));
            set(&mut out, "exportToTime", json!(iso(t)));
        }
    } else if let Some(at) = ms_of(shown.get("ExportTime")) {
        set(&mut out, "exportTime", json!(iso(at)));
    }
    out
}

fn gzip(lines: &[String]) -> Result<Vec<u8>, ApiError> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    for l in lines {
        enc.write_all(l.as_bytes())
            .and_then(|()| enc.write_all(b"\n"))
            .map_err(|e| ApiError::internal(e.to_string()))?;
    }
    enc.finish().map_err(|e| ApiError::internal(e.to_string()))
}

fn md5_base64(bytes: &[u8]) -> String {
    aws_core::base64::encode(&md5::Md5::digest(bytes))
}

/// Writes an export's files, the data first and the manifests last, as AWS's appear.
pub(crate) async fn write(peers: &Arc<Peers>, job: &Job) -> Result<Written, Failure> {
    let dir = format!("{}AWSDynamoDB/{}", job.base, job.id);
    put(peers, job, &format!("{dir}/_started"), Vec::new()).await?;
    let name = aws_seal::alphanumeric(26)
        .unwrap_or_else(|| job.id.clone())
        .to_ascii_lowercase();
    let ext = if job.ion { "ion.gz" } else { "json.gz" };
    let data_key = if job.incremental {
        format!("{}AWSDynamoDB/data/{name}.{ext}", job.base)
    } else {
        format!("{dir}/data/{name}.{ext}")
    };
    let md5 = md5_base64(&job.data);
    let etag = put(peers, job, &data_key, job.data.clone()).await?;
    let files = json!({
        "itemCount": job.items,
        "md5Checksum": md5,
        "etag": etag,
        "dataFileS3Key": data_key,
    })
    .to_string()
        + "\n";
    let files_key = format!("{dir}/manifest-files.json");
    put(peers, job, &files_key, files.clone().into_bytes()).await?;
    put(peers, job, &format!("{dir}/manifest-files.md5"), md5_base64(files.as_bytes()).into_bytes()).await?;
    let mut summary = job.summary.clone();
    let iso = aws_core::time::format_iso8601(job.caller_time());
    set(&mut summary, "endTime", json!(iso));
    set(&mut summary, "manifestFilesS3Key", json!(files_key));
    let summary = summary.to_string();
    let summary_key = format!("{dir}/manifest-summary.json");
    put(peers, job, &summary_key, summary.clone().into_bytes()).await?;
    put(peers, job, &format!("{dir}/manifest-summary.md5"), md5_base64(summary.as_bytes()).into_bytes()).await?;
    Ok(Written {
        manifest: summary_key,
    })
}

impl Job {
    /// The time the files are written, in seconds.
    fn caller_time(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
    }
}

/// Puts one object; answers its ETag.
async fn put(peers: &Arc<Peers>, job: &Job, key: &str, body: Vec<u8>) -> Result<String, Failure> {
    let reply = peers
        .call_rest_with_body(
            "dynamodb",
            &job.caller,
            ("s3", "PutObject"),
            &json!({"Bucket": job.bucket, "Key": key}),
            bytes::Bytes::from(body),
        )
        .await
        .map_err(|e| s3_failure(&job.bucket, &e))?;
    Ok(reply
        .json
        .get("ETag")
        .and_then(Json::as_str)
        .unwrap_or_default()
        .trim_matches('"')
        .to_owned())
}

/// An S3 refusal as an export's or import's failure.
pub(crate) fn s3_failure(bucket: &str, e: &ApiError) -> Failure {
    match e.code.as_ref() {
        "NoSuchBucket" => Failure::new(
            "S3NoSuchBucket",
            format!("The specified bucket does not exist (Service: Amazon S3; Status Code: 404; Error Code: NoSuchBucket; Bucket: {bucket})"),
        ),
        "AccessDenied" => Failure::new("S3AccessDenied", e.message.clone()),
        code => Failure::new("UNKNOWN", format!("{code}: {}", e.message)),
    }
}
