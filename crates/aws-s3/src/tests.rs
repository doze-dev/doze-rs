//! What the service serves and what it refuses by name.

use std::sync::Arc;
use std::time::Duration;

use aws_model_s3::Operation;
use doze_runtime::{Ctx, SystemClock, Tasks};

use super::*;

fn service() -> S3 {
    let ctx = Ctx {
        dir: std::env::temp_dir().join(format!("aws-s3-unit-{}", std::process::id())),
        idle: Duration::from_secs(60),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: None,
    };
    S3::new(Arc::new(ctx))
}

#[tokio::test]
async fn what_has_no_local_meaning_is_refused_by_name() {
    let s3 = service();
    let serves = |name: &str| s3.serves(TABLES.find_op(name).unwrap());
    for refused in [
        "GetBucketAcl",
        "PutBucketAcl",
        "GetObjectAcl",
        "PutObjectAcl",
        "RestoreObject",
        "SelectObjectContent",
        "GetObjectTorrent",
        "CreateSession",
        "ListDirectoryBuckets",
        "UpdateObjectEncryption",
        "WriteGetObjectResponse",
        "RenameObject",
        "CreateBucketMetadataConfiguration",
        "GetObjectAnnotation",
    ] {
        assert!(!serves(refused), "{refused} must be refused");
    }
    for built in [
        "CreateBucket",
        "PutObject",
        "GetObject",
        "HeadObject",
        "DeleteObjects",
        "CopyObject",
        "ListObjectsV2",
        "ListObjectVersions",
        "CreateMultipartUpload",
        "UploadPartCopy",
        "CompleteMultipartUpload",
        "PutBucketPolicy",
        "PutBucketLifecycleConfiguration",
        "PutBucketNotificationConfiguration",
        "PutBucketVersioning",
        "GetObjectAttributes",
        "PutObjectTagging",
        "GetObjectRetention",
        "PutObjectRetention",
        "GetObjectLegalHold",
        "PutObjectLegalHold",
        "GetBucketPolicyStatus",
    ] {
        assert!(serves(built), "{built} must be served");
    }
    let n = (0..TABLES.n_ops()).filter(|i| s3.serves(*i)).count();
    assert!((80..=90).contains(&n), "{n} operations served");
}

#[test]
fn every_operation_the_core_runs_is_in_the_model() {
    for i in 0..TABLES.n_ops() {
        assert!(Operation::from_index(i).is_some());
    }
}
