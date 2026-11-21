//! Export tasks: a group's events in a time range written to a bucket of the local S3, through the peer bus. The task is made
//! `PENDING`, and before the call is answered it is run (`RUNNING`) and ends `COMPLETED` (or `FAILED`, with S3's reason), so a
//! `DescribeExportTasks` after it sees the end; AWS runs it in the background. A bucket S3 does not have refuses the call, as AWS
//! checks the bucket when the task is made.
//!
//! The objects (memory: AWS's layout): `<prefix>/aws-logs-write-test` with `Permission Check Successful`, then for each stream with
//! events in the range `<prefix>/<taskId>/<stream>/000000.gz`, gzip of one line per event: the time in ISO 8601 with milliseconds, a
//! space, the message. The prefix defaults to `exportedlogs` (the model).

use std::io::Write;

use aws_wire::ApiError;
use aws_wire::service::Call;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Doc, doc_key, stream_key};
use crate::req::{Put, Req, invalid, no_group};

pub(crate) const EXPORT: &str = "export-task";
/// Rows read from a stream at a time.
const CHUNK: usize = 1000;

/// An export made by the last call, run once it is answered.
#[derive(Debug)]
pub(crate) struct Pending {
    id: String,
    group: String,
    prefix: Option<String>,
    from: i64,
    to: i64,
    bucket: String,
    key_prefix: String,
}

/// (memory)
fn no_task() -> ApiError {
    ApiError::modeled(
        "ResourceNotFoundException",
        "The specified export task does not exist.",
    )
}

impl Core {
    fn task(&self, id: &str) -> Option<Value> {
        self.docs
            .get(&doc_key(EXPORT, id))
            .and_then(|d| serde_json::from_str(&d.json).ok())
    }

    fn keep_task(&mut self, id: &str, v: &Value) {
        let updated_ms = self.now_ms();
        self.docs.insert(
            doc_key(EXPORT, id),
            Doc {
                json: v.to_string(),
                updated_ms,
            },
        );
    }

    fn set_status(&mut self, id: &str, code: &str, message: &str) {
        let Some(mut t) = self.task(id) else {
            return;
        };
        t.put("status", json!({"code": code, "message": message}));
        if code == "COMPLETED" || code == "FAILED" || code == "CANCELLED" {
            let now = self.now_ms();
            if let Some(info) = t.get_mut("executionInfo") {
                info.put("completionTime", json!(now));
            }
        }
        self.keep_task(id, &t);
    }

    pub(crate) fn create_export(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let group = r.s("logGroupName").to_owned();
        if self.groups.get(&group).is_none() {
            return Err(no_group());
        }
        let (from, to) = (r.int("from").unwrap_or(0), r.int("to").unwrap_or(0));
        if to < from {
            // (memory)
            return Err(invalid(
                "The specified time range is invalid: 'to' must not be earlier than 'from'.",
            ));
        }
        let id = aws_seal::uuid_v4().unwrap_or_default();
        let key_prefix = r
            .opt("destinationPrefix")
            .unwrap_or("exportedlogs")
            .trim_end_matches('/')
            .to_owned();
        let mut task = json!({
            "taskId": id,
            "logGroupName": group,
            "from": from,
            "to": to,
            "destination": r.s("destination"),
            "destinationPrefix": key_prefix,
            "status": {"code": "PENDING", "message": "Pending"},
            "executionInfo": {"creationTime": self.now_ms()},
        });
        if let Some(n) = r.opt("taskName") {
            task.put("taskName", json!(n));
        }
        self.keep_task(&id, &task);
        self.export = Some(Pending {
            id: id.clone(),
            group,
            prefix: r.opt("logStreamNamePrefix").map(str::to_owned),
            from,
            to,
            bucket: r.s("destination").to_owned(),
            key_prefix,
        });
        Ok(json!({"taskId": id}))
    }

    pub(crate) fn cancel_export(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let id = r.s("taskId");
        let t = self.task(id).ok_or_else(no_task)?;
        let code = t
            .pointer("/status/code")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if code != "PENDING" && code != "RUNNING" {
            // The model: "The task must be in the PENDING or RUNNING state." Recorded 2026-11-19.
            return Err(ApiError::modeled(
                "InvalidOperationException",
                "The specified export task has already finished",
            ));
        }
        self.set_status(id, "CANCELLED", "Cancelled by user");
        Ok(json!({}))
    }

