//! EventBridge rules: the list (a bus at a time), a rule's page, and what the pages do to a rule and its targets.

use super::super::Format::{Json, Mono, Tail, Text as Plain};
use super::super::eventbridge_blockers::TARGETS;
use super::super::{
    Create, Delete, Field, Id, Input, InputKind, Kind, LinkTo, List, Options, Param, ParamKind,
    Shape, Step, Sub, Tags, Text, Write, col, field, input, op, ops, says, state,
};
use super::RULE_ARN;

/// A rule's name in an address, from its ARN.
pub(super) const RULE_NAME: Text = Text::after("Arn", ":rule/");

pub(super) const RULES: Kind = Kind {
    id: "rules",
    title: "Rules",
    one: "rule",
    about: "A rule watches one event bus: an event pattern or a schedule, and the targets it sends to.",
    members: &["Name", "Rule"],
    list: List {
        step: Step::new("ListRules", r#"{"Limit":100,"NextToken":"{token}"}"#),
        items: "Rules",
        item_as: "Name",
        token_out: "NextToken",
        id: "Name",
        name: RULE_NAME,
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        state("State", "State", Plain),
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
            // What is sent: the event as it is (nothing set), a constant, a part of it, or a template over its parts.
            col(
                "Input",
                "Input|InputPath|InputTransformer.InputTemplate",
                Mono,
            ),
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
        words: "EventBridge refuses while the rule still has targets: the console removes them first. Events stop matching it at once.",
    }),
    reveal: None,
    deep: &[],
    // The wizard's PutRule with its tags and PutTargets, the enable and disable of a list and a page, the one-by-one removal
    // of targets before a delete, the pattern test, and what the target and bus choices read.
    ops: &[
        ops::free("PutRule"),
        ops::free("PutTargets"),
        ops::free("RemoveTargets"),
        ops::free("DeleteRule"),
        ops::free("EnableRule"),
        ops::free("DisableRule"),
        ops::free("ListRules"),
        ops::free("ListTargetsByRule"),
        ops::free("TestEventPattern"),
        ops::free("ListEventBuses"),
        ops::free("ListApiDestinations"),
        ops::free("ListTagsForResource"),
    ],
    tags: Some(Tags::new(
        Step::new("ListTagsForResource", RULE_ARN),
        "Tags",
        Shape::KEY_VALUE,
        Write::Delta {
            add: op("TagResource", RULE_ARN),
            at: "Tags",
            remove: op("UntagResource", RULE_ARN),
            keys: "TagKeys",
        },
    )),
    // EventBridge refuses to delete a rule that still has targets.
    blockers: &[TARGETS],
    params: &[Param {
        key: "bus",
        label: "Event bus",
        input: ParamKind::Select,
        options: Options::Listed {
            step: Step::new("ListEventBuses", r#"{"Limit":100}"#),
            items: "EventBuses",
            item_as: "Name",
            value: Text::at("Name"),
        },
        default: "",
        apply: super::super::Apply::Member("EventBusName"),
    }],
    page: says::eventbridge::RULE,
    ..Kind::BARE
};
