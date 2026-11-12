//! A log with nothing in it: the ruler says so, and the page says how the first record gets there.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::format::{bytes, span_ms};
use crate::model::LogView;

use crate::ui::button::{Size, Variant, link};
use crate::ui::glyph::Glyph;
use crate::ui::glyph_log::{LogGlyph, log_glyph};

/// The empty state, as the board draws it: a glyph, what is true, the command that fills it, and the way to Produce.
#[must_use]
pub(super) fn empty(v: &LogView, base: &Base, listen: &str) -> Markup {
    let produce = base.url(&format!("/topics/{}/produce", segment(&v.topic)));
    let rolls = v
        .segment_ms
        .map_or_else(String::new, |ms| format!(" or is {} old", span_ms(ms)));
    html! {
        div class="empty" {
            div class="ic" { (log_glyph(LogGlyph::Log, 24, "")) }
            h3 { "This log has nothing in it" }
            p {
                "The first record written lands at offset " (v.end) ". Segments appear when the active one reaches "
                span class="mono" { "segment.bytes" } " (" (bytes(v.segment_bytes)) ")" (rolls) "."
            }
            pre class="code" style="text-align:left;margin-bottom:20px" { (format!("echo '{{\"order_id\":\"ord-88412\"}}' | kcat -P -b {listen} -t {}", v.topic)) }
            (link(&produce, Variant::Plain, Size::Normal, Some(Glyph::Send), "Produce a record"))
        }
    }
}
