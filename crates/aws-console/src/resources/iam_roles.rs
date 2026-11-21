//! IAM: roles (made from a trust policy template). Customer managed policies are in [`super::iam_policies`].

use super::Format::{Json, Mono, Seconds, Text as Plain, Time};
use super::iam::{PATH, attached};
use super::iam_blockers::ROLE_BLOCKERS;
use super::iam_ops::ROLE_OPS;
use super::{
    Create, Delete, Id, Input, InputKind, Kind, List, Shape, Step, Sub, Tags, Text, Write, col,
    field, input, op, or,
};

/// What a role call names: this role.
const ROLE: &str = r#"{"RoleName":"{id}"}"#;

/// The trust policy a role starts from: Lambda may assume it. The person changes the service, or the principal, before
/// creating.
const TRUST: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": { "Service": "lambda.amazonaws.com" },
      "Action": "sts:AssumeRole"
    }
  ]
}"#;

pub(super) const ROLES: Kind = Kind {
    id: "roles",
    title: "Roles",
    one: "role",
    about: "Identities a service or a person assumes for short-lived credentials. The trust policy says who may.",
    members: &["RoleName"],
    list: List {
        step: Step::new("ListRoles", r#"{"MaxItems":100,"Marker":"{token}"}"#),
        items: "Roles",
        item_as: "RoleName",
        token_out: "Marker",
        id: "RoleName",
        name: Text::at("RoleName"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Description", "Description", Plain),
        col("Path", "Path", Mono),
        col("Created", "CreateDate", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new("GetRole", r#"{"RoleName":"{id}"}"#)],
    fields: &[
        field("ARN", "Role.Arn", Mono),
        field("Role ID", "Role.RoleId", Mono),
        field("Description", "Role.Description", Plain),
        field("Path", "Role.Path", Mono),
        field("Maximum session", "Role.MaxSessionDuration", Seconds),
        field(
            "Permissions boundary",
            "Role.PermissionsBoundary.PermissionsBoundaryArn",
            Mono,
        ),
        or("Last used", "Role.RoleLastUsed.LastUsedDate", Time, "Never"),
        field("Created", "Role.CreateDate", Time),
        field("Tags", "Role.Tags", super::Format::Pairs),
        field("Trust policy", "Role.AssumeRolePolicyDocument", Json),
    ],
    subs: &[
        attached(Step::new(
            "ListAttachedRolePolicies",
            r#"{"RoleName":"{id}"}"#,
        )),
        Sub {
            title: "Inline policies",
            step: Step::new("ListRolePolicies", r#"{"RoleName":"{id}"}"#),
            items: "PolicyNames",
            item_as: "PolicyName",
            columns: &[col("Policy", "PolicyName", Mono)],
            link: None,
            more: "IsTruncated",
            add: None,
        },
    ],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "orders-handler",
                ..input(
                    "RoleName",
                    "Name",
                    "Up to 64 letters, digits and +=,.@_-.",
                    InputKind::Text,
                )
            },
            Input {
                required: true,
                value: TRUST,
                ..input(
                    "AssumeRolePolicyDocument",
                    "Trust policy",
                    "Who may assume the role. It starts as Lambda's: change the service (states.amazonaws.com, events.amazonaws.com, scheduler.amazonaws.com) or the principal.",
                    InputKind::Textarea,
                )
            },
            input(
                "Description",
                "Description",
                "What the role is for.",
                InputKind::Text,
            ),
            PATH,
        ],
        steps: &[Step::new(
            "CreateRole",
            r#"{"RoleName":"{form:RoleName}","AssumeRolePolicyDocument":"{form:AssumeRolePolicyDocument}","Description":"{form:Description}","Path":"{form:Path}"}"#,
        )],
        name: Text::at("Role.RoleName"),
        key: "RoleName",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteRole", r#"{"RoleName":"{id}"}"#),
        words: "IAM refuses (DeleteConflict) while policies are attached to the role or it has inline policies: detach and delete those first. Sessions already made from it keep working until they expire.",
    }),
    reveal: None,
    deep: &[],
    tags: Some(Tags::new(
        Step::new("ListRoleTags", ROLE),
        "Tags",
        Shape::KEY_VALUE,
        Write::Delta {
            add: op("TagRole", ROLE),
            at: "Tags",
            remove: op("UntagRole", ROLE),
            keys: "TagKeys",
        },
    )),
    blockers: ROLE_BLOCKERS,
    ops: ROLE_OPS,
    ..Kind::BARE
};
