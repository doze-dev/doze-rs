//! Add partitions: the dialog (board `topic-add-partitions`) and the refused dialog (board
//! `topic-add-partitions-refused`).

use maud::{Markup, html};

use super::{field, opt, submit, topic_url};
use crate::base::Base;
use crate::model::AddView;
use crate::ui::dialog::{cancel, modal};
use crate::ui::write::{apicall, field_error, refused, teach, text_input};
use crate::write_words::{add_adds as adds, add_button as button_label, add_teaching as teaching};

fn body(v: &AddView) -> Markup {
    let bad = v.error.is_some();
    let unit = adds(v);
    html! {
        div class="form" style="gap:18px" {
            (field("c", html! { "Partitions after" (opt(&format!("now {}", v.have))) },
                text_input("c", "count", &v.count, true, unit.as_deref(), bad),
                html! {
                    @if let Some(e) = &v.error { (field_error(e)) }
                    @if v.refusal.is_none() { (teach(&teaching(v))) }
                }))
            @if v.refusal.is_none() {
                (field("ra", html! { "Replica assignment" }, html! {
                    div class="input ro" { input id="ra" type="text" value="ignored: this broker is the only replica" readonly; }
                }, html! {}))
                div class="help" { "Never fewer. To get fewer partitions, create a new topic." }
            }
            @if let Some(r) = &v.refusal { (refused(r)) }
            @if let Some(a) = &v.answer { (apicall("The answer", a, None)) } @else { (apicall("The call", &v.call, None)) }
        }
    }
}

/// The dialog, in its two states: the form, and the form after a refusal.
#[must_use]
pub fn add_partitions_dialog(v: &AddView, base: &Base) -> Markup {
    let back = topic_url(base, &v.topic, "/partitions");
    let foot = if v.refusal.is_some() {
        html! {
            (cancel(&back))
            a class="btn pri" href=(topic_url(base, &v.topic, "/partitions/add")) { "Change the count" }
        }
    } else {
        html! {
            (cancel(&back))
            (submit("btn pri", Some("CreatePartitions"), None, html! { (button_label(v)) }))
        }
    };
    modal(
        "add-partitions",
        &topic_url(base, &v.topic, "/partitions/add"),
        &back,
        html! { "Add partitions to " span class="mono" { (v.topic) } },
        body(v),
        foot,
    )
}
