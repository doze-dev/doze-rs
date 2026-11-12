//! The boards of slice 9h that write: the edit dialog of a broker config (IncrementalAlterConfigs on the BROKER resource) and the
//! two ACL dialogs (CreateAcls, DeleteAcls). `actions_match_boards` compares their `data-op` names with
//! [`super::Action::ALL`] besides the writes of the earlier slices.

/// The boards whose `data-op` names are writes.
pub const BOARDS: [&str; 3] = [
    "broker-config-edit",
    "access-acl-create",
    "access-acl-delete",
];
