//! What a request asks to do, for the authorizer: the IAM action (`s3:GetObject`, which for some operations is not the operation's
//! name), the ARN of the bucket or object it acts on, and the bucket's policy.

use aws_model_s3::Operation;
use aws_peers::Need;
use serde_json::Value;

use crate::core::Core;
use crate::req::Req;

/// The IAM action of `op`, with the version-specific form when the request names a version.
fn action(op: Operation, versioned: bool) -> String {
    use Operation as O;
    let v = |plain: &'static str, with: &'static str| if versioned { with } else { plain };
    let name: &str = match op {
        O::ListObjects | O::ListObjectsV2 | O::HeadBucket => "ListBucket",
        O::ListObjectVersions => "ListBucketVersions",
        O::ListBuckets => "ListAllMyBuckets",
        O::GetObject | O::HeadObject | O::GetObjectAttributes => v("GetObject", "GetObjectVersion"),
        O::DeleteObject => v("DeleteObject", "DeleteObjectVersion"),
        O::DeleteObjects => "DeleteObject",
        O::PutObject
        | O::CreateMultipartUpload
        | O::UploadPart
        | O::UploadPartCopy
        | O::CompleteMultipartUpload
        | O::CopyObject => "PutObject",
        O::ListParts => "ListMultipartUploadParts",
        O::ListMultipartUploads => "ListBucketMultipartUploads",
        O::GetObjectTagging => v("GetObjectTagging", "GetObjectVersionTagging"),
        O::PutObjectTagging => v("PutObjectTagging", "PutObjectVersionTagging"),
        O::DeleteObjectTagging => v("DeleteObjectTagging", "DeleteObjectVersionTagging"),
        O::PutBucketCors | O::DeleteBucketCors => "PutBucketCORS",
        O::GetBucketCors => "GetBucketCORS",
        O::PutBucketLifecycleConfiguration | O::DeleteBucketLifecycle => {
            "PutLifecycleConfiguration"
        }
        O::GetBucketLifecycleConfiguration => "GetLifecycleConfiguration",
        O::PutBucketEncryption | O::DeleteBucketEncryption => "PutEncryptionConfiguration",
        O::GetBucketEncryption => "GetEncryptionConfiguration",
        O::PutBucketNotificationConfiguration => "PutBucketNotification",
        O::GetBucketNotificationConfiguration => "GetBucketNotification",
        O::PutBucketAccelerateConfiguration => "PutAccelerateConfiguration",
        O::GetBucketAccelerateConfiguration => "GetAccelerateConfiguration",
        O::PutBucketReplication | O::DeleteBucketReplication => "PutReplicationConfiguration",
        O::GetBucketReplication => "GetReplicationConfiguration",
        O::PutObjectLockConfiguration => "PutBucketObjectLockConfiguration",
        O::GetObjectLockConfiguration => "GetBucketObjectLockConfiguration",
        O::PutBucketAnalyticsConfiguration | O::DeleteBucketAnalyticsConfiguration => {
            "PutAnalyticsConfiguration"
        }
        O::GetBucketAnalyticsConfiguration | O::ListBucketAnalyticsConfigurations => {
            "GetAnalyticsConfiguration"
        }
        O::PutBucketInventoryConfiguration | O::DeleteBucketInventoryConfiguration => {
            "PutInventoryConfiguration"
        }
        O::GetBucketInventoryConfiguration | O::ListBucketInventoryConfigurations => {
            "GetInventoryConfiguration"
        }
        O::PutBucketMetricsConfiguration | O::DeleteBucketMetricsConfiguration => {
            "PutMetricsConfiguration"
        }
        O::GetBucketMetricsConfiguration | O::ListBucketMetricsConfigurations => {
            "GetMetricsConfiguration"
        }
        O::PutBucketIntelligentTieringConfiguration
        | O::DeleteBucketIntelligentTieringConfiguration => "PutIntelligentTieringConfiguration",
        O::GetBucketIntelligentTieringConfiguration
        | O::ListBucketIntelligentTieringConfigurations => "GetIntelligentTieringConfiguration",
        O::DeleteBucketTagging => "PutBucketTagging",
        O::DeletePublicAccessBlock => "PutBucketPublicAccessBlock",
        O::DeleteBucketOwnershipControls => "PutBucketOwnershipControls",
        // The rest are named as the operation is: CreateBucket, DeleteBucket, GetBucketLocation, Get/PutBucketVersioning,
        // Get/PutBucketTagging, Get/PutBucketPolicy, DeleteBucketPolicy, Get/PutBucketWebsite, DeleteBucketWebsite, Get/PutBucketLogging,
        // Get/PutBucketOwnershipControls, Get/PutBucketPublicAccessBlock, Get/PutBucketRequestPayment, AbortMultipartUpload.
        other => return format!("s3:{other:?}"),
    };
    format!("s3:{name}")
}

