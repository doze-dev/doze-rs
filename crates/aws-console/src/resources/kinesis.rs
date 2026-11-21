//! Kinesis Data Streams: streams, and a stream's shards on its page.

use super::Format::{Mono, Number, Text as Plain, Time};
use super::{
    Create, Deep, Delete, Descriptor, Field, Id, Input, InputKind, Kind, LinkTo, List, Shape, Step,
    Sub, Tags, Text, Write, col, field, input, op, ops, says, state,
};

/// What a tag call names: this stream.
const STREAM: &str = r#"{"StreamName":"{id}"}"#;

/// What a call on the stream's settings names: this stream, by its name.
const NAMED: &str = r#"{"StreamName":"{id}"}"#;
/// What a call that names a stream by its ARN takes: this stream.
const ARN: &str = r#"{"StreamARN":"arn:aws:kinesis:{region}:{account}:stream/{id}"}"#;
/// What a resource policy call names.
const POLICY: &str = r#"{"ResourceARN":"arn:aws:kinesis:{region}:{account}:stream/{id}"}"#;

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
                "Created",
                "StreamDescriptionSummary.StreamCreationTimestamp",
                Time,
            ),
            // Not on the board's list: what its sentence counts (the consumers a stream has).
            col(
                "Consumers",
                "StreamDescriptionSummary.ConsumerCount",
                Number,
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
            // The console deletes the consumers with the stream (its dialog says which): Kinesis otherwise refuses.
            step: Step::new(
                "DeleteStream",
                r#"{"StreamName":"{id}","EnforceConsumerDeletion":true}"#,
            ),
            words: "Deleting removes the stream, every record in it and its registered consumers.",
        }),
        reveal: None,
        // What the pages of a stream do to it: each setting its own call, the Data Viewer's reads, the sharing policy.
        deep: &[
            Deep {
                id: "configuration",
                title: "Configuration",
                ops: &[
                    op("UpdateStreamMode", ARN),
                    op("UpdateShardCount", NAMED),
                    op("IncreaseStreamRetentionPeriod", NAMED),
                    op("DecreaseStreamRetentionPeriod", NAMED),
                    op("StartStreamEncryption", NAMED),
                    op("StopStreamEncryption", NAMED),
                    op("EnableEnhancedMonitoring", NAMED),
                    op("DisableEnhancedMonitoring", NAMED),
                    op("DescribeStreamSummary", NAMED),
                    op("ListStreamConsumers", ARN),
                ],
            },
            Deep {
                id: "viewer",
                title: "Data Viewer",
                ops: &[
                    op("ListShards", NAMED),
                    op("GetShardIterator", NAMED),
                    op("GetRecords", "{}"),
                ],
            },
            Deep {
                id: "sharing",
                title: "Data stream sharing",
                ops: &[
                    op("GetResourcePolicy", POLICY),
                    op("PutResourcePolicy", POLICY),
                    op("DeleteResourcePolicy", POLICY),
                ],
            },
        ],
        // The create with its form (the tags), and the consumers a stream's sentence and delete dialog name.
        ops: &[ops::free("CreateStream"), ops::free("ListStreamConsumers")],
        page: says::kinesis::STREAM,
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
