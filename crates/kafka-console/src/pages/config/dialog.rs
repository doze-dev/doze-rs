//! The edit dialogs (boards `config-edit`, `config-edit-list`, `config-edit-lag`): one value, a list with append and
//! subtract, and the two compaction lags together. Each is one POST form; Save sends IncrementalAlterConfigs, Check
//! sends the same call with `validate_only` and changes nothing.

use maud::{Markup, html};

use super::common::config_url;
use crate::base::Base;
use crate::config_words::gloss;
use crate::model::{ConfigChecked, ConfigEditView, ConfigForm, ConfigOp, ConfigTeach, Span};
use crate::ui::dialog::{cancel, modal};
use crate::ui::glyph::Glyph;
use crate::ui::notice::{Tone, callout};
use crate::ui::write::{apicall, field_error, spans, text_input};

fn field(id: &str, label: Markup, control: Markup, under: Markup) -> Markup {
    html! { div class="field" { label for=(id) { (label) } (control) (under) } }
}

fn line(label: &str, parts: &[Span]) -> Markup {
    html! { @if !parts.is_empty() { div { b { (label) } " " (spans(parts)) } } }
}

fn teaching(t: &ConfigTeach) -> Markup {
    html! {
        div class="teach" data-extra="teaching" {
            (line("What it is", &t.what))
            (line("Now", &t.now))
            (line("Allowed", &t.allowed))
            (line("Takes effect", &t.takes_effect))
            (line("Here", &t.here))
        }
    }
}

/// The unit under a number: what the typed value comes to in words.
fn unit(key: &str, typed: &str) -> Option<String> {
    gloss(key, typed)
}

fn ops(v: &ConfigEditView) -> Markup {
    const OPS: [(ConfigOp, &str, &str); 3] = [
        (ConfigOp::Append, "Append", "add to the list"),
        (ConfigOp::Subtract, "Subtract", "take out of it"),
        (ConfigOp::Set, "Set", "replace it all"),
    ];
    html! {
        div class="field" {
            span class="lab" { "It is a list. What to do with it?" }
            div class="choice" style="gap:10px" {
                @for (op, name, sub) in OPS {
                    @let on = v.input.op == op;
                    label class={"opt" @if on { " on" }} {
                        div class="opt-row" {
                            input type="radio" name="op" value=(op.form()) checked[on];
                            span class={"radio" @if on { " on" }} {}
                            div { div class="t1" { (name) } div class="t2" { (sub) } }
                        }
                    }
                }
            }
        }
    }
}

fn list_label(v: &ConfigEditView) -> String {
    let what = if v.row.name == "cleanup.policy" {
        "Policy"
    } else {
        "Value"
    };
    let to = match v.input.op {
        ConfigOp::Subtract => "to take out",
        ConfigOp::Set => "to use",
        _ => "to append",
    };
    format!("{what} {to}")
}

fn list_field(v: &ConfigEditView) -> Markup {
    let have: Vec<&str> = v
        .row
        .value
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .collect();
    let mark = match v.input.op {
        ConfigOp::Append => "+",
        ConfigOp::Subtract => "-",
        _ => "",
    };
    html! {
        div class="field" {
            label for="p" { (list_label(v)) }
            div class="row" style="gap:8px" {
                @if v.input.op != ConfigOp::Set {
                    @for e in &have { span class="pill mono" { (e) } }
                    span class="dim" { (mark) }
                }
                div class="input mono err-none" style="flex:1" {
                    input id="p" name="value" type="text" value=(v.input.value) autocomplete="off" spellcheck="false" aria-invalid=[v.error.is_some().then_some("true")];
                }
            }
            @if let Some(e) = &v.error { (field_error(e)) }
            (teaching(&v.teach))
        }
    }
}

fn single_field(v: &ConfigEditView) -> Markup {
    let bad = v.error.is_some();
    field(
        "v",
        html! { "New value for " span class="mono" { (v.topic) } },
        text_input(
            "v",
            "value",
            &v.input.value,
            true,
            unit(&v.row.name, &v.input.value).as_deref(),
            bad,
        ),
        html! {
            @if let Some(e) = &v.error { (field_error(e)) }
            (teaching(&v.teach))
        },
    )
}

fn lag_fields(v: &ConfigEditView) -> Markup {
    let max = v
        .other
        .as_ref()
        .map_or("max.compaction.lag.ms", |o| o.name.as_str());
    html! {
        (field("a", html! { (v.row.name) },
            text_input("a", "value", &v.input.value, true, unit(&v.row.name, &v.input.value).as_deref(), v.error.is_some()),
            html! {}))
        (field("b", html! { (max) },
            text_input("b", "other", &v.input.other, true, unit(max, &v.input.other).as_deref(), v.error.is_some()),
            html! {
                @if let Some(e) = &v.error { (field_error(e)) }
                (teaching(&v.teach))
            }))
    }
}

/// What a dry run said: Kafka would take it, what each config is now and would be, and when it would count.
fn checked(v: &ConfigEditView, c: &ConfigChecked) -> Markup {
    callout(
        Tone::Plain,
        Glyph::Check,
        html! {
            b { "Kafka would take this." }
            " The same call with validate_only answered 0, and nothing was changed."
            dl class="kv sm" style="margin-top:12px" {
                @for (key, now, after) in &c.after {
                    dt { (key) }
                    dd class="mono" { (now) " " span class="dim" { "to" } " " (after) }
                }
                dt { "Takes effect" } dd { (spans(&v.teach.takes_effect)) }
            }
        },
    )
}

fn body(v: &ConfigEditView) -> Markup {
    html! {
        div class="form" style="gap:18px" {
            input type="hidden" name="key" value=(v.row.name);
            @match v.form {
                ConfigForm::Single => (single_field(v)),
                ConfigForm::List => { (ops(v)) (list_field(v)) },
                ConfigForm::Lags => (lag_fields(v)),
            }
            @if let Some(c) = &v.checked { (checked(v, c)) }
            (apicall("The call", &v.call, None))
        }
    }
}

fn title(v: &ConfigEditView) -> String {
    match v.form {
        ConfigForm::Lags => "Edit compaction lag".to_owned(),
        _ => format!("Edit {}", v.row.name),
    }
}

/// The dialog, for any of the three shapes.
#[must_use]
pub fn edit_dialog(v: &ConfigEditView, base: &Base, inline: bool) -> Markup {
    modal(
        "config-edit",
        &config_url(base, &v.topic, "/edit"),
        inline,
        html! { (title(v)) },
        body(v),
        html! {
            (cancel(&config_url(base, &v.topic, "")))
            button class="btn" type="submit" name="check" value="1" { "Check" }
            button class="btn pri" type="submit" data-op="IncrementalAlterConfigs" { "Save" }
        },
    )
}

#[must_use]
pub fn dialog_title(v: &ConfigEditView) -> String {
    title(v)
}
