//! The short ways a descriptor writes its columns, fields and inputs.

use super::{Column, Field, Format, Input, InputKind};

/// A field with no link that is never hidden.
#[must_use]
pub(crate) const fn field(label: &'static str, path: &'static str, format: Format) -> Field {
    Field {
        label,
        path,
        format,
        link: &[],
        hide_when: None,
        empty: "",
        retires: false,
    }
}

/// A field whose absence means `empty` (`None`, `Disabled`).
#[must_use]
pub(crate) const fn or(
    label: &'static str,
    path: &'static str,
    format: Format,
    empty: &'static str,
) -> Field {
    Field {
        empty,
        ..field(label, path, format)
    }
}

/// A column.
#[must_use]
pub(crate) const fn col(title: &'static str, path: &'static str, format: Format) -> Column {
    Column {
        title,
        path,
        format,
    }
}

/// An input that is not required, empty at first.
#[must_use]
pub(crate) const fn input(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    kind: InputKind,
) -> Input {
    Input {
        key,
        label,
        help,
        kind,
        required: false,
        placeholder: "",
        value: "",
    }
}
