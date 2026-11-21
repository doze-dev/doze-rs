//! S3: buckets, and a bucket's objects (the first page) on its page.

use super::Format::{Json, Mono, Text as Plain, Time};
use super::{
    Batch, Blocker, Clear, Create, Deep, Delete, Descriptor, Id, Input, InputKind, Kind, List,
    Shape, Step, Tags, Text, Write, col, field, input, op, ops, or,
};

/// What every object call names: this bucket.
const BUCKET: &str = r#"{"Bucket":"{id}"}"#;

/// What a bucket holds, listed as versions (a bucket with no versioning lists its objects as versions `null`).
const VERSIONS: Blocker = Blocker {
    title: "Objects",
    list: Step::new("ListObjectVersions", r#"{"Bucket":"{id}","MaxKeys":1000}"#),
    items: "Versions",
    item_as: "Key",
    name: Text::at("Key"),
    link: None,
    except: None,
    blocks: true,
    clear: Some(Clear::Batch(Batch {
        step: Step::new(
            "DeleteObjects",
            r#"{"Bucket":"{id}","Delete":{"Quiet":true}}"#,
        ),
        at: "Delete.Objects",
        pick: &["Key", "VersionId"],
        size: 1000,
        flat: false,
        said: "Deleted {count} objects",
    })),
};

/// A versioned bucket's delete markers.
const MARKERS: Blocker = Blocker {
    title: "Delete markers",
    items: "DeleteMarkers",
    clear: Some(Clear::Batch(Batch {
        step: Step::new(
            "DeleteObjects",
            r#"{"Bucket":"{id}","Delete":{"Quiet":true}}"#,
        ),
        at: "Delete.Objects",
        pick: &["Key", "VersionId"],
        size: 1000,
        flat: false,
        said: "Deleted {count} delete markers",
    })),
    ..VERSIONS
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
        // The objects are the Objects tab, not a flat list under the details.
        subs: &[],
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
        // Its objects; an upload (a folder too: an empty object whose key ends in `/`) and a download are the console's own
        // addresses (`api/deep.rs`), each one PutObject or GetObject.
        deep: &[Deep {
            id: "objects",
            title: "Objects",
            ops: &[
                op("ListObjectsV2", BUCKET),
                op("ListObjectVersions", BUCKET),
                op("HeadObject", BUCKET),
                op("GetObjectTagging", BUCKET),
                op("DeleteObject", BUCKET),
                op("DeleteObjects", BUCKET),
            ],
        }],
        ops: &[ops::free("CreateBucket")],
        // S3 writes a bucket's tags whole: a change reads the set, changes it and puts it back (an empty set is a delete).
        tags: Some(
            Tags::new(
                Step::new("GetBucketTagging", BUCKET),
                "TagSet",
                Shape::KEY_VALUE,
                Write::Replace {
                    put: op("PutBucketTagging", BUCKET),
                    at: "Tagging.TagSet",
                    clear: Some(op("DeleteBucketTagging", BUCKET)),
                    carry: None,
                },
            )
            .none("NoSuchTagSet"),
        ),
        // S3 refuses to delete a bucket that holds anything, versions and delete markers included; emptying clears them.
        blockers: &[VERSIONS, MARKERS],
        ..Kind::BARE
    }],
};
