//! Step Functions' state machines: the list, the page of one, and every call its tabs and Workflow Studio make.

use super::super::Format::{Flag, Json, Mono, Text as Plain, Time};
use super::super::{
    Blocker, Deep, Delete, Field, Id, Kept, Kind, LinkTo, List, Shape, Step, Tags, Text, Write,
    col, field, op, op_within, ops, or, says,
};

/// What a tag call names: this machine.
const MACHINE: &str = r#"{"resourceArn":"{id}"}"#;
/// What every call of a machine's own pages names: this machine.
const THIS: &str = r#"{"stateMachineArn":"{id}"}"#;

/// What its executions' ARNs begin with: an execution page acts only on an execution of this machine.
const EXECUTION: &str = "arn:aws:states:{region}:{account}:execution:{name}:";
/// A machine's ARN: a call that takes one may take this machine's, or a version's or an alias's of it (`{id}:2`, `{id}:PROD`).
const SELF: &str = "{id}";
/// What a version's or an alias's ARN begins with.
const PART: &str = "{id}:";

pub(super) const MACHINES: Kind = Kind {
    id: "machines",
    title: "State machines",
    one: "state machine",
    about: "Standard and Express workflows. Choose a name to open it.",
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
    elsewhere: "",
    each: &[],
    columns: &[
        col("Type", "type", Plain),
        col("Created", "creationDate", Time),
    ],
    id_of: Id::Template("arn:aws:states:{region}:{account}:stateMachine:{name}"),
    detail: &[Step::new("DescribeStateMachine", THIS)],
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
            ..field("IAM role ARN", "roleArn", Mono)
        },
        or("Logging", "loggingConfiguration.level", Plain, "OFF"),
        field(
            "Include execution data",
            "loggingConfiguration.includeExecutionData",
            Flag("Yes", "No"),
        ),
        field(
            "Log group",
            "loggingConfiguration.destinations.0.cloudWatchLogsLogGroup.logGroupArn",
            Mono,
        ),
        field(
            "Tracing",
            "tracingConfiguration.enabled",
            Flag("Enabled", "Disabled"),
        ),
        field("Revision ID", "revisionId", Mono),
        field("Creation date", "creationDate", Time),
        field("Definition", "definition", Json),
    ],
    subs: &[],
    create: None,
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteStateMachine", THIS),
        words: "Deleting removes the state machine at once, with its versions and aliases.",
    }),
    reveal: None,
    // Its executions, versions and aliases are tabs; each execution's page (addressed by its ARN) makes its calls here, on an
    // execution of this machine alone. The studio saves and publishes through the Edit entry.
    deep: &[
        Deep {
            id: "executions",
            title: "Executions",
            ops: &[
                op(
                    "ListExecutions",
                    r#"{"stateMachineArn":"{id}","mapRunArn":null}"#,
                ),
                // The machine, or a version or alias of it (the Versions and Aliases tabs start by those ARNs).
                op_within("StartExecution", "stateMachineArn", SELF),
                op_within("DescribeExecution", "executionArn", EXECUTION),
                op_within(
                    "DescribeStateMachineForExecution",
                    "executionArn",
                    EXECUTION,
                ),
                op_within("GetExecutionHistory", "executionArn", EXECUTION),
                op_within("StopExecution", "executionArn", EXECUTION),
                op_within("RedriveExecution", "executionArn", EXECUTION),
            ],
        },
        Deep {
            id: "definition",
            title: "Definition",
            ops: &[],
        },
        Deep {
            id: "edit",
            title: "Edit",
            ops: &[
                op("UpdateStateMachine", THIS),
                op("PublishStateMachineVersion", THIS),
            ],
        },
        // A version and an alias are listed by their ARNs only; each is read for its description.
        Deep {
            id: "versions",
            title: "Versions",
            ops: &[
                op("ListStateMachineVersions", THIS),
                op_within("DescribeStateMachine", "stateMachineArn", PART),
            ],
        },
        Deep {
            id: "aliases",
            title: "Aliases",
            ops: &[
                op("ListStateMachineAliases", THIS),
                op_within("DescribeStateMachineAlias", "stateMachineAliasArn", PART),
                // The routing names versions by ARN, which the page builds from this machine's.
                ops::free("CreateStateMachineAlias"),
            ],
        },
    ],
    // Workflow Studio: the definition is checked as it is typed; the machine is made with its role, which Create new role makes first
    // (IAM's CreateRole, then PutRolePolicy for what the definition's tasks call).
    ops: &[
        ops::free("CreateStateMachine"),
        ops::free("ValidateStateMachineDefinition"),
        ops::free("CreateRole").of("iam"),
        ops::free("PutRolePolicy").of("iam"),
        // A log level other than OFF needs a log group to write to: the studio makes the one Step Functions names by default.
        ops::free("CreateLogGroup").of("logs"),
    ],
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
    // The service deletes a machine with its versions and aliases and leaves its running executions to finish: all are listed
    // so the person knows what goes and what carries on, and none is cleared by the console.
    blockers: &[
        Blocker {
            title: "Versions",
            list: Step::new("ListStateMachineVersions", THIS),
            items: "stateMachineVersions",
            item_as: "stateMachineVersionArn",
            name: Text::after("stateMachineVersionArn", ":stateMachine:"),
            link: None,
            except: None,
            blocks: false,
            clear: None,
        },
        Blocker {
            title: "Aliases",
            list: Step::new("ListStateMachineAliases", THIS),
            items: "stateMachineAliases",
            item_as: "stateMachineAliasArn",
            name: Text::after("stateMachineAliasArn", ":stateMachine:"),
            link: None,
            except: None,
            blocks: false,
            clear: None,
        },
        Blocker {
            title: "Running executions",
            list: Step::new(
                "ListExecutions",
                r#"{"stateMachineArn":"{id}","statusFilter":"RUNNING","maxResults":50}"#,
            ),
            items: "executions",
            item_as: "name",
            name: Text::at("name"),
            link: None,
            except: None,
            blocks: false,
            clear: None,
        },
    ],
    kept: &[Kept::Text(
        "Executions that are running are not stopped: they run on with the definition they started with.",
    )],
    page: says::stepfunctions::MACHINE,
    ..Kind::BARE
};
