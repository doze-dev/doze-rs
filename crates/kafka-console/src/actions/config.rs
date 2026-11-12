//! The boards of slice 9d, the Config pages: the ones whose `data-op` names `actions_match_boards` compares with
//! [`super::Action::ALL`] besides the topic writes of 9b.

/// The eight boards of the Config pages, by id.
pub const BOARDS: [&str; 8] = [
    "topic-config",
    "config-edit",
    "config-edit-list",
    "config-edit-lag",
    "config-saved",
    "config-refused",
    "config-refused-value",
    "config-reset",
];
