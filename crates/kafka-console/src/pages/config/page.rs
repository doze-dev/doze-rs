//! The Config page of a topic (board `topic-config`): every config the broker lists, with its value, where it comes from,
//! what it does and when a change counts.
//!
//! What the board shows and the broker cannot say, and what the page does instead (debt rows J121 on):
//! - The cleaner's state ("61 % of the log is dirty") that the dialogs' "Here" lines draw: the cleaner keeps none a
//!   console may read. The lines say what the typed values mean and nothing about the log.
//! - The name of the flag behind a Static flag (`--segment-bytes`): the table knows the broker config, not the flag.

use maud::{Markup, html};

use super::common::config_url;
use super::links::url;
use super::rows::{Ctx, row};
use crate::base::Base;
use crate::config_effects::page_moment;
use crate::config_words::{Group, group_of, is_compaction, is_segments, page_sentence};
use crate::format::number;
use crate::model::{Cleanup, ConfigPage, ConfigRow, ConfigShow, TopicHead};
use crate::pages::topics::{TopicTab, topic_frame};
use crate::ui::disclosure::{Look, disclosure};
use crate::ui::glyph::Glyph;
use crate::ui::kafka::filter_box;
use crate::ui::notice::{Tone, callout};

const HEAD: [(&str, &str); 4] = [
    ("Config", "46%"),
    ("Value", "19%"),
    ("Source", "15%"),
    ("", ""),
];

fn table(rows: &[&ConfigRow], c: &Ctx<'_>) -> Markup {
    html! {
        table class="t" {
            thead { tr {
                @for (name, width) in HEAD {
                    th scope="col" style=[(!width.is_empty()).then(|| format!("width:{width}"))] { (name) }
                }
            } }
            tbody { @for r in rows { (row(r, c)) } }
        }
    }
}

fn pill(n: usize) -> Markup {
    html! { span class="pill" { (n) } }
}

fn section(title: &str, lede: &str, rows: &[&ConfigRow], c: &Ctx<'_>) -> Markup {
    html! {
        div class="sec" {
            div class="row sb" { h2 { (title) " " (pill(rows.len())) } }
            @if !lede.is_empty() { p class="lede" { (lede) } }
            (table(rows, c))
        }
    }
}

fn matches(r: &ConfigRow, q: &str, show: ConfigShow) -> bool {
    let q = q.to_lowercase();
    let by_q = q.is_empty()
        || r.name.to_lowercase().contains(&q)
        || crate::config_words::plain_text(&r.doc)
            .to_lowercase()
            .contains(&q);
    by_q && match show {
        ConfigShow::All => true,
        ConfigShow::Changed => r.changed(),
        ConfigShow::Compaction => is_compaction(&r.name),
        ConfigShow::Segments => is_segments(&r.name),
    }
}

fn chip(label: &str, on: bool, href: &str, count: Option<usize>) -> Markup {
    html! {
        a class={"fchip" @if on { " on" }} href=(href) {
            (label)
            @if let Some(n) = count { span class="tab" { span class="n" { (number(i64::try_from(n).unwrap_or(0))) } } }
        }
    }
}

fn toolbar(p: &ConfigPage, base: &Base) -> Markup {
    let ask = &p.ask;
    let with = |show| {
        let mut a = ask.clone();
        a.show = show;
        url(base, &p.head.name, &a, None)
    };
    let changed = p.rows.iter().filter(|r| r.changed()).count();
    html! {
        form class="toolbar" method="get" action=(config_url(base, &p.head.name, "")) {
            (filter_box("q", "Filter by name or by what it does", "Filter configs", &ask.q))
            @if ask.show != ConfigShow::All { input type="hidden" name="show" value=(ask.show.word()); }
            (chip("All", ask.show == ConfigShow::All, &with(ConfigShow::All), Some(p.rows.len())))
            (chip("Changed", ask.show == ConfigShow::Changed, &with(ConfigShow::Changed), Some(changed)))
            (chip("Compaction", ask.show == ConfigShow::Compaction, &with(ConfigShow::Compaction), None))
            (chip("Segments", ask.show == ConfigShow::Segments, &with(ConfigShow::Segments), None))
        }
    }
}

