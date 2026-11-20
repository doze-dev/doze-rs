//! SQS: queues.

use super::Format::{Bytes, Flag, Json, Mono, Number, Seconds, Tail, Time};
use super::{
    Create, Deep, Delete, Descriptor, Field, Id, Input, InputKind, Kind, LinkTo, List, Step, Sub,
    Text, col, field, input, op,
};

const ATTRIBUTES: Step = Step::new(
    "GetQueueAttributes",
    r#"{"QueueUrl":"{id}","AttributeNames":["All"]}"#,
);

/// What every message call names: this queue.
const QUEUE: &str = r#"{"QueueUrl":"{id}"}"#;
/// What a redrive names: this queue as the dead-letter queue the messages leave.
const SOURCE: &str = r#"{"SourceArn":"arn:aws:sqs:{region}:{account}:{name}"}"#;

pub(super) static SQS: Descriptor = Descriptor {
    service: "sqs",
    target: "sqs",
    kinds: &[Kind {
        id: "queues",
        title: "Queues",
        one: "queue",
        about: "Standard and FIFO queues. Choose a name to open it.",
        members: &["QueueUrl", "QueueName"],
        list: List {
            step: Step::new("ListQueues", r#"{"MaxResults":1000,"NextToken":"{token}"}"#),
            items: "QueueUrls",
            item_as: "QueueUrl",
            token_out: "NextToken",
            id: "QueueUrl",
            name: Text::tail("QueueUrl"),
            label: None,
            split: None,
        },
        elsewhere: "",
        each: &[ATTRIBUTES],
        columns: &[
            col("Type", "Attributes.FifoQueue", Flag("FIFO", "Standard")),
            col(
                "Messages available",
                "Attributes.ApproximateNumberOfMessages",
                Number,
            ),
            col(
                "In flight",
                "Attributes.ApproximateNumberOfMessagesNotVisible",
                Number,
            ),
            col(
                "Dead-letter queue",
                "Attributes.RedrivePolicy.deadLetterTargetArn",
                Tail,
            ),
            col("Created", "Attributes.CreatedTimestamp", Time),
        ],
        id_of: Id::Call(
            Step::new("GetQueueUrl", r#"{"QueueName":"{name}"}"#),
            "QueueUrl",
        ),
        detail: &[ATTRIBUTES],
        fields: &[
            field("URL", "$id", Mono),
            field("ARN", "Attributes.QueueArn", Mono),
            field("Type", "Attributes.FifoQueue", Flag("FIFO", "Standard")),
            field(
                "Messages available",
                "Attributes.ApproximateNumberOfMessages",
                Number,
            ),
            field(
                "Messages in flight",
                "Attributes.ApproximateNumberOfMessagesNotVisible",
                Number,
            ),
            field(
                "Messages delayed",
                "Attributes.ApproximateNumberOfMessagesDelayed",
                Number,
            ),
            Field {
                link: &[LinkTo {
                    service: "sqs",
                    kind: "queues",
                    when: None,
                }],
                ..field(
                    "Dead-letter queue",
                    "Attributes.RedrivePolicy.deadLetterTargetArn",
                    Tail,
                )
            },
            field(
                "Maximum receives",
                "Attributes.RedrivePolicy.maxReceiveCount",
                Number,
            ),
            field(
                "Visibility timeout",
                "Attributes.VisibilityTimeout",
                Seconds,
            ),
            field(
                "Message retention period",
                "Attributes.MessageRetentionPeriod",
                Seconds,
            ),
            field("Delivery delay", "Attributes.DelaySeconds", Seconds),
            field(
                "Receive message wait time",
                "Attributes.ReceiveMessageWaitTimeSeconds",
                Seconds,
            ),
            field(
                "Maximum message size",
                "Attributes.MaximumMessageSize",
                Bytes,
            ),
            field(
                "Content-based deduplication",
                "Attributes.ContentBasedDeduplication",
                Flag("Enabled", "Disabled"),
            ),
            field(
                "SQS-managed encryption",
                "Attributes.SqsManagedSseEnabled",
                Flag("Enabled", "Disabled"),
            ),
            Field {
                link: &[LinkTo {
                    service: "kms",
                    kind: "keys",
                    when: None,
                }],
                ..field("KMS key", "Attributes.KmsMasterKeyId", Mono)
            },
            field("Created", "Attributes.CreatedTimestamp", Time),
            field("Last modified", "Attributes.LastModifiedTimestamp", Time),
            field("Access policy", "Attributes.Policy", Json),
            field(
                "Redrive allow policy",
                "Attributes.RedriveAllowPolicy",
                Json,
            ),
        ],
        subs: &[
            Sub {
                title: "Lambda triggers",
                step: Step::new(
                    "ListEventSourceMappings",
                    r#"{"EventSourceArn":"{out:Attributes.QueueArn}"}"#,
                )
                .of("lambda")
                .optional(),
                items: "EventSourceMappings",
                item_as: "UUID",
                columns: &[
                    col("Function", "FunctionArn", Tail),
                    col("State", "State", super::Format::Text),
                    col("Batch size", "BatchSize", Number),
                    col("Mapping", "UUID", Mono),
                ],
                link: Some(("lambda", "functions", Text::tail("FunctionArn"))),
                more: "NextMarker",
                add: None,
            },
            Sub {
                title: "Queues that use it as their dead-letter queue",
                step: Step::new("ListDeadLetterSourceQueues", r#"{"QueueUrl":"{id}"}"#),
                items: "queueUrls",
                item_as: "QueueUrl",
                columns: &[col("Queue", "QueueUrl", Tail)],
                link: Some(("sqs", "queues", Text::tail("QueueUrl"))),
                more: "NextToken",
                add: None,
            },
        ],
        create: Some(Create {
            inputs: &[
                Input {
                    required: true,
                    placeholder: "orders",
                    ..input(
                        "QueueName",
                        "Name",
                        "Up to 80 letters, digits, hyphens and underscores. A FIFO queue's name ends in .fifo.",
                        InputKind::Text,
                    )
                },
                input(
                    "FifoQueue",
                    "FIFO",
                    "Messages are delivered once, in the order sent. Set at creation only.",
                    InputKind::Toggle,
                ),
                Input {
                    placeholder: "30",
                    ..input(
                        "VisibilityTimeout",
                        "Visibility timeout (seconds)",
                        "How long a received message is hidden from other consumers, 0 to 43,200. Default: 30 seconds.",
                        InputKind::Number,
                    )
                },
                Input {
                    placeholder: "345600",
                    ..input(
                        "MessageRetentionPeriod",
                        "Message retention period (seconds)",
                        "60 to 1,209,600 (14 days). Default: 4 days.",
                        InputKind::Number,
                    )
                },
                Input {
                    placeholder: "0",
                    ..input(
                        "DelaySeconds",
                        "Delivery delay (seconds)",
                        "0 to 900. Default: 0 seconds.",
                        InputKind::Number,
                    )
                },
            ],
            steps: &[Step::new(
                "CreateQueue",
                r#"{"QueueName":"{form:QueueName}","Attributes":{"FifoQueue":"{form:FifoQueue}","VisibilityTimeout":"{form:VisibilityTimeout}","MessageRetentionPeriod":"{form:MessageRetentionPeriod}","DelaySeconds":"{form:DelaySeconds}"}}"#,
            )],
            name: Text::tail("QueueUrl"),
            key: "QueueName",
        }),
        delete: Some(Delete {
            verb: "Delete",
            step: Step::new("DeleteQueue", r#"{"QueueUrl":"{id}"}"#),
            words: "Deleting removes the queue and every message in it at once.",
        }),
        reveal: None,
        deep: &[
            Deep {
                id: "messages",
                title: "Messages",
                ops: &[
                    op("SendMessage", QUEUE),
                    // A peek: the messages stay visible to the queue's consumers, and the console never waits.
                    op(
                        "ReceiveMessage",
                        r#"{"QueueUrl":"{id}","VisibilityTimeout":0,"WaitTimeSeconds":0}"#,
                    ),
                    op("DeleteMessage", QUEUE),
                    op("PurgeQueue", QUEUE),
                ],
            },
            // Shown when other queues use this one as their dead-letter queue.
            Deep {
                id: "redrive",
                title: "Dead-letter redrive",
                ops: &[
                    op("StartMessageMoveTask", SOURCE),
                    op("ListMessageMoveTasks", SOURCE),
                    op("CancelMessageMoveTask", "{}"),
                ],
            },
            Deep {
                id: "settings",
                title: "Settings",
                ops: &[op("SetQueueAttributes", QUEUE)],
            },
        ],
    }],
};
