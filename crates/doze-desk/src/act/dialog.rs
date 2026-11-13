//! The confirmations (Go `view/dialog.go`): the question, who is using it, what happens, the buttons in the dash's
//! order, the foot. `needed` is false when nothing asks: sleeping or restarting an idle service never does. The
//! sentences are `doze-display`'s; this lays them out as the Confirm, Dialogs and Reset boards do, and serializes to
//! the Go app's view JSON, which the goldens compare.

use doze_display::{self as words, Instance, Kind, Snapshot};
use serde::Serialize;

use crate::view::StackView;

/// A confirmation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialog {
    /// `sleep`, `off`, `restart`, `reset`, `down`, `remove`, `action`, `setup`.
    pub kind: String,
    pub needed: bool,
    pub title: String,
    pub danger: bool,
    /// Who is using it, the names and figures strong.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sentence: Vec<Span>,
    /// "What happens" and "Data".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Kv>,
    /// A reset's rows: deletes, stops, then, way back.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<Fact>,
    /// The name a reset asks to be typed.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub type_name: String,
    /// How a script does it without being asked.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub command: String,
    /// It asks for a line of text, with `hint` in the box.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub input: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hint: String,
    /// The buttons run a module action (their verb is its id, their first target the row) on `instance`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub action: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub instance: String,
    /// The buttons run a stack action (down, remove) on the stack, not a verb on its services.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stack: bool,
    /// The folder the buttons set up (the verb is the way: `add`; the input is the name to add).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub setup: String,
    pub buttons: Vec<Button>,
    /// The button ⏎ presses.
    pub default: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub foot: Vec<String>,
}

/// A run of a sentence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Span {
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strong: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mono: bool,
}

impl Span {
    pub(crate) fn plain(text: impl Into<String>) -> Span {
        Span {
            text: text.into(),
            ..Span::default()
        }
    }

    pub(crate) fn strong(text: impl Into<String>) -> Span {
        Span {
            text: text.into(),
            strong: true,
            mono: false,
        }
    }
}

/// A row of "what happens".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Kv {
    pub k: String,
    pub v: String,
    pub mono: bool,
}

pub(crate) fn kv(k: &str, v: impl Into<String>) -> Kv {
    Kv {
        k: k.to_owned(),
        v: v.into(),
        mono: false,
    }
}

/// One row of a reset dialog.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Fact {
    pub k: String,
    pub spans: Vec<Span>,
    pub tone: String,
}

/// One choice: the verb it runs on which targets, in order, or a cancel with no verb.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Button {
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub verb: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub primary: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub danger: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cancel: bool,
}

impl Button {
    pub fn run(label: impl Into<String>, verb: &str, targets: &[&str]) -> Button {
        Button {
            label: label.into(),
            verb: verb.to_owned(),
            targets: targets.iter().map(|t| (*t).to_owned()).collect(),
            ..Button::default()
        }
    }

    pub fn primary(mut self, on: bool) -> Button {
        self.primary = on;
        self
    }

    pub fn cancel() -> Button {
        Button {
            label: "Cancel".into(),
            cancel: true,
            ..Button::default()
        }
    }
}

/// The confirmation for `verb` on the instance `name` of `st` (Go `DialogOf`); with no name, `down` and `remove` are
/// the stack's own. `home` is where `~` stands, for the reset's folder.
pub fn dialog_of(st: &StackView, name: &str, verb: &str, home: Option<&str>) -> Dialog {
    if name.is_empty() && verb == "down" {
        return super::asks::stop_dialog(st);
    }
    if name.is_empty() && (verb == "forget" || verb == "remove") {
        return super::asks::forget_dialog(st);
    }
    let snap = st.words();
    let Some(v) = snap.find(name) else {
        return Dialog {
            kind: verb.to_owned(),
            ..Dialog::default()
        };
    };
    match verb {
        "off" => off_dialog(v),
        "sleep" => sleep_dialog(v, &snap),
        "restart" => restart_dialog(v),
        "reset" => reset_dialog(v, home),
        _ => Dialog {
            kind: verb.to_owned(),
            ..Dialog::default()
        },
    }
}

fn spans(sentence: Vec<words::Span>) -> Vec<Span> {
    sentence
        .into_iter()
        .map(|s| Span {
            text: s.text.into_owned(),
            strong: s.strong,
            mono: false,
        })
        .collect()
}

const KEPT: &str = "Kept. Nothing is deleted.";

