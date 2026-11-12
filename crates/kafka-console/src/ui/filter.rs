//! Filters and choices: the segmented control and the toolbar's input.

use maud::{Markup, html};

/// The class of a segmented button.
#[must_use]
pub const fn seg_class(on: bool) -> &'static str {
    if on { "on" } else { "" }
}

/// The box of a segmented control around its buttons.
#[must_use]
pub fn seg(buttons: Markup) -> Markup {
    html! { div class="seg" { (buttons) } }
}

/// A filter input of the toolbar: a plain GET form field named `name`, so it works with no script.
#[must_use]
pub fn input(name: &str, placeholder: &str, value: &str) -> Markup {
    html! {
        label class="finput" {
            input type="search" name=(name) placeholder=(placeholder) value=(value) aria-label=(placeholder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_input_is_a_named_field_with_its_value_escaped() {
        assert_eq!(
            input("q", "Filter", "a\"b").into_string(),
            r#"<label class="finput"><input type="search" name="q" placeholder="Filter" value="a&quot;b" aria-label="Filter"></label>"#
        );
        assert_eq!(seg_class(true), "on");
        assert_eq!(seg_class(false), "");
    }
}
