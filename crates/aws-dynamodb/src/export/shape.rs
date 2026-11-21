//! What an export reads (its time or window, held to the recovery window) and what its describe and summary manifest say.

use aws_wire::ApiError;
use serde_json::{Value as Json, json};

use super::{INCREMENTAL_MAX_MS, INCREMENTAL_MIN_MS};
use crate::core::Cx;
use crate::describe::seconds;
use crate::meta::Table;
use crate::req::{set, str_of, time_ms_of, validation};

/// What an export reads.
#[derive(Clone, Copy, Debug)]
pub(super) enum Window {
    Full(i64),
    Incremental { from: i64, to: i64, new_only: bool },
}

/// The time (or window) an export asks for, held to what recovery can serve.
pub(super) fn window(t: &Table, body: &Json, now_ms: i64) -> Result<Window, ApiError> {
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
            let from = time_ms_of(spec, "ExportFromTime").ok_or_else(|| {
                validation("ExportFromTime must be specified for an incremental export")
            })?;
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
pub(super) fn fingerprint(body: &Json) -> String {
    let mut b = body.clone();
    if let Json::Object(m) = &mut b {
        m.remove("ClientToken");
    }
    b.to_string()
}

pub(super) fn shown(
    cx: &Cx<'_>,
    t: &Table,
    arn: &str,
    window: &Window,
    items: u64,
    billed: u64,
) -> Json {
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
            // Recorded 2026-11-19: a full export shows its type only when the request named it.
            if let Some(kind) = str_of(body, "ExportType") {
                set(&mut out, "ExportType", json!(kind));
            }
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
pub(super) fn summary(shown: &Json, now_ms: i64) -> Json {
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
        set(
            &mut out,
            "outputView",
            spec.get("ExportViewType").cloned().unwrap_or_default(),
        );
        if let (Some(f), Some(t)) = (
            ms_of(spec.get("ExportFromTime")),
            ms_of(spec.get("ExportToTime")),
        ) {
            set(&mut out, "exportFromTime", json!(iso(f)));
            set(&mut out, "exportToTime", json!(iso(t)));
        }
    } else if let Some(at) = ms_of(shown.get("ExportTime")) {
        set(&mut out, "exportTime", json!(iso(at)));
    }
    out
}
