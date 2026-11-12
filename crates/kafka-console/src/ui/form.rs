//! Forms: a plain `<form method=post>` that works with no script, and its fields.

use maud::{Markup, html};

/// A labelled field with its help line. `control` is the input; `id` ties the label to it.
#[must_use]
pub fn field(id: &str, label: &str, help: Option<&str>, control: Markup) -> Markup {
    html! {
        div class="field" {
            label for=(id) { (label) }
            (control)
            @if let Some(h) = help { div class="help" { (h) } }
        }
    }
}

/// The teaching line under a field: what to know in one sentence, the bold lead and the rest.
#[must_use]
pub fn teach(lead: &str, rest: &str) -> Markup {
    html! { div class="teach" { b { (lead) } " " span class="dim" { (rest) } } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_ties_its_label_to_its_control() {
        let m = field(
            "name",
            "Topic",
            Some("Up to 249 bytes"),
            html! { input id="name" name="name"; },
        )
        .into_string();
        assert_eq!(
            m,
            r#"<div class="field"><label for="name">Topic</label><input id="name" name="name"><div class="help">Up to 249 bytes</div></div>"#
        );
        assert_eq!(
            teach("A", "b").into_string(),
            r#"<div class="teach"><b>A</b> <span class="dim">b</span></div>"#
        );
    }
}
