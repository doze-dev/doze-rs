//! The Data section: where everything the broker keeps lives, how big it is, what the last stop vouched for, and what a
//! restart does and does not carry over.

use maud::{Markup, html};

use crate::format::{bytes, count, number};
use crate::model::{SettingsData, SettingsIntervals, StopView};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};

fn stop(s: StopView) -> Markup {
    match s {
        StopView::Clean => html! {
            span class="st ok" { "Clean" }
            span class="sub" { "the stop was recorded, so every partition was trusted at start without being read back" }
        },
        StopView::Unclean => html! {
            span class="st warn" { "Not clean" }
            span class="sub" { "the last run did not vouch for its data (a crash or a kill), so every partition was checked at start; nothing acknowledged is lost" }
        },
        StopView::None => html! {
            span class="st idle" { "None yet" }
            span class="sub" { "there was no earlier run: this data directory is new" }
        },
    }
}

fn size(d: &SettingsData) -> Markup {
    match d.size {
        Some((b, cut)) => html! {
            (bytes(b)) @if cut { " or more" }
            span class="sub" { (count(usize::try_from(d.files).unwrap_or(usize::MAX), "file", "files")) @if cut { ", counted until the walk was cut" } }
        },
        None => html! { "unknown" },
    }
}

fn directory(d: &SettingsData) -> Markup {
    html! {
        div class="card" {
            h2 { "The data directory" }
            div class="row" style="margin:12px 0 18px" {
                span class="conn" style="padding:10px 14px" { span class="node self" { (d.dir) } }
                (client::copy_button("btn sm", &d.dir, html! { (glyph(Glyph::Copy, 14, "")) "Copy path" }))
            }
            dl class="kv sm" {
                dt { "Set by" } dd class="mono" { "--data-dir" @if d.flag_set { span class="sub" style="font-family:Inter,sans-serif" { "given" } } }
                dt { "Size" } dd class="mono" { (size(d)) }
                dt { "Partitions on disk" } dd {
                    (number(i64::try_from(d.partitions).unwrap_or(0)))
                    span class="sub" { "in " (count(d.topics, "topic", "topics")) "; __consumer_offsets keeps no log here" }
                }
                dt { "Last stop" } dd { (stop(d.last_stop)) }
                dt { "Inside" } dd {
                    "The metadata database"
                    span class="sub" { "topics and their configs, committed offsets, transactional ids, the five dynamic broker configs, the ACLs and the users made over the API" }
                    span class="sub" { "and one directory for each partition, named for its topic and number, such as orders-0, with its segments and their indexes" }
                }
            }
        }
    }
}

fn restart(i: &SettingsIntervals) -> Markup {
    let flush = i.rows.iter().find(|r| r.flag == "--fsync-interval");
    html! {
        div class="card" {
            h2 { "What a restart does" }
            p class="sent" style="margin:8px 0 14px" {
                "SIGTERM or Ctrl-C is a clean stop: every open partition is flushed and closed and the stop is recorded, while a kill is not, and the next start distrusts every partition and the first flush covers what it reads back (nothing acknowledged is lost). Your data is where it was, including topics, records, configs, committed offsets, producer ids and open transactions, while connections and group members start again from nothing. Stored ACLs and the users made over the API stay; users from flags are re-applied."
            }
            p class="help" style="margin:0" {
                "A write that asked for acks=all is flushed before it is answered; one with acks=1 within "
                @if let Some(f) = flush { span class="mono" { (f.value) } ", set by " span class="mono" { "--fsync-interval" } } @else { "the flush interval" }
                "."
            }
        }
    }
}

/// The section.
#[must_use]
pub(super) fn data_section(d: &SettingsData, i: &SettingsIntervals) -> Markup {
    html! {
        div class="sec" id="data" {
            h2 { "Data" }
            p class="lede" {
                "Everything this broker keeps lives in one directory. Stop it cleanly and the directory can be copied, moved or deleted as a whole."
                @if let Some((b, cut)) = d.size { " It holds " b { (bytes(b)) @if cut { " or more" } } "." }
            }
            div class="g2" style="align-items:start" { (directory(d)) (restart(i)) }
        }
    }
}
