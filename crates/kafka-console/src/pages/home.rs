//! Home, in its three states (boards `home`, `home-quiet`, `home-attention`).
//!
//! What a board shows and the broker cannot say yet, and what the page does instead:
//! - "N apps appear on the wire, which reaches back to HH:MM, and the last request landed N s ago" (REALITY 1, the
//!   recorder, slice 9e): the sentence says what the broker holds.
//! - "No partition is offline and the data directory is writable" in the all-clear (GROUNDING 2.2, `partition_state()`
//!   of slice 9b): only the transaction rule is checked, and the all-clear says so.
//! - The all-clear's link to the attention state: that state is chosen by data, so the example is plain text.
//! - "Waiting for a client id this broker has not seen" (the first-request detector, slice 9e), and the link to
//!   `connect-first-request`: the card says how to connect, and claims nothing about who has.
//! - The quiet state's "no clients": the broker has no connection table yet (slice 9e), so only "no topics" is said.

use maud::{Markup, html};

use super::common::{
    PROBE_HOME, address_row, check_again, listener_help, probe_fragment_url, rules, spans,
    verdict_state,
};
use crate::base::Base;
use crate::format::{bytes, number};
use crate::model::{Attention, HomeState, HomeView, InstanceView, ProbeView, StopView};
use crate::snippets::first_command;
use crate::ui::button::{Size, State, Variant, link, state};
use crate::ui::client;
use crate::ui::code::pre;
use crate::ui::glyph::Glyph;
use crate::ui::layout::{card_extra, hr, kv, page_header, title_row, value_with_sub};
use crate::ui::notice::{Tone, callout};
use crate::words::{
    self, attention_headline, home_advertised_lede, home_sub, home_title, unchecked_note,
};

/// The id of Home's one live fragment: the title, the sentence, the callout and both columns. What the page says in each
/// state (the title, the sentence, the callout, which card sits where) follows the data together, so it is fetched, and
/// swapped, together (audit A13); one tick is one look at the broker.
pub const HOME_LIVE: &str = "home-live";

fn url(base: &Base, id: &str) -> String {
    base.url(&format!("/fragments/{id}"))
}

/// The sentence under the title.
fn sentence(v: &HomeView) -> Markup {
    html! { p class="sub" data-extra="live-sentence" { (spans(&home_sub(v))) } }
}

fn fraction(held: i64, of: i64) -> i64 {
    if of <= 0 {
        0
    } else {
        (held * 100 / of).clamp(0, 100)
    }
}

fn attention(a: &Attention, unchecked: usize, base: &Base) -> Markup {
    let p = fraction(a.elapsed_secs, a.timeout_secs);
    let txn_url = base.url(&format!(
        "/topics/{}/producers",
        crate::base::segment(&a.topic)
    ));
    html! {
        div style="flex:1" {
            div class="row sb" style="align-items:flex-start;gap:24px" {
                div {
                    b { (attention_headline(a.count, unchecked)) } br;
                    @if let Some(note) = unchecked_note(unchecked) { (note) " " }
                    "Transaction " span class="mono" { (a.transactional_id) }
                    " (producer " (a.producer_id) ", epoch " (a.producer_epoch) ") has been open on "
                    span class="mono" { (a.partition) } " since " (a.since)
                    ". If its app does not commit or abort before the " (a.timeout_secs) " s timeout, the coordinator aborts it and the producer is fenced."
                }
                div class="row" style="flex:none" { (link(&txn_url, Variant::Plain, Size::Small, Some(Glyph::ArrowR), "Open the transaction")) }
            }
            div class="lagbar" style="margin-top:14px;max-width:560px" {
                div class="tr" { i class="warn" style={"--p:" (p) "%"} {} }
                span class="n" style="min-width:110px" { (a.elapsed_secs) " s of " (a.timeout_secs) " s" }
            }
            @if let Some(h) = a.held_back {
                div class="help" style="margin-top:10px" {
                    "Meanwhile a read_committed consumer of " span class="mono" { (a.partition) } " is held "
                    (h.high_watermark - h.last_stable) " records behind: the last stable offset is " (number(h.last_stable))
                    ", the high watermark " (number(h.high_watermark))
                    ". A topic that is merely due for compaction would not appear here: that is the cleaner's job, not yours."
                }
            }
        }
    }
}

