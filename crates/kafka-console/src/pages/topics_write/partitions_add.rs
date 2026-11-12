//! Add partitions: the dialog (board `topic-add-partitions`) and the refused dialog (board
//! `topic-add-partitions-refused`).

use maud::{Markup, html};

use super::{field, opt, submit, topic_url};
use crate::base::Base;
use crate::format::number;
use crate::model::{AddView, Span, Teach};
use crate::ui::dialog::{cancel, modal};
use crate::ui::write::{apicall, field_error, refused, teach, text_input};

/// What the field's unit says: how many partitions the count adds, and their names.
fn adds(v: &AddView) -> Option<String> {
    let want: i32 = v.count.trim().parse().ok()?;
    let more = want.checked_sub(v.have).filter(|n| *n > 0)?;
    let names: Vec<String> = (v.have..want)
        .take(3)
        .map(|p| format!("{}-{p}", v.topic))
        .collect();
    let tail = if more > 3 { ", ..." } else { "" };
    let joined = match names.as_slice() {
        [a, b] => format!("{a} and {b}"),
        _ => names.join(", "),
    };
    Some(format!("adds {more}: {joined}{tail}"))
}

fn teaching(v: &AddView) -> Teach {
    let range = match v.most {
        Some(m) => vec![
            Span::text(format!(
                "{} to {} (",
                number(i64::from(v.have) + 1),
                number(i64::from(m))
            )),
            Span::mono("--max-partitions"),
            Span::text(")."),
        ],
        None => vec![Span::text(format!(
            "{} or more.",
            number(i64::from(v.have) + 1)
        ))],
    };
    Teach {
        says: "the new total, not the number to add.".to_owned(),
        default: vec![Span::text(format!("none; it must be above {}.", v.have))],
        range,
        takes_effect: vec![Span::text(
            "at once: producers and consumers see the new partitions in their next Metadata. They start empty.",
        )],
        here: vec![Span::text(
            "records with the same key may now hash to a different partition, so order for a key is not kept across the change.",
        )],
    }
}

fn button_label(v: &AddView) -> String {
    match v
        .count
        .trim()
        .parse::<i32>()
        .ok()
        .and_then(|w| w.checked_sub(v.have))
        .filter(|n| *n > 0)
    {
        Some(1) => "Add 1 partition".to_owned(),
        Some(n) => format!("Add {n} partitions"),
        None => "Add partitions".to_owned(),
    }
}

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
