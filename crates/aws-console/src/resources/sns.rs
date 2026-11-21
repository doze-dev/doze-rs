//! SNS: topics and subscriptions.

use super::Format::{Flag, Json, Mono, Number, Tail, Text as Plain};
use super::{
    Create, Delete, Descriptor, Field, Id, Input, InputKind, Kind, LinkTo, List, Pending, Shape,
    Step, Sub, Tags, Text, Write, col, field, input, op, ops, says,
};

const TOPIC_ARN: &str = "arn:aws:sns:{region}:{account}:{name}";

/// What a tag call names: this topic, by its ARN.
const RESOURCE: &str = r#"{"ResourceArn":"{id}"}"#;

/// Where a subscription's endpoint is in this console, by its protocol.
const ENDPOINT: &[LinkTo] = &[
    LinkTo {
        service: "sqs",
        kind: "queues",
        when: Some(("Attributes.Protocol", "sqs")),
    },
    LinkTo {
        service: "lambda",
        kind: "functions",
        when: Some(("Attributes.Protocol", "lambda")),
    },
];

const TOPICS: Kind = Kind {
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
    each: &[Step::new("GetTopicAttributes", r#"{"TopicArn":"{id}"}"#)],
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
    detail: &[Step::new("GetTopicAttributes", r#"{"TopicArn":"{id}"}"#)],
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
        Field {
            link: &[LinkTo {
                service: "kms",
                kind: "keys",
                when: None,
            }],
            ..field("KMS key", "Attributes.KmsMasterKeyId", Mono)
        },
        field("Access policy", "Attributes.Policy", Json),
        field(
            "Delivery policy",
            "Attributes.EffectiveDeliveryPolicy",
            Json,
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
        step: Step::new("DeleteTopic", r#"{"TopicArn":"{id}"}"#),
        words: "Deleting removes the topic and all of its subscriptions. Messages already delivered stay where they went.",
    }),
    reveal: None,
    deep: &[],
    ops: &[ops::free("CreateTopic")],
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

/// A subscription not yet confirmed is listed as `PendingConfirmation`, whatever its topic: it is named by its topic, protocol and
/// endpoint, and read from its topic's list.
static PENDING: Pending = Pending {
    id: "SubscriptionArn",
    value: "PendingConfirmation",
    by: &["TopicArn", "Protocol", "Endpoint"],
    step: Step::new(
        "ListSubscriptionsByTopic",
        r#"{"TopicArn":"{out:TopicArn}","NextToken":"{token}"}"#,
    ),
    items: "Subscriptions",
    token_out: "NextToken",
    into: "Attributes",
    flag: "PendingConfirmation",
};

const SUBSCRIPTIONS: Kind = Kind {
    id: "subscriptions",
    title: "Subscriptions",
    one: "subscription",
    about: "Where each topic delivers: a queue, a function, an HTTP endpoint, an email address.",
    members: &["SubscriptionArn"],
    list: List {
        step: Step::new("ListSubscriptions", r#"{"NextToken":"{token}"}"#),
        items: "Subscriptions",
        item_as: "SubscriptionArn",
        token_out: "NextToken",
        id: "SubscriptionArn",
        name: Text::at("SubscriptionArn"),
        label: Some(Text::tail("SubscriptionArn")),
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Topic", "TopicArn", Tail),
        col("Protocol", "Protocol", Plain),
        col("Endpoint", "Endpoint", Mono),
    ],
    id_of: Id::Pending(&PENDING),
    detail: &[Step::new(
        "GetSubscriptionAttributes",
        r#"{"SubscriptionArn":"{id}"}"#,
    )],
    fields: &[
        field("ARN", "$id", Mono),
        Field {
            link: &[LinkTo {
                service: "sns",
                kind: "topics",
                when: None,
            }],
            ..field("Topic", "Attributes.TopicArn", Tail)
        },
        field("Protocol", "Attributes.Protocol", Plain),
        Field {
            link: ENDPOINT,
            ..field("Endpoint", "Attributes.Endpoint", Mono)
        },
        field(
            "Pending confirmation",
            "Attributes.PendingConfirmation",
            Flag("Yes", "No"),
        ),
        field(
            "Raw message delivery",
            "Attributes.RawMessageDelivery",
            Flag("Enabled", "Disabled"),
        ),
        field("Filter policy", "Attributes.FilterPolicy", Json),
        field("Filter policy scope", "Attributes.FilterPolicyScope", Plain),
        field("Redrive policy", "Attributes.RedrivePolicy", Json),
        field("Owner", "Attributes.Owner", Mono),
    ],
    subs: &[],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "arn:aws:sns:us-east-1:000000000000:order-events",
                ..input(
                    "TopicArn",
                    "Topic ARN",
                    "The topic to subscribe to.",
                    InputKind::Text,
                )
            },
            Input {
                required: true,
                value: "sqs",
                ..input(
                    "Protocol",
                    "Protocol",
                    "What the endpoint is.",
                    InputKind::Select(&[
                        "sqs",
                        "lambda",
                        "http",
                        "https",
                        "email",
                        "email-json",
                        "sms",
                        "application",
                        "firehose",
                    ]),
                )
            },
            Input {
                required: true,
                placeholder: "arn:aws:sqs:us-east-1:000000000000:orders",
                ..input(
                    "Endpoint",
                    "Endpoint",
                    "A queue's or function's ARN, a URL, an address or a number, as the protocol takes.",
                    InputKind::Text,
                )
            },
        ],
        steps: &[Step::new(
            "Subscribe",
            r#"{"TopicArn":"{form:TopicArn}","Protocol":"{form:Protocol}","Endpoint":"{form:Endpoint}","ReturnSubscriptionArn":true}"#,
        )],
        name: Text::at("SubscriptionArn"),
        key: "Endpoint",
    }),
    delete: Some(Delete {
        verb: "Unsubscribe",
        step: Step::new("Unsubscribe", r#"{"SubscriptionArn":"{id}"}"#),
        words: "The topic stops delivering to this endpoint.",
    }),
    reveal: None,
    deep: &[],
    ops: &[ops::free("Subscribe")],
    page: says::sns::SUBSCRIPTION,
    ..Kind::BARE
};

pub(super) static SNS: Descriptor = Descriptor {
    service: "sns",
    target: "sns",
    kinds: &[TOPICS, SUBSCRIPTIONS],
};
