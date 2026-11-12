//! The pages of the topic writes: Create topic (three steps, refused, created), Produce (dialog, refused, sent), Add
//! partitions, Delete records, Delete topic (dialogs, and their results). Each is written from its board.
//!
//! What a board shows and the broker cannot say, and what the page does instead (each is a debt row, J41 on):
//! - The wire table under a refused Produce ("What the wire recorded"): the recorder, slice 9e. Left out.
//! - The "Size" of a topic in the Produced and Deleted pages: the partition files (`ConsoleView`, the read builder).
//!   Left out; the figures are records, which Kafka's offsets give.
//! - "Last written just now" and the transaction sentence of the Produced page are read, not assumed.

mod create;
mod created;
mod delete;
mod delete_records;
mod partitions_add;
mod produce;
mod produced;

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::ui::button::{Size, Variant, link};
use crate::ui::glyph::Glyph;
use crate::ui::layout::page_header;

pub use create::{create_page, create_refused};
pub use created::created;
pub use delete::{delete_dialog, deleted_page};
pub use delete_records::{delete_records_dialog, delete_records_refused};
pub use partitions_add::add_partitions_dialog;
pub use produce::{produce_dialog, produce_refused};
pub use produced::produced;

/// `/topics/{topic}{tail}` under the base, with the name made safe for a path.
fn topic_url(base: &Base, topic: &str, tail: &str) -> String {
    base.url(&format!("/topics/{}{tail}", segment(topic)))
}

/// The page for a topic that is not there: the name is shown as text.
#[must_use]
pub fn no_topic(topic: &str, base: &Base) -> Markup {
    html! {
        (page_header("No such topic", html! { p class="sub" { "There is no topic called " span class="mono" { (topic) } "." } }, html! {}))
        div class="row" {
            (link(&base.url("/topics"), Variant::Primary, Size::Normal, Some(Glyph::ArrowR), "Back to topics"))
            (link(&base.url("/jump"), Variant::Plain, Size::Normal, Some(Glyph::Search), "Find a topic"))
        }
    }
}

/// A field: its label bound to its control, then whatever goes under it.
fn field(id: &str, label: Markup, control: Markup, under: Markup) -> Markup {
    html! { div class="field" { label for=(id) { (label) } (control) (under) } }
}

/// The small grey word beside a label (`optional on this topic`, `now 3`).
fn opt(text: &str) -> Markup {
    html! { " " span class="opt" { (text) } }
}

/// A kit button: a submit button of a form. `op` is the Kafka API it calls (`data-op`).
fn submit(class: &str, op: Option<&str>, name: Option<(&str, &str)>, label: Markup) -> Markup {
    html! {
        button class=(class) type="submit" data-op=[op] name=[name.map(|n| n.0)] value=[name.map(|n| n.1)] { (label) }
    }
}

/// A hidden input per pair.
fn hidden(pairs: &[(&str, &str)]) -> Markup {
    html! { @for (n, v) in pairs { input type="hidden" name=(n) value=(v); } }
}

#[cfg(test)]
mod tests;
