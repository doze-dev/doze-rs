//! CloudWatch Logs: log groups, with their subscription filters on their page; their streams, search and Live Tail are pages of
//! their own.

use super::Format::{Bytes, Mono, Number, Text as Plain, Time};
use super::{
    Create, Deep, Delete, Descriptor, Id, Input, InputKind, Kind, List, Shape, Step, Sub, Tags,
    Text, Write, col, field, input, op, ops, or, says,
};

/// What a tag call names: this group, by its ARN.
const ARN: &str = r#"{"resourceArn":"arn:aws:logs:{region}:{account}:log-group:{name}"}"#;

/// What a read of its streams names: this group, and no other by `logGroupIdentifier`. By its name as the address gives it, so
/// no `DescribeLogGroups` is made first to find it (Logs itself refuses a group that is not there).
const GROUP: &str = r#"{"logGroupName":"{name}","logGroupIdentifier":null}"#;

/// What a filter or retention call names: this group.
const NAMED: &str = r#"{"logGroupName":"{name}"}"#;

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
            col("Class", "logGroupClass", Plain),
            col("Retention (days)", "retentionInDays", Number),
            col("Metric filters", "metricFilterCount", Number),
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
        subs: &[
            // Its streams, the newest event first: the tab's rows and its count.
            Sub {
                title: "Log streams",
                step: Step::new(
                    "DescribeLogStreams",
                    r#"{"logGroupName":"{id}","orderBy":"LastEventTime","descending":true,"limit":50}"#,
                ),
                items: "logStreams",
                item_as: "logStreamName",
                columns: &[
                    col("Log stream", "logStreamName", Mono),
                    col("Last event time", "lastEventTimestamp", Time),
                ],
                link: None,
                more: "nextToken",
                add: None,
            },
            Sub {
                title: "Metric filters",
                step: Step::new("DescribeMetricFilters", r#"{"logGroupName":"{id}"}"#),
                items: "metricFilters",
                item_as: "filterName",
                columns: &[
                    col("Filter name", "filterName", Mono),
                    col("Filter pattern", "filterPattern", Mono),
                    col("Metric name", "metricTransformations.0.metricName", Mono),
                    col(
                        "Metric namespace",
                        "metricTransformations.0.metricNamespace",
                        Mono,
                    ),
                    col("Metric value", "metricTransformations.0.metricValue", Mono),
                    col("Unit", "metricTransformations.0.unit", Plain),
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
                    // The whole ARN: the page names the kind (Lambda, Kinesis) from it and links the function or stream.
                    col("Destination", "destinationArn", Mono),
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
            Deep {
                id: "retention",
                title: "Retention",
                ops: &[
                    op("PutRetentionPolicy", NAMED),
                    op("DeleteRetentionPolicy", NAMED),
                ],
            },
            Deep {
                id: "metric-filters",
                title: "Metric filters",
                ops: &[
                    op("PutMetricFilter", NAMED),
                    op("DeleteMetricFilter", NAMED),
                ],
            },
            Deep {
                id: "subscription-filters",
                title: "Subscription filters",
                ops: &[
                    op("PutSubscriptionFilter", NAMED),
                    op("DeleteSubscriptionFilter", NAMED),
                ],
            },
            // Read at its own address (`GET …/tail`) as server-sent events.
            Deep {
                id: "tail",
                title: "Live Tail",
                ops: &[op(
                    "StartLiveTail",
                    r#"{"logGroupIdentifiers":["arn:aws:logs:{region}:{account}:log-group:{name}"]}"#,
                )],
            },
        ],
        tags: Some(Tags::new(
            Step::new("ListTagsForResource", ARN),
            "tags",
            Shape::Map,
            Write::Delta {
                add: op("TagResource", ARN),
                at: "tags",
                remove: op("UntagResource", ARN),
                keys: "tagKeys",
            },
        )),
        // The create with its own form (the KMS key, the deletion protection), the test a metric filter's pattern gets, and the
        // account's metric filters (a new filter's namespace says what the others already hold).
        ops: &[
            ops::free("CreateLogGroup"),
            ops::free("TestMetricFilter"),
            ops::free("DescribeMetricFilters"),
        ],
        page: says::logs::GROUP,
        ..Kind::BARE
    }],
};
