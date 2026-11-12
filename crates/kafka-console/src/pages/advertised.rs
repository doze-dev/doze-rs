//! The advertised-listener page (board `connect-advertised`): the check, a bad case for comparison, and what to set.
//!
//! "A bad case, for comparison" is the board's own illustration (a made-up host name that cannot be resolved), kept as
//! the board has it: it says in its first line that it is not this instance.

use maud::{Markup, html};

use super::common::{PROBE_ADVERTISED, check_again, probe_fragment_url, rules, verdict_state};
use crate::base::Base;
use crate::model::ProbeView;
use crate::ui::client;
use crate::ui::code::pre;
use crate::ui::glyph::Glyph;
use crate::ui::layout::{card, page_header, title_row};
use crate::ui::notice::{Tone, callout};
use crate::words::advertised_instance_lede;

fn host_port(addr: &str) -> (&str, &str) {
    addr.rsplit_once(':').unwrap_or((addr, ""))
}

/// The check, as the fragment the page and a fetch of it share.
#[must_use]
pub fn probe_advertised(p: &ProbeView, base: &Base) -> Markup {
    client::fragment_static(
        PROBE_ADVERTISED,
        &probe_fragment_url(base, PROBE_ADVERTISED),
        html! {
            (title_row("This instance", verdict_state(p.verdict)))
            p class="lede" { (advertised_instance_lede(p)) }
            (rules(p))
        },
    )
}

fn bad_case() -> Markup {
    let rule = |ok: bool, title: &str, detail: Markup, value: &str| {
        html! {
            div class={"r " @if ok { "ok" } @else { "no" }} {
                (crate::ui::glyph::glyph(if ok { Glyph::Check } else { Glyph::X }, 16, ""))
                div { (title) span class="sub" { (detail) } }
                span class="v" { (value) }
            }
        }
    };
    card(html! {
        (title_row("A bad case, for comparison", crate::ui::button::state(crate::ui::button::State::Bad, "Not reachable")))
        p class="lede" {
            "Not this instance: the same broker started with " span class="mono" { "--advertised-host broker.internal" }
            " on a machine that cannot resolve that name."
        }
        div class="rules" {
            (rule(true, "The listener answers", html! { "Dialled 127.0.0.1:9092 and asked ApiVersions" }, "2 ms"))
            (rule(true, "It says where clients should reconnect", html! { "Metadata names this broker at " span class="mono" { "broker.internal:9092" } }, "Metadata"))
            (rule(false, "That address is not reachable", html! { "Dialled " span class="mono" { "broker.internal:9092" } ": no such host" }, "failed"))
        }
    })
}

fn call_behind(p: &ProbeView) -> Markup {
    let (host, port) = host_port(&p.advertised);
    let text = format!(
        "Metadata v{}\n  topics: []\n\n→ brokers: [{{ host: \"{host}\", port: {port} }}]\n  then dial {} and send ApiVersions",
        p.metadata_version, p.advertised
    );
    html! {
        div class="card" data-extra="api-preview" {
            h2 { "The call behind the check" }
            p class="lede" { "One request, the same one every client sends first." }
            (pre(&text, None, None))
        }
    }
}

fn what_to_set(p: &ProbeView) -> Markup {
    let (_, port) = host_port(&p.listen);
    let recipe = format!(
        "doze-kafka --listen 0.0.0.0:{port} \\\n  --advertised-host 192.168.1.20 \\\n  --advertised-port {port}"
    );
    html! {
        div class="card" data-extra="teaching" {
            h2 { "What to set" }
            p class="lede" { "One broker, one address: set the advertised host to a name or IP the client can reach, and the port it can reach it on." }
            (pre(&recipe, None, None))
            div class="teach" style="margin-top:16px" {
                b { "From a container, 127.0.0.1 is the container." } " "
                span class="dim" { "This check runs on the broker's side and cannot see inside your container's network: if clients there fail while this says Reachable, set a host name that the container resolves." }
            }
        }
    }
}

/// The advertised-listener page.
#[must_use]
pub fn advertised(p: &ProbeView, base: &Base) -> Markup {
    let sub = html! { p class="sub" { "The trap: a client reaches the broker, is told another address, and cannot reach that one. This check does the same two steps a client does, from the broker's side." } };
    html! {
        (page_header("Advertised listener", sub, check_again(base, PROBE_ADVERTISED, "advertised", "")))
        div class="g2" style="align-items:start" { (card(probe_advertised(p, base))) (bad_case()) }
        div class="sec" style="margin-top:40px" {
            h2 { "What your client sees in the bad case" }
            (callout(Tone::Warn, Glyph::Warn, html! {
                "The first request works, which is why this is confusing. Then the client logs a failure to connect to "
                span class="mono" { "broker.internal:9092" } " and every produce and fetch times out. Nothing is wrong with the address you typed."
            }))
        }
        div class="g2" style="margin-top:40px;align-items:start" { (call_behind(p)) (what_to_set(p)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_splits_at_its_last_colon() {
        assert_eq!(host_port("127.0.0.1:9092"), ("127.0.0.1", "9092"));
        assert_eq!(host_port("[::1]:9092"), ("[::1]", "9092"));
        assert_eq!(host_port("nohost"), ("nohost", ""));
    }
}
