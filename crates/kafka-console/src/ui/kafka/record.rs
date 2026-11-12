//! The fields of one record: a labelled block with the size beside the label, and the list of headers.

use maud::{Markup, html};

/// A field of a drawer: its label, a note beside the label (`9 bytes · text`), and the block under it.
#[must_use]
pub fn field(label: &str, note: &str, body: Markup) -> Markup {
    html! {
        div class="field" style="margin-bottom:16px" {
            div class="lab" { (label) " " span class="opt" { (note) } }
            (body)
        }
    }
}

/// The headers of a record, name and value on a row each.
#[must_use]
pub fn headers(rows: &[(String, String)]) -> Markup {
    html! {
        div class="hdrs" {
            @for (k, v) in rows { div class="h" { span { (k) } span { (v) } } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_escaped_names_and_values() {
        let m = headers(&[("a<b".to_owned(), "\"c\"".to_owned())]).into_string();
        assert_eq!(
            m,
            r#"<div class="hdrs"><div class="h"><span>a&lt;b</span><span>&quot;c&quot;</span></div></div>"#
        );
    }
}
