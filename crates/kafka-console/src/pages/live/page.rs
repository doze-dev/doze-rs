//! The Live page and its fragment: one fragment holds the title, the sentence, the filters, the refusals and the tape, so a
//! tick refreshes them together. The page is complete with no script: every filter is a plain GET, Pause is a link to the page
//! as it was (`?until=`), and a row's time is a link to its drawer.

use maud::{Markup, html};

use super::parts::{LIVE_FRAGMENT, request_url, wire_url};
use super::tape::{Layout, table};
use crate::base::Base;
use crate::format::number;
use crate::model::{LiveState, LiveView, Span};
use crate::ui::button::{Size, Variant, link};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_live::{LiveGlyph, live_glyph};
use crate::ui::kafka::{filter_box, pager};
use crate::ui::layout::page_header;
use crate::ui::notice::{Tone, callout};
use crate::words::live::{live_sentence, load_line, pager_line, paused_clock};

fn spans(parts: &[Span]) -> Markup {
    html! { @for p in parts { @match p { Span::Text(t) => (t), Span::Mono(t) => span class="mono" { (t) }, Span::Strong(t) => b { (t) } } } }
}

/// The Pause control: a link to the page as it is now (`?until=<newest row>`), which a script turns into a client-side pause.
fn pause(v: &LiveView, base: &Base) -> Markup {
    if v.paused_at_ms.is_some() {
        let mut q = v.query.clone();
        q.until = None;
        let resume = format!("{}{}", base.url("/live"), q.to_query());
        return html! {
            a class="btn pri" href=(resume) data-resume { (live_glyph(LiveGlyph::Play, 16)) "Resume" }
        };
    }
    let mut q = v.query.clone();
    q.until = Some(v.newest_id);
    q.before = None;
    html! {
        a class="btn" href={(base.url("/live")) (q.to_query())} data-ui=[(v.state != LiveState::Steady).then_some("pause")] data-pause data-seen=(v.seen) { "Pause" }
    }
}

fn select(name: &str, all: &str, current: &str, options: &[String]) -> Markup {
    html! {
        label class="fchip" {
            select name=(name) aria-label=(all) data-autosubmit {
                option value="" selected[current.is_empty()] { (all) }
                @for o in options { option value=(o) selected[o == current] { (o) } }
                @if !current.is_empty() && !options.iter().any(|o| o == current) { option value=(current) selected { (current) } }
            }
            (live_glyph(LiveGlyph::ChevD, 14))
        }
    }
}

fn toggle(name: &str, on: bool, label: Markup) -> Markup {
    html! {
        button class={"fchip" @if on { " on" }} type="submit" name=(name) value=(if on { "0" } else { "1" }) data-ui="filter" aria-pressed=(on) { (label) }
    }
}

fn toolbar(v: &LiveView, base: &Base, with_search: bool) -> Markup {
    let q = &v.query;
    html! {
        form class="toolbar" method="get" action=(base.url("/live")) data-live-filters {
            input type="submit" value="Filter" aria-label="Filter the wire" hidden data-scaffold;
            @if with_search { (filter_box("q", "Filter by client, API or topic", "Filter the wire", &q.text)) }
            (select("client", "All apps", &q.client, &v.clients_ids))
            (select("topic", "All topics", &q.topic, &v.topics_seen))
            // A toggle's own button carries its new value; the hidden fields keep the other's state.
            (toggle("refused", q.refused_only, html! { (glyph(Glyph::Filter, 14, "")) "Refused only" }))
            // "Idle polls hidden" is on by default: its button says what a click does by what it is now.
            button class={"fchip" @if q.hide_idle { " on" }} type="submit" name="idle" value=(if q.hide_idle { "0" } else { "1" }) data-ui="toggle" aria-pressed=(q.hide_idle) { "Idle polls hidden" }
            @if !q.api.is_empty() { input type="hidden" name="api" value=(q.api); }
            @if !q.group.is_empty() { input type="hidden" name="group" value=(q.group); }
            @if let Some(u) = q.until { input type="hidden" name="until" value=(u); }
            span class="sp" {}
            @if v.clients > 0 {
                (client::dialog_link("btn sm", &base.url("/live/clients"), html! { (live_glyph(LiveGlyph::Network, 16)) "Clients · " (v.clients) }))
            }
        }
    }
}

