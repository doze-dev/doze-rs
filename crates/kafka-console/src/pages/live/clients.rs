//! The clients drawer (board `live-clients`): who is connected now, by `client.id`, with the software and the APIs each has used.

use maud::{Markup, html};

use crate::base::{Base, segment};
use crate::format::ago;
use crate::model::ClientsView;
use crate::ui::button::pill_mono;
use crate::ui::glyph::{Glyph, glyph};

/// The heading id of the drawer.
pub(super) const CLIENTS_HEADING: &str = "live-clients";

/// The body of the drawer.
#[must_use]
pub fn clients_body(v: &ClientsView, base: &Base, close: &str) -> Markup {
    let n = v.rows.len();
    html! {
        div class="row sb" style="align-items:flex-start" {
            div {
                h2 id=(CLIENTS_HEADING) { "Clients" }
                div class="sent" {
                    (n) @if n == 1 { " client" } @else { " clients" } " connected, by " span class="mono" { "client.id" }
                    ". Software and versions come from the ApiVersions request each one sent first."
                }
            }
            a class="btn sm ghost" href=(close) data-dialog-close data-ui="close" aria-label="Close" { (glyph(Glyph::X, 16, "")) "Close" }
        }
        div class="hr" style="margin:16px 0 20px" {}
        @if v.rows.is_empty() {
            p class="help" { "No client is connected. A connection that closed is gone from this list; its requests stay on the wire." }
        } @else {
            div class="stack-y" style="gap:0" {
                @for c in &v.rows {
                    div style="padding:14px 0;border-bottom:1px solid var(--line)" {
                        div class="row sb" {
                            div class="row" style="gap:8px" {
                                a class="mono bright" href={(base.url("/live")) "?client=" (segment(&c.client))} title="Show only this client on the wire" { @if c.client.is_empty() { "(no client.id)" } @else { (c.client) } }
                                @if let Some(t) = &c.tag { (pill_mono(t)) }
                            }
                            span class="mono dim" style="font-size:12.5px" { @if let Some(p) = &c.peer { (p) } @if c.more > 0 { " + " (c.more) " more" } }
                        }
                        div class="help" style="margin-top:4px" {
                            @if let Some(s) = &c.software { (s) " · " }
                            span class="mono" { (c.apis.iter().take(6).map(|(a, v)| format!("{a} v{v}")).collect::<Vec<_>>().join(" · ")) }
                            " · last request " span data-vary { (ago(v.now_ms - c.last_ms)) }
                            @if c.errors > 0 { " · " (c.errors) " of " (c.requests) " not answered cleanly" }
                        }
                    }
                }
            }
        }
        p class="help" style="margin-top:16px" { "Only clients that are connected now. A connection that closed is gone from this list; its requests stay on the wire." }
    }
}
