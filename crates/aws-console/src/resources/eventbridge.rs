//! EventBridge: event buses, rules (each with its targets), and archives.
//!
//! A rule is named in the console's addresses as its ARN names it: `on-order` on the default bus, `orders/on-order` on the bus
//! `orders` (`{name:head}` and `{name:last}` in the bodies).

use super::Format::{Bytes, Json, Mono, Number, Tail, Text as Plain, Time};
use super::{
    Create, Delete, Descriptor, Field, Id, Input, InputKind, Kind, LinkTo, List, Step, Sub, Text,
    col, field, input, or,
};

pub(super) static EVENTBRIDGE: Descriptor = Descriptor {
    service: "eventbridge",
    target: "events",
    kinds: &[RULES, BUSES, ARCHIVES],
};

/// A rule's name in an address, from its ARN.
const RULE: Text = Text::after("Arn", ":rule/");

const RULES: Kind = Kind {
    id: "rules",
    title: "Rules",
    one: "rule",
    about: "Rules on the default bus: an event pattern or a schedule, and the targets it sends to. A custom bus's rules are on its page.",
    members: &["Name", "Rule"],
    list: List {
        step: Step::new("ListRules", r#"{"Limit":100,"NextToken":"{token}"}"#),
        items: "Rules",
        item_as: "Name",
        token_out: "NextToken",
        id: "Name",
        name: RULE,
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("State", "State", Plain),
        col("Schedule", "ScheduleExpression", Mono),
        col("Description", "Description", Plain),
    ],
    id_of: Id::Name,
    detail: &[Step::new(
        "DescribeRule",
        r#"{"Name":"{name:last}","EventBusName":"{name:head}"}"#,
    )],
    fields: &[
        field("ARN", "Arn", Mono),
        field("State", "State", Plain),
        Field {
            link: &[LinkTo {
                service: "eventbridge",
                kind: "buses",
                when: None,
            }],
            ..field("Event bus", "EventBusName", Mono)
        },
        field("Description", "Description", Plain),
        field("Schedule", "ScheduleExpression", Mono),
        field("Role", "RoleArn", Mono),
        field("Managed by", "ManagedBy", Mono),
        field("Event pattern", "EventPattern", Json),
    ],
    subs: &[Sub {
        title: "Targets",
        step: Step::new(
            "ListTargetsByRule",
            r#"{"Rule":"{name:last}","EventBusName":"{name:head}"}"#,
        ),
        items: "Targets",
        item_as: "Id",
        columns: &[
            col("Target", "Arn", Tail),
            col("ID", "Id", Mono),
            col("ARN", "Arn", Mono),
            col("Input", "Input", Mono),
            col("Dead-letter queue", "DeadLetterConfig.Arn", Tail),
        ],
        link: None,
        more: "NextToken",
        add: None,
    }],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "on-order-placed",
                ..input(
                    "Name",
                    "Name",
                    "Up to 64 letters, digits and .-_.",
                    InputKind::Text,
                )
            },
            Input {
                placeholder: "orders",
                ..input(
                    "EventBusName",
                    "Event bus",
                    "Default: the default bus.",
                    InputKind::Text,
                )
            },
            Input {
                placeholder: r#"{"source": ["larder.orders"]}"#,
                ..input(
                    "EventPattern",
                    "Event pattern",
                    "The events it matches, as JSON. A rule has a pattern, a schedule, or both.",
                    InputKind::Textarea,
                )
            },
            Input {
                placeholder: "rate(5 minutes)",
                ..input(
                    "ScheduleExpression",
                    "Schedule",
                    "rate(…) or cron(…), on the default bus only.",
                    InputKind::Text,
                )
            },
            Input {
                required: true,
                value: "ENABLED",
                ..input(
                    "State",
                    "State",
                    "A disabled rule matches nothing.",
                    InputKind::Select(&["ENABLED", "DISABLED"]),
                )
            },
            input("Description", "Description", "", InputKind::Text),
        ],
        steps: &[Step::new(
            "PutRule",
            r#"{"Name":"{form:Name}","EventBusName":"{form:EventBusName}","EventPattern":"{form:EventPattern}","ScheduleExpression":"{form:ScheduleExpression}","State":"{form:State}","Description":"{form:Description}"}"#,
        )],
        name: Text::after("RuleArn", ":rule/"),
        key: "Name",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new(
            "DeleteRule",
            r#"{"Name":"{name:last}","EventBusName":"{name:head}"}"#,
        ),
        words: "EventBridge refuses while the rule still has targets: remove them first (aws events remove-targets). Events stop matching it at once.",
    }),
    reveal: None,
};