fn refusals(v: &LiveView, base: &Base) -> Markup {
    if v.refused.is_empty() {
        return html! {};
    }
    let denied = v.denied_total;
    html! {
        div style="margin-bottom:24px" { (callout(Tone::Refused, Glyph::Warn, html! {
            div style="flex:1" {
                b { "Refused among the rows kept: " (v.refused_total + denied) "." }
                " Kafka's own refusals" @if denied > 0 { " and the ACLs' denials (" (denied) ")" } ", with the rule behind each."
                div class="row wrap" style="gap:28px;margin-top:8px" {
                    @for r in &v.refused {
                        a class="lnk" href=(request_url(base, r.id)) {
                            span class="mono" { (r.code_name) " (" (r.code) ")" } " · " (r.client) " · " span data-vary { (crate::format::clock(r.at_ms)) }
                        }
                    }
                }
            }
        })) }
    }
}

fn sentence(v: &LiveView) -> Markup {
    html! { p class="sub" data-extra="live-sentence" aria-live="polite" { (spans(&live_sentence(v))) } }
}

fn held_banner(v: &LiveView) -> Markup {
    let held = v.held.unwrap_or(0);
    html! {
        div data-paused-banner data-scaffold[v.paused_at_ms.is_none()] hidden[v.paused_at_ms.is_none()] style="margin-bottom:24px" { (callout(Tone::Plain, Glyph::Info, html! {
            div style="flex:1" {
                b { "Paused. " span data-held { (number(i64::try_from(held).unwrap_or(0))) } " requests are held back." }
                " The wire stops where it is. Nothing is lost: the recorder carries on, and Resume brings them in."
            }
        })) }
    }
}

fn quiet_card(v: &LiveView, base: &Base) -> Markup {
    html! {
        div class="card" { div class="row" style="gap:32px;align-items:flex-start" {
            div style="flex:1" {
                h2 style="margin:0 0 6px;font-size:16px" { "Send the first request" }
                p class="help" style="margin:0 0 12px" { "Any Kafka client will do. Name it with " span class="mono" { "client.id" } " and it shows up as itself." }
                (crate::ui::code::pre(&format!("echo hi | kcat -P -b {} -t orders -X client.id=admin-cli", v.listen), None, None))
            }
            div class="row" style="flex:none;padding-top:30px;gap:10px" {
                (link(&base.url("/connect"), Variant::Primary, Size::Normal, Some(Glyph::Code), "Snippets for your client"))
                (link(&base.url("/topics/new"), Variant::Plain, Size::Normal, Some(Glyph::Plus), "Create a topic"))
            }
        } }
    }
}

fn pager_row(v: &LiveView, base: &Base) -> Markup {
    let mut newer = v.query.clone();
    newer.before = None;
    let older = v.older.map(|id| {
        let mut q = v.query.clone();
        q.before = Some(id);
        format!("{}{}", base.url("/live"), q.to_query())
    });
    pager(
        &pager_line(v),
        html! {
            @if v.query.before.is_some() {
                a class="btn sm" href={(base.url("/live")) (newer.to_query())} data-ui="page" { (live_glyph(LiveGlyph::ChevL, 14)) "Newer" }
            } @else {
                button class="btn sm" type="button" data-ui="page" disabled { (live_glyph(LiveGlyph::ChevL, 14)) "Newer" }
            }
            @if let Some(href) = &older {
                a class="btn sm" href=(href) data-ui="page" { "Older" (live_glyph(LiveGlyph::ChevR, 14)) }
            } @else {
                button class="btn sm" type="button" data-ui="page" disabled { "Older" (live_glyph(LiveGlyph::ChevR, 14)) }
            }
        },
    )
}

