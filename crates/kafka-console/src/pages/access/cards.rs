//! The three cards of the switches: Authentication, Authorizer, Encryption. Each says whether it is on, what it means, and the
//! facts under it, one eyebrow and one value at a time.

use maud::{Markup, html};

use crate::access_words::{acl_split, users_line};
use crate::model::AccessView;

/// One fact: its eyebrow, its value and the help line under it.
fn fact(label: &str, value: Markup, help: Option<Markup>) -> Markup {
    html! {
        div {
            div class="eyebrow" style="margin-bottom:4px" { (label) }
            (value)
            @if let Some(h) = help { div class="help" style="margin:4px 0 0" { (h) } }
        }
    }
}

fn mono(text: &str) -> Markup {
    html! { div class="mono" { (text) } }
}

fn plain(text: &str) -> Markup {
    html! { div { (text) } }
}

fn head(title: &str, word: Markup) -> Markup {
    html! { div class="row sb" { h2 { (title) } (word) } }
}

/// Authentication: who a client proves it is.
#[must_use]
pub(super) fn auth_card(v: &AccessView) -> Markup {
    let a = &v.auth;
    let mechanisms = if a.on {
        fact(
            "Mechanisms",
            mono(&a.mechanisms.join(", ")),
            (!a.plain).then(|| html! { "PLAIN is off: a stored SCRAM user cannot use it" }),
        )
    } else {
        fact(
            "Mechanisms",
            plain("None"),
            Some(
                html! { "PLAIN, SCRAM-SHA-256 and SCRAM-SHA-512 come with " span class="mono" { "--sasl-mechanisms" } },
            ),
        )
    };
    let third = if a.on {
        fact(
            "A failed login",
            mono(a.failed_login.as_deref().unwrap_or("is slowed")),
            Some(
                html! { span class="mono" { "--sasl-failed-login-delay" } ", then the connection closes" },
            ),
        )
    } else {
        fact(
            "Caller",
            html! { div { "Its own " span class="mono" { "client.id" } } },
            Some(
                html! { "self-declared and not checked; for an ACL it is " span class="mono" { "User:ANONYMOUS" } },
            ),
        )
    };
    html! {
        div class="card" {
            (head("Authentication", html! { span class={"st " (if a.on { "ok" } else { "idle" })} { (if a.on { "On" } else { "Off" }) } }))
            p class="lede" {
                @if a.on { "A client must log in before it may send anything but ApiVersions." }
                @else { "Who a client proves it is. Off, so every connection is accepted as it arrives." }
            }
            div class="stack-y" style="gap:18px" {
                (mechanisms)
                (fact("Users", mono(&users_line(a.flag_users, a.stored_users)), Some(html! { "names only; a password or key is never shown" })))
                (third)
            }
        }
    }
}

/// Authorizer: what a caller may do.
#[must_use]
pub(super) fn authz_card(v: &AccessView) -> Markup {
    let z = &v.authz;
    let acls = if z.acls == 0 {
        fact("ACLs", mono("0"), None)
    } else {
        fact(
            "ACLs",
            mono(&z.acls.to_string()),
            Some(html! { (acl_split(z.allow, z.deny)) }),
        )
    };
    let supers = if z.super_users.is_empty() {
        fact(
            "Super users",
            plain("None"),
            Some(html! { "a super user is never refused, DENY included" }),
        )
    } else {
        fact(
            "Super users",
            html! { @for s in &z.super_users { div class="mono" { (s) } } },
            Some(html! { "never refused, DENY included" }),
        )
    };
    html! {
        div class="card" {
            (head("Authorizer", html! { span class="st ok" { (if z.default_allow { "Allows by default" } else { "Denies by default" }) } }))
            p class="lede" {
                @if z.default_allow && z.acls == 0 { "What a caller may do. No ACL covers anything, so the default decides, and it says allow." }
                @else if z.default_allow { "What a caller may do. A resource an ACL covers is decided by its ACLs; one none covers, by the default, which says allow." }
                @else { "A caller is the principal it logged in as. A resource no ACL covers is refused to everyone." }
            }
            div class="stack-y" style="gap:18px" {
                (fact(
                    "Default",
                    mono(if z.default_allow { "--acl-default allow" } else { "--acl-default deny" }),
                    z.default_allow.then(|| html! { "for a resource no ACL covers; " span class="mono" { "deny" } " is Kafka's own" }),
                ))
                (acls)
                (supers)
            }
        }
    }
}

/// Encryption: TLS on the Kafka listener.
#[must_use]
pub(super) fn tls_card(v: &AccessView) -> Markup {
    let t = &v.tls;
    let first = if t.on {
        fact(
            "Certificate",
            mono(t.cert.as_deref().unwrap_or("given at start")),
            Some(html! { "the chain, leaf first" }),
        )
    } else {
        fact(
            "TLS",
            plain("Off"),
            Some(
                html! { "an optional feature of the build, turned on with " span class="mono" { "--tls-cert" } " and " span class="mono" { "--tls-key" } },
            ),
        )
    };
    let second = if t.on {
        fact(
            "Private key",
            plain("Read at start"),
            Some(html! { "its contents are printed nowhere" }),
        )
    } else {
        fact("Certificate", plain("None"), None)
    };
    html! {
        div class="card" {
            (head("Encryption", html! { span class={"st " (if t.on { "ok" } else { "idle" })} { (if t.on { "TLS on" } else { "Off" }) } }))
            p class="lede" {
                @if t.on { "Clients must connect with TLS 1.2 or newer; a plaintext client is closed with no answer." }
                @else { "Plain TCP between client and broker. Messages and, with SASL, a PLAIN password cross it as they are." }
            }
            div class="stack-y" style="gap:18px" {
                (first)
                (second)
                (fact("Listener", mono(&t.listener), t.loopback.then(|| html! { "loopback: this machine only" })))
            }
        }
    }
}
