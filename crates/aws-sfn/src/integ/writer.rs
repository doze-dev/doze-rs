//! A distributed Map's `ResultWriter` (`arn:aws:states:::s3:putObject`): the results go to S3 under `<Prefix>/<Map Run id>/` as
//! `SUCCEEDED_0.json` and `FAILED_0.json` (arrays of the items' results) and a `manifest.json` that lists them; the state's result is
//! `{MapRunArn, ResultWriterDetails: {Bucket, Key}}` (the file names and the members are from memory).

use std::sync::Arc;

use aws_asl::Failure;
use aws_peers::Peers;
use aws_wire::service::Caller;
use bytes::Bytes;
use serde_json::{Value, json};

use super::sdk::Whole;

fn fail(why: impl Into<String>) -> Failure {
    Failure::new(aws_asl::errors::RESULT_WRITER_FAILED, why)
}

async fn put(
    peers: &Arc<Peers>,
    caller: &Caller,
    bucket: &str,
    key: &str,
    body: &Value,
) -> Result<u64, Failure> {
    let text = body.to_string();
    let len = text.len() as u64;
    let upload = Box::new(Whole(Some(Bytes::from(text.into_bytes())), len));
    peers
        .call_rest_as(
            caller,
            "s3",
            "PutObject",
            &json!({"Bucket": bucket, "Key": key}),
            Some(upload),
        )
        .await
        .map_err(|e| fail(format!("S3.{}: {}", e.code, e.message)))?;
    Ok(len)
}

pub(super) async fn write(
    peers: &Arc<Peers>,
    caller: &Caller,
    input: &Value,
) -> Result<Value, Failure> {
    let resource = input
        .get("Resource")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if resource != "arn:aws:states:::s3:putObject" {
        return Err(fail(format!(
            "doze-aws cannot write results with {resource}"
        )));
    }
    let params = input
        .get("Parameters")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let bucket = params
        .get("Bucket")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let prefix = params
        .get("Prefix")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim_end_matches('/');
    let run = input
        .get("MapRunArn")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = run.rsplit(':').next().unwrap_or("run");
    let base = if prefix.is_empty() {
        id.to_owned()
    } else {
        format!("{prefix}/{id}")
    };
    let results = input
        .get("Results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (ok, bad): (Vec<Value>, Vec<Value>) = results
        .into_iter()
        .partition(|r| r["Status"] == "SUCCEEDED");
    let mut files = json!({"SUCCEEDED": [], "FAILED": [], "PENDING": []});
    for (name, list) in [("SUCCEEDED", ok), ("FAILED", bad)] {
        if list.is_empty() {
            continue;
        }
        let key = format!("{base}/{name}_0.json");
        let size = put(peers, caller, bucket, &key, &Value::Array(list)).await?;
        if let Some(a) = files.get_mut(name).and_then(Value::as_array_mut) {
            a.push(json!({"Key": key, "Size": size}));
        }
    }
    let manifest = format!("{base}/manifest.json");
    let doc = json!({"DestinationBucket": bucket, "MapRunArn": run, "ResultFiles": files});
    put(peers, caller, bucket, &manifest, &doc).await?;
    Ok(json!({"MapRunArn": run, "ResultWriterDetails": {"Bucket": bucket, "Key": manifest}}))
}
