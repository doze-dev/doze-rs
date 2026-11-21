//! Step Functions: state machines; a machine's executions (the newest first) are a page of its own. The console does not
//! create a machine: its definition is code, written in Amazon States Language and deployed with a tool or the AWS CLI.

use super::Format::{Json, Mono, Tail, Text as Plain, Time};
use super::{
    Deep, Delete, Descriptor, Field, Id, Kind, LinkTo, List, Shape, Step, Tags, Text, Write, col,
    field, op, op_within, or, says,
};

/// What a tag call names: this machine.
const MACHINE: &str = r#"{"resourceArn":"{id}"}"#;

/// What its executions' ARNs begin with: an execution page acts only on an execution of this machine.
const EXECUTION: &str = "arn:aws:states:{region}:{account}:execution:{name}:";

pub(super) static STEPFUNCTIONS: Descriptor = Descriptor {
    service: "stepfunctions",
    target: "states",
    kinds: &[Kind {
        id: "machines",
        title: "State machines",
        one: "state machine",
        about: "Workflows of steps, each run an execution: its input, its output, and where it failed.",
        members: &["stateMachineArn"],
        list: List {
            step: Step::new(
                "ListStateMachines",
                r#"{"maxResults":100,"nextToken":"{token}"}"#,
            ),
            items: "stateMachines",
            item_as: "name",
            token_out: "nextToken",
            id: "stateMachineArn",
            name: Text::at("name"),
            label: None,
            split: None,
        },
        elsewhere: "A state machine's definition is code: deploy it with your own tool, or with aws stepfunctions create-state-machine --definition file://machine.json.",
        each: &[],
        columns: &[
            col("Type", "type", Plain),
            col("Created", "creationDate", Time),
        ],
        id_of: Id::Template("arn:aws:states:{region}:{account}:stateMachine:{name}"),
        detail: &[Step::new(
            "DescribeStateMachine",
            r#"{"stateMachineArn":"{id}"}"#,
        )],
        fields: &[
            field("ARN", "stateMachineArn", Mono),
            field("Type", "type", Plain),
            field("Status", "status", Plain),
            field("Description", "description", Plain),
            Field {
                link: &[LinkTo {
                    service: "iam",
                    kind: "roles",
                    when: None,
                }],
                ..field("Role", "roleArn", Tail)
            },
            or("Logging", "loggingConfiguration.level", Plain, "OFF"),
            field(
                "Tracing",
                "tracingConfiguration.enabled",
                super::Format::Flag("Enabled", "Disabled"),
            ),
            field("Revision", "revisionId", Mono),
            field("Created", "creationDate", Time),
            field("Definition", "definition", Json),
        ],
        subs: &[],
        create: None,
        delete: Some(Delete {
            verb: "Delete",
            step: Step::new("DeleteStateMachine", r#"{"stateMachineArn":"{id}"}"#),
            words: "Deleting removes the machine at once, and no execution can start on it. (AWS marks it DELETING until its running executions end; doze-aws does not wait.)",
        }),
        reveal: None,
        // Its executions are a page of their own, not a list on its details; each execution's page (addressed by its ARN)
        // makes its calls here, on an execution of this machine alone.
        deep: &[Deep {
            id: "executions",
            title: "Executions",
            ops: &[
                op(
                    "ListExecutions",
                    r#"{"stateMachineArn":"{id}","mapRunArn":null}"#,
                ),
                op("StartExecution", r#"{"stateMachineArn":"{id}"}"#),
                op_within("DescribeExecution", "executionArn", EXECUTION),
                op_within(
                    "DescribeStateMachineForExecution",
                    "executionArn",
                    EXECUTION,
                ),
                op_within("GetExecutionHistory", "executionArn", EXECUTION),
                op_within("StopExecution", "executionArn", EXECUTION),
            ],
        }],
        tags: Some(Tags::new(
            Step::new("ListTagsForResource", MACHINE),
            "tags",
            Shape::Pairs("key", "value"),
            Write::Delta {
                add: op("TagResource", MACHINE),
                at: "tags",
                remove: op("UntagResource", MACHINE),
                keys: "tagKeys",
            },
        )),
        page: super::Page {
            badges: Some(says::machine),
            ..super::Page::NONE
        },
        ..Kind::BARE
    }],
};
