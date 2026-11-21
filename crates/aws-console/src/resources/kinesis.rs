//! Kinesis Data Streams: streams, and a stream's shards on its page.

use super::Format::{Mono, Number, Text as Plain, Time};
use super::{
    Create, Delete, Descriptor, Field, Id, Input, InputKind, Kind, LinkTo, List, Shape, Step, Sub,
    Tags, Text, Write, col, field, input, op, state,
};

/// What a tag call names: this stream.
const STREAM: &str = r#"{"StreamName":"{id}"}"#;

const SUMMARY: Step = Step::new("DescribeStreamSummary", r#"{"StreamName":"{id}"}"#);

pub(super) static KINESIS: Descriptor = Descriptor {
    service: "kinesis",
    target: "kinesis",
    kinds: &[Kind {
        id: "streams",
        title: "Streams",
        one: "stream",
        about: "Ordered records split across shards, kept for the retention period and read by many consumers.",
        members: &["StreamName", "StreamARN"],
        list: List {
            step: Step::new("ListStreams", r#"{"Limit":100,"NextToken":"{token}"}"#),
            items: "StreamNames",
            item_as: "StreamName",
            token_out: "NextToken",
            id: "StreamName",
            name: Text::at("StreamName"),
            label: None,
            split: None,
        },
        elsewhere: "",
        each: &[SUMMARY],
        columns: &[
            state("Status", "StreamDescriptionSummary.StreamStatus", Plain),
            col(
                "Capacity mode",
                "StreamDescriptionSummary.StreamModeDetails.StreamMode",
                Plain,
            ),
            col(
                "Open shards",
                "StreamDescriptionSummary.OpenShardCount",
                Number,
            ),
            col(
                "Retention (hours)",
                "StreamDescriptionSummary.RetentionPeriodHours",
                Number,
            ),
            col(
                "Created",
                "StreamDescriptionSummary.StreamCreationTimestamp",
                Time,
            ),
        ],
        id_of: Id::Name,
        detail: &[SUMMARY],
        fields: &[
            field("ARN", "StreamDescriptionSummary.StreamARN", Mono),
            field("Status", "StreamDescriptionSummary.StreamStatus", Plain),
            field(
                "Capacity mode",
                "StreamDescriptionSummary.StreamModeDetails.StreamMode",
                Plain,
            ),
            field(
                "Open shards",
                "StreamDescriptionSummary.OpenShardCount",
                Number,
            ),
            field(
                "Consumers",
                "StreamDescriptionSummary.ConsumerCount",
                Number,
            ),
            field(
                "Retention (hours)",
                "StreamDescriptionSummary.RetentionPeriodHours",
                Number,
            ),
            field(
                "Encryption",
                "StreamDescriptionSummary.EncryptionType",
                Plain,
            ),
            Field {
                link: &[LinkTo {
                    service: "kms",
                    kind: "keys",
                    when: None,
                }],
                ..field("KMS key", "StreamDescriptionSummary.KeyId", Mono)
            },
            field(
                "Created",
                "StreamDescriptionSummary.StreamCreationTimestamp",
                Time,
            ),
        ],
        subs: &[Sub {
            title: "Shards",
            step: Step::new("ListShards", r#"{"StreamName":"{id}","MaxResults":100}"#),
            items: "Shards",
            item_as: "ShardId",
            columns: &[
                col("Shard", "ShardId", Mono),
                col("Parent", "ParentShardId", Mono),
                col("Hash key from", "HashKeyRange.StartingHashKey", Mono),
                col("Hash key to", "HashKeyRange.EndingHashKey", Mono),
                col(
                    "Closed at sequence",
                    "SequenceNumberRange.EndingSequenceNumber",
                    Mono,
                ),
            ],
            link: None,
            more: "NextToken",
            add: None,
        }],
        create: Some(Create {
            inputs: &[
                Input {
                    required: true,
                    placeholder: "clickstream",
                    ..input(
                        "StreamName",
                        "Name",
                        "Up to 128 letters, digits, hyphens, underscores and dots.",
                        InputKind::Text,
                    )
                },
                Input {
                    required: true,
                    value: "ON_DEMAND",
                    ..input(
                        "StreamMode",
                        "Capacity mode",
                        "On demand sizes itself; provisioned has the shards you ask for.",
                        InputKind::Select(&["ON_DEMAND", "PROVISIONED"]),
                    )
                },
                Input {
                    placeholder: "2",
                    ..input(
                        "ShardCount",
                        "Shards",
                        "Provisioned streams only, where Kinesis needs it.",
                        InputKind::Number,
                    )
                },
            ],
            steps: &[Step::new(
                "CreateStream",
                r#"{"StreamName":"{form:StreamName}","StreamModeDetails":{"StreamMode":"{form:StreamMode}"},"ShardCount":"{num:ShardCount}"}"#,
            )],
            name: Text::at(""),
            key: "StreamName",
        }),
        delete: Some(Delete {
            verb: "Delete",
            step: Step::new("DeleteStream", r#"{"StreamName":"{id}"}"#),
            words: "Deleting removes the stream and every record in it. Kinesis refuses while it has registered consumers.",
        }),
        reveal: None,
        deep: &[],
        // Kinesis answers a stream's tags as pairs and takes them as a map.
        tags: Some(
            Tags::new(
                Step::new("ListTagsForStream", STREAM),
                "Tags",
                Shape::KEY_VALUE,
                Write::Delta {
                    add: op("AddTagsToStream", STREAM),
                    at: "Tags",
                    remove: op("RemoveTagsFromStream", STREAM),
                    keys: "TagKeys",
                },
            )
            .sent(Shape::Map),
        ),
        ..Kind::BARE
    }],
};
