//! The interactive components, in the shadcn/ui look: Select, Combobox, DropdownMenu, Tooltip, Popover and the segmented
//! control. Each is a maud function that renders COMPLETE, working markup with no script (a real `<select>`, links in a list, a
//! `title`), and carries the `x-data` of the Alpine component in `assets/components.js` that turns it into the enhanced control
//! when the script runs. The contract of each (markup, attributes, keys, events) is in `docs/port/console/components.md`.
//!
//! Every label, value and description is text: it is escaped by maud, and the script copies it with `textContent`, never as markup.

use maud::{Markup, PreEscaped, html};

mod combobox;
#[cfg(feature = "examples")]
pub mod gallery;
mod menu;
mod seg;
mod select;
mod tip;

pub use combobox::{Combobox, combobox};
pub use menu::{Menu, MenuItem, menu};
pub use seg::{SegItem, seg};
pub use select::{Select, select};
pub use tip::{popover, tip};

/// One choice of a Select or a Combobox.
#[derive(Debug, Clone, Copy)]
pub struct Choice<'a> {
    /// What is submitted. Empty is allowed ("All topics").
    pub value: &'a str,
    /// What is shown.
    pub label: &'a str,
    /// A muted second line, if any.
    pub desc: Option<&'a str>,
    pub disabled: bool,
}

impl<'a> Choice<'a> {
    #[must_use]
    pub const fn new(value: &'a str, label: &'a str) -> Self {
        Self {
            value,
            label,
            desc: None,
            disabled: false,
        }
    }

    #[must_use]
    pub const fn desc(mut self, desc: &'a str) -> Self {
        self.desc = Some(desc);
        self
    }

    #[must_use]
    pub const fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

/// maud cannot spell an attribute with a dot in its name, and Alpine's modifiers are dotted (`x-on:click.outside`,
/// `x-anchor.bottom-start.offset.4`, `:aria-expanded`): the components write the attribute with a stand-in name, and this puts the real one back
/// in the finished markup. The stand-ins are constants of the components, never data; text that happens to contain one is only text.
pub(crate) fn alpine(m: Markup) -> Markup {
    const NAMES: [(&str, &str); 7] = [
        (" xb-data-open=", " :data-open="),
        (" xb-aria-expanded=", " :aria-expanded="),
        (" xo-click=", " x-on:click.outside="),
        (" xa-bottom-start-4=", " x-anchor.bottom-start.offset.4="),
        (" xa-bottom-start-6=", " x-anchor.bottom-start.offset.6="),
        (" xa-bottom-end-4=", " x-anchor.bottom-end.offset.4="),
        (" xa-top-6=", " x-anchor.top.offset.6="),
    ];
    let mut s = m.into_string();
    for (from, to) in NAMES {
        s = s.replace(from, to);
    }
    PreEscaped(s)
}

/// The glyphs of the components (their own, 16 by 16, 1.5 stroke like the kit's).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Icon {
    Chevron,
    Check,
    Dots,
    Search,
}

impl Icon {
    const fn paths(self) -> &'static str {
        match self {
            Self::Chevron => r#"<path d="M4 6.2 8 10l4-3.8"/>"#,
            Self::Check => r#"<path d="M3.4 8.6l3 3L12.6 4.6"/>"#,
            Self::Dots => {
                r#"<circle cx="3.4" cy="8" r="1.1" fill="currentColor"/><circle cx="8" cy="8" r="1.1" fill="currentColor"/><circle cx="12.6" cy="8" r="1.1" fill="currentColor"/>"#
            }
            Self::Search => r#"<circle cx="7" cy="7" r="4.5"/><path d="M10.4 10.4 14 14"/>"#,
        }
    }
}

pub(crate) fn icon(i: Icon, class: &str) -> Markup {
    html! { svg class={"gl " (class)} width="16" height="16" viewBox="0 0 16 16" aria-hidden="true" { (PreEscaped(i.paths())) } }
}

/// The list of options both Select and Combobox draw: one `role=option` row per choice, with a check that CSS shows for the
/// chosen one. `id` is the component's id; the rows are `{id}-o{n}`.
pub(crate) fn options(id: &str, choices: &[Choice<'_>], value: &str) -> Markup {
    html! {
        @for (n, c) in choices.iter().enumerate() {
            div class="dz-opt" role="option" id=(format!("{id}-o{n}")) data-value=(c.value) data-label=(c.label)
                aria-selected=(if c.value == value { "true" } else { "false" }) aria-disabled=[c.disabled.then_some("true")] {
                (icon(Icon::Check, "dz-check"))
                span class="dz-opt-t" { span class="dz-opt-label" { (c.label) } @if let Some(d) = c.desc { span class="dz-desc" { (d) } } }
            }
        }
    }
}

/// The chosen choice's label, or the placeholder.
pub(crate) fn shown<'a>(
    choices: &[Choice<'a>],
    value: &str,
    placeholder: &'a str,
) -> (&'a str, bool) {
    choices
        .iter()
        .find(|c| c.value == value)
        .map_or((placeholder, false), |c| (c.label, true))
}

#[cfg(test)]
mod tests;
