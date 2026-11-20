//! CloudWatch Logs: log groups, and a group's streams (the most recently written first) and filters on its page.

use super::Format::{Bytes, Mono, Number, Tail, Text as Plain, Time};
use super::{
    Create, Delete, Descriptor, Id, Input, InputKind, Kind, List, Step, Sub, Text, col, field,
    input, or,
};

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
        detail: &[Step::new(
            "DescribeLogStreams",
            r#"{"logGroupName":"{id}","orderBy":"LastEventTime","descending":true,"limit":50}"#,
        )],
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
        subs: &[
            Sub {
                title: "Log streams",
                step: Step::answered("DescribeLogStreams"),
                items: "logStreams",
                item_as: "logStreamName",
                columns: &[
                    col("Stream", "logStreamName", Mono),
                    col("Last event", "lastEventTimestamp", Time),
                    col("First event", "firstEventTimestamp", Time),
                    col("Created", "creationTime", Time),
                ],
                link: None,
                more: "nextToken",
                add: None,
            },
            Sub {
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
            },
        ],
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
        deep: &[],
    }],
};
