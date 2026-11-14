//! `AWS::S3::Bucket` and `AWS::S3::BucketPolicy`. A bucket's settings are each put with S3's own operation after the bucket is
//! made, translated from the template's spelling (`CorsRules`, `ExpirationInDays`, `S3Key`...) to S3's (`CORSRules`,
//! `Expiration.Days`, `Key.FilterRules`...); an update puts what changed and deletes what the template dropped.

use serde_json::{Value, json};

use super::s3_config::{
    accelerate, cors, encryption, lifecycle, logging, notifications, object_lock, ownership,
    public_access, replication, versioning, website,
};
use super::{Done, Made, Rc, document, gone, tag_list, text};
use crate::j::J;
use crate::model::Res;

const SVC: &str = "s3";

/// A bucket setting: the operation that puts it, the member it goes in, how it is read from the template, and how it is removed.
struct Setting {
    put: &'static str,
    member: &'static str,
    read: fn(&Value) -> Option<Value>,
    delete: Option<&'static str>,
}

const SETTINGS: &[Setting] = &[
    Setting {
        put: "PutPublicAccessBlock",
        member: "PublicAccessBlockConfiguration",
        read: public_access,
        delete: Some("DeletePublicAccessBlock"),
    },
    Setting {
        put: "PutBucketOwnershipControls",
        member: "OwnershipControls",
        read: ownership,
        delete: Some("DeleteBucketOwnershipControls"),
    },
    Setting {
        put: "PutBucketVersioning",
        member: "VersioningConfiguration",
        read: versioning,
        delete: None,
    },
    Setting {
        put: "PutBucketEncryption",
        member: "ServerSideEncryptionConfiguration",
        read: encryption,
        delete: Some("DeleteBucketEncryption"),
    },
    Setting {
        put: "PutBucketCors",
        member: "CORSConfiguration",
        read: cors,
        delete: Some("DeleteBucketCors"),
    },
    Setting {
        put: "PutBucketLifecycleConfiguration",
        member: "LifecycleConfiguration",
        read: lifecycle,
        delete: Some("DeleteBucketLifecycle"),
    },
    Setting {
        put: "PutBucketWebsite",
        member: "WebsiteConfiguration",
        read: website,
        delete: Some("DeleteBucketWebsite"),
    },
    Setting {
        put: "PutBucketLogging",
        member: "BucketLoggingStatus",
        read: logging,
        delete: None,
    },
    Setting {
        put: "PutBucketAccelerateConfiguration",
        member: "AccelerateConfiguration",
        read: accelerate,
        delete: None,
    },
    Setting {
        put: "PutObjectLockConfiguration",
        member: "ObjectLockConfiguration",
        read: object_lock,
        delete: None,
    },
    Setting {
        put: "PutBucketReplication",
        member: "ReplicationConfiguration",
        read: replication,
        delete: Some("DeleteBucketReplication"),
    },
    Setting {
        put: "PutBucketNotificationConfiguration",
        member: "NotificationConfiguration",
        read: notifications,
        delete: None,
    },
];

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    if rc.ty == "AWS::S3::BucketPolicy" {
        let bucket = text(p, "Bucket").unwrap_or_default();
        rc.rest(
            SVC,
            "PutBucketPolicy",
            json!({"Bucket": bucket, "Policy": document(p, "PolicyDocument").unwrap_or_default()}),
        )
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
        return Ok(Made::new(bucket));
    }
    if rc.ty != "AWS::S3::Bucket" {
        return Err(format!("{} is not a type doze-aws makes", rc.ty));
    }
    let name = text(p, "BucketName").unwrap_or_else(|| super::generated_name(rc, 63, true));
    if rc
        .rest(SVC, "HeadBucket", json!({"Bucket": name}))
        .await
        .is_ok()
    {
        return Err(rc.taken(&name));
    }
    let mut body = json!({"Bucket": name});
    if rc.caller.region != "us-east-1" {
        body.put(
            "CreateBucketConfiguration",
            json!({"LocationConstraint": rc.caller.region}),
        );
    }
    if text(p, "ObjectLockEnabled").as_deref() == Some("true") {
        body.put("ObjectLockEnabledForBucket", true.into());
    }
    rc.rest(SVC, "CreateBucket", body)
        .await
        .map_err(|e| rc.fail(SVC, &e))?;
    for s in SETTINGS {
        if let Some(v) = (s.read)(p) {
            put(rc, &name, s, v).await?;
        }
    }
    retag(rc, &name, &[], &tag_list(rc, p)).await?;
    let mut made = described(rc, &name);
    if let Some(acl) = text(p, "AccessControl").filter(|a| a != "Private") {
        // ACLs are refused by doze-aws (AWS turned them off for new buckets in 2023); the canned ACL is not applied.
        made.note = Some(format!(
            "AccessControl {acl} is stored and not applied locally: doze-aws does not serve ACLs"
        ));
    }
    Ok(made)
}

