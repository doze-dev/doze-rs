//! CloudWatch Logs: log groups, with their subscription filters on their page; their streams, search and Live Tail are pages of
//! their own.

use super::Format::{Bytes, Mono, Number, Tail, Text as Plain, Time};
use super::{
    Create, Deep, Delete, Descriptor, Id, Input, InputKind, Kind, List, Step, Sub, Text, col,
    field, input, op, or,
};

/// What a read of its streams names: this group, and no other by `logGroupIdentifier`.
const GROUP: &str = r#"{"logGroupName":"{id}","logGroupIdentifier":null}"#;

pub(super) static LOGS: Descriptor = Descriptor {
    service: "logs",
    target: "logs",
    kinds: &[Kind {
        id: "groups",
        title: "Log groups",
        one: "log group",
        about: "Where logs are written, a stream per source (a function's instance, a task). Retention says how long they stay.",
        members: &["logGroupName", "logGroupIdentifier"],
        list: List {
            step: Step::new("DescribeLogGroups", r#"{"limit":50,"nextToken":"{token}"}"#),
            items: "logGroups",
            item_as: "logGroupName",
            token_out: "nextToken",
            id: "logGroupName",
            name: Text::at("logGroupName"),
            label: None,
            split: None,
        },
        elsewhere: "",
        each: &[],
        columns: &[
            col("Retention (days)", "retentionInDays", Number),
            col("Stored", "storedBytes", Bytes),
            col("Class", "logGroupClass", Plain),
            col("Created", "creationTime", Time),
        ],
        // Logs has no call that reads one group: the groups whose name begins with it are asked for, and the one of that name
        // read out (none is a refusal).
        id_of: Id::Call(
            Step::new(
                "DescribeLogGroups",
                r#"{"logGroupNamePrefix":"{name}","limit":50}"#,
            ),
            "logGroups.@logGroupName.logGroupName",
        ),
        // What it is comes from finding it (above); its streams are a page of their own.
        detail: &[],
        fields: &[
            field("ARN", "logGroups.@logGroupName.logGroupArn", Mono),
            or(
                "Retention (days)",
                "logGroups.@logGroupName.retentionInDays",
                Plain,
                "Never expire",
            ),
            field("Stored", "logGroups.@logGroupName.storedBytes", Bytes),
            field("Class", "logGroups.@logGroupName.logGroupClass", Plain),
            field(
                "Metric filters",
                "logGroups.@logGroupName.metricFilterCount",
                Number,
            ),
            or("KMS key", "logGroups.@logGroupName.kmsKeyId", Mono, "None"),
            field("Created", "logGroups.@logGroupName.creationTime", Time),
        ],
        subs: &[Sub {
            title: "Subscription filters",
            step: Step::new("DescribeSubscriptionFilters", r#"{"logGroupName":"{id}"}"#),
            items: "subscriptionFilters",
            item_as: "filterName",
            columns: &[
                col("Filter", "filterName", Mono),
                col("Pattern", "filterPattern", Mono),
                col("Destination", "destinationArn", Tail),
            ],
            link: None,
            more: "nextToken",
            add: None,
        }],
        create: Some(Create {
            inputs: &[
                Input {
                    required: true,
                    placeholder: "/larder/api",
                    ..input(
                        "logGroupName",
                        "Name",
                        "Up to 512 letters, digits and _-/.#. Lambda writes to /aws/lambda/<function>.",
                        InputKind::Text,
                    )
                },
                Input {
                    required: true,
                    value: "STANDARD",
                    ..input(
                        "logGroupClass",
                        "Class",
                        "Infrequent access keeps fewer features for less. Set at creation only.",
                        InputKind::Select(&["STANDARD", "INFREQUENT_ACCESS"]),
                    )
                },
            ],
            steps: &[Step::new(
                "CreateLogGroup",
                r#"{"logGroupName":"{form:logGroupName}","logGroupClass":"{form:logGroupClass}"}"#,
            )],
            name: Text::at(""),
            key: "logGroupName",
        }),
        delete: Some(Delete {
            verb: "Delete",
            step: Step::new("DeleteLogGroup", r#"{"logGroupName":"{id}"}"#),
            words: "Deleting removes the group, its streams and every event in them.",
        }),
        reveal: None,
        // Its streams are a page of their own, not a list on its details; a stream's viewer reads through this group.
        deep: &[
            Deep {
                id: "streams",
                title: "Log streams",
                ops: &[op("DescribeLogStreams", GROUP), op("GetLogEvents", GROUP)],
            },
            Deep {
                id: "search",
                title: "Search",
                ops: &[op("FilterLogEvents", GROUP)],
            },
            // Read at its own address (`GET …/tail`) as server-sent events.
            Deep {
                id: "tail",
                title: "Live Tail",
                ops: &[op(
                    "StartLiveTail",
                    r#"{"logGroupIdentifiers":["arn:aws:logs:{region}:{account}:log-group:{id}"]}"#,
                )],
            },
        ],
    }],
};
