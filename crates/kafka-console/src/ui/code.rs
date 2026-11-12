//! Code blocks: a snippet's tokens are coloured by kind, never by HTML that came from elsewhere.

use maud::{Markup, html};

use crate::model::Tok;

/// A code block of tokens (`Tok`), one line per entry; `style` is the block's inline style, if the board has one.
#[must_use]
pub fn block(lines: &[Vec<(Tok, String)>], id: Option<&str>, style: Option<&str>) -> Markup {
    html! {
        pre class="code" id=[id] style=[style] {
            @for (i, line) in lines.iter().enumerate() {
                @if i > 0 { "\n" }
                @for (kind, text) in line {
                    @match kind {
                        Tok::Plain => (text),
                        Tok::Keyword => span class="k" { (text) },
                        Tok::Str => span class="s" { (text) },
                        Tok::Comment => span class="c" { (text) },
                    }
                }
            }
        }
    }
}

/// A block of plain text (a command, a flag list), with its own id when something copies from it.
#[must_use]
pub fn pre(text: &str, id: Option<&str>, style: Option<&str>) -> Markup {
    html! { pre class="code" id=[id] style=[style] { (text) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_coloured_by_kind_and_escaped() {
        let lines = vec![
            vec![
                (Tok::Keyword, "cl".to_owned()),
                (Tok::Plain, " := <x>".to_owned()),
            ],
            vec![(Tok::Str, "\"a\"".to_owned())],
        ];
        assert_eq!(
            block(&lines, None, None).into_string(),
            "<pre class=\"code\"><span class=\"k\">cl</span> := &lt;x&gt;\n<span class=\"s\">&quot;a&quot;</span></pre>"
        );
    }
}
