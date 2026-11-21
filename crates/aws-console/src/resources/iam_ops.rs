//! What the IAM pages of the console may ask of IAM and STS without a resource bound: the forms that make a user, group, role or
//! policy (with the memberships and attachments the create wizards add after it), the lists a page reads for each row (a user's
//! access keys, a group's policies), the access key and policy version calls, the account's alias and summary, and STS's
//! `AssumeRole` for the Switch Role page. Each kind lists only what its own pages call; the account's calls ride on the users,
//! which the dashboard opens, and STS's on the roles, which a session is taken of.

use super::{Op, ops::free};

/// A user's pages: the wizard (`CreateUser`, its groups and policies), the keys (made, used, switched off, deleted), what a user
/// belongs to and has attached, and the account the dashboard shows.
pub(super) const USER_OPS: &[Op] = &[
    free("CreateUser"),
    free("ListAccessKeys"),
    free("GetAccessKeyLastUsed"),
    free("CreateAccessKey"),
    free("UpdateAccessKey"),
    free("DeleteAccessKey"),
    free("ListGroupsForUser"),
    free("ListAttachedUserPolicies"),
    free("ListUserPolicies"),
    free("AddUserToGroup"),
    free("RemoveUserFromGroup"),
    free("AttachUserPolicy"),
    free("DetachUserPolicy"),
    // The account: the dashboard's counts and the alias form.
    free("GetAccountSummary"),
    free("ListAccountAliases"),
    free("CreateAccountAlias"),
    free("DeleteAccountAlias"),
];

/// A group's pages: the form (with its members and policies), and what each group holds.
pub(super) const GROUP_OPS: &[Op] = &[
    free("CreateGroup"),
    free("GetGroup"),
    free("ListAttachedGroupPolicies"),
    free("AddUserToGroup"),
    free("RemoveUserFromGroup"),
    free("AttachGroupPolicy"),
    free("DetachGroupPolicy"),
];

/// A role's pages: the wizard (`CreateRole` and the policies attached after), the trust policy, and the session Switch Role takes.
pub(super) const ROLE_OPS: &[Op] = &[
    free("CreateRole"),
    free("ListAttachedRolePolicies"),
    free("AttachRolePolicy"),
    free("DetachRolePolicy"),
    free("UpdateAssumeRolePolicy"),
    free("AssumeRole").of("sts"),
];

/// A policy's pages: the editor, its versions, and what a policy is attached to.
pub(super) const POLICY_OPS: &[Op] = &[
    free("CreatePolicy"),
    free("ListPolicies"),
    free("ListPolicyVersions"),
    free("GetPolicyVersion"),
    free("CreatePolicyVersion"),
    free("SetDefaultPolicyVersion"),
    free("DeletePolicyVersion"),
    free("ListEntitiesForPolicy"),
    free("AttachUserPolicy"),
    free("AttachGroupPolicy"),
    free("AttachRolePolicy"),
];