    pub(crate) fn describe_exports(&self, r: &Req<'_>) -> Value {
        let (id, code) = (r.opt("taskId"), r.opt("statusCode"));
        let mut all: Vec<Value> = self
            .docs
            .with_prefix(&doc_key(EXPORT, ""))
            .filter_map(|(_, d)| serde_json::from_str::<Value>(&d.json).ok())
            .filter(|t| id.is_none_or(|i| t.get("taskId").and_then(Value::as_str) == Some(i)))
            .filter(|t| {
                code.is_none_or(|c| t.pointer("/status/code").and_then(Value::as_str) == Some(c))
            })
            .collect();
        // Newest first. (memory)
        all.sort_by_key(|t| {
            std::cmp::Reverse(
                t.pointer("/executionInfo/creationTime")
                    .and_then(Value::as_i64),
            )
        });
        let size = r
            .int("limit")
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(50);
        let from = r
            .opt("nextToken")
            .and_then(|t| t.parse::<usize>().ok())
            .unwrap_or(0);
        let mut out = json!({"exportTasks": all.iter().skip(from).take(size).collect::<Vec<_>>()});
        if from + size < all.len() {
            out.put("nextToken", json!((from + size).to_string()));
        }
        out
    }

    /// Runs the export the call made, if it made one: the objects into S3, and the task's end. A bucket S3 refuses refuses the call
    /// (the task is removed); a failure part way fails the task.
    pub(crate) async fn run_export(&mut self, call: &Call, reply: &mut aws_wire::service::Reply) {
        let Some(p) = self.export.take() else {
            return;
        };
        if reply.is_err() {
            return;
        }
        let Some(peers) = self.peers.clone() else {
            self.set_status(&p.id, "FAILED", "S3 is not running in doze-aws.");
            return;
        };
        let check = (
            format!("{}/aws-logs-write-test", p.key_prefix),
            b"Permission Check Successful".to_vec(),
        );
        if let Err(e) = put_object(&peers, call, &p.bucket, check).await {
            self.docs.remove(&doc_key(EXPORT, &p.id));
            // (memory)
            *reply = Err(invalid(format!(
                "The given bucket could not be written to: {}. Check that the bucket exists in this Region.",
                e.message
            )));
            return;
        }
        self.set_status(&p.id, "RUNNING", "Started successfully");
        let outcome = self.write_streams(&peers, call, &p).await;
        match outcome {
            Ok(()) => self.set_status(&p.id, "COMPLETED", "Completed successfully"),
            Err(e) => self.set_status(&p.id, "FAILED", &e.message),
        }
    }

    async fn write_streams(
        &self,
        peers: &aws_peers::Peers,
        call: &Call,
        p: &Pending,
    ) -> Result<(), ApiError> {
        let streams: Vec<(u32, String)> = self
            .streams
            .with_prefix(&stream_key(
                &p.group,
                p.prefix.as_deref().unwrap_or_default(),
            ))
            .map(|(_, s)| (s.no, s.name.clone()))
            .collect();
        let range = (
            format!("{:016x}", p.from.max(0)),
            format!("{:016x}", p.to.max(0).saturating_add(1)),
        );
        for (no, name) in streams {
            let mut gz = GzEncoder::new(Vec::new(), Compression::default());
            let mut lines = 0usize;
            let mut from = range.0.clone();
            loop {
                let rows = self.rows(no, (&from, &range.1), CHUNK, false, &|_| true)?;
                for row in &rows {
                    let line = format!("{} {}\n", iso(row.ts), row.event.message);
                    gz.write_all(line.as_bytes())
                        .map_err(|e| ApiError::internal(e.to_string()))?;
                }
                lines += rows.len();
                match rows.last() {
                    Some(last) if rows.len() == CHUNK => from = format!("{}\u{0}", last.pos),
                    _ => break,
                }
            }
            if lines == 0 {
                continue;
            }
            let body = gz.finish().map_err(|e| ApiError::internal(e.to_string()))?;
            put_object(
                peers,
                call,
                &p.bucket,
                (format!("{}/{}/{name}/000000.gz", p.key_prefix, p.id), body),
            )
            .await?;
        }
        Ok(())
    }
}

async fn put_object(
    peers: &aws_peers::Peers,
    call: &Call,
    bucket: &str,
    (key, body): (String, Vec<u8>),
) -> Result<(), ApiError> {
    peers
        .call_rest_with_body(
            "logs",
            &call.caller,
            ("s3", "PutObject"),
            &json!({"Bucket": bucket, "Key": key}),
            bytes::Bytes::from(body),
        )
        .await
        .map(drop)
}

/// `2026-11-18T09:30:00.000Z`.
fn iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let base = aws_core::time::format_iso8601(secs);
    format!(
        "{}.{:03}Z",
        base.trim_end_matches(".000Z"),
        ms.rem_euclid(1000)
    )
}
