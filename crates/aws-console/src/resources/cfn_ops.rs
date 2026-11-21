//! CloudFormation: what the console's pages of a stack may call beyond reading it (the create and update wizards, a change set's
//! page, the exports), and the kind of the exports.
//!
//! Each is a kind-level operation ([`super::Kind::ops`]): the wizard names the stack in the input, which a bound resource
//! operation could not (a stack that does not exist yet, a deleted one read by its id). They are the operations the boards call,
//! each named; none passes another service's call through.

use super::Format::{Mono, Text as Plain};
use super::{Kind, List, Op, Step, Text, col, field, op, ops::free};

/// The calls of the stacks' pages.
pub(super) const STACK_OPS: &[Op] = &[
    // The wizards: the template read, its parameters, the stack made or changed, a change set made to preview or to defer.
    free("ValidateTemplate"),
    free("GetTemplateSummary"),
    free("CreateStack"),
    free("UpdateStack"),
    free("CreateChangeSet"),
    // A change set's page.
    free("DescribeChangeSet"),
    free("ExecuteChangeSet"),
    free("DeleteChangeSet"),
    free("ListChangeSets"),
    // What a stack page reads: the stack (by name, or by id when it was deleted), its events, resources, template and policy.
    free("DescribeStacks"),
    free("DescribeStackEvents"),
    free("ListStackResources"),
    free("GetTemplate"),
    free("GetStackPolicy"),
    // Termination protection, the one setting of a made stack the Actions menu changes.
    // Who imports what: the delete dialog and the exports.
    free("UpdateTerminationProtection"),
    free("ListImports"),
    free("ListExports"),
    // The deleted stacks (CloudFormation keeps a deleted stack's record by its id), and only those.
    op("ListStacks", r#"{"StackStatusFilter":["DELETE_COMPLETE"]}"#),
];

/// The values stacks export, which other stacks read with `Fn::ImportValue`. The page of one reads `ListImports` for who does.
pub(super) const EXPORTS: Kind = Kind {
    id: "exports",
    title: "Exports",
    one: "export",
    about: "Output values that other stacks in this account and Region can import.",
    members: &["ExportName"],
    list: List {
        step: Step::new("ListExports", r#"{"NextToken":"{token}"}"#),
        items: "Exports",
        item_as: "Name",
        token_out: "NextToken",
        id: "Name",
        name: Text::at("Name"),
        label: None,
        split: None,
    },
    elsewhere: "A stack exports a value with the Export field of one of its outputs.",
    columns: &[
        col("Value", "Value", Mono),
        col("Exporting stack", "ExportingStackId", Plain),
    ],
    // Nothing is read for one: the page asks `ListExports` and `ListImports` itself. The field makes a row openable.
    fields: &[field("Export name", "Name", Mono)],
    ..Kind::BARE
};
