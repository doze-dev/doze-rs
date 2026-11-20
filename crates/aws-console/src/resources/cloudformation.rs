//! CloudFormation: stacks, with their resources, outputs, parameters and events (the newest first) on their page. The console
//! does not create a stack: it needs a template, deployed with the AWS CLI or a tool.

use super::Format::{Flag, Mono, Pairs, Text as Plain, Time};
use super::{Delete, Descriptor, Id, Kind, List, Step, Sub, Text, col, field};

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
            col("Status", "StackStatus", Plain),
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
            Sub {
                title: "Events",
                step: Step::new("DescribeStackEvents", r#"{"StackName":"{id}"}"#),
                items: "StackEvents",
                item_as: "EventId",
                columns: &[
                    col("Time", "Timestamp", Time),
                    col("Logical ID", "LogicalResourceId", Mono),
                    col("Status", "ResourceStatus", Plain),
                    col("Why", "ResourceStatusReason", Plain),
                ],
                link: None,
                more: "NextToken",
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
        deep: &[],
    }],
};
