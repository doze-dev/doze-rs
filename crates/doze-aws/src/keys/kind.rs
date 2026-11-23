//! What a setting is: its kind, and the row of the table in `keys.rs` that describes it.

/// What kind of value a setting takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Text.
    Text,
    /// A whole number of at least this.
    Count(u64),
    /// On or off (a flag with no value on the command line).
    Switch,
    /// A list (comma-separated on the command line and in the environment, an array in the file).
    List,
    /// One of these words.
    Choice(&'static [&'static str]),
}

/// One setting.
#[derive(Debug)]
pub(crate) struct Key {
    /// The flag without its dashes, the file key, and (upper-cased, `-` as `_`, after `DOZE_AWS_`) the variable.
    pub(crate) name: &'static str,
    /// What it takes.
    pub(crate) kind: Kind,
    /// Its default, as written in the file (a list's items comma-separated).
    pub(crate) default: &'static str,
    /// The placeholder in help (`ADDR`).
    pub(crate) value_name: &'static str,
    /// One line of help.
    pub(crate) help: &'static str,
    /// The heading it is listed under.
    pub(crate) group: &'static str,
}

/// A row of the table.
pub(super) const fn key(
    name: &'static str,
    kind: Kind,
    default: &'static str,
    value_name: &'static str,
    help: &'static str,
    group: &'static str,
) -> Key {
    Key {
        name,
        kind,
        default,
        value_name,
        help,
        group,
    }
}
