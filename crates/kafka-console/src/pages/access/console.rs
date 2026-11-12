//! The console's own card (who it acts as, what protects it) and the three start-up warnings with whether each holds now.

use maud::{Markup, html};

use crate::model::AccessView;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::write::spans;

fn sub(inner: Markup) -> Markup {
    html! { span class="sub" { (inner) } }
}

/// The console: it reads and writes the broker as one more principal and has no login of its own.
#[must_use]
pub(super) fn console_card(v: &AccessView) -> Markup {
    let c = &v.console;
    html! {
        div class="card" data-extra="teaching" {
            h2 { "The console" }
            p class="lede" { "It reads and writes the broker the way a client does, as one more principal, and it has no login of its own." }
            dl class="kv sm" {
                dt { "Principal" }
                dd class="mono" { (c.principal) (sub(html! { span style="font-family:Inter,sans-serif" { "reserved: no user, credential or " span class="mono" { "--acl-super-user" } " may take the name" } })) }
                dt { "Rights" }
                dd { "Super user" (sub(html! { "so the ACLs above never stop the console; " span class="mono" { "--acl-super-user" } " does not list it" })) }
                dt { "Who can use it" }
                dd { "Anyone who can reach it" (sub(html! {
                    "there is no login: whoever opens the console acts as " span class="mono" { (c.principal) }
                    " and may do what a super user may, including starting Lab rules that delay, fail or drop requests (" span class="mono" { "--lab=false" } " turns that off). What keeps others out is the address and the Host check below, nothing else."
                })) }
                dt { "Listens on" }
                dd class="mono" { (c.listens) (sub(html! { span style="font-family:Inter,sans-serif" {
                    @if c.beyond_loopback { "beyond loopback: other machines can reach it (" span class="mono" { "--console-unprotected" } ")" }
                    @else { "loopback; " span class="mono" { "--console-addr" } "; with SASL or TLS on, anywhere else is refused at start unless " span class="mono" { "--console-unprotected" } }
                } })) }
                dt { "Allowed hosts" }
                dd { "IP addresses and " span class="mono" { "localhost" } (sub(html! {
                    "any other " span class="mono" { "Host" } " header is answered 403; add one with " span class="mono" { "--console-allow-host" }
                    @if c.allow_hosts.is_empty() { " (none added)" } @else { " (added: " span class="mono" { (c.allow_hosts.join(", ")) } ")" }
                })) }
                dt { "Writes" }
                dd { "Same origin only" (sub(html! { "a change from another site's page is answered 403" })) }
            }
        }
    }
}

/// What the broker says at start: three warnings, each only when its condition holds.
#[must_use]
pub(super) fn warnings_card(v: &AccessView) -> Markup {
    let holding = v.warnings.iter().filter(|w| w.holds).count();
    html! {
        div class="card" {
            h2 { "What the broker says at start" }
            p class="lede" {
                "Three warnings, each printed once and only when its condition holds. "
                @if holding == 0 { "None does here, so it started quietly." }
                @else { (holding) @if holding == 1 { " does" } @else { " do" } " here, so " @if holding == 1 { "it was" } @else { "they were" } " said." }
            }
            div class="rules" {
                @for w in &v.warnings {
                    div class={"r " (if w.holds { "no" } else { "ok" })} {
                        (glyph(if w.holds { Glyph::Warn } else { Glyph::Check }, 16, ""))
                        div { (spans(&w.title)) span class="sub" { (spans(&w.because)) } }
                        span class="v" { (if w.holds { "applies" } else { "not the case" }) }
                    }
                }
            }
            p class="help" style="margin:14px 0 0" {
                "Past loopback with SASL or TLS on, the broker refuses to start the console (exit 2) unless " span class="mono" { "--console-unprotected" } " accepts it."
            }
        }
    }
}
