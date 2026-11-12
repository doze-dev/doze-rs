//! Connect (board `connect`): the address, the advertised listener, and a snippet for each client.
//!
//! Where the board shows something the console builds from a different source than the board's own words:
//! - "No credentials needed. The top bar says Open" is the board's sentence for an open broker; with SASL on the page says
//!   the opposite, in the same shape (new copy, the board draws only the open broker).
//! - The "Copy as" control (Code, Properties, Environment) changes what the Copy buttons copy; it needs a script and is
//!   hidden without one.

use maud::{Markup, html};

use super::common::{
    PROBE_CONNECT, address_row, listener_help, probe_fragment_url, rules, spans, verdict_state,
};
use crate::base::Base;
use crate::model::{ConnectView, ProbeView};
use crate::ui::button::{Size, Variant, link};
use crate::ui::client;
use crate::ui::code::{block, pre};
use crate::ui::filter::seg;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::layout::{card, hr, page_header};
use crate::words::connect_advertised_lede;

fn port_of(address: &str) -> &str {
    address.rsplit_once(':').map_or("9092", |(_, p)| p)
}

fn address_card(v: &ConnectView, base: &Base) -> Markup {
    let port = port_of(&v.listen);
    let recipe = format!(
        "doze-kafka --listen 0.0.0.0:{port} \\\n  --advertised-host 192.168.1.20 \\\n  --advertised-port {port}"
    );
    html! {
        div class="card" data-extra="endpoint" {
            h2 { "Address" }
            p class="lede" { "Bootstrap from this address. Clients are then told where to reconnect." }
            (address_row(&v.listen, listener_help(v.loopback, false)))
            (hr())
            h2 style="margin:0 0 6px" { "From another machine or a container" }
            p class="help" style="margin:0 0 12px" {
                "Listen on every interface and advertise an address the other side can use. "
                a class="lnk" href=(base.url("/connect/advertised")) { "Why the advertised host matters" } "."
            }
            (pre(&recipe, None, None))
        }
    }
}

/// The check, as the fragment the page and a fetch of it share.
#[must_use]
pub fn probe_connect(p: &ProbeView, base: &Base) -> Markup {
    client::fragment_static(
        PROBE_CONNECT,
        &probe_fragment_url(base, PROBE_CONNECT),
        html! {
                div class="row sb" { h2 { "Advertised listener" } (verdict_state(p.verdict)) }
                p class="lede" { (spans(&connect_advertised_lede(p))) }
                (rules(p))
                div class="row" style="margin-top:16px" {
                    (link(&base.url("/connect/advertised"), Variant::Plain, Size::Normal, Some(Glyph::Info), "How the check works, and the bad case"))
                }
        },
    )
}

fn advertised_card(v: &ConnectView, base: &Base) -> Markup {
    html! { div class="card" { (probe_connect(&v.probe, base)) } }
}

fn snippet_card(i: usize, s: &crate::model::ClientSnippet) -> Markup {
    let id = format!("snippet-{i}");
    card(html! {
        div class="row sb" {
            div class="title-row" style="gap:10px" { (glyph(Glyph::Code, 16, "d-data")) h2 style="margin:0" { (s.name) } span class="pill mono" { (s.tag) } }
            (client::copy_snippet_button("btn sm", &id, html! { (glyph(Glyph::Copy, 16, "")) "Copy" }))
        }
        (block(&s.code, Some(&id), Some("margin-top:16px")))
    })
}

fn name_your_app(v: &ConnectView, base: &Base) -> Markup {
    card(html! {
        h2 { "Name your app" }
        p class="lede" {
            "Set " span class="mono" { "client.id" } " to the app's own name. The wire shows every request under that name, so "
            span class="mono" { "payments" } " and " span class="mono" { "fulfilment" } " appear as themselves, not as a default like "
            span class="mono" { "rdkafka" } "."
        }
        div class="teach" {
            @if v.access.sasl {
                b { "Credentials are needed." } " "
                span class="dim" { "The top bar says " (v.access.label) ": every client must log in. Add the lines below. "
                    a class="lnk" href=(base.url("/access")) { "Access" } " lists the mechanisms the broker offers." }
            } @else {
                b { "No credentials needed." } " "
                span class="dim" { "The top bar says Open: any client that can reach the address may connect. If SASL is on, add the three lines below. "
                    a class="lnk" href=(base.url("/access")) { "Access" } " lists the mechanisms the broker offers." }
            }
        }
    })
}

fn sasl_card() -> Markup {
    let lines = "security.protocol=SASL_PLAINTEXT\nsasl.mechanism=SCRAM-SHA-256\nsasl.jaas.config=org.apache.kafka.common.security.scram.ScramLoginModule required \\\n  username=\"<user>\" password=\"<password>\";";
    html! {
        div class="card" style="margin-top:24px" data-extra="teaching" {
            div class="row sb" {
                div class="title-row" style="gap:10px" { (glyph(Glyph::Lock, 16, "d-config")) h2 style="margin:0" { "When SASL is on" } }
                (client::copy_from_button("btn sm", "sasl-lines", html! { (glyph(Glyph::Copy, 16, "")) "Copy" }))
            }
            p class="lede" style="margin-top:8px" {
                "These are Kafka's own client settings. Use " span class="mono" { "SASL_SSL" }
                " when the broker serves TLS; a PLAIN password crosses the network as it is without it."
            }
            (pre(lines, Some("sasl-lines"), Some("margin-top:8px")))
        }
    }
}

/// Connect.
#[must_use]
pub fn connect(v: &ConnectView, base: &Base) -> Markup {
    let properties = format!("bootstrap.servers={}\nclient.id=my-app", v.listen);
    let acts = client::copy_button(
        "btn",
        &properties,
        html! { (glyph(Glyph::Copy, 16, "")) "Copy as client.properties" },
    );
    let sub = html! { p class="sub" { "It is an ordinary Kafka broker, so it is an ordinary Kafka client. Only the address is yours to fill in; here it already is." } };
    html! {
        (client::address_holder(&v.listen, html! {
            (page_header("Connect an app", sub, acts))
            div class="g2" style="align-items:start" { (address_card(v, base)) (advertised_card(v, base)) }
            div class="sec" style="margin-top:40px" {
                div class="row sb" style="margin-bottom:16px" {
                    div { h2 { "Snippets by client" } p class="help" style="margin:0" { "The address is filled in. Pick how to copy it." } }
                    div class="row" data-extra="copy-as" {
                        span class="dim" { "Copy as" }
                        (seg(html! {
                            (client::copy_as_button("code", true, "Code"))
                            (client::copy_as_button("properties", false, "Properties"))
                            (client::copy_as_button("environment", false, "Environment"))
                        }))
                    }
                }
                div class="g2" {
                    @for (i, s) in v.snippets.iter().enumerate() { (snippet_card(i, s)) }
                    (name_your_app(v, base))
                }
                (sasl_card())
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_is_what_follows_the_last_colon() {
        assert_eq!(port_of("127.0.0.1:9092"), "9092");
        assert_eq!(port_of("[::1]:19092"), "19092");
        assert_eq!(port_of("nothing"), "9092");
    }
}
