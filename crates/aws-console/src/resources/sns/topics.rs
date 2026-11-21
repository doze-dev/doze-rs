//! SNS topics: the list, the page, and what the pages do to a topic (settings, tags, Publish, Create).

use super::super::Format::{Flag, Json, Mono, Number, Tail, Text as Plain};
use super::super::{
    Create, Deep, Delete, Field, Id, Input, InputKind, Kind, LinkTo, List, Shape, Step, Sub, Tags,
    Text, Write, col, field, input, op, ops, says,
};

const TOPIC_ARN: &str = "arn:aws:sns:{region}:{account}:{name}";

/// What a topic's own calls name: this topic, by its ARN.
const TOPIC: &str = r#"{"TopicArn":"{id}"}"#;
/// What a tag call names: this topic, by its ARN.
const RESOURCE: &str = r#"{"ResourceArn":"{id}"}"#;

/// The delivery-status attributes of a topic, a protocol's three: its success role, its sample rate and its failure role.
/// Kept by the service and read back (nothing is written to CloudWatch Logs: `crates/aws-sns/src/topic_attrs.rs`).
const fn feedback(label: &'static str, at: &'static str) -> Field {
    field(label, at, Mono)
}

const GET: Step = Step::new("GetTopicAttributes", TOPIC);

pub(super) const TOPICS: Kind = Kind {
    id: "topics",
    title: "Topics",
    one: "topic",
    about: "Standard and FIFO topics. A message published to one goes to each of its subscriptions.",
    members: &["TopicArn", "TargetArn"],
    list: List {
        step: Step::new("ListTopics", r#"{"NextToken":"{token}"}"#),
        items: "Topics",
        item_as: "TopicArn",
        token_out: "NextToken",
        id: "TopicArn",
        name: Text::tail("TopicArn"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[GET],
    columns: &[
        col("Type", "Attributes.FifoTopic", Flag("FIFO", "Standard")),
        col(
            "Confirmed subscriptions",
            "Attributes.SubscriptionsConfirmed",
            Number,
        ),
        col("Pending", "Attributes.SubscriptionsPending", Number),
        col("Display name", "Attributes.DisplayName", Plain),
    ],
    id_of: Id::Template(TOPIC_ARN),
    detail: &[GET],
    fields: &[
        field("ARN", "$id", Mono),
        field("Type", "Attributes.FifoTopic", Flag("FIFO", "Standard")),
        field("Display name", "Attributes.DisplayName", Plain),
        field("Owner", "Attributes.Owner", Mono),
        field(
            "Confirmed subscriptions",
            "Attributes.SubscriptionsConfirmed",
            Number,
        ),
        field(
            "Pending subscriptions",
            "Attributes.SubscriptionsPending",
            Number,
        ),
        field(
            "Content-based deduplication",
            "Attributes.ContentBasedDeduplication",
            Flag("Enabled", "Disabled"),
        ),
        field(
            "FIFO throughput scope",
            "Attributes.FifoThroughputScope",
            Plain,
        ),
        field(
            "Maximum message size",
            "Attributes.MaximumMessageSize",
            Number,
        ),
        Field {
            link: &[LinkTo {
                service: "kms",
                kind: "keys",
                when: None,
            }],
            ..field("KMS key", "Attributes.KmsMasterKeyId", Mono)
        },
        field("Active tracing", "Attributes.TracingConfig", Plain),
        field("Archive policy", "Attributes.ArchivePolicy", Json),
        field("Access policy", "Attributes.Policy", Json),
        field(
            "Delivery policy",
            "Attributes.EffectiveDeliveryPolicy",
            Json,
        ),
        field("Own delivery policy", "Attributes.DeliveryPolicy", Json),
        feedback(
            "HTTPSuccessFeedbackRoleArn",
            "Attributes.HTTPSuccessFeedbackRoleArn",
        ),
        feedback(
            "HTTPSuccessFeedbackSampleRate",
            "Attributes.HTTPSuccessFeedbackSampleRate",
        ),
        feedback(
            "HTTPFailureFeedbackRoleArn",
            "Attributes.HTTPFailureFeedbackRoleArn",
        ),
        feedback(
            "LambdaSuccessFeedbackRoleArn",
            "Attributes.LambdaSuccessFeedbackRoleArn",
        ),
        feedback(
            "LambdaSuccessFeedbackSampleRate",
            "Attributes.LambdaSuccessFeedbackSampleRate",
        ),
        feedback(
            "LambdaFailureFeedbackRoleArn",
            "Attributes.LambdaFailureFeedbackRoleArn",
        ),
        feedback(
            "SQSSuccessFeedbackRoleArn",
            "Attributes.SQSSuccessFeedbackRoleArn",
        ),
        feedback(
            "SQSSuccessFeedbackSampleRate",
            "Attributes.SQSSuccessFeedbackSampleRate",
        ),
        feedback(
            "SQSFailureFeedbackRoleArn",
            "Attributes.SQSFailureFeedbackRoleArn",
        ),
    ],
    subs: &[Sub {
        title: "Subscriptions",
        step: Step::new("ListSubscriptionsByTopic", r#"{"TopicArn":"{id}"}"#),
        items: "Subscriptions",
        item_as: "SubscriptionArn",
        columns: &[
            col("Subscription", "SubscriptionArn", Tail),
            col("Protocol", "Protocol", Plain),
            col("Endpoint", "Endpoint", Mono),
        ],
        link: Some(("sns", "subscriptions", Text::at("SubscriptionArn"))),
        more: "NextToken",
        add: Some(("subscriptions", "TopicArn")),
    }],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "order-events",
                ..input(
                    "Name",
                    "Name",
                    "Up to 256 letters, digits, hyphens and underscores. A FIFO topic's name ends in .fifo.",
                    InputKind::Text,
                )
            },
            input(
                "FifoTopic",
                "FIFO",
                "Ordered, deduplicated delivery to FIFO queues. Set at creation only.",
                InputKind::Toggle,
            ),
            input(
                "DisplayName",
                "Display name",
                "Used as the sender of SMS and email messages.",
                InputKind::Text,
            ),
        ],
        steps: &[Step::new(
            "CreateTopic",
            r#"{"Name":"{form:Name}","Attributes":{"FifoTopic":"{form:FifoTopic}","DisplayName":"{form:DisplayName}"}}"#,
        )],
        name: Text::tail("TopicArn"),
        key: "Name",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteTopic", TOPIC),
        words: "Deleting removes the topic and all of its subscriptions. Messages already delivered stay where they went.",
    }),
    reveal: None,
    deep: &[
        // SetTopicAttributes sets one attribute a call: Edit topic makes one for each setting that changed.
        Deep {
            id: "settings",
            title: "Settings",
            ops: &[op("SetTopicAttributes", TOPIC)],
        },
        // The message goes to this topic and no other.
        Deep {
            id: "publish",
            title: "Publish message",
            ops: &[op("Publish", TOPIC)],
        },
    ],
    // CreateTopic with every attribute and its tags; the KMS aliases the encryption field offers.
    ops: &[ops::free("CreateTopic"), ops::free("ListAliases").of("kms")],
    tags: Some(Tags::new(
        Step::new("ListTagsForResource", RESOURCE),
        "Tags",
        Shape::KEY_VALUE,
        Write::Delta {
            add: op("TagResource", RESOURCE),
            at: "Tags",
            remove: op("UntagResource", RESOURCE),
            keys: "TagKeys",
        },
    )),
    page: says::sns::TOPIC,
    ..Kind::BARE
};
