//! IAM: users (their access keys a related list, a secret key never shown: IAM gives it only once, to whoever made the key),
//! groups, and with [`super::iam_roles`] roles and customer managed policies.

use super::Format::{Mono, Text as Plain, Time};
use super::iam_blockers::{GROUP_BLOCKERS, USER_BLOCKERS};
use super::iam_ops::{GROUP_OPS, USER_OPS};
use super::iam_roles::{POLICIES, ROLES};
use super::{
    Create, Delete, Descriptor, Id, Input, InputKind, Kind, List, Shape, Step, Sub, Tags, Text,
    Write, col, field, input, op,
};

pub(super) static IAM: Descriptor = Descriptor {
    service: "iam",
    target: "iam",
    kinds: &[USERS, ROLES, POLICIES, GROUPS],
};

/// What a user call names: this user.
const USER: &str = r#"{"UserName":"{id}"}"#;

const ATTACHED: &[super::Column] = &[
    col("Policy", "PolicyName", Mono),
    col("ARN", "PolicyArn", Mono),
];

/// A list of managed policies attached to something, each linked to its page.
pub(super) const fn attached(step: Step) -> Sub {
    Sub {
        title: "Attached policies",
        step,
        items: "AttachedPolicies",
        item_as: "PolicyArn",
        columns: ATTACHED,
        link: Some(("iam", "policies", Text::at("PolicyArn"))),
        more: "IsTruncated",
        add: None,
    }
}

/// A path input (`/`, `/engineering/`).
pub(super) const PATH: Input = input(
    "Path",
    "Path",
    "Groups names under a prefix, beginning and ending with a slash. Default: /.",
    InputKind::Text,
);

const USERS: Kind = Kind {
    id: "users",
    title: "Users",
    one: "user",
    about: "People and programs with long-term access keys. A secret key is shown once, when it is made, and never again.",
    members: &["UserName"],
    list: List {
        step: Step::new("ListUsers", r#"{"MaxItems":100,"Marker":"{token}"}"#),
        items: "Users",
        item_as: "UserName",
        token_out: "Marker",
        id: "UserName",
        name: Text::at("UserName"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Path", "Path", Mono),
        col("User ID", "UserId", Mono),
        col("Created", "CreateDate", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new("GetUser", r#"{"UserName":"{id}"}"#)],
    fields: &[
        field("ARN", "User.Arn", Mono),
        field("User ID", "User.UserId", Mono),
        field("Path", "User.Path", Mono),
        field(
            "Permissions boundary",
            "User.PermissionsBoundary.PermissionsBoundaryArn",
            Mono,
        ),
        field("Password last used", "User.PasswordLastUsed", Time),
        field("Created", "User.CreateDate", Time),
        field("Tags", "User.Tags", super::Format::Pairs),
    ],
    subs: &[
        Sub {
            title: "Access keys",
            step: Step::new("ListAccessKeys", r#"{"UserName":"{id}"}"#),
            items: "AccessKeyMetadata",
            item_as: "AccessKeyId",
            columns: &[
                col("Access key ID", "AccessKeyId", Mono),
                col("Status", "Status", Plain),
                col("Created", "CreateDate", Time),
            ],
            link: None,
            more: "IsTruncated",
            add: None,
        },
        Sub {
            title: "Groups",
            step: Step::new("ListGroupsForUser", r#"{"UserName":"{id}"}"#),
            items: "Groups",
            item_as: "GroupName",
            columns: &[
                col("Group", "GroupName", Mono),
                col("Created", "CreateDate", Time),
            ],
            link: Some(("iam", "groups", Text::at("GroupName"))),
            more: "IsTruncated",
            add: None,
        },
        attached(Step::new(
            "ListAttachedUserPolicies",
            r#"{"UserName":"{id}"}"#,
        )),
    ],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "ci-deployer",
                ..input(
                    "UserName",
                    "Name",
                    "Up to 64 letters, digits and +=,.@_-. The console makes no access key for it.",
                    InputKind::Text,
                )
            },
            PATH,
        ],
        steps: &[Step::new(
            "CreateUser",
            r#"{"UserName":"{form:UserName}","Path":"{form:Path}"}"#,
        )],
        name: Text::at("User.UserName"),
        key: "UserName",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteUser", r#"{"UserName":"{id}"}"#),
        words: "IAM refuses (DeleteConflict) while the user still has access keys, policies or groups: remove those first. Its access keys stop working when it goes.",
    }),
    reveal: None,
    deep: &[],
    tags: Some(Tags::new(
        Step::new("ListUserTags", USER),
        "Tags",
        Shape::KEY_VALUE,
        Write::Delta {
            add: op("TagUser", USER),
            at: "Tags",
            remove: op("UntagUser", USER),
            keys: "TagKeys",
        },
    )),
    blockers: USER_BLOCKERS,
    ops: USER_OPS,
    ..Kind::BARE
};

const GROUPS: Kind = Kind {
    id: "groups",
    title: "Groups",
    one: "group",
    about: "Users gathered to share the policies attached to the group.",
    members: &["GroupName"],
    list: List {
        step: Step::new("ListGroups", r#"{"MaxItems":100,"Marker":"{token}"}"#),
        items: "Groups",
        item_as: "GroupName",
        token_out: "Marker",
        id: "GroupName",
        name: Text::at("GroupName"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        col("Path", "Path", Mono),
        col("Group ID", "GroupId", Mono),
        col("Created", "CreateDate", Time),
    ],
    id_of: Id::Name,
    detail: &[Step::new("GetGroup", r#"{"GroupName":"{id}"}"#)],
    fields: &[
        field("ARN", "Group.Arn", Mono),
        field("Group ID", "Group.GroupId", Mono),
        field("Path", "Group.Path", Mono),
        field("Created", "Group.CreateDate", Time),
    ],
    subs: &[
        Sub {
            title: "Users",
            step: Step::answered("GetGroup"),
            items: "Users",
            item_as: "UserName",
            columns: &[
                col("User", "UserName", Mono),
                col("Created", "CreateDate", Time),
            ],
            link: Some(("iam", "users", Text::at("UserName"))),
            more: "IsTruncated",
            add: None,
        },
        attached(Step::new(
            "ListAttachedGroupPolicies",
            r#"{"GroupName":"{id}"}"#,
        )),
    ],
    create: Some(Create {
        inputs: &[
            Input {
                required: true,
                placeholder: "developers",
                ..input(
                    "GroupName",
                    "Name",
                    "Up to 128 letters, digits and +=,.@_-.",
                    InputKind::Text,
                )
            },
            PATH,
        ],
        steps: &[Step::new(
            "CreateGroup",
            r#"{"GroupName":"{form:GroupName}","Path":"{form:Path}"}"#,
        )],
        name: Text::at("Group.GroupName"),
        key: "GroupName",
    }),
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteGroup", r#"{"GroupName":"{id}"}"#),
        words: "IAM refuses (DeleteConflict) while the group still has users or policies: remove those first.",
    }),
    reveal: None,
    deep: &[],
    blockers: GROUP_BLOCKERS,
    ops: GROUP_OPS,
    ..Kind::BARE
};
