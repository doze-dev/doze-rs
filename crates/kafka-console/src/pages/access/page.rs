//! The page: the title with the posture word, the three cards, the users, the ACLs, then the console and the warnings.

use maud::{Markup, html};

use super::acls::acls_section;
use super::cards::{auth_card, authz_card, tls_card};
use super::console::{console_card, warnings_card};
use super::users::users_section;
use crate::access_words::{posture, sub};
use crate::base::Base;
use crate::model::AccessView;
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::notice::{Tone, callout};
use crate::ui::write::spans;

fn exposed(v: &AccessView) -> Markup {
    html! {
        @if v.console.beyond_loopback {
            div style="margin-bottom:32px" { (callout(Tone::Warn, Glyph::Warn, html! {
                b { "The console is served beyond loopback with no login." }
                " It listens on " span class="mono" { (v.console.listens) }
                ", which other machines can reach, and anyone who can reach it acts as " span class="mono" { (v.console.principal) }
                ", a super user. It was accepted with " span class="mono" { "--console-unprotected" } "."
            })) }
        }
    }
}

/// The Access page.
#[must_use]
pub fn access_page(v: &AccessView, base: &Base) -> Markup {
    let p = posture(v.auth.on, v.authz.acls, v.authz.default_allow);
    let secure = matches!(p, crate::access_words::Posture::SignIn);
    let tone = if secure { "st ok" } else { "st idle" };
    html! {
        div class="page-h" {
            div {
                div class="title-row" { h1 { "Access" } span class=(tone) { (p.word()) } }
                p class="sub" { (spans(&sub(p, &v.listen, &v.auth.mechanisms, v.tls.on))) }
            }
            div class="acts" {
                form method="get" action=(super::access_url(base)) class="inline" {
                    button class="btn" type="submit" data-op="DescribeAcls" { (glyph(Glyph::Refresh, 16, "")) "Read again" }
                }
            }
        }
        (exposed(v))
        div class="g3" { (auth_card(v)) (authz_card(v)) (tls_card(v)) }
        @if v.auth.on || !v.users.is_empty() { (users_section(v)) }
        (acls_section(v, base))
        div class="g2" style="margin-top:40px" { (console_card(v)) (warnings_card(v)) }
    }
}
