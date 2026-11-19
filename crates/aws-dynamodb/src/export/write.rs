//! Writing an export's files to S3.

use std::io::Write as _;
use std::sync::Arc;

use aws_peers::Peers;
use aws_wire::ApiError;
use md5::Digest as _;
use serde_json::{Value as Json, json};

use super::{Job, Written};
use crate::req::set;
use crate::work::Failure;

pub(crate) fn gzip(lines: &[String]) -> Result<Vec<u8>, ApiError> {
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
    put(
        peers,
        job,
        &format!("{dir}/manifest-files.md5"),
        md5_base64(files.as_bytes()).into_bytes(),
    )
    .await?;
    let mut summary = job.summary.clone();
    let iso = aws_core::time::format_iso8601(job.caller_time());
    set(&mut summary, "endTime", json!(iso));
    set(&mut summary, "manifestFilesS3Key", json!(files_key));
    let summary = summary.to_string();
    let summary_key = format!("{dir}/manifest-summary.json");
    put(peers, job, &summary_key, summary.clone().into_bytes()).await?;
    put(
        peers,
        job,
        &format!("{dir}/manifest-summary.md5"),
        md5_base64(summary.as_bytes()).into_bytes(),
    )
    .await?;
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
            format!(
                "The specified bucket does not exist (Service: Amazon S3; Status Code: 404; Error Code: NoSuchBucket; Bucket: {bucket})"
            ),
        ),
        "AccessDenied" => Failure::new("S3AccessDenied", e.message.clone()),
        code => Failure::new("UNKNOWN", format!("{code}: {}", e.message)),
    }
}
