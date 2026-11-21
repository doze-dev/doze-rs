//! A fix pressed outside the window (Go `Service.FixItem`, `app/notify.go`'s `firstFix`): from a notification's
//! button or the menu bar's Needs you, where there is no dialog. Only the fixes that ask nothing are pressed there;
//! the rest open the item's page.

use doze_control::Verb;

use super::Note;
use crate::StackKey;
use crate::act::StackAction;
use crate::attention::{Attention, Fix, FixKind, Item};

/// What pressing a fix outside the window does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outside {
    /// A verb on the instance, at once (restart, run the task again).
    Verb(StackKey, String, Verb),
    /// Stop the process holding the port the instance's crash names (its pid looked at again first), then restart
    /// the instance.
    Holder(StackKey, String, i64),
    /// A stack's action: Up still looks first, and one that would ask opens the window.
    Stack(StackKey, StackAction),
    /// Put a failed start away.
    Dismiss(StackKey),
    /// Clean up what stacks left behind (Go `ClearLeftover("all")`): the app runs it from Settings › Machine, where
    /// a step that needs the password shows in the run sheet first.
    Clean,
    /// This one asks first: open the item's page.
    Page,
}

/// What pressing `f` on `it` outside the window does (Go `FixItem`).
#[must_use]
pub fn outside(it: &Item, f: &Fix) -> Outside {
    let key = StackKey::new(&it.key);
    match (f.kind, f.verb) {
        (FixKind::Do, "restart") => Outside::Verb(key, it.instance.clone(), Verb::Restart),
        (FixKind::Do, "task") => Outside::Verb(key, it.instance.clone(), Verb::Task),
        (FixKind::Stack, "restart") => Outside::Stack(key, StackAction::Restart),
        (FixKind::Stack, "up-last") => Outside::Stack(key, StackAction::UpLast),
        (FixKind::Holder, _) if f.pid > 0 => Outside::Holder(key, it.instance.clone(), f.pid),
        (FixKind::Dismiss, _) => Outside::Dismiss(key),
        (FixKind::Clean, _) => Outside::Clean,
        _ => Outside::Page,
    }
}

/// The item `id`'s first fix that a notification can carry as a button: a port's holder, a restart or a task run
/// (Go `firstFix`).
#[must_use]
pub fn first_fix(a: &Attention, id: &str) -> Option<(Item, Fix)> {
    a.items
        .iter()
        .filter(|it| it.id == id && !it.fixed)
        .find_map(|it| {
            it.fixes
                .iter()
                .find(|f| {
                    f.kind == FixKind::Holder
                        || (f.kind == FixKind::Do && matches!(f.verb, "restart" | "task"))
                })
                .map(|f| (it.clone(), f.clone()))
        })
}

/// A notification as the system shows it: the note's words, with its sub-line under the body, and the item's first
/// fix as its one button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Native {
    /// Replaces an earlier one with the same tag (the note's episode).
    pub tag: String,
    pub title: String,
    pub body: String,
    /// The button's label, when the item has a fix the notification can press.
    pub action: Option<String>,
    /// The Needs you item the button fixes, and the fix as it was offered (a holder's pid is pressed only while it
    /// is still the one named).
    pub fix: Option<(Item, Fix)>,
}

/// The notification for `note`, with the first fix Needs you has for its instance.
#[must_use]
pub fn native(note: &Note, a: &Attention) -> Native {
    let mut body = note.body.clone();
    if !note.sub.is_empty() {
        body = format!("{body}\n{}", note.sub);
    }
    let id = format!("instance:{}/{}", note.key, note.instance);
    let fix = first_fix(a, &id);
    Native {
        tag: note.id.clone(),
        title: note.title.clone(),
        body,
        action: fix.as_ref().map(|(_, f)| f.label.clone()),
        fix,
    }
}
