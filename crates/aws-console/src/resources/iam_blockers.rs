//! IAM: what IAM refuses to delete an entity over (`DeleteConflict`), each kind of dependent listed and cleared with IAM's own
//! calls: a policy attached to a role, user or group; an inline policy; an access key; a group membership; an instance profile.

use super::{Blocker, Clear, Step, Text};

/// One kind of dependent: listed by `list` (`op`, `body`), found at `items` (a bare value read as `member`), named by `name`,
/// and cleared by the call `clear` that takes the dependent's members as `{form:<member>}`.
#[allow(clippy::too_many_arguments)]
const fn dependent(
    title: &'static str,
    list: (&'static str, &'static str),
    (items, member): (&'static str, &'static str),
    link: Option<(&'static str, &'static str, Text)>,
    clear: (&'static str, &'static str),
    said: &'static str,
) -> Blocker {
    Blocker {
        title,
        list: Step::new(list.0, list.1),
        items,
        item_as: member,
        name: Text::at(member),
        link,
        except: None,
        blocks: true,
        clear: Some(Clear::Each {
            step: Step::new(clear.0, clear.1),
            said,
        }),
    }
}

/// A managed policy attached to something: named by its policy name, linked by its ARN.
const fn attached(
    list: (&'static str, &'static str),
    detach: (&'static str, &'static str),
) -> Blocker {
    Blocker {
        name: Text::at("PolicyName"),
        ..dependent(
            "Attached policies",
            list,
            ("AttachedPolicies", "PolicyArn"),
            Some(("iam", "policies", Text::at("PolicyArn"))),
            detach,
            "Detached {name}",
        )
    }
}

const fn inline(
    list: (&'static str, &'static str),
    delete: (&'static str, &'static str),
) -> Blocker {
    dependent(
        "Inline policies",
        list,
        ("PolicyNames", "PolicyName"),
        None,
        delete,
        "Deleted inline policy {name}",
    )
}

const USER: &str = r#"{"UserName":"{id}"}"#;
const GROUP: &str = r#"{"GroupName":"{id}"}"#;
const ROLE: &str = r#"{"RoleName":"{id}"}"#;
const POLICY: &str = r#"{"PolicyArn":"{id}"}"#;

/// A user: policies, access keys, groups.
pub(super) const USER_BLOCKERS: &[Blocker] = &[
    attached(
        ("ListAttachedUserPolicies", USER),
        (
            "DetachUserPolicy",
            r#"{"UserName":"{id}","PolicyArn":"{form:PolicyArn}"}"#,
        ),
    ),
    inline(
        ("ListUserPolicies", USER),
        (
            "DeleteUserPolicy",
            r#"{"UserName":"{id}","PolicyName":"{form:PolicyName}"}"#,
        ),
    ),
    dependent(
        "Access keys",
        ("ListAccessKeys", USER),
        ("AccessKeyMetadata", "AccessKeyId"),
        None,
        (
            "DeleteAccessKey",
            r#"{"UserName":"{id}","AccessKeyId":"{form:AccessKeyId}"}"#,
        ),
        "Deleted access key {name}",
    ),
    dependent(
        "Groups",
        ("ListGroupsForUser", USER),
        ("Groups", "GroupName"),
        Some(("iam", "groups", Text::at("GroupName"))),
        (
            "RemoveUserFromGroup",
            r#"{"UserName":"{id}","GroupName":"{form:GroupName}"}"#,
        ),
        "Removed from group {name}",
    ),
];

/// A group: policies and members.
pub(super) const GROUP_BLOCKERS: &[Blocker] = &[
    attached(
        ("ListAttachedGroupPolicies", GROUP),
        (
            "DetachGroupPolicy",
            r#"{"GroupName":"{id}","PolicyArn":"{form:PolicyArn}"}"#,
        ),
    ),
    inline(
        ("ListGroupPolicies", GROUP),
        (
            "DeleteGroupPolicy",
            r#"{"GroupName":"{id}","PolicyName":"{form:PolicyName}"}"#,
        ),
    ),
    dependent(
        "Members",
        ("GetGroup", GROUP),
        ("Users", "UserName"),
        Some(("iam", "users", Text::at("UserName"))),
        (
            "RemoveUserFromGroup",
            r#"{"GroupName":"{id}","UserName":"{form:UserName}"}"#,
        ),
        "Removed {name} from the group",
    ),
];

/// A role: policies and instance profiles.
pub(super) const ROLE_BLOCKERS: &[Blocker] = &[
    attached(
        ("ListAttachedRolePolicies", ROLE),
        (
            "DetachRolePolicy",
            r#"{"RoleName":"{id}","PolicyArn":"{form:PolicyArn}"}"#,
        ),
    ),
    inline(
        ("ListRolePolicies", ROLE),
        (
            "DeleteRolePolicy",
            r#"{"RoleName":"{id}","PolicyName":"{form:PolicyName}"}"#,
        ),
    ),
    dependent(
        "Instance profiles",
        ("ListInstanceProfilesForRole", ROLE),
        ("InstanceProfiles", "InstanceProfileName"),
        None,
        (
            "RemoveRoleFromInstanceProfile",
            r#"{"RoleName":"{id}","InstanceProfileName":"{form:InstanceProfileName}"}"#,
        ),
        "Removed from instance profile {name}",
    ),
];

/// What a policy is attached to, and the versions beside its default.
pub(super) const POLICY_BLOCKERS: &[Blocker] = &[
    dependent(
        "Roles",
        ("ListEntitiesForPolicy", POLICY),
        ("PolicyRoles", "RoleName"),
        Some(("iam", "roles", Text::at("RoleName"))),
        (
            "DetachRolePolicy",
            r#"{"RoleName":"{form:RoleName}","PolicyArn":"{id}"}"#,
        ),
        "Detached from role {name}",
    ),
    dependent(
        "Users",
        ("ListEntitiesForPolicy", POLICY),
        ("PolicyUsers", "UserName"),
        Some(("iam", "users", Text::at("UserName"))),
        (
            "DetachUserPolicy",
            r#"{"UserName":"{form:UserName}","PolicyArn":"{id}"}"#,
        ),
        "Detached from user {name}",
    ),
    dependent(
        "Groups",
        ("ListEntitiesForPolicy", POLICY),
        ("PolicyGroups", "GroupName"),
        Some(("iam", "groups", Text::at("GroupName"))),
        (
            "DetachGroupPolicy",
            r#"{"GroupName":"{form:GroupName}","PolicyArn":"{id}"}"#,
        ),
        "Detached from group {name}",
    ),
    Blocker {
        except: Some(("IsDefaultVersion", "true")),
        ..dependent(
            "Other versions",
            ("ListPolicyVersions", POLICY),
            ("Versions", "VersionId"),
            None,
            (
                "DeletePolicyVersion",
                r#"{"PolicyArn":"{id}","VersionId":"{form:VersionId}"}"#,
            ),
            "Deleted version {name}",
        )
    },
];