fn held_row(v: &LiveView) -> Markup {
    let held = v.held.unwrap_or(0);
    html! {
        @if v.paused_at_ms.is_some() && held > 0 {
            tr class="hot" { td colspan="6" style="height:48px" {
                span class="fold" { (held) " newer" } " " b style="margin-left:8px" { (held) " requests arrived after you paused and are held back." }
                " " span class="dim" { "The broker keeps recording; resuming shows them." }
            } }
        }
    }
}

fn tape(v: &LiveView, base: &Base) -> Markup {
    let table = table(&v.rows, base, &v.query, Layout::Live);
    if v.state == LiveState::Steady && v.paused_at_ms.is_none() {
        return html! { (table) (pager_row(v, base)) };
    }
    let (title, lede) = if let Some(at) = v.paused_at_ms {
        (
            format!("The wire, as of {}", paused_clock(at)),
            format!("The last {} requests before the pause.", v.rows.len()),
        )
    } else if v.state == LiveState::Busy {
        (
            "The wire, folded".to_owned(),
            load_line(v.rows.len(), v.window.requests),
        )
    } else {
        (
            "The wire".to_owned(),
            "Rows appear here as requests arrive, newest first.".to_owned(),
        )
    };
    html! {
        div class="wire-dock" style="margin-top:8px" {
            h2 { (title) }
            p class="lede" { (lede) }
            @if v.state == LiveState::Quiet {
                div class="empty" style="padding:40px 32px" {
                    div class="ic" { (glyph(Glyph::Live, 22, "")) }
                    h3 { "No requests yet" }
                    p style="margin:0" { "The first one is a good one to watch: ApiVersions, then Metadata, then whatever your client does next." }
                }
            } @else {
                (held_row_table(v, base))
            }
        }
    }
}

fn held_row_table(v: &LiveView, base: &Base) -> Markup {
    // The paused board's first row says what is held back, inside the table.
    let t = table(&v.rows, base, &v.query, Layout::Live).into_string();
    let held = held_row(v).into_string();
    let t = t.replacen("<tbody>", &format!("<tbody>{held}"), 1);
    html! { (maud::PreEscaped(t)) }
}

/// The tape as a fragment: everything under the page's address bar that follows the recorder.
#[must_use]
pub fn live_fragment(v: &LiveView, base: &Base) -> Markup {
    let acts = html! { (pause(v, base)) };
    let url = wire_url(base, LIVE_FRAGMENT, &v.query.to_query());
    client::fragment(
        LIVE_FRAGMENT,
        &url,
        html! {
            span hidden data-seen=(v.seen) data-newest=(v.newest_id) {}
            (page_header("Live", sentence(v), acts))
            (held_banner(v))
            (toolbar(v, base, v.state != LiveState::Quiet && v.paused_at_ms.is_none()))
            @if v.state == LiveState::Quiet { (quiet_card(v, base)) }
            @if v.paused_at_ms.is_none() { (refusals(v, base)) }
            @if v.state == LiveState::Busy && v.paused_at_ms.is_none() {
                div style="margin-bottom:24px" { (callout(Tone::Plain, Glyph::Info, html! {
                    b { "Repeats fold into one row." } " A line carrying more than a request a second would only be a wall of identical rows, so the wire folds the repeats into one row with a count (" span class="fold" { "× " (v.rows.iter().map(|r| r.count).max().unwrap_or(2)) } ") and the span they cover."
                })) }
            }
            (tape(v, base))
        },
    )
}

/// The Live page's main content.
#[must_use]
pub fn live_page(v: &LiveView, base: &Base) -> Markup {
    live_fragment(v, base)
}
