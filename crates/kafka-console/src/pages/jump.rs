//! Jump to a topic: the palette (an overlay a script opens with ⌘K or Ctrl+K, or by a click on the top bar's search) and its
//! page (the same list as links, for a browser with no script). The names are the broker's own, from Metadata; every one is
//! text. Groups join the list when the Groups pages exist (slice 9c): a link to a page that is not built would be worse than
//! no link.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::layout::page_header;
use crate::ui::overlay::{Kind, transient};

/// The most names the palette lists; the page says when there are more.
pub(super) const MOST: usize = 500;

fn list(names: &[String], base: &Base) -> Markup {
    html! {
        ul class="jump-list" id="jump-list" aria-label="Topics" {
            @for n in names.iter().take(MOST) {
                li { a class="jump-item" href=(base.url(&format!("/topics/{}", segment(n)))) data-jump=(n.to_lowercase()) {
                    (glyph(Glyph::Topic, 16, "")) span class="mono" { (n) }
                } }
            }
        }
    }
}

fn more(total: usize) -> Markup {
    html! { @if total > MOST { p class="help" { "Showing the first " (MOST) " of " (total) " topics. Type a longer name to narrow them on the Topics page." } } }
}

/// The palette: a field, the topics, one line of keys. Opened in place by the script; it has no parent page of its own.
#[must_use]
pub fn jump_overlay(names: &[String], total: usize, base: &Base) -> Markup {
    transient(
        Kind::Dialog,
        "jump-title",
        &base.url("/jump"),
        html! {
            div class="jump" {
                h2 id="jump-title" class="sr" { "Jump to a topic" }
                div class="input" {
                    (glyph(Glyph::Search, 16, ""))
                    input id="jump-q" type="text" placeholder="Jump to a topic" aria-label="Jump to a topic" autocomplete="off" spellcheck="false" data-jump-input;
                }
                (list(names, base))
                p class="help jump-empty" hidden { "No topic is named like that." }
                @if names.is_empty() { p class="help" { "This broker holds no topics yet." } }
                (more(total))
                p class="help jump-keys" { kbd { "↑" } " " kbd { "↓" } " move " kbd { "Enter" } " open " kbd { "Esc" } " close" }
            }
        },
    )
}

/// The same list as a page, for a browser with no script: a plain GET filter and plain links.
#[must_use]
pub fn jump_page(names: &[String], total: usize, filter: &str, base: &Base) -> Markup {
    html! {
        (page_header("Jump to a topic", html! { p class="sub" { "Every topic of this broker, by name. A topic page has its Messages, Partitions and the rest." } }, html! {}))
        form class="toolbar jump-form" method="get" action=(base.url("/jump")) role="search" {
            div class="input mono" {
                (glyph(Glyph::Search, 16, ""))
                input id="q" type="text" name="q" value=(filter) placeholder="Part of a name" aria-label="Part of a topic name";
            }
            button class="btn" type="submit" { "Filter" }
        }
        div class="jump-page" {
            @if names.is_empty() { p class="help" { "No topic is named like that." } } @else { (list(names, base)) }
            (more(total))
        }
    }
}
