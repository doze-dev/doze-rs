//! The two ACL dialogs, written from the boards `access-acl-create` and `access-acl-delete`. Each is an overlay
//! (`ui::overlay`) over the Access page: these functions return the overlay element, and the handler puts it over the page.
//! Each is one POST form that works with no script.

use maud::{Markup, html};

use super::access_url;
use crate::acl_words::{KINDS, OPERATIONS, kind_form, kind_label, operation_form, operation_label};
use crate::base::Base;
use crate::model::{AclCreateView, AclDeleteView, AclEntry, AclPermission};
use crate::ui::dialog::{cancel, modal};
use crate::ui::glyph::Glyph;
use crate::ui::glyph_extra::{Extra, glyph_extra};
use crate::ui::notice::{Tone, callout};
use crate::ui::write::{apicall, field_error, refused, spans, text_input};

fn error_of<'a>(v: &'a AclCreateView, field: &str) -> Option<&'a str> {
    v.error
        .as_ref()
        .filter(|(f, _)| f == field)
        .map(|(_, t)| t.as_str())
}

fn select(id: &str, label: &str, options: Vec<(&str, String)>, now: &str) -> Markup {
    html! {
        div class="field" {
            label for=(id) { (label) }
            div class="input sel" {
                select id=(id) name=(id) {
                    @for (word, text) in &options { option value=(*word) selected[*word == now] { (text) } }
                }
                (glyph_extra(Extra::ChevD, 14))
            }
        }
    }
}

fn text_field(v: &AclCreateView, id: &str, label: &str, value: &str) -> Markup {
    let err = error_of(v, id);
    html! {
        div class="field" {
            label for=(id) { (label) }
            (text_input(id, id, value, true, None, err.is_some()))
            @if let Some(e) = err { (field_error(e)) }
        }
    }
}

fn choice(group: &str, label: &str, name: &str, options: [(&str, &str); 2], now: &str) -> Markup {
    let id = format!("{name}-label");
    html! {
        div class="field" {
            span class="lab" id=(id) { (label) }
            div class="seg" role="radiogroup" aria-labelledby=(id) {
                @for (word, text) in options {
                    label { input type="radio" name=(name) value=(word) checked[word == now]; (text) }
                }
            }
            @if group == "pattern" {
                div class="help" style="margin-top:8px" { "Literal names this resource only. Prefixed covers every name that starts with it." }
            }
        }
    }
}

fn teaching() -> Markup {
    html! {
        div class="teach" data-extra="teaching" {
            div { b { "Kafka says" } " an ACL covers a resource when its name matches; once one does, the default no longer applies to that resource." }
            div { b { "Allow implies" } " Describe for Read, Write, Delete and Alter. " b { "Deny is exact" } ": Deny Read does not deny Describe, and a Deny wins over any Allow." }
            div { b { "Host" } " is " span class="mono" { "*" } " or the client's address as text, not a range." }
        }
    }
}

fn create_body(v: &AclCreateView) -> Markup {
    let i = &v.input;
    let kinds: Vec<(&str, String)> = KINDS.iter().map(|k| (k.0, k.1.to_owned())).collect();
    let ops: Vec<(&str, String)> = OPERATIONS.iter().map(|o| (o.0, o.1.to_owned())).collect();
    html! {
        @if let Some(r) = &v.refused { div style="margin-bottom:18px" { (refused(r)) } }
        div class="form" style="gap:18px" {
            div class="g2" style="gap:16px" { (select("type", "Resource type", kinds, &i.kind)) (text_field(v, "name", "Resource name", &i.name)) }
            (choice("pattern", "Name matches", "pattern", [("literal", "Literal"), ("prefixed", "Prefixed")], &i.pattern))
            div class="g2" style="gap:16px" { (text_field(v, "principal", "Principal", &i.principal)) (text_field(v, "host", "Host", &i.host)) }
            div class="g2" style="gap:16px" {
                (select("op", "Operation", ops, &i.operation))
                (choice("perm", "Permission", "perm", [("allow", "Allow"), ("deny", "Deny")], &i.permission))
            }
            (teaching())
            (apicall("The call, asked as User:__console", &v.call, None))
        }
    }
}

/// The title of the Create dialog.
#[must_use]
pub fn acl_create_title() -> &'static str {
    "Create an ACL"
}

/// The Create ACL dialog: the overlay element over the Access page.
#[must_use]
pub fn acl_create_dialog(v: &AclCreateView, base: &Base) -> Markup {
    modal(
        "acl-create",
        &base.url("/access/acls/new"),
        &access_url(base),
        html! { (acl_create_title()) },
        create_body(v),
        html! {
            (cancel(&access_url(base)))
            button class="btn pri" type="submit" data-op="CreateAcls" { "Create ACL" }
        },
    )
}

fn entry_rows(e: &AclEntry) -> Markup {
    html! {
        dl class="kv sm" style="margin-bottom:16px" {
            dt { "Resource" } dd { (kind_label(e.kind)) " " span class="mono" { (e.name) } ", " (e.pattern.label().to_lowercase()) }
            dt { "Principal" } dd class="mono" { (e.principal) }
            dt { "Host" } dd class="mono" { (e.host) }
            dt { "Operation" } dd {
                span class="mono" { (operation_label(e.operation)) } " "
                @match e.permission {
                    AclPermission::Allow => span class="st ok" { "Allow" },
                    AclPermission::Deny => span class="st warn" { "Deny" },
                }
            }
        }
    }
}

fn hidden(e: &AclEntry) -> Markup {
    html! {
        input type="hidden" name="type" value=(kind_form(e.kind));
        input type="hidden" name="name" value=(e.name);
        input type="hidden" name="pattern" value=(e.pattern.form());
        input type="hidden" name="principal" value=(e.principal);
        input type="hidden" name="host" value=(e.host);
        input type="hidden" name="op" value=(operation_form(e.operation));
        input type="hidden" name="perm" value=(e.permission.form());
    }
}

fn delete_body(v: &AclDeleteView) -> Markup {
    html! {
        (entry_rows(&v.entry))
        (hidden(&v.entry))
        @if let Some(r) = &v.refused { div style="margin-bottom:16px" { (refused(r)) } }
        @if v.gone {
            div style="margin-bottom:16px" { (callout(Tone::Plain, Glyph::Info, html! {
                b { "This ACL is not there any more." } " Nobody needs to delete it: it was removed since this page was drawn, or by another tool."
            })) }
        } @else if !v.after.is_empty() {
            div style="margin-bottom:16px" { (callout(Tone::Warn, Glyph::Warn, html! {
                b { "It takes effect on the next request." } " " (spans(&v.after))
                " A connection that is already open stays open, and every request on it is checked again. Only the exact ACL is removed."
            })) }
        }
        (apicall("The call, asked as User:__console", &v.call, None))
    }
}

/// The title of the Delete dialog.
#[must_use]
pub fn acl_delete_title() -> &'static str {
    "Delete this ACL?"
}

/// The Delete ACL dialog: the overlay element over the Access page.
#[must_use]
pub fn acl_delete_dialog(v: &AclDeleteView, base: &Base) -> Markup {
    modal(
        "acl-delete",
        &base.url("/access/acls/delete"),
        &access_url(base),
        html! { (acl_delete_title()) },
        delete_body(v),
        html! {
            (cancel(&access_url(base)))
            @if !v.gone { button class="btn danger fill" type="submit" data-op="DeleteAcls" { "Delete ACL" } }
        },
    )
}
