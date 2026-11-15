//! The words of the confirmations (sleep, reset, and the restart question): who is using the instance, what the
//! buttons offer, what a reset deletes. A surface draws them in its own boxes; the sentences and the order of the
//! buttons are the same everywhere.

use std::borrow::Cow;

use crate::format::{plural, tilde};
use crate::look::Tone;
use crate::model::Instance;
use crate::note::dependant;
use crate::reach::connections;

/// The instances holding `v` awake because they need it. (Go: `Dependants`.)
#[must_use]
pub fn dependants(v: &Instance) -> Vec<&str> {
    v.holders.iter().filter_map(dependant).collect()
}

/// The choices of the sleep question, in order, the first being the default: with what depends on it, or without.
/// The last is always Cancel. (Go: `SleepButtons`.)
#[must_use]
pub fn sleep_buttons(v: &Instance) -> Vec<String> {
    let deps = dependants(v);
    if deps.is_empty() {
        return vec![format!("Sleep {}", v.name), "Cancel".to_owned()];
    }
    vec![
        format!("Sleep {} and {}", v.name, deps.join(", ")),
        format!("Sleep {} only", v.name),
        "Cancel".to_owned(),
    ]
}

/// The choices of the restart question, Restart first. (Go: `RestartButtons`.)
#[must_use]
pub fn restart_buttons(v: &Instance) -> [String; 2] {
    [format!("Restart {}", v.name), "Cancel".to_owned()]
}

/// A run of a sentence, and whether it is the part to draw bright: a name or a figure the sentence is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// The words.
    pub text: Cow<'static, str>,
    /// Draw it bright.
    pub strong: bool,
}

impl Span {
    fn strong(text: String) -> Self {
        Self {
            text: Cow::Owned(text),
            strong: true,
        }
    }

    fn plain(text: impl Into<Cow<'static, str>>) -> Self {
        Self {
            text: text.into(),
            strong: false,
        }
    }
}

/// Who is using `v`, for the sleep and restart questions: "api is running and needs it, and 3 connections are
/// open to it right now", "gateway, orders and recommendations are running and need it." The names and the count
/// are strong. (Go: `HeldSentence`.)
#[must_use]
pub fn held_sentence(v: &Instance) -> Vec<Span> {
    let (deps, conns) = (dependants(v), connections(v));
    let count = format!("{conns} connection{}", plural(u64::from(conns)));
    let open = if conns == 1 {
        " is open to it right now."
    } else {
        " are open to it right now."
    };
    let need = if deps.len() > 1 {
        " are running and need it"
    } else {
        " is running and needs it"
    };
    if deps.is_empty() {
        return vec![Span::strong(count), Span::plain(open)];
    }
    if conns == 0 {
        return vec![
            Span::strong(and_list(&deps)),
            Span::plain(format!("{need}.")),
        ];
    }
    vec![
        Span::strong(and_list(&deps)),
        Span::plain(format!("{need}, and ")),
        Span::strong(count),
        Span::plain(open),
    ]
}

/// Names joined as a sentence joins them: "a", "a and b", "a, b and c".
fn and_list(names: &[&str]) -> String {
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

/// One row of a confirmation: a key down the left and what it says, in the tone it is drawn in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// The key.
    pub key: &'static str,
    /// What it says.
    pub text: String,
    /// The tone it is drawn in.
    pub tone: Tone,
}

/// The rows of the reset question: what it deletes, what it stops, what happens then, and that there is no way
/// back. `home` is where `~` stands ([`crate::home`]). (Go: `ResetFacts`.)
#[must_use]
pub fn reset_facts(v: &Instance, home: Option<&str>) -> [Fact; 4] {
    let folder = tilde(&v.data_dir, home);
    let folder = if folder.is_empty() {
        "its data folder"
    } else {
        &folder
    };
    [
        Fact {
            key: "deletes",
            text: format!("everything in {folder}"),
            tone: Tone::Bright,
        },
        Fact {
            key: "stops",
            text: format!("{} if it is awake", v.name),
            tone: Tone::Text,
        },
        Fact {
            key: "then",
            text: format!(
                "{} starts empty on its next wake, and its setup runs again",
                v.name
            ),
            tone: Tone::Text,
        },
        Fact {
            key: "way back",
            text: "none. There is no snapshot.".to_owned(),
            tone: Tone::Warn,
        },
    ]
}

/// How a script resets an instance without being asked. (Go: `ResetCommand`.)
#[must_use]
pub fn reset_command(name: &str) -> String {
    format!("doze reset {name} --yes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Holder;

    // Go: TestHeldSentenceAgrees. The held sentence agrees with its subject: one dependant is running, three are;
    // one connection is open, two are.
    #[test]
    fn held_sentence_agrees() {
        let dep = |n: &str| Holder::Dependant(n.to_owned());
        for (holders, want) in [
            (vec![dep("api")], "api is running and needs it."),
            (
                vec![dep("gateway"), dep("orders"), dep("recommendations")],
                "gateway, orders and recommendations are running and need it.",
            ),
            (
                vec![dep("orders"), dep("gateway")],
                "orders and gateway are running and need it.",
            ),
            (
                vec![dep("api"), Holder::Connections(3)],
                "api is running and needs it, and 3 connections are open to it right now.",
            ),
            (
                vec![dep("api"), Holder::Connections(1)],
                "api is running and needs it, and 1 connection is open to it right now.",
            ),
            (
                vec![Holder::Connections(1)],
                "1 connection is open to it right now.",
            ),
        ] {
            let v = Instance {
                holders,
                ..Instance::default()
            };
            let said: String = held_sentence(&v).iter().map(|s| s.text.as_ref()).collect();
            assert_eq!(said, want);
        }
    }
}
