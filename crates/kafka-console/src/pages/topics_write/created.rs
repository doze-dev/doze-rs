//! The state after a topic is made (board `topic-created`).

use maud::{Markup, html};

use super::topic_url;
use crate::base::Base;
use crate::calls::{kcat_consume, kcat_produce};
use crate::model::Created;
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_extra::{Extra, glyph_extra};
use crate::validate::retention_words;

fn command(id: &str, lines: &[String]) -> Markup {
    html! { pre class="code" id=(id) data-extra="copy-as" { (lines.join("\n")) } }
}

fn copy(id: &str) -> Markup {
    client::copy_from_button("btn sm", id, html! { (glyph(Glyph::Copy, 14, "")) "Copy" })
}

/// The page: the banner that says what was made, the empty topic, and the two next steps.
#[must_use]
pub fn created(c: &Created, base: &Base) -> Markup {
    let words = retention_words(&c.retention);
    let retention = if words.is_empty() {
        c.retention.clone()
    } else {
        format!("retention {}", words.trim_start_matches("= "))
    };
    let sub = if c.empty {
        format!(
            "Empty: offsets 0 to 0 on all {} partitions. Nothing has written to it yet.",
            c.partitions
        )
    } else {
        format!("{} partitions.", c.partitions)
    };
    html! {
        div class="banner" style="margin-top:24px" {
            (glyph(Glyph::Check, 18, ""))
            span { b { "Created " (c.name) "." } " " (c.partitions) " partitions, cleanup.policy " (c.cleanup) ", " (retention) ". It is on disk now: it will be there after a power cut." }
        }
        div class="page-h" {
            div { h1 class="mono" { (c.name) } p class="sub" { (sub) } }
            div class="acts" {
                a class="btn pri" href=(topic_url(base, &c.name, "/produce")) { (glyph_extra(Extra::Send, 16)) "Produce a record" }
                a class="btn" href=(topic_url(base, &c.name, "")) { "Open topic" }
            }
        }
        div class="sec" {
            h2 { "Next steps" }
            p class="lede" { "Write a first record, then read it back with a consumer in a group." }
            div class="g2" {
                div class="card" {
                    h2 { "1 · Produce" }
                    p class="lede" { "From your app, or from a terminal." }
                    (command("kcat-produce", &kcat_produce(&c.address, &c.name)))
                    div class="row" style="margin-top:12px" { (copy("kcat-produce")) }
                }
                div class="card" {
                    h2 { "2 · Add a consumer" }
                    p class="lede" { "A group id makes it a consumer group; the console lists it under Groups." }
                    (command("kcat-consume", &kcat_consume(&c.address, &c.name)))
                    div class="row" style="margin-top:12px" {
                        (copy("kcat-consume"))
                        a class="btn sm" href=(base.url("/groups")) { "Open Groups" }
                    }
                }
            }
        }
    }
}
