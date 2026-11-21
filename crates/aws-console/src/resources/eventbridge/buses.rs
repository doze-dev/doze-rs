//! EventBridge event buses: the list, a bus's page (its rules and archives), and the create with its archive, the events put on it.

use super::super::Format::{Bytes, Json, Mono, Number, Text as Plain, Time};
use super::super::eventbridge_blockers::TARGETS_OF_RULE;
use super::super::{
    Blocker, Clear, Create, Delete, Id, Input, InputKind, Kind, List, Shape, Step, Sub, Tags, Text,
    Write, col, field, input, op, ops, or, says,
};
use super::BUS_ARN;

pub(super) const BUSES: Kind = Kind {
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
        col("Last modified", "LastModifiedTime", Time),
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
    subs: &[
        Sub {
            title: "Rules",
            step: Step::new("ListRules", r#"{"EventBusName":"{id}","Limit":100}"#),
            items: "Rules",
            item_as: "Name",
            columns: &[
                col("Rule", "Name", Mono),
                col("State", "State", Plain),
                col("Schedule", "ScheduleExpression", Mono),
            ],
            link: Some(("eventbridge", "rules", super::rules::RULE_NAME)),
            more: "NextToken",
            add: None,
        },
        Sub {
            title: "Archives",
            step: Step::new(
                "ListArchives",
                r#"{"EventSourceArn":"arn:aws:events:{region}:{account}:event-bus/{id}","Limit":100}"#,
            ),
            items: "Archives",
            item_as: "ArchiveName",
            columns: &[
                col("Name", "ArchiveName", Mono),
                col("State", "State", Plain),
                super::retention(),
                col("Events", "EventCount", Number),
                col("Size", "SizeBytes", Bytes),
            ],
            link: Some(("eventbridge", "archives", Text::at("ArchiveName"))),
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
    deep: &[],
    // The create with its settings and tags, the archive made with it, the events put on a bus, and what the pages read.
    ops: &[
        ops::free("CreateEventBus"),
        ops::free("CreateArchive"),
        ops::free("PutEvents"),
        ops::free("ListRules"),
        ops::free("ListArchives"),
        ops::free("ListAliases").of("kms"),
    ],
    tags: Some(Tags::new(
        Step::new("ListTagsForResource", BUS_ARN),
        "Tags",
        Shape::KEY_VALUE,
        Write::Delta {
            add: op("TagResource", BUS_ARN),
            at: "Tags",
            remove: op("UntagResource", BUS_ARN),
            keys: "TagKeys",
        },
    )),
    // A bus is deleted with its rules, which are removed with their targets first.
    blockers: &[Blocker {
        title: "Rules",
        list: Step::new("ListRules", r#"{"EventBusName":"{id}","Limit":100}"#),
        items: "Rules",
        item_as: "Name",
        name: Text::at("Name"),
        link: None,
        except: None,
        blocks: true,
        clear: Some(Clear::Cascade {
            under: &[TARGETS_OF_RULE],
            step: Step::new(
                "DeleteRule",
                r#"{"Name":"{form:Name}","EventBusName":"{id}"}"#,
            ),
            said: "Deleted rule {name}",
        }),
    }],
    page: says::eventbridge::BUS,
    ..Kind::BARE
};
