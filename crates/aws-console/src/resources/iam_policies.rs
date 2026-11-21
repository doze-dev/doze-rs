//! IAM: customer managed policies (and the AWS managed ones something has attached), their tags, blockers and the operations the policy pages make.

use super::Format::{Flag, Json, Mono, Number, Text as Plain, Time};
use super::iam::PATH;
use super::iam_blockers::POLICY_BLOCKERS;
use super::iam_ops::POLICY_OPS;
use super::{
    Apply, Create, Delete, Id, Input, InputKind, Kind, List, Options, Param, ParamKind, Shape,
    Step, Sub, Tags, Text, Write, col, field, input, op,
};

/// What a policy call names: this policy, by its ARN.
const POLICY: &str = r#"{"PolicyArn":"{id}"}"#;

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
    blockers: POLICY_BLOCKERS,
    ops: POLICY_OPS,
    tags: Some(Tags::new(
        Step::new("ListPolicyTags", POLICY),
        "Tags",
        Shape::KEY_VALUE,
        Write::Delta {
            add: op("TagPolicy", POLICY),
            at: "Tags",
            remove: op("UntagPolicy", POLICY),
            keys: "TagKeys",
        },
    )),
    // Customer managed policies (the default), AWS managed ones, or both.
    params: &[Param {
        key: "scope",
        label: "Scope",
        input: ParamKind::Chips,
        options: Options::Fixed(&[
            ("Local", "Customer managed"),
            ("AWS", "AWS managed"),
            ("All", "All"),
        ]),
        default: "Local",
        apply: Apply::Member("Scope"),
    }],
    ..Kind::BARE
};
