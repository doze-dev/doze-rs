//! SNS subscriptions: the list, the page (a confirmed one by its ARN, one waiting for its endpoint by what tells it apart) and
//! what the pages do to one (its settings, Subscribe, ConfirmSubscription).

use super::super::Format::{Flag, Json, Mono, Tail, Text as Plain};
use super::super::{
    Create, Deep, Delete, Field, Id, Input, InputKind, Kind, LinkTo, List, Pending, Step, Text,
    col, field, input, op, ops, says,
};

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

pub(super) const SUBSCRIPTIONS: Kind = Kind {
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
        field("Topic ARN", "Attributes.TopicArn", Mono),
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
        field("Delivery policy", "Attributes.DeliveryPolicy", Json),
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
        verb: "Delete",
        step: Step::new("Unsubscribe", r#"{"SubscriptionArn":"{id}"}"#),
        words: "The topic stops delivering to this endpoint.",
    }),
    reveal: None,
    deep: &[
        // SetSubscriptionAttributes sets one attribute a call: Edit subscription makes one for each setting that changed.
        Deep {
            id: "settings",
            title: "Settings",
            ops: &[op(
                "SetSubscriptionAttributes",
                r#"{"SubscriptionArn":"{id}"}"#,
            )],
        },
    ],
    // Subscribe with its attributes; ConfirmSubscription with the token of the SubscribeURL the endpoint was sent.
    ops: &[ops::free("Subscribe"), ops::free("ConfirmSubscription")],
    page: says::sns::SUBSCRIPTION,
    ..Kind::BARE
};