fn sleep_dialog(v: &Instance, snap: &Snapshot) -> Dialog {
    let mut d = Dialog {
        kind: "sleep".into(),
        title: format!("Sleep {}?", v.name),
        ..Dialog::default()
    };
    if words::in_use(v).is_none() {
        return d;
    }
    d.needed = true;
    d.sentence = spans(words::held_sentence(v));
    d.rows = vec![
        kv(
            "What happens",
            "Connections drop. The next connection wakes it again, so it is back within seconds unless what uses it sleeps too.",
        ),
        kv("Data", KEPT),
    ];
    // What uses it, less what is pinned awake: a pin is the person's word that it stays up.
    let (pinned, deps): (Vec<&str>, Vec<&str>) = words::dependants(v)
        .into_iter()
        .partition(|n| snap.find(n).is_some_and(|o| o.keep_awake));
    if !pinned.is_empty() {
        d.rows.push(kv("Pinned", format!("{} stays up, pinned awake, and wakes {n} again when it connects. Turn {n} off to keep it down.", and_words(&pinned), n = v.name)));
    }
    let n = v.name.as_str();
    if !deps.is_empty() {
        // Dependants first, then the target, as the dash's sleepKey did.
        let mut targets = deps.clone();
        targets.push(n);
        d.buttons.push(
            Button::run(
                format!("Sleep {n} and {}", deps.join(", ")),
                "sleep",
                &targets,
            )
            .primary(true),
        );
    }
    d.buttons.extend([
        Button::run(format!("Sleep {n} only"), "sleep", &[n]).primary(deps.is_empty()),
        // Off keeps it down whatever connects, until it is switched on.
        Button::run(format!("Turn {n} off"), "off", &[n]),
        Button::cancel(),
    ]);
    d.foot = vec![
        "doze only asks when something else is affected. Sleeping an idle service never asks."
            .into(),
    ];
    d
}

/// Names joined as a sentence joins them.
pub(crate) fn and_words(names: &[&str]) -> String {
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

fn restart_dialog(v: &Instance) -> Dialog {
    let mut d = Dialog {
        kind: "restart".into(),
        title: format!("Restart {}?", v.name),
        ..Dialog::default()
    };
    if words::in_use(v).is_none() || !words::awake_now(v) || v.kind == Kind::Task {
        return d;
    }
    d.needed = true;
    d.sentence = spans(words::held_sentence(v));
    d.rows = vec![
        kv(
            "What happens",
            "It stops and starts again. Every connection drops; clients that reconnect find it back in a few seconds, those that do not stay down.",
        ),
        kv("Data", KEPT),
    ];
    d.buttons = words::restart_buttons(v)
        .into_iter()
        .enumerate()
        .map(|(i, label)| {
            if label == "Cancel" {
                Button::cancel()
            } else {
                Button::run(label, "restart", &[v.name.as_str()]).primary(i == 0)
            }
        })
        .collect();
    d.foot = vec![
        "doze only asks when something is using it. Restarting an idle one never asks.".into(),
    ];
    d
}

fn reset_dialog(v: &Instance, home: Option<&str>) -> Dialog {
    let n = v.name.as_str();
    let command = words::reset_command(n);
    Dialog {
        kind: "reset".into(),
        needed: true,
        danger: true,
        title: format!("reset {n} · this deletes data"),
        type_name: n.to_owned(),
        facts: words::reset_facts(v, home)
            .into_iter()
            .map(reset_fact)
            .collect(),
        buttons: vec![
            Button {
                danger: true,
                ..Button::run(format!("Reset {n}"), "reset", &[n])
            },
            Button::cancel(),
        ],
        // Cancel is the default; Reset is disabled until the name is typed.
        default: 1,
        foot: vec![
            "Typing the name is only asked when data is deleted. Cancel is the default button."
                .into(),
            format!("From a script: {command}"),
        ],
        command,
        ..Dialog::default()
    }
}

/// One of the reset's facts as the Reset board draws it: the folder in mono, "none." in bold, the rest plain.
fn reset_fact(f: words::Fact) -> Fact {
    let fact = |tone: &str, spans: Vec<Span>| Fact {
        k: f.key.to_owned(),
        spans,
        tone: tone.to_owned(),
    };
    match f.key {
        "deletes" => {
            if let Some(rest) = f.text.strip_prefix("everything in ") {
                let folder = Span {
                    text: rest.to_owned(),
                    strong: true,
                    mono: true,
                };
                return fact("text", vec![Span::plain("everything in "), folder]);
            }
        }
        "way back" => {
            if let Some((first, rest)) = f.text.split_once(' ') {
                return fact(
                    "text",
                    vec![Span::strong(first), Span::plain(format!(" {rest}"))],
                );
            }
        }
        _ => {}
    }
    fact(f.tone.as_str(), vec![Span::plain(f.text.clone())])
}

/// Asks before a service in use is turned off: unlike sleep, a connection will not bring it back.
fn off_dialog(v: &Instance) -> Dialog {
    let mut d = Dialog {
        kind: "off".into(),
        title: format!("Turn {} off?", v.name),
        ..Dialog::default()
    };
    if words::in_use(v).is_none() {
        return d;
    }
    let n = v.name.as_str();
    d.needed = true;
    d.sentence = spans(words::held_sentence(v));
    d.rows = vec![
        kv(
            "What happens",
            "It stops, and nothing wakes it: what uses it fails to connect until you switch it on.",
        ),
        kv("Data", KEPT),
    ];
    d.buttons = vec![
        Button::run(format!("Turn {n} off"), "off", &[n]),
        Button::run("Sleep it instead", "sleep", &[n]).primary(true),
        Button::cancel(),
    ];
    d.default = 1;
    d.foot = vec![format!("From a script: doze off {n}")];
    d
}