impl Core {
    /// Every thing `req` asks to do. A request that asks to bypass GOVERNANCE retention also needs `s3:BypassGovernanceRetention` on
    /// each object it acts on.
    pub(crate) fn needs(&self, req: &Req<'_>) -> Vec<Need> {
        let mut needs = self.object_needs(req);
        let bypasses = matches!(
            Operation::from_index(req.call.op),
            Some(
                Operation::DeleteObject | Operation::DeleteObjects | Operation::PutObjectRetention
            )
        ) && req.flag("BypassGovernanceRetention") == Some(true);
        if bypasses {
            let extra: Vec<Need> = needs
                .iter()
                .map(|n| Need {
                    action: "s3:BypassGovernanceRetention".to_owned(),
                    ..n.clone()
                })
                .collect();
            needs.extend(extra);
        }
        needs
    }

    fn object_needs(&self, req: &Req<'_>) -> Vec<Need> {
        let Some(op) = Operation::from_index(req.call.op) else {
            return Vec::new();
        };
        let bucket = req.bucket();
        let policy = self
            .buckets
            .get(bucket)
            .and_then(|b| b.configs.get("policy"))
            .cloned();
        let bucket_arn = format!("arn:aws:s3:::{bucket}");
        let on_bucket =
            |act: String| Need::new(act, bucket_arn.clone()).with_policy(policy.clone());
        let on_object = |act: String, key: &str| {
            Need::new(act, format!("{bucket_arn}/{key}")).with_policy(policy.clone())
        };
        let versioned = req.s("VersionId").is_some();
        let act = action(op, versioned);
        use Operation as O;
        match op {
            O::ListBuckets => vec![Need::new(act, "*")],
            O::CreateBucket => vec![Need::new(act, bucket_arn)],
            O::ListObjects | O::ListObjectsV2 | O::ListObjectVersions => {
                let prefix = req.s("Prefix").unwrap_or_default().to_owned();
                vec![on_bucket(act).with("s3:prefix", vec![prefix])]
            }
            O::DeleteObjects => req
                .get("Delete")
                .and_then(|d| d.get("Objects"))
                .and_then(Value::as_array)
                .map(|o| {
                    o.iter()
                        .filter_map(|x| x.get("Key").and_then(Value::as_str))
                        .map(|k| on_object(act.clone(), k))
                        .collect()
                })
                .unwrap_or_default(),
            O::CopyObject => {
                let mut v = vec![on_object(act, req.key())];
                if let Some((b, k, _)) = req.s("CopySource").and_then(crate::copy::parse_source) {
                    let src_policy = self
                        .buckets
                        .get(&b)
                        .and_then(|x| x.configs.get("policy"))
                        .cloned();
                    v.push(
                        Need::new("s3:GetObject", format!("arn:aws:s3:::{b}/{k}"))
                            .with_policy(src_policy),
                    );
                }
                v
            }
            _ if req.get("Key").is_some() => vec![on_object(act, req.key())],
            _ => vec![on_bucket(act)],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_are_named_as_iam_names_them() {
        use Operation as O;
        let cases = [
            (O::ListObjectsV2, false, "s3:ListBucket"),
            (O::HeadBucket, false, "s3:ListBucket"),
            (O::ListBuckets, false, "s3:ListAllMyBuckets"),
            (O::GetObject, false, "s3:GetObject"),
            (O::GetObject, true, "s3:GetObjectVersion"),
            (O::DeleteObject, true, "s3:DeleteObjectVersion"),
            (O::CreateMultipartUpload, false, "s3:PutObject"),
            (O::ListParts, false, "s3:ListMultipartUploadParts"),
            (
                O::ListMultipartUploads,
                false,
                "s3:ListBucketMultipartUploads",
            ),
            (O::PutBucketCors, false, "s3:PutBucketCORS"),
            (
                O::DeleteBucketLifecycle,
                false,
                "s3:PutLifecycleConfiguration",
            ),
            (
                O::GetBucketEncryption,
                false,
                "s3:GetEncryptionConfiguration",
            ),
            (
                O::PutBucketNotificationConfiguration,
                false,
                "s3:PutBucketNotification",
            ),
            (
                O::PutBucketAnalyticsConfiguration,
                false,
                "s3:PutAnalyticsConfiguration",
            ),
            (O::CreateBucket, false, "s3:CreateBucket"),
            (O::AbortMultipartUpload, false, "s3:AbortMultipartUpload"),
            (O::PutBucketPolicy, false, "s3:PutBucketPolicy"),
        ];
        for (op, versioned, want) in cases {
            assert_eq!(action(op, versioned), want, "{op:?}");
        }
    }
}