fn described(rc: &Rc, name: &str) -> Made {
    let region = &rc.caller.region;
    Made::new(name)
        .with("Arn", format!("arn:aws:s3:::{name}"))
        .with("DomainName", format!("{name}.s3.amazonaws.com"))
        .with(
            "RegionalDomainName",
            format!("{name}.s3.{region}.amazonaws.com"),
        )
        .with(
            "DualStackDomainName",
            format!("{name}.s3.dualstack.{region}.amazonaws.com"),
        )
        .with(
            "WebsiteURL",
            format!("http://{name}.s3-website-{region}.amazonaws.com"),
        )
}

async fn put(rc: &Rc, bucket: &str, s: &Setting, v: Value) -> Result<(), String> {
    let mut body = json!({"Bucket": bucket});
    body.put(s.member, v);
    rc.rest(SVC, s.put, body)
        .await
        .map(|_| ())
        .map_err(|e| rc.fail(SVC, &e))
}

async fn retag(
    rc: &Rc,
    bucket: &str,
    then: &[(String, String)],
    now: &[(String, String)],
) -> Result<(), String> {
    if then == now {
        return Ok(());
    }
    let r = if now.is_empty() {
        rc.rest(SVC, "DeleteBucketTagging", json!({"Bucket": bucket}))
            .await
    } else {
        let set: Vec<Value> = now
            .iter()
            .map(|(k, v)| json!({"Key": k, "Value": v}))
            .collect();
        rc.rest(
            SVC,
            "PutBucketTagging",
            json!({"Bucket": bucket, "Tagging": {"TagSet": set}}),
        )
        .await
    };
    r.map(|_| ()).map_err(|e| rc.fail(SVC, &e))
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let name = old.physical.clone().unwrap_or_default();
    let before = old.props_value();
    if rc.ty == "AWS::S3::BucketPolicy" {
        return create(rc, p).await;
    }
    for s in SETTINGS {
        let (then, now) = ((s.read)(&before), (s.read)(p));
        if then == now {
            continue;
        }
        match (now, s.delete) {
            (Some(v), _) => put(rc, &name, s, v).await?,
            (None, Some(op)) => {
                rc.rest(SVC, op, json!({"Bucket": name}))
                    .await
                    .map_err(|e| rc.fail(SVC, &e))?;
            }
            (None, None) if s.put == "PutBucketVersioning" => {
                put(rc, &name, s, json!({"Status": "Suspended"})).await?;
            }
            (None, None) if s.put == "PutBucketNotificationConfiguration" => {
                put(rc, &name, s, json!({})).await?
            }
            (None, None) if s.put == "PutBucketLogging" => put(rc, &name, s, json!({})).await?,
            (None, None) => {}
        }
    }
    retag(rc, &name, &tag_list(rc, &before), &tag_list(rc, p)).await?;
    Ok(described(rc, &name))
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let name = res.physical.clone().unwrap_or_default();
    let op = if rc.ty == "AWS::S3::BucketPolicy" {
        "DeleteBucketPolicy"
    } else {
        "DeleteBucket"
    };
    match rc.rest(SVC, op, json!({"Bucket": name})).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
