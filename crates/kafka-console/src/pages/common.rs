//! Parts several pages share: the address row, the checklist of the advertised listener and its form.

use maud::{Markup, html};

use crate::base::Base;
use crate::model::{ProbeView, Rule, Span, Verdict};
use crate::ui::button::{State, state};
use crate::ui::client;
use crate::ui::glyph::{Glyph, glyph};
use crate::words::verdict_word;

/// The ids of the fragments that hold the advertised-listener check on each page.
pub const PROBE_HOME: &str = "probe-home";
pub const PROBE_CONNECT: &str = "probe-connect";
pub const PROBE_ADVERTISED: &str = "probe-advertised";

/// Where a fragment of the check is fetched.
#[must_use]
pub fn probe_fragment_url(base: &Base, id: &str) -> String {
    base.url(&format!("/fragments/{id}"))
}

/// Spans of a sentence: plain, monospace, bold.
#[must_use]
pub(super) fn spans(parts: &[Span]) -> Markup {
    html! {
        @for p in parts {
            @match p {
                Span::Text(t) => (t),
                Span::Mono(t) => span class="mono" { (t) },
                Span::Strong(t) => b { (t) },
            }
        }
    }
}

/// The address, as the boards draw it (a node and a Copy button), and the line under it.
#[must_use]
pub(super) fn address_row(address: &str, help: Markup) -> Markup {
    html! {
        div class="stack-y" style="gap:8px" {
            div class="conn" {
                span class="node self mono" style="height:36px;font-size:15px" { (address) }
                span class="sp" {}
                (client::copy_button("btn sm", address, html! { (glyph(Glyph::Copy, 16, "")) "Copy" }))
            }
            div class="help" { (help) }
        }
    }
}

/// The listener's line under the address on Home and Connect.
#[must_use]
pub(super) fn listener_help(loopback: bool, on_home: bool) -> Markup {
    html! {
        "The listener, set by " span class="mono" { "--listen" } ". "
        @if loopback {
            @if on_home { "On loopback it is reachable from this machine only." }
            @else { "It is on loopback, so only this machine can reach it." }
        } @else {
            "It is not on loopback, so other machines can reach it."
        }
    }
}

fn rule(r: &Rule) -> Markup {
    html! {
        div class={"r " @if r.ok { "ok" } @else { "no" }} {
            (glyph(if r.ok { Glyph::Check } else { Glyph::X }, 16, ""))
            div { (r.title) span class="sub" { (spans(&r.detail)) } }
            span class="v" { (r.value) }
        }
    }
}

/// The checklist of the check: three rules, or a sentence when the check did not finish.
#[must_use]
pub(super) fn rules(p: &ProbeView) -> Markup {
    html! {
        @if p.rules.is_empty() {
            p class="help" { "The check did not finish in time, so nothing is claimed. Use Check again." }
        } @else {
            div class="rules" { @for r in &p.rules { (rule(r)) } }
        }
    }
}

/// The word of a verdict, as a state.
#[must_use]
pub(super) fn verdict_state(v: Verdict) -> Markup {
    let s = match v {
        Verdict::Reachable => State::Ok,
        Verdict::NotReachable => State::Bad,
        Verdict::NotChecked => State::Idle,
    };
    state(s, verdict_word(v))
}

/// The "Check again" form: a plain POST that comes back to the page, and with a script swaps the fragment `target`.
#[must_use]
pub(super) fn check_again(base: &Base, target: &str, back: &str, size: &str) -> Markup {
    let button = html! {
        button class=(format!("btn{size}")) type="submit" data-op="Metadata" { (glyph(Glyph::Refresh, 16, "")) "Check again" }
    };
    client::form(
        &base.url("/connect/advertised/check"),
        target,
        &[("back", back)],
        button,
    )
}
