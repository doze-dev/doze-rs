//! The ACLs section: what DescribeAcls returns, one row per ACL, with Create ACL and a Delete on each row. When the broker
//! refuses to describe them, the table is Kafka's own refusal.

use maud::{Markup, html};

use super::{create_url, delete_url};
use crate::acl_words::{kind_label, operation_label};
use crate::base::Base;
use crate::model::{AccessView, AclEntry, AclPermission};
use crate::ui::glyph::{Glyph, glyph};
use crate::ui::glyph_extra::{Extra, glyph_extra};
use crate::ui::write::refused;

fn row(e: &AclEntry, base: &Base) -> Markup {
    html! {
        tr {
            td class="name" { span class="dim" { (kind_label(e.kind)) } " " span class="mono" { (e.name) } }
            td { (e.pattern.label()) }
            td class="mono" { (e.principal) }
            td class="mono" { (e.host) }
            td class="mono" { (operation_label(e.operation)) }
            td { @match e.permission {
                AclPermission::Allow => span class="st ok" { "Allow" },
                AclPermission::Deny => span class="st warn" { "Deny" },
            } }
            td class="r" {
                a class="btn sm" href=(delete_url(base, &e.query())) data-dialog data-op="DeleteAcls" {
                    (glyph_extra(Extra::Trash, 14)) "Delete"
                }
            }
        }
    }
}

fn empty(v: &AccessView) -> Markup {
    html! {
        tr { td colspan="6" style="padding:0" { div class="empty" style="border:0;border-radius:0;padding:36px 32px" {
            h3 { "No ACLs" }
            p style="margin:0" {
                @if v.authz.default_allow {
                    "With " span class="mono" { "--acl-default allow" } " nothing is refused for lack of one. The first ACL on a topic turns the default off for that topic and refuses everyone it does not name."
                } @else {
                    "With " span class="mono" { "--acl-default deny" } " and no ACL, everyone but a super user is refused everything."
                }
            }
        } } }
    }
}

fn table(v: &AccessView, base: &Base) -> Markup {
    html! {
        table class="t" style="margin-top:16px" {
            thead { tr {
                th scope="col" { "Resource" } th scope="col" { "Pattern" } th scope="col" { "Principal" }
                th scope="col" { "Host" } th scope="col" { "Operation" } th scope="col" { "Permission" }
                @if !v.acls.rows.is_empty() { th scope="col" aria-label="Delete" {} }
            } }
            tbody {
                @if v.acls.rows.is_empty() { (empty(v)) }
                @for e in &v.acls.rows { (row(e, base)) }
            }
        }
        @if v.acls.more > 0 {
            p class="help" style="margin:10px 0 0" { (v.acls.more) " more ACLs are not listed: the table shows the first " (v.acls.rows.len()) "." }
        }
    }
}

/// The section.
#[must_use]
pub(super) fn acls_section(v: &AccessView, base: &Base) -> Markup {
    html! {
        div class="sec" style="margin-top:40px" {
            div class="row sb" {
                div {
                    h2 { "ACLs" }
                    p class="lede" style="margin:0" {
                        "What DescribeAcls returns, grouped as Kafka groups them. A DENY wins over any ALLOW; an ALLOW of Read, Write, Delete or Alter also allows Describe."
                        @if !v.auth.on { " Even with no sign-in, ACLs bind " span class="mono" { "User:ANONYMOUS" } ", so a broker that has them is not open to everything." }
                    }
                }
                a class="btn pri" href=(create_url(base)) data-dialog { (glyph(Glyph::Plus, 16, "")) "Create ACL" }
            }
            @if let Some(r) = &v.acls.refused {
                div style="margin-top:16px" { (refused(r)) }
            } @else {
                (table(v, base))
            }
        }
    }
}