/// The callout under the sentence.
#[must_use]
pub fn callout_body(v: &HomeView, base: &Base) -> Markup {
    match &v.state {
        HomeState::Attention(a) => html! {
            div class="callout warn" data-extra="live-sentence" style="margin-bottom:32px" {
                (crate::ui::glyph::glyph(Glyph::Warn, 18, "")) (attention(a, v.unchecked, base))
            }
        },
        HomeState::Quiet => html! {
            div style="margin-bottom:32px" { (callout(Tone::Plain, Glyph::Info, html! {
                b { "Nothing to look after yet." } " This broker has no topics. The next step is to send it something."
            })) }
        },
        // The all-clear is claimed only for what was looked at all of: with open transactions left unchecked it says so.
        HomeState::Steady => match unchecked_note(v.unchecked) {
            Some(note) => html! {
                div style="margin-bottom:32px" data-extra="live-sentence" { (callout(Tone::Plain, Glyph::Info, html! {
                    b { "No transaction that was checked is near its timeout." }
                    " " (note) " This is not the all-clear."
                })) }
            },
            None => html! {
                div style="margin-bottom:32px" data-extra="live-sentence" { (callout(Tone::Plain, Glyph::Check, html! {
                    b { "Nothing needs you." }
                    " No transaction is near its timeout. When something does, it shows here, for example a transaction close to its timeout."
                })) }
            },
        },
    }
}

fn rows(i: &InstanceView, quiet: bool) -> Vec<(&'static str, Markup)> {
    let mono = |s: String| html! { span class="mono" { (s) } };
    // The board's time has no zone; this one is UTC, said on a second line (the card's value column is narrow).
    let (day, zone) = i
        .started
        .rsplit_once(' ')
        .unwrap_or((i.started.as_str(), ""));
    let mut out = vec![("Started", value_with_sub(mono(day.to_owned()), zone))];
    if !quiet {
        out.push(("Up for", mono(crate::format::span(i.up_secs))));
    }
    out.push((
        "Last stop",
        match i.last_stop {
            StopView::Clean => state(State::Ok, "Clean"),
            StopView::Unclean => state(State::Warn, "Not clean"),
            StopView::None => html! { "None yet" },
        },
    ));
    out.push((
        "Open partitions",
        if quiet || i.max_open_partitions == 0 {
            mono(words::open_partitions(i))
        } else {
            value_with_sub(mono(words::open_partitions(i)), "Opened on first use")
        },
    ));
    if let (false, Some(n)) = (quiet, i.open_files) {
        out.push((
            "Descriptors",
            value_with_sub(
                mono(number(i64::try_from(n).unwrap_or(0))),
                "Open files and sockets",
            ),
        ));
    }
    // The board's label is "Idle memory"; what is read is the resident memory now (VmRSS), which is not an idle figure when the
    // broker is busy. The label is the board's (debt J-row on the label), the line under the figure says what it is.
    out.push((
        "Idle memory",
        match i.resident_bytes {
            Some(b) => value_with_sub(mono(bytes(b)), "Resident now. Measured on Linux only"),
            None => value_with_sub(html! { "Not measured" }, "Measured on Linux only"),
        },
    ));
    out.push((
        "Threads",
        i.threads
            .map_or_else(|| html! { "Not measured" }, |t| mono(t.to_string())),
    ));
    out.push(("Version profile", html! { (i.profile) }));
    out
}

/// The "This instance" card.
fn instance_card(v: &HomeView, base: &Base) -> Markup {
    let i = &v.instance;
    let quiet = v.state == HomeState::Quiet;
    let mut list = rows(i, quiet);
    list.push((
        "Access",
        value_with_sub(
            html! { a class="lnk" href=(base.url("/access")) { (i.access.label) } },
            &i.access.detail,
        ),
    ));
    card_extra(
        "live-sentence",
        html! {
            (title_row("This instance", state(State::Ok, "Running")))
            p class="lede" { (words::instance_lede(i)) }
            (kv(list))
            p class="help" style="margin:14px 0 0" { "Data directory " span class="mono" { (i.data_dir) } }
            div class="row" style="margin-top:20px" {
                (link(&base.url("/settings"), Variant::Plain, Size::Normal, Some(Glyph::Settings), "Weight and data in Settings"))
            }
        },
    )
}

