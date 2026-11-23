//! EventBridge Scheduler: schedules and schedule groups. A schedule is named in addresses as its ARN names it, with its group
//! (`default/nightly-report`).

use super::Format::{Json, Mono, Tail, Text as Plain, Time};
use super::{
    Create, Delete, Descriptor, Id, Input, InputKind, Kind, List, Step, Sub, Text, col, field,
    input, or, state,
};

pub(super) static SCHEDULER: Descriptor = Descriptor {
    service: "scheduler",
    target: "scheduler",
    kinds: &[SCHEDULES, GROUPS],
};

const SCHEDULE: Text = Text::after("Arn", ":schedule/");

const SCHEDULES: Kind = Kind {
    id: "schedules",
    title: "Schedules",
    one: "schedule",
    about: "One-time and recurring invocations of a target (a queue, a function, a state machine), in every group.",
    members: &["Name"],
    list: List {
        step: Step::new(
            "ListSchedules",
            r#"{"MaxResults":100,"NextToken":"{token}"}"#,
        ),
        items: "Schedules",
        item_as: "Name",
        token_out: "NextToken",
        id: "Name",
        name: SCHEDULE,
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Group", "GroupName", Mono),
        state("State", "State", Plain),
        col("Target", "Target.Arn", Tail),
        col("Last modified", "LastModificationDate", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new(
        "GetSchedule",
        r#"{"Name":"{name:last}","GroupName":"{name:head}"}"#,
    )],
    fields: &[
        field("ARN", "Arn", Mono),
        field("Group", "GroupName", Mono),
        field("State", "State", Plain),
        field("Expression", "ScheduleExpression", Mono),
        or("Time zone", "ScheduleExpressionTimezone", Mono, "UTC"),
        field("Description", "Description", Plain),
        field("Target", "Target.Arn", Mono),
        field("Target role", "Target.RoleArn", Mono),
        field("Flexible window", "FlexibleTimeWindow.Mode", Plain),
        field("Starts", "StartDate", Time),
        field("Ends", "EndDate", Time),
        field("After completion", "ActionAfterCompletion", Plain),
        field("Dead-letter queue", "Target.DeadLetterConfig.Arn", Tail),
        field("Created", "CreationDate", Time),
        field("Last modified", "LastModificationDate", Time),
        field("Input", "Target.Input", Json),
    ],
    subs: &[],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "nightly-report",
                ..input(
                    "Name",
                    "Name",
                    "Up to 64 letters, digits and .-_.",
                    InputKind::Text,
                )
            },
            Input {
                placeholder: "reports",
                ..input(
                    "GroupName",
                    "Group",
                    "Default: default (the group every account has).",
                    InputKind::Text,
                )
            },
            Input {
                required: true,
                placeholder: "rate(1 hour)",
                ..input(
                    "ScheduleExpression",
                    "Expression",
                    "rate(…), cron(…) or at(2026-12-01T09:00:00) for once.",
                    InputKind::Text,
                )
            },
            Input {
                required: true,
                placeholder: "arn:aws:sqs:us-east-1:000000000000:orders",
                ..input(
                    "TargetArn",
                    "Target",
                    "The ARN of a queue, topic, function or state machine.",
                    InputKind::Text,
                )
            },
            Input {
                required: true,
                placeholder: "arn:aws:iam::000000000000:role/scheduler",
                ..input(
                    "RoleArn",
                    "Role",
                    "The role Scheduler assumes to call the target.",
                    InputKind::Text,
                )
            },
            input(
                "Input",
                "Input",
                "What the target is sent (a message body, an event). Default: nothing.",
                InputKind::Textarea,
            ),
        ],
        steps: &[Step::new(
            "CreateSchedule",
            r#"{"Name":"{form:Name}","GroupName":"{form:GroupName}","ScheduleExpression":"{form:ScheduleExpression}","Target":{"Arn":"{form:TargetArn}","RoleArn":"{form:RoleArn}","Input":"{form:Input}"},"FlexibleTimeWindow":{"Mode":"OFF"}}"#,
        )],
        name: Text::after("ScheduleArn", ":schedule/"),
        key: "Name",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new(
            "DeleteSchedule",
            r#"{"Name":"{name:last}","GroupName":"{name:head}"}"#,
        ),
        words: "Deleting stops the schedule: the target is not called again.",
    }),
    reveal: None,
    deep: &[],
    ..Kind::BARE
};

const GROUPS: Kind = Kind {
    id: "groups",
    title: "Schedule groups",
    one: "schedule group",
    about: "Schedules gathered to be tagged and deleted together. Every account has the default group.",
    members: &["GroupName"],
    list: List {
        step: Step::new(
            "ListScheduleGroups",
            r#"{"MaxResults":100,"NextToken":"{token}"}"#,
        ),
        items: "ScheduleGroups",
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
        col("State", "State", Plain),
        col("Created", "CreationDate", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new("GetScheduleGroup", r#"{"Name":"{id}"}"#)],
    fields: &[
        field("ARN", "Arn", Mono),
        field("State", "State", Plain),
        field("Created", "CreationDate", Time),
        field("Last modified", "LastModificationDate", Time),
    ],
    subs: &[Sub {
        title: "Schedules",
        step: Step::new("ListSchedules", r#"{"GroupName":"{id}","MaxResults":100}"#),
        items: "Schedules",
        item_as: "Name",
        columns: &[
            col("Schedule", "Name", Mono),
            col("State", "State", Plain),
            col("Target", "Target.Arn", Tail),
        ],
        link: Some(("scheduler", "schedules", SCHEDULE)),
        more: "NextToken",
        add: None,
    }],
    create: Some(Create {
        inputs: &[Input {
            required: true,
            placeholder: "reports",
            ..input(
                "Name",
                "Name",
                "Up to 64 letters, digits and .-_.",
                InputKind::Text,
            )
        }],
        steps: &[Step::new(
            "CreateScheduleGroup",
            r#"{"Name":"{form:Name}"}"#,
        )],
        name: Text::at(""),
        key: "Name",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteScheduleGroup", r#"{"Name":"{id}"}"#),
        words: "Deleting the group deletes every schedule in it. The default group cannot be deleted.",
    }),
    reveal: None,
    deep: &[],
    ..Kind::BARE
};
