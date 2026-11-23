//! IAM: roles (made from a trust policy template) and customer managed policies.

use super::Format::{Flag, Json, Mono, Number, Seconds, Text as Plain, Time};
use super::iam::{PATH, attached};
use super::{
    Create, Delete, Id, Input, InputKind, Kind, List, Step, Sub, Text, col, field, input, or,
};

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

/// A policy document to start from: read access to one bucket.
const DOCUMENT: &str = r#"{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": ["s3:GetObject", "s3:ListBucket"],
      "Resource": ["arn:aws:s3:::example-bucket", "arn:aws:s3:::example-bucket/*"]
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
};

/// The entities a policy is attached to, from its detail's `ListEntitiesForPolicy`.
const fn entities(
    title: &'static str,
    items: &'static str,
    member: &'static str,
    kind: &'static str,
) -> Sub {
    Sub {
        title,
        step: Step::answered("ListEntitiesForPolicy"),
        items,
        item_as: member,
        columns: &[],
        link: Some(("iam", kind, Text::at(member))),
        more: "",
        add: None,
    }
}

pub(super) const POLICIES: Kind = Kind {
    id: "policies",
    title: "Policies",
    one: "policy",
    about: "Customer managed policies: permissions you attach to users, groups and roles. AWS managed policies open from where they are attached.",
    members: &["PolicyArn"],
    list: List {
        step: Step::new(
            "ListPolicies",
            r#"{"Scope":"Local","MaxItems":100,"Marker":"{token}"}"#,
        ),
        items: "Policies",
        item_as: "Arn",
        token_out: "Marker",
        // A policy's address is its ARN (an AWS managed one has no account in it), shown as its name.
        id: "Arn",
        name: Text::at("Arn"),
        label: Some(Text::at("PolicyName")),
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Description", "Description", Plain),
        col("Attached to", "AttachmentCount", Number),
        col("Version", "DefaultVersionId", Mono),
        col("Updated", "UpdateDate", Time),
    ],
    id_of: Id::Name,
    detail: &[
        Step::new("GetPolicy", r#"{"PolicyArn":"{id}"}"#),
        Step::new(
            "GetPolicyVersion",
            r#"{"PolicyArn":"{id}","VersionId":"{out:Policy.DefaultVersionId}"}"#,
        ),
        Step::new("ListEntitiesForPolicy", r#"{"PolicyArn":"{id}"}"#),
    ],
    fields: &[
        field("ARN", "Policy.Arn", Mono),
        field("Name", "Policy.PolicyName", Mono),
        field("Description", "Policy.Description", Plain),
        field("Path", "Policy.Path", Mono),
        field("Default version", "Policy.DefaultVersionId", Mono),
        field("Attached to", "Policy.AttachmentCount", Number),
        field("Attachable", "Policy.IsAttachable", Flag("Yes", "No")),
        field("Created", "Policy.CreateDate", Time),
        field("Updated", "Policy.UpdateDate", Time),
        field("Document", "PolicyVersion.Document", Json),
    ],
    subs: &[
        Sub {
            columns: &[col("User", "UserName", Mono)],
            ..entities("Users", "PolicyUsers", "UserName", "users")
        },
        Sub {
            columns: &[col("Role", "RoleName", Mono)],
            ..entities("Roles", "PolicyRoles", "RoleName", "roles")
        },
        Sub {
            columns: &[col("Group", "GroupName", Mono)],
            ..entities("Groups", "PolicyGroups", "GroupName", "groups")
        },
    ],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "orders-read",
                ..input(
                    "PolicyName",
                    "Name",
                    "Up to 128 letters, digits and +=,.@_-.",
                    InputKind::Text,
                )
            },
            Input {
                required: true,
                value: DOCUMENT,
                ..input(
                    "PolicyDocument",
                    "Document",
                    "The permissions. It starts as read access to one bucket: change the actions and resources.",
                    InputKind::Textarea,
                )
            },
            input(
                "Description",
                "Description",
                "Set at creation only.",
                InputKind::Text,
            ),
            PATH,
        ],
        steps: &[Step::new(
            "CreatePolicy",
            r#"{"PolicyName":"{form:PolicyName}","PolicyDocument":"{form:PolicyDocument}","Description":"{form:Description}","Path":"{form:Path}"}"#,
        )],
        name: Text::at("Policy.Arn"),
        key: "PolicyName",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeletePolicy", r#"{"PolicyArn":"{id}"}"#),
        words: "IAM refuses (DeleteConflict) while the policy is attached or has versions besides its default: detach it and delete those first.",
    }),
    reveal: None,
    deep: &[],
};