const BUSES: Kind = Kind {
    id: "buses",
    title: "Event buses",
    one: "event bus",
    about: "Where events are put. Every account has the default bus; rules on a bus send its events on.",
    members: &["EventBusName"],
    list: List {
        step: Step::new("ListEventBuses", r#"{"Limit":100,"NextToken":"{token}"}"#),
        items: "EventBuses",
        item_as: "Name",
        token_out: "NextToken",
        id: "Name",
        name: Text::at("Name"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Description", "Description", Plain),
        col("Created", "CreationTime", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new("DescribeEventBus", r#"{"Name":"{id}"}"#)],
    fields: &[
        field("ARN", "Arn", Mono),
        field("Description", "Description", Plain),
        or("KMS key", "KmsKeyIdentifier", Mono, "AWS owned"),
        field("Dead-letter queue", "DeadLetterConfig.Arn", Mono),
        field("Created", "CreationTime", Time),
        field("Last modified", "LastModifiedTime", Time),
        or("Resource policy", "Policy", Json, "None"),
    ],
    subs: &[Sub {
        title: "Rules",
        step: Step::new("ListRules", r#"{"EventBusName":"{id}","Limit":100}"#),
        items: "Rules",
        item_as: "Name",
        columns: &[
            col("Rule", "Name", Mono),
            col("State", "State", Plain),
            col("Schedule", "ScheduleExpression", Mono),
        ],
        link: Some(("eventbridge", "rules", RULE)),
        more: "NextToken",
        add: None,
    }],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "orders",
                ..input(
                    "Name",
                    "Name",
                    "Up to 256 letters, digits and .-_; not default.",
                    InputKind::Text,
                )
            },
            input("Description", "Description", "", InputKind::Text),
        ],
        steps: &[Step::new(
            "CreateEventBus",
            r#"{"Name":"{form:Name}","Description":"{form:Description}"}"#,
        )],
        name: Text::at(""),
        key: "Name",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteEventBus", r#"{"Name":"{id}"}"#),
        words: "Deleting removes the bus and its rules. The default bus cannot be deleted.",
    }),
    reveal: None,
};

const ARCHIVES: Kind = Kind {
    id: "archives",
    title: "Archives",
    one: "archive",
    about: "Events a bus kept, to replay later.",
    members: &["ArchiveName"],
    list: List {
        step: Step::new("ListArchives", r#"{"Limit":100,"NextToken":"{token}"}"#),
        items: "Archives",
        item_as: "ArchiveName",
        token_out: "NextToken",
        id: "ArchiveName",
        name: Text::at("ArchiveName"),
        label: None,
        split: None,
    },
    elsewhere: "An archive is made for a bus with its event pattern: use aws events create-archive.",
    each: &[],
    columns: &[
        col("Source", "EventSourceArn", Tail),
        col("State", "State", Plain),
        col("Events", "EventCount", Number),
        col("Size", "SizeBytes", Bytes),
        col("Retention (days)", "RetentionDays", Number),
        col("Created", "CreationTime", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new("DescribeArchive", r#"{"ArchiveName":"{id}"}"#)],
    fields: &[
        field("ARN", "ArchiveArn", Mono),
        Field {
            link: &[LinkTo {
                service: "eventbridge",
                kind: "buses",
                when: None,
            }],
            ..field("Source bus", "EventSourceArn", Tail)
        },
        field("Description", "Description", Plain),
        field("State", "State", Plain),
        field("Why", "StateReason", Plain),
        field("Events", "EventCount", Number),
        field("Size", "SizeBytes", Bytes),
        or("Retention (days)", "RetentionDays", Plain, "Forever"),
        field("Created", "CreationTime", Time),
        or("Event pattern", "EventPattern", Json, "Every event"),
    ],
    subs: &[],
    create: None,
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteArchive", r#"{"ArchiveName":"{id}"}"#),
        words: "Deleting removes the archive and the events in it; they can no longer be replayed.",
    }),
    reveal: None,
};