fn explainer() -> Markup {
    callout(
        Tone::Plain,
        Glyph::Info,
        html! {
            b { "Where a value comes from, first match wins:" }
            " set on this topic (Dynamic topic), then set for this broker or all brokers (Dynamic broker), then a start flag that differs from Kafka's default (Static flag), then Kafka's default. Open "
            b { "Where from" }
            " on any row to see every place."
        },
    )
}

fn replication(rows: &[&ConfigRow], c: &Ctx<'_>, base: &Base, topic: &str) -> Markup {
    let n = rows.len();
    let _ = (base, topic);
    html! {
        div class="sec" {
            // The section is a disclosure: its header is the summary, so it opens in place; `?replication=1` is a deep link that opens it.
            (disclosure(
                Look::Section,
                Some("replication"),
                c.ask.replication,
                html! {
                    div { h2 { (Group::Replication.title()) " " (pill(n)) }
                        p class="lede" { (n) " configs Kafka lists that this broker has no use for: one broker, no replicas, no tiered storage." } }
                    span class="btn sm when-shut" { "Show " (n) }
                    span class="btn sm when-open" { "Hide " (n) }
                },
                table(rows, c),
            ))
        }
    }
}

fn lede_of(g: Group, cleanup: Cleanup) -> &'static str {
    match g {
        Group::Cleanup => "How the cleaner decides.",
        Group::Retention if cleanup == Cleanup::Compact => {
            "Not used by this topic: it only compacts."
        }
        Group::Retention => "When old records are deleted by age or size.",
        _ => "",
    }
}

fn sections(p: &ConfigPage, c: &Ctx<'_>, base: &Base) -> Markup {
    let shown: Vec<&ConfigRow> = p
        .rows
        .iter()
        .filter(|r| matches(r, &p.ask.q, p.ask.show))
        .collect();
    let changed: Vec<&ConfigRow> = shown.iter().copied().filter(|r| r.changed()).collect();
    let in_group = |g: Group| -> Vec<&ConfigRow> {
        shown
            .iter()
            .copied()
            .filter(|r| !r.changed() && group_of(&r.name) == g)
            .collect()
    };
    html! {
        @if p.ask.show != ConfigShow::All || !p.ask.q.is_empty() || !changed.is_empty() {
            (section("Changed from Kafka's default", "Set here or by a flag. These are the ones worth reading first.", &changed, c))
        } @else {
            (section("Changed from Kafka's default", "Nothing is set on this topic and no flag differs from Kafka's defaults.", &changed, c))
        }
        @for g in Group::ALL {
            @let rows = in_group(g);
            @if g == Group::Replication {
                @if !rows.is_empty() { (replication(&rows, c, base, &p.head.name)) }
            } @else if !rows.is_empty() {
                (section(g.title(), lede_of(g, p.head.cleanup), &rows, c))
            }
        }
        @if shown.is_empty() { p class="help" { "No config matches. Clear the filter to see all " (p.rows.len()) "." } }
    }
}

/// The page.
#[must_use]
pub fn config_page(p: &ConfigPage, head: &TopicHead, base: &Base) -> Markup {
    let sentence = page_sentence(&p.rows, &page_moment(p.pace));
    let c = Ctx {
        base,
        topic: &p.head.name,
        ask: &p.ask,
        pace: p.pace,
    };
    html! {
        (topic_frame(head, base, TopicTab::Config))
        // What this tab says of the topic, under the tabs: the head above them is the topic's own, the same on every tab.
        p class="sent" data-extra="live-sentence" style="margin:0 0 20px;max-width:900px" { (crate::ui::write::spans(&sentence)) }
        (toolbar(p, base))
        div style="margin-bottom:28px" { (explainer()) }
        (sections(p, &c, base))
    }
}

/// The page for a config the topic does not have: the name is shown as text.
#[must_use]
pub fn no_config(topic: &str, key: &str, base: &Base) -> Markup {
    html! {
        (crate::ui::layout::page_header("No such config", html! { p class="sub" {
            "The topic " span class="mono" { (topic) } " has no config called " span class="mono" { (key) } "."
        } }, html! {}))
        div class="row" { (crate::ui::button::link(&config_url(base, topic, ""), crate::ui::button::Variant::Primary, crate::ui::button::Size::Normal, Some(Glyph::ArrowR), "Back to the config")) }
    }
}