/// The advertised-listener part of the Address card, as the fragment the check form swaps.
#[must_use]
pub fn probe_home(p: &ProbeView, base: &Base) -> Markup {
    client::fragment_static(
        PROBE_HOME,
        &probe_fragment_url(base, PROBE_HOME),
        html! {
            div class="row sb" {
                div class="title-row" { h2 style="margin:0" { "Advertised listener" } (verdict_state(p.verdict)) }
                (check_again(base, PROBE_HOME, "home", " sm"))
            }
            p class="lede" style="margin-top:6px" { (home_advertised_lede(p)) }
            (rules(p))
        },
    )
}

fn address_card(v: &HomeView, base: &Base) -> Markup {
    card_extra(
        "endpoint",
        html! {
            (title_row("Address", html! { a class="lnk" href=(base.url("/connect")) { "Snippets for every client" } }))
            p class="lede" { "Point any Kafka client at this address. It is where the broker listens and, below, where it tells clients to reconnect." }
            (address_row(&v.listen, listener_help(v.loopback, true)))
            (hr())
            (probe_home(&v.probe, base))
        },
    )
}

fn next_card(v: &HomeView, base: &Base) -> Markup {
    let connect = base.url("/connect");
    let btn = |label: &str, variant, glyph, href: &str| {
        link(href, variant, Size::Normal, Some(glyph), label)
    };
    if v.state == HomeState::Quiet {
        let step = |n: &str, title: &str, body: Markup| {
            html! {
                div class="opt-row" { span class="pill mono" { (n) } div style="flex:1" { div class="t1" { (title) } (body) } }
            }
        };
        return html! { div class="card" {
            (title_row("Waiting for your first request", state(State::Idle, "Listening")))
            p class="lede" { "Three steps get a client through, each a command you can paste." }
            div class="stack-y" style="gap:16px" {
                (step("1", "Ask the broker what it has", pre(&format!("kcat -b {} -L", v.listen), None, Some("margin-top:8px"))))
                (step("2", "Send a message (auto-create makes the topic, 1 partition)",
                    pre(&format!("echo \"hello\" | kcat -P -b {} -t orders -X client.id=admin-cli", v.listen), None, Some("margin-top:8px"))))
                (step("3", "Watch it arrive", html! { div class="help" { "Open Live: the request is the first row on the wire, " a class="lnk" href=(base.url("/live")) { "with its caller, topic and result" } "." } }))
            }
            div class="row" style="margin-top:20px" {
                (btn("Snippets for your client", Variant::Primary, Glyph::Code, &connect))
                (btn("Create a topic first", Variant::Plain, Glyph::Plus, &base.url("/topics/new")))
            }
        } };
    }
    html! { div class="card" {
        (title_row("Next: connect something new", state(State::Ok, "Listening")))
        p class="lede" { "Waiting for a client id this broker has not seen. When one sends its first request it appears here, with the versions it speaks. See " a class="lnk" href=(base.url("/connect/first-request")) { "what a first request looks like" } ". Point a client at the address and name it with client.id, so that it appears as itself." }
        div class="stack-y" style="gap:12px" { (pre(&first_command(&v.listen), None, None)) }
        div class="row" style="margin-top:20px" {
            (btn("Connect an app", Variant::Primary, Glyph::Code, &connect))
            (btn("Open Live", Variant::Plain, Glyph::Live, &base.url("/live")))
        }
    } }
}

/// Home's live fragment, and Home.
#[must_use]
pub fn live_fragment(v: &HomeView, base: &Base) -> Markup {
    let quiet = v.state == HomeState::Quiet;
    let acts = if quiet {
        html! {}
    } else {
        link(
            &base.url("/connect"),
            Variant::Primary,
            Size::Normal,
            Some(Glyph::Code),
            "Connect an app",
        )
    };
    client::fragment(
        HOME_LIVE,
        &url(base, HOME_LIVE),
        html! {
            (page_header(home_title(&v.state), sentence(v), acts))
            (callout_body(v, base))
            div class="split" {
                div class="stack-y" { (address_card(v, base)) @if quiet { (next_card(v, base)) } }
                div class="stack-y" { (instance_card(v, base)) @if !quiet { (next_card(v, base)) } }
            }
        },
    )
}

/// Home.
#[must_use]
pub fn home(v: &HomeView, base: &Base) -> Markup {
    live_fragment(v, base)
}
