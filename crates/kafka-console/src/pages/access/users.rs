//! The Users section: what DescribeUserScramCredentials returns, by name, mechanism and iteration count. The iteration count
//! is a public parameter of a credential (the broker sends it to every client in the server-first message); no password,
//! salt, stored key or server key is read by the console, so none can be here.

use maud::{Markup, html};

use crate::access_words::iterations_text;
use crate::model::{AccessFrom, AccessUser, AccessView};

fn from(u: &AccessUser) -> Markup {
    html! {
        @match u.from {
            AccessFrom::Flags => span class="pill" { "Flags" },
            AccessFrom::Stored => span class="pill" { "Stored" },
        }
        @if u.super_user { span class="sub" { "a super user" } }
    }
}

fn row(u: &AccessUser) -> Markup {
    html! {
        tr {
            td class="name mono" { (u.name) }
            td { (from(u)) }
            td class="mono" { @if u.mechanisms.is_empty() { span class="dim" { "none" } } @else { (u.mechanisms.join(", ")) } }
            td class="mono" { @if let Some(t) = iterations_text(&u.iterations) { (t) } @else { span class="dim" { "none" } } }
        }
    }
}

/// The section.
#[must_use]
pub(super) fn users_section(v: &AccessView) -> Markup {
    html! {
        div class="sec" style="margin-top:40px" {
            div class="row sb" { div {
                h2 { "Users" }
                p class="lede" style="margin:0" {
                    "What DescribeUserScramCredentials returns: a name, a mechanism and an iteration count. A user from the flags or a file is re-applied at every start and cannot be changed over the API."
                }
            } }
            table class="t" style="margin-top:16px" {
                thead { tr { th scope="col" { "User" } th scope="col" { "From" } th scope="col" { "Mechanisms" } th scope="col" { "Iterations" } } }
                tbody {
                    @if v.users.is_empty() {
                        tr { td colspan="4" class="dim" { "No user can sign in." } }
                    }
                    @for u in &v.users { (row(u)) }
                }
            }
            p class="help" style="margin:14px 0 0" {
                "A stored user is made and removed with Kafka's own tools, for example "
                span class="mono" { "kafka-configs.sh --alter --add-config 'SCRAM-SHA-256=[password=...]'" }
                ", which call AlterUserScramCredentials. This page does not take a password, and shows no secret part of one."
            }
        }
    }
}
