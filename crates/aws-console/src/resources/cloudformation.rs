//! CloudFormation: stacks, with their resources, outputs and parameters on their page, and their events, template and change
//! sets on pages of their own. The console does not create a stack: it needs a template, deployed with the AWS CLI or a tool.

use super::Format::{Flag, Mono, Pairs, Text as Plain, Time};
use super::cfn_kept::retained;
use super::{
    Apply, Deep, Delete, Descriptor, Id, Kept, Kind, List, Options, Param, ParamKind, Shape, Step,
    Sub, Tags, Text, Write, col, field, op, says, state,
};

/// What every call of its pages names: this stack.
const STACK: &str = r#"{"StackName":"{id}"}"#;

/// What a tag read names: this stack.
const STACK_NAME: &str = r#"{"StackName":"{id}"}"#;

/// Whether a stack's status is in the group of statuses `group` names.
fn in_group(stack: &serde_json::Value, group: &str) -> bool {
    let status = stack
        .get("StackStatus")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let rolled_back = status.contains("ROLLBACK");
    match group {
        "COMPLETE" => status.ends_with("_COMPLETE") && !rolled_back,
        "IN_PROGRESS" => status.ends_with("_IN_PROGRESS"),
        "FAILED" => status.ends_with("_FAILED"),
        "ROLLBACK" => {
            rolled_back && !status.ends_with("_IN_PROGRESS") && !status.ends_with("_FAILED")
        }
        _ => true,
    }
}

/// The one stack `DescribeStacks` answered for a name.
macro_rules! stack {
    ($member:literal) => {
        concat!("Stacks.0.", $member)
    };
}

pub(super) static CLOUDFORMATION: Descriptor = Descriptor {
    service: "cloudformation",
    target: "cloudformation",
    kinds: &[Kind {
        id: "stacks",
        title: "Stacks",
        one: "stack",
        about: "Stacks that exist now (a deleted one is gone from this list): what each made, and what it outputs.",
        members: &["StackName"],
        list: List {
            step: Step::new("DescribeStacks", r#"{"NextToken":"{token}"}"#),
            items: "Stacks",
            item_as: "StackName",
            token_out: "NextToken",
            id: "StackName",
            name: Text::at("StackName"),
            label: None,
            split: None,
        },
        elsewhere: "A stack is made from a template: use aws cloudformation deploy --template-file template.yaml --stack-name <name>, or your deploy tool.",
        each: &[],
        columns: &[
            state("Status", "StackStatus", Plain),
            col("Description", "Description", Plain),
            col("Created", "CreationTime", Time),
            col("Updated", "LastUpdatedTime", Time),
        ],
        id_of: Id::Name,
        detail: &[Step::new("DescribeStacks", r#"{"StackName":"{id}"}"#)],
        fields: &[
            field("Stack ID", stack!("StackId"), Mono),
            field("Status", stack!("StackStatus"), Plain),
            field("Why", stack!("StackStatusReason"), Plain),
            field("Description", stack!("Description"), Plain),
            field("Capabilities", stack!("Capabilities"), Mono),
            field("Role", stack!("RoleARN"), Mono),
            field(
                "Termination protection",
                stack!("EnableTerminationProtection"),
                Flag("On", "Off"),
            ),
            field(
                "Rollback on failure",
                stack!("DisableRollback"),
                Flag("Disabled", "Enabled"),
            ),
            field("Created", stack!("CreationTime"), Time),
            field("Updated", stack!("LastUpdatedTime"), Time),
            field("Tags", stack!("Tags"), Pairs),
        ],
        subs: &[
            Sub {
                title: "Outputs",
                step: Step::answered("DescribeStacks"),
                items: stack!("Outputs"),
                item_as: "OutputKey",
                columns: &[
                    col("Key", "OutputKey", Mono),
                    col("Value", "OutputValue", Mono),
                    col("Description", "Description", Plain),
                    col("Export", "ExportName", Mono),
                ],
                link: None,
                more: "",
                add: None,
            },
            Sub {
                title: "Resources",
                step: Step::new("ListStackResources", r#"{"StackName":"{id}"}"#),
                items: "StackResourceSummaries",
                item_as: "LogicalResourceId",
                columns: &[
                    col("Logical ID", "LogicalResourceId", Mono),
                    col("Physical ID", "PhysicalResourceId", Mono),
                    col("Type", "ResourceType", Mono),
                    col("Status", "ResourceStatus", Plain),
                    col("Updated", "LastUpdatedTimestamp", Time),
                ],
                link: None,
                more: "NextToken",
                add: None,
            },
            Sub {
                title: "Parameters",
                step: Step::answered("DescribeStacks"),
                items: stack!("Parameters"),
                item_as: "ParameterKey",
                columns: &[
                    col("Key", "ParameterKey", Mono),
                    col("Value", "ParameterValue", Mono),
                    col("Resolved", "ResolvedValue", Mono),
                ],
                link: None,
                more: "",
                add: None,
            },
        ],
        create: None,
        delete: Some(Delete {
            verb: "Delete",
            step: Step::new("DeleteStack", r#"{"StackName":"{id}"}"#),
            words: "Deleting the stack deletes every resource it made, except those its template retains. CloudFormation refuses while termination protection is on.",
        }),
        reveal: None,
        // Its events are a page of their own, not a list on its details.
        deep: &[
            Deep {
                id: "events",
                title: "Events",
                ops: &[op("DescribeStackEvents", STACK)],
            },
            Deep {
                id: "template",
                title: "Template",
                // A change set named by its ARN could be another stack's.
                ops: &[op(
                    "GetTemplate",
                    r#"{"StackName":"{id}","ChangeSetName":null}"#,
                )],
            },
            Deep {
                id: "changesets",
                title: "Change sets",
                ops: &[op("ListChangeSets", STACK)],
            },
        ],
        // CloudFormation has no tag call: a stack's tags are set by updating the stack, which keeps its template.
        tags: Some(Tags::new(
            Step::new("DescribeStacks", STACK_NAME),
            "Stacks.0.Tags",
            Shape::KEY_VALUE,
            Write::Replace {
                put: op(
                    "UpdateStack",
                    r#"{"StackName":"{id}","UsePreviousTemplate":true}"#,
                ),
                at: "Tags",
                clear: None,
            },
        )),
        // The list is of every stack that exists; the filter narrows a page to a group of statuses, with how many each has.
        params: &[Param {
            key: "status",
            label: "Status",
            input: ParamKind::Chips,
            options: Options::Fixed(&[
                ("", "All"),
                ("COMPLETE", "Complete"),
                ("IN_PROGRESS", "In progress"),
                ("FAILED", "Failed"),
                ("ROLLBACK", "Rolled back"),
            ]),
            default: "",
            apply: Apply::Filter(in_group),
        }],
        // What the template keeps when the stack goes.
        kept: &[Kept::Read(
            Step::new("GetTemplate", r#"{"StackName":"{id}"}"#),
            retained,
        )],
        page: super::Page {
            badges: Some(says::stack),
            ..super::Page::NONE
        },
        ..Kind::BARE
    }],
};
