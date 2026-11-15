//! The questions that are not about one service's verb: a whole stack going down, a stack removed from doze, a
//! module's action that takes input or deletes data, and the empty folder's "Add one thing" (Go `stopDialog`,
//! `forgetDialog`, `ActionDialog`, `EmptyOf`'s `Add`).

use doze_display as words;

use super::dialog::{Button, Dialog, Span, kv};
use crate::page::console::Action;
use crate::view::StackView;

fn stack_name(st: &StackView, none: &str) -> String {
    if st.name.is_empty() {
        none.to_owned()
    } else {
        st.name.clone()
    }
}

/// The question before a whole stack goes down: asked only when something in it is in use.
pub fn stop_dialog(st: &StackView) -> Dialog {
    let name = stack_name(st, "the stack");
    let mut d = Dialog {
        kind: "down".into(),
        title: format!("Stop {name}?"),
        stack: true,
        ..Dialog::default()
    };
    let snap = st.words();
    let t = words::tally(&snap.instances);
    if t.conns == 0 {
        return d;
    }
    let up = t.awake + t.dozing + t.waking;
    d.needed = true;
    d.sentence = vec![
        Span::strong(format!(
            "{} client connection{}",
            t.conns,
            words::plural(t.conns)
        )),
        Span::plain(" are open to it, and "),
        Span::strong(format!("{up} service{}", words::plural(up as u64))),
        Span::plain(" are up."),
    ];
    d.rows = vec![
        kv(
            "What happens",
            "Every service stops and every connection drops. Nothing wakes until the stack is up again; Up as last time brings back this set.",
        ),
        kv("Data", "Kept. Nothing is deleted."),
    ];
    d.buttons = vec![
        Button::run(format!("Stop {name}"), "down", &[]).primary(true),
        Button::cancel(),
    ];
    d.foot = vec!["From a script: doze down".into()];
    d
}

/// The question before a stack is removed from doze, running or not: its services stop, its files stay. Always asked.
pub fn forget_dialog(st: &StackView) -> Dialog {
    let name = stack_name(st, "this stack");
    let what = if st.running {
        "Its services stop, then doze stops listing it: on Home, as a tab and in the menu bar."
    } else {
        "doze stops listing it: on Home, as a tab and in the menu bar."
    };
    Dialog {
        kind: "remove".into(),
        title: format!("Remove {name} from doze?"),
        stack: true,
        needed: true,
        rows: vec![
            kv("What happens", what),
            kv(
                "Files",
                "Kept: the folder, its doze.hcl and its data stay on disk. Open the folder again (⌘O, or drop it on the window) to bring it back, data and all.",
            ),
        ],
        buttons: vec![
            Button::run(format!("Remove {name}"), "remove", &[]).primary(true),
            Button::cancel(),
        ],
        default: 1,
        ..Dialog::default()
    }
}

/// The confirmation for a module's action on `name`: input when it takes some, a question when it deletes something;
/// `needed` is false when it should just run (Go `ActionDialog`).
pub fn action_dialog(name: &str, a: &Action) -> Dialog {
    let mut d = Dialog {
        kind: "action".into(),
        needed: a.asks(),
        title: a.label.clone(),
        instance: name.to_owned(),
        action: true,
        ..Dialog::default()
    };
    if !a.resource.is_empty() {
        d.sentence = vec![
            Span::plain("for "),
            Span {
                text: a.resource.clone(),
                strong: true,
                mono: true,
            },
            Span::plain(format!(" · {name}")),
        ];
    }
    let mut b = Button::run(a.label.clone(), &a.id, &[a.resource.as_str()]).primary(true);
    if !a.hint.is_empty() {
        d.input = true;
        d.hint.clone_from(&a.hint);
        "Do it".clone_into(&mut b.label);
    } else if a.destructive {
        d.danger = true;
        d.title = format!("{}?", a.label);
        d.rows = vec![kv("", "This deletes data and cannot be undone.")];
        (b.primary, b.danger) = (false, true);
    }
    d.buttons = vec![b, Button::cancel()];
    d
}

/// "Add one thing" in a folder with no doze.hcl: the name to add, and `doze add` writes the file.
pub fn add_dialog(dir: &str) -> Dialog {
    Dialog {
        kind: "setup".into(),
        needed: true,
        title: "Add one thing".into(),
        setup: dir.to_owned(),
        input: true,
        hint: "postgres, valkey, mongo, …".into(),
        sentence: vec![
            Span::plain("name a module or a service; "),
            Span {
                text: "doze add".into(),
                strong: true,
                mono: true,
            },
            Span::plain(" writes doze.hcl with it"),
        ],
        buttons: vec![
            Button::run("Add", "add", &[]).primary(true),
            Button::cancel(),
        ],
        ..Dialog::default()
    }
}
