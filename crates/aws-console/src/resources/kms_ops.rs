//! What the KMS pages of the console may ask of KMS without a resource bound: the wizard that makes a key (and the alias it gives
//! the key with a second call), the key actions of the list (Enable, Disable, Schedule key deletion, Cancel key deletion), and what a
//! key's own page reads and writes (its policy, rotation, description and aliases). A key page names its key in the input, so the
//! page can make these on a key it was not opened on; each is the service's own call, made as the caller `console`.

use super::{Op, ops::free};

/// The keys' pages: the list's actions, the wizard's `CreateKey`, and a key's policy, rotation and aliases.
pub(super) const KEY_OPS: &[Op] = &[
    free("CreateKey"),
    free("EnableKey"),
    free("DisableKey"),
    free("ScheduleKeyDeletion"),
    free("CancelKeyDeletion"),
    free("DescribeKey"),
    free("GetKeyPolicy"),
    free("PutKeyPolicy"),
    free("GetKeyRotationStatus"),
    free("EnableKeyRotation"),
    free("DisableKeyRotation"),
    free("RotateKeyOnDemand"),
    free("ListKeyRotations"),
    free("ListAliases"),
];

/// The aliases' pages: the form that makes one, the delete, and the list a key's page reads for its own.
pub(super) const ALIAS_OPS: &[Op] = &[
    free("CreateAlias"),
    free("DeleteAlias"),
    free("ListAliases"),
];
