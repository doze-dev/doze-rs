//! The CDK's custom resources, done natively: `Custom::S3AutoDeleteObjects` (empties a bucket before it is deleted),
//! `Custom::S3BucketNotifications` (puts a bucket's notification configuration), `Custom::LogRetention` (a log group's retention),
//! `Custom::CDKBucketDeployment*` (copies asset archives into a bucket) and `Custom::AWS` (one SDK call per lifecycle event).

use serde_json::{Value, json};

use super::Request;
use crate::j::J;
use crate::model::Res;
use crate::res::{Done, Made, Rc, gone, text};

/// The native outcome for the CDK types; `None` for any other custom resource.
pub(super) async fn native(rc: &Rc, req: Request, old: Option<&Res>, p: &Value) -> Option<Done> {
    let ty = rc.ty.as_str();
    Some(match ty {
        "Custom::S3AutoDeleteObjects" => auto_delete(rc, req, p).await,
        "Custom::S3BucketNotifications" => notifications(rc, req, p).await,
        "Custom::LogRetention" => log_retention(rc, req, p).await,
        "Custom::AWS" => super::aws_call::run(rc, req, old, p).await,
        t if t.starts_with("Custom::CDKBucketDeployment") => {
            super::deploy::run(rc, req, old, p).await
        }
        _ => return None,
    })
}

/// Empties a bucket (every version and delete marker) when the resource is deleted, so the bucket can go after it.
async fn auto_delete(rc: &Rc, req: Request, p: &Value) -> Done {
    let bucket = text(p, "BucketName").unwrap_or_default();
    if req == Request::Delete {
        empty_bucket(rc, &bucket, "").await?;
    }
    Ok(Made::new(bucket))
}

/// Deletes every object (all versions) under `prefix` of `bucket`. A bucket already gone is empty.
pub(super) async fn empty_bucket(rc: &Rc, bucket: &str, prefix: &str) -> Result<(), String> {
    for _ in 0..1000 {
        let listed = match rc
            .rest(
                "s3",
                "ListObjectVersions",
                json!({"Bucket": bucket, "Prefix": prefix}),
            )
            .await
        {
            Ok(l) => l,
            Err(e) if gone(&e) => return Ok(()),
            Err(e) => return Err(rc.fail("s3", &e)),
        };
        let mut objects: Vec<Value> = Vec::new();
        for kind in ["Versions", "DeleteMarkers"] {
            for v in listed.at(kind).as_array().into_iter().flatten() {
                objects.push(json!({"Key": v.at("Key"), "VersionId": v.at("VersionId")}));
            }
        }
        if objects.is_empty() {
            return Ok(());
        }
        let body = json!({"Bucket": bucket, "Delete": {"Objects": objects, "Quiet": true}});
        rc.rest("s3", "DeleteObjects", body)
            .await
            .map_err(|e| rc.fail("s3", &e))?;
    }
    Ok(())
}

/// The CDK passes the configuration in S3's own spelling; a delete leaves the bucket with none.
async fn notifications(rc: &Rc, req: Request, p: &Value) -> Done {
    let bucket = text(p, "BucketName").unwrap_or_default();
    let config = if req == Request::Delete {
        json!({})
    } else {
        p.get("NotificationConfiguration")
            .cloned()
            .unwrap_or(json!({}))
    };
    match rc
        .rest(
            "s3",
            "PutBucketNotificationConfiguration",
            json!({"Bucket": bucket, "NotificationConfiguration": config}),
        )
        .await
    {
        Ok(_) => {}
        Err(e) if req == Request::Delete && gone(&e) => {}
        Err(e) => return Err(rc.fail("s3", &e)),
    }
    Ok(Made::new(format!("{bucket}-notifications")))
}

/// A log group's retention, making the group when it is not there yet (a function's group appears at its first line).
async fn log_retention(rc: &Rc, req: Request, p: &Value) -> Done {
    let group = text(p, "LogGroupName").unwrap_or_default();
    match req {
        Request::Delete => {
            if text(p, "RemovalPolicy").as_deref() == Some("destroy") {
                match rc
                    .call("logs", "DeleteLogGroup", json!({"logGroupName": group}))
                    .await
                {
                    Ok(_) => {}
                    Err(e) if gone(&e) => {}
                    Err(e) => return Err(rc.fail("logs", &e)),
                }
            }
        }
        _ => {
            match rc
                .call("logs", "CreateLogGroup", json!({"logGroupName": group}))
                .await
            {
                Ok(_) => {}
                Err(e) if e.code.contains("AlreadyExists") => {}
                Err(e) => return Err(rc.fail("logs", &e)),
            }
            let r = match text(p, "RetentionInDays").filter(|d| d != "infinite") {
                Some(days) => {
                    rc.call(
                        "logs",
                        "PutRetentionPolicy",
                        json!({"logGroupName": group, "retentionInDays": days}),
                    )
                    .await
                }
                None => {
                    rc.call(
                        "logs",
                        "DeleteRetentionPolicy",
                        json!({"logGroupName": group}),
                    )
                    .await
                }
            };
            r.map_err(|e| rc.fail("logs", &e))?;
        }
    }
    Ok(Made::new(&group).with("LogGroupName", group.clone()))
}
