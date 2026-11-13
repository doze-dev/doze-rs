//! `Custom::CDKBucketDeployment*` (the CDK's `BucketDeployment`): each source archive is read from S3, unpacked, its markers filled in,
//! and every file put under the destination prefix; with `Prune` (the default) what was there and is not in the sources is deleted.
//! A delete removes the files only with `RetainOnDelete: false`. Not done: `Include`/`Exclude` filters (refused by name),
//! CloudFront invalidation (no CloudFront locally; the distribution is ignored and the log says so).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::Request;
use crate::j::J;
use crate::model::Res;
use crate::res::{Done, Made, Rc, text};

fn list(p: &Value, key: &str) -> Vec<Value> {
    p.get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn flag(p: &Value, key: &str, default: bool) -> bool {
    text(p, key).map_or(default, |v| v == "true")
}

/// A file's content type from its extension (the common web ones; anything else is binary).
fn content_type(key: &str) -> &'static str {
    match key
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html",
        "js" | "mjs" => "application/javascript",
        "css" => "text/css",
        "json" | "map" => "application/json",
        "txt" => "text/plain",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// `<<marker:0xbaba:N>>` replaced by the source's markers, as the CDK's handler does.
fn fill(contents: Vec<u8>, markers: &Value) -> Vec<u8> {
    let Some(m) = markers.as_object().filter(|m| !m.is_empty()) else {
        return contents;
    };
    let Ok(mut text) = String::from_utf8(contents.clone()) else {
        return contents;
    };
    for (k, v) in m {
        text = text.replace(k, v.as_str().unwrap_or_default());
    }
    text.into_bytes()
}

pub(super) async fn run(rc: &Rc, req: Request, old: Option<&Res>, p: &Value) -> Done {
    let bucket = text(p, "DestinationBucketName").unwrap_or_default();
    let prefix = text(p, "DestinationBucketKeyPrefix").unwrap_or_default();
    let physical = old.and_then(|o| o.physical.clone()).unwrap_or_else(|| {
        format!(
            "aws.cdk.s3deployment.{}",
            aws_seal::uuid_v4().unwrap_or_default()
        )
    });
    let made = Made::new(&physical).with("DestinationBucketArn", format!("arn:aws:s3:::{bucket}"));
    if req == Request::Delete {
        if !flag(p, "RetainOnDelete", true) {
            super::cdk::empty_bucket(rc, &bucket, &prefix).await?;
        }
        return Ok(made);
    }
    if !list(p, "Include").is_empty() || !list(p, "Exclude").is_empty() {
        return Err(rc.handler(
            "BucketDeployment include and exclude filters are not built in doze-aws",
            "InvalidRequest",
        ));
    }
    if p.get("DistributionId").is_some() {
        tracing::warn!(
            "cloudformation: {}: no CloudFront locally; the invalidation is not made",
            rc.logical
        );
    }
    let files = gather(rc, p).await?;
    for (key, contents) in &files {
        let mut body = json!({"Bucket": bucket, "Key": key, "ContentType": content_type(key)});
        if let Some(Value::Object(meta)) = p.get("UserMetadata") {
            body.put("Metadata", Value::Object(meta.clone()));
        }
        let peers = rc.peers.as_ref().ok_or("S3 is not running")?;
        peers
            .call_rest_with_body(
                "cloudformation",
                &rc.caller,
                ("s3", "PutObject"),
                &body,
                bytes::Bytes::from(contents.clone()),
            )
            .await
            .map_err(|e| rc.fail("s3", &e))?;
    }
    if flag(p, "Prune", true) {
        prune(rc, &bucket, &prefix, &files).await?;
    }
    Ok(made)
}

/// Every file of every source, keyed by its destination key (a later source wins).
async fn gather(rc: &Rc, p: &Value) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let prefix = text(p, "DestinationBucketKeyPrefix").unwrap_or_default();
    let (buckets, keys, markers) = (
        list(p, "SourceBucketNames"),
        list(p, "SourceObjectKeys"),
        list(p, "SourceMarkers"),
    );
    let mut out = BTreeMap::new();
    for (i, (b, k)) in buckets.iter().zip(&keys).enumerate() {
        let (b, k) = (
            b.as_str().unwrap_or_default(),
            k.as_str().unwrap_or_default(),
        );
        let archive =
            crate::transform::include::object(rc.peers.as_deref(), &rc.caller, b, k, 1 << 30)
                .await
                .map_err(|e| rc.handler(&format!("Source s3://{b}/{k}: {e}"), "NotFound"))?;
        let marks = markers.get(i).cloned().unwrap_or(Value::Null);
        if flag(p, "Extract", true) {
            for (name, contents) in
                super::unzip::files(&archive).map_err(|e| rc.handler(&e, "InvalidRequest"))?
            {
                out.insert(format!("{prefix}{name}"), fill(contents, &marks));
            }
        } else {
            let name = k.rsplit('/').next().unwrap_or(k);
            out.insert(format!("{prefix}{name}"), archive);
        }
    }
    Ok(out)
}

/// Deletes what is under the prefix and not among `files`.
async fn prune(
    rc: &Rc,
    bucket: &str,
    prefix: &str,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    let listed = rc
        .rest(
            "s3",
            "ListObjectsV2",
            json!({"Bucket": bucket, "Prefix": prefix}),
        )
        .await
        .map_err(|e| rc.fail("s3", &e))?;
    let stale: Vec<Value> = listed
        .at("Contents")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| o.at("Key").as_str())
        .filter(|k| !files.contains_key(*k))
        .map(|k| json!({"Key": k}))
        .collect();
    if !stale.is_empty() {
        let body = json!({"Bucket": bucket, "Delete": {"Objects": stale, "Quiet": true}});
        rc.rest("s3", "DeleteObjects", body)
            .await
            .map_err(|e| rc.fail("s3", &e))?;
    }
    Ok(())
}
