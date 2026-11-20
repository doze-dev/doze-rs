//! S3: buckets, and a bucket's objects (the first page) on its page.

use super::Format::{Bytes, Json, Mono, Text as Plain, Time};
use super::{
    Create, Delete, Descriptor, Id, Input, InputKind, Kind, List, Step, Sub, Text, col, field,
    input, or,
};

pub(super) static S3: Descriptor = Descriptor {
    service: "s3",
    target: "s3",
    kinds: &[Kind {
        id: "buckets",
        title: "Buckets",
        one: "bucket",
        about: "Buckets and the objects in them. Choose a name to see its objects.",
        members: &["Bucket"],
        list: List {
            step: Step::new(
                "ListBuckets",
                r#"{"MaxBuckets":1000,"ContinuationToken":"{token}"}"#,
            ),
            items: "Buckets",
            item_as: "Name",
            token_out: "ContinuationToken",
            id: "Name",
            name: Text::at("Name"),
            label: None,
            split: None,
        },
        elsewhere: "",
        each: &[],
        columns: &[
            col("Region", "BucketRegion", Mono),
            col("Created", "CreationDate", Time),
        ],
        id_of: Id::Name,
        detail: &[
            Step::new("GetBucketLocation", r#"{"Bucket":"{id}"}"#),
            Step::new("GetBucketVersioning", r#"{"Bucket":"{id}"}"#).optional(),
            // Each of these answers a refusal (NoSuchBucketPolicy, ServerSideEncryptionConfigurationNotFoundError,
            // NoSuchCORSConfiguration, NoSuchLifecycleConfiguration) when the bucket has none: shown as "None".
            Step::new("GetBucketPolicy", r#"{"Bucket":"{id}"}"#).optional(),
            Step::new("GetBucketEncryption", r#"{"Bucket":"{id}"}"#).optional(),
            Step::new("GetBucketCors", r#"{"Bucket":"{id}"}"#).optional(),
            Step::new("GetBucketLifecycleConfiguration", r#"{"Bucket":"{id}"}"#).optional(),
        ],
        fields: &[
            field("Name", "$id", Mono),
            // AWS's rule: a bucket in us-east-1 answers an empty LocationConstraint.
            or("Region", "LocationConstraint", Mono, "us-east-1"),
            or("Versioning", "Status", Plain, "Disabled"),
            or(
                "Default encryption",
                "ServerSideEncryptionConfiguration.Rules.0.ApplyServerSideEncryptionByDefault.SSEAlgorithm",
                Mono,
                "None",
            ),
            or("Bucket policy", "Policy", Json, "None"),
            or("CORS rules", "CORSRules", Json, "None"),
            or("Lifecycle rules", "Rules", Json, "None"),
        ],
        subs: &[Sub {
            title: "Objects",
            step: Step::new("ListObjectsV2", r#"{"Bucket":"{id}","MaxKeys":100}"#),
            items: "Contents",
            item_as: "Key",
            columns: &[
                col("Key", "Key", Mono),
                col("Size", "Size", Bytes),
                col("Last modified", "LastModified", Time),
                col("Storage class", "StorageClass", Plain),
            ],
            link: None,
            more: "IsTruncated",
            add: None,
        }],
        create: Some(Create {
            inputs: &[Input {
                required: true,
                placeholder: "product-images",
                ..input(
                    "Bucket",
                    "Name",
                    "3 to 63 lower-case letters, digits, dots and hyphens, starting and ending with a letter or digit.",
                    InputKind::Text,
                )
            }],
            steps: &[Step::new("CreateBucket", r#"{"Bucket":"{form:Bucket}"}"#)],
            name: Text::at(""),
            key: "Bucket",
        }),
        delete: Some(Delete {
            verb: "Delete",
            step: Step::new("DeleteBucket", r#"{"Bucket":"{id}"}"#),
            words: "Only an empty bucket can be deleted: S3 refuses one that still holds objects (BucketNotEmpty).",
        }),
        reveal: None,
    }],
};
