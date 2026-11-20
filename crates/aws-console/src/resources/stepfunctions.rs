//! Step Functions: state machines, and a machine's executions (the newest first) on its page. The console does not create a
//! machine: its definition is code, written in Amazon States Language and deployed with a tool or the AWS CLI.

use super::Format::{Json, Mono, Tail, Text as Plain, Time};
use super::{Delete, Descriptor, Field, Id, Kind, LinkTo, List, Step, Sub, Text, col, field, or};

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
        subs: &[Sub {
            title: "Executions",
            step: Step::new(
                "ListExecutions",
                r#"{"stateMachineArn":"{id}","maxResults":100}"#,
            ),
            items: "executions",
            item_as: "name",
            columns: &[
                col("Execution", "name", Mono),
                col("Status", "status", Plain),
                col("Started", "startDate", Time),
                col("Stopped", "stopDate", Time),
            ],
            link: None,
            more: "nextToken",
            add: None,
        }],
        create: None,
        delete: Some(Delete {
            verb: "Delete",
            step: Step::new("DeleteStateMachine", r#"{"stateMachineArn":"{id}"}"#),
            words: "The machine is marked DELETING and no execution can start; it goes once its running executions end.",
        }),
        reveal: None,
        deep: &[],
    }],
};
