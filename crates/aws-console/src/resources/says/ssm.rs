//! Parameter Store: how a parameter's page speaks. Its sentence needs who reads it and the page's own reads, which the app has; the
//! header offers Edit (a form of its own).

use super::super::{Page, Verb};

/// A parameter's header: Edit.
pub const PARAMETER: Page = Page {
    sentence: None,
    badges: None,
    verbs: &[Verb::ui("edit", "Edit")],
    summary: None,
};
