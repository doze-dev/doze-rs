//! S3: buckets, and a bucket's objects (the first page) on its page.

use super::Format::Tail;
use super::Format::{Json, Mono, Text as Plain, Time};
use super::s3_ops::{BUCKET, PAGES};
use super::{
    Batch, Blocker, Clear, Column, Create, Delete, Descriptor, Id, Input, InputKind, Kind, List,
    Shape, Step, Sub, Tags, Text, Write, col, col_or, field, input, op, ops, or, says,
};

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

const RULE: &[Column] = &[
    col("Event name", "Id", Plain),
    col("Events", "Events", Json),
];

/// One of the three lists of what a bucket notifies: its rows are the rules of one destination kind, each linking to the
/// resource it names.
const fn notifies(
    title: &'static str,
    items: &'static str,
    service: &'static str,
    kind: &'static str,
    arn: &'static str,
) -> Sub {
    Sub {
        title,
        step: Step::answered("GetBucketNotificationConfiguration"),
        items,
        item_as: "Id",
        columns: RULE,
        link: Some((service, kind, Text::tail(arn))),
        more: "",
        add: None,
    }
}

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
        // What the line under the list says of the buckets that are versioned or notify something.
        each: &[
            Step::new("GetBucketVersioning", BUCKET).optional(),
            Step::new("GetBucketNotificationConfiguration", BUCKET).optional(),
        ],
        columns: &[
            col("Region", "BucketRegion", Mono),
            col("Created", "CreationDate", Time),
            col_or("Versioning", "Status", Plain, "Disabled"),
            col(
                "Notifies Lambda function",
                "LambdaFunctionConfigurations.0.LambdaFunctionArn",
                Tail,
            ),
            col("Notifies SQS queue", "QueueConfigurations.0.QueueArn", Tail),
            col("Notifies SNS topic", "TopicConfigurations.0.TopicArn", Tail),
            // The first rule's first event (`s3:ObjectCreated:*`).
            col(
                "Notifies on",
                "LambdaFunctionConfigurations.0.Events.0|QueueConfigurations.0.Events.0|TopicConfigurations.0.Events.0",
                Plain,
            ),
        ],
        id_of: Id::Name,
        detail: &[
            // The creation date is the list's: the one bucket whose name begins with this name (its own name sorts first).
            Step::new("ListBuckets", r#"{"Prefix":"{id}","MaxBuckets":10}"#),
            Step::new("GetBucketLocation", BUCKET),
            Step::new("GetBucketVersioning", BUCKET).optional(),
            Step::new("GetBucketNotificationConfiguration", BUCKET).optional(),
            // The top level, for the sentence: how many objects and folders are at it.
            Step::new(
                "ListObjectsV2",
                r#"{"Bucket":"{id}","Delimiter":"/","MaxKeys":1000}"#,
            )
            .optional(),
        ],
        fields: &[
            field("Name", "$id", Mono),
            // AWS's rule: a bucket in us-east-1 answers an empty LocationConstraint.
            or("Region", "LocationConstraint", Mono, "us-east-1"),
            or("Versioning", "Status", Plain, "Disabled"),
            field("Created", "Buckets.0.CreationDate", Time),
        ],
        // What a bucket tells of where it sends events: the strip of what it is wired to (an object created, a Lambda function).
        subs: &[
            notifies(
                "Notifies Lambda function",
                "LambdaFunctionConfigurations",
                "lambda",
                "functions",
                "LambdaFunctionArn",
            ),
            notifies(
                "Notifies SQS queue",
                "QueueConfigurations",
                "sqs",
                "queues",
                "QueueArn",
            ),
            notifies(
                "Notifies SNS topic",
                "TopicConfigurations",
                "sns",
                "topics",
                "TopicArn",
            ),
        ],
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
        // Its pages: the objects, and what is set on the bucket (each page's operations are in `s3_ops`). An upload (a folder too: an
        // empty object whose key ends in `/`), a download and a presigned link are the console's own addresses (`api/deep.rs`,
        // `api/objects.rs`), each one PutObject or GetObject.
        deep: PAGES,
        // A copy is to any bucket (the destination and the source are in the call).
        ops: &[ops::free("CreateBucket"), ops::free("CopyObject")],
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
        page: says::s3::BUCKET,
        ..Kind::BARE
    }],
};
