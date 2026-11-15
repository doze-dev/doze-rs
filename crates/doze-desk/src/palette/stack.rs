//! What can be done to a stack, and in the app wherever you are (Go `stackCommands`, `appCommands`). Tables, kept
//! one row to a line (`rustfmt::skip`).

use doze_display as words;

use super::commands::{cmd, doing, page, stack_do, verb_on, with_text};
use super::{DoKind, Doing, Row, key_of};
use crate::view::StackView;

/// A stack action with its command line.
fn stack(
    key: &str,
    (glyph, label): (&'static str, String),
    verb: &str,
    cli: &str,
    also: &'static str,
) -> Row {
    Row {
        cli: cli.to_owned(),
        ..cmd(glyph, label, also, stack_do(key, verb))
    }
}

/// Everything a stack can have done to it, for its state (Go `stackCommands`).
#[must_use]
#[rustfmt::skip]
pub fn stack_commands(st: &StackView) -> Vec<Row> {
    let (key, name) = (key_of(st), st.name.as_str());
    let mut out = if st.running {
        vec![
            Row { note: "frees memory; the next connection wakes each".into(), cli: "doze sleep".into(), ..cmd("↓", format!("Sleep everything in {name}"), "stop all", verb_on(&key, "sleep", "", false)) },
            Row { cli: "doze wake".into(), ..cmd("↑", format!("Wake everything in {name}"), "start all", verb_on(&key, "wake", "", false)) },
            stack(&key, ("↻", format!("Restart {name}")), "restart", "doze down && doze up --last", "kill reboot"),
            stack(&key, ("■", format!("Down {name}")), "down", "doze down", "stop off"),
            stack(&key, ("↑", format!("Start everything else in {name}")), "up-all", "doze up --all", "up all"),
        ]
    } else {
        vec![
            stack(&key, ("↑", format!("Up {name} as last time")), "up-last", "doze up --last", "start on"),
            stack(&key, ("↑", format!("Up everything in {name}")), "up-all", "doze up --all", "start all"),
            cmd("☐", format!("Choose what starts in {name}…"), "pick select", doing(DoKind::Choose, &key)),
        ]
    };
    out.push(Row { note: "its overview".into(), ..cmd("→", format!("Open {name}"), "go show", page(&key, "home", "")) });
    out.push(Row { cli: "doze logs".into(), ..cmd("≡", format!("All logs of {name}"), "tail", page(&key, "logs", "")) });
    out.push(Row { note: "how long each service slept, what woke it, what crashed".into(), key: "A".into(), ..cmd("◷", format!("Activity of {name}"), "timeline history events savings", page(&key, "activity", "")) });
    if !st.dir.is_empty() {
        out.extend(folder_rows(st, &key));
    }
    out.push(Row { note: "stops it and stops listing it; every file stays".into(), ..cmd("⌫", format!("Remove {name} from doze…"), "forget delete", stack_do(&key, "remove")) });
    out
}

/// The rows about a stack's folder.
#[rustfmt::skip]
fn folder_rows(st: &StackView, key: &str) -> Vec<Row> {
    let (name, dir) = (st.name.as_str(), st.dir.as_str());
    let home = words::home();
    let tilde = words::tilde(dir, home.as_deref()).into_owned();
    let up = format!("cd {} && doze up", shell_dir(dir));
    vec![
        cmd("✎", format!("Edit {name}'s doze.hcl"), "config hcl", with_text(DoKind::Open, key, &format!("file://{dir}/doze.hcl"), "")),
        Row { note: tilde.clone(), ..cmd("▤", format!("Show {name} in Finder"), "folder reveal", with_text(DoKind::Open, key, &format!("file://{dir}"), "")) },
        Row { note: tilde.clone(), ..cmd("›_", format!("Open a terminal in {name}"), "shell cd", with_text(DoKind::Terminal, key, dir, "")) },
        Row { note: format!("cd {tilde} && doze up"), ..cmd("⧉", format!("Copy the command to bring {name} up"), "cli", with_text(DoKind::Copy, key, &up, "")) },
    ]
}

/// A folder as a shell takes it (Go `shellDir`).
fn shell_dir(dir: &str) -> String {
    if dir.contains([' ', '\'', '"', '$', '`', '\\']) {
        format!("'{}'", dir.replace('\'', r"'\''"))
    } else {
        dir.to_owned()
    }
}

/// The app's commands: label, glyph, id, key, the words it is also found by.
#[rustfmt::skip]
const APP: [(&str, &str, &str, &str, &str); 10] = [
    ("Home", "⌂", "home", "⌘0", "library stacks"),
    ("Needs you", "◉", "bell", "⌘⇧N", "problems alerts"),
    ("Settings", "⚙", "settings", "⌘,", "preferences"),
    ("Machine", "⚙", "settings:machine", "", "doctor setup"),
    ("New stack…", "+", "new", "", "create init"),
    ("Set up this Mac", "⚙", "repair", "", "repair doctor fix"),
    ("Clean up what was left behind", "✧", "cleanup", "", "zombies leftovers"),
    ("Keyboard shortcuts", "?", "help", "?", "keys help"),
    ("Check for updates", "↻", "updates", "", "upgrade new version release about"),
    ("Copy doze's version", "⧉", "version", "", "about"),
];

/// The app's choices: a prefix, a glyph, the id's prefix, the words, the values.
#[rustfmt::skip]
const CHOICES: [(&str, &str, &str, &str, &[&str]); 4] = [
    ("Mode", "◐", "mode", "appearance theme", &["system", "light", "dark"]),
    ("Theme", "●", "theme", "colour color", &["violet", "teal", "burgundy", "amber", "slate"]),
    ("Rows", "≡", "density", "density spacing", &["compact", "cosy", "roomy"]),
    ("Open at login", "⏻", "login", "startup", &["on", "off"]),
];

/// What the app itself can do, wherever you are (Go `appCommands`).
#[must_use]
pub fn app_commands() -> Vec<Row> {
    let app = |label: String, glyph, id: &str, key: &str, also| Row {
        key: key.to_owned(),
        ..cmd(glyph, label, also, with_text(DoKind::App, "", id, ""))
    };
    let mut out: Vec<Row> = APP
        .iter()
        .map(|&(label, glyph, id, key, also)| app(label.into(), glyph, id, key, also))
        .collect();
    // Open folder sits after Machine, as Go lists it.
    let folder = Row {
        note: "add a project to Home".into(),
        key: "⌘O".into(),
        ..cmd(
            "⌘O",
            "Open folder…".into(),
            "add project",
            Doing {
                kind: DoKind::Folder,
                ..Doing::default()
            },
        )
    };
    out.insert(4.min(out.len()), folder);
    for &(prefix, glyph, id, also, values) in &CHOICES {
        out.extend(values.iter().map(|v| {
            app(
                format!("{prefix}: {v}"),
                glyph,
                &format!("{id}:{v}"),
                "",
                also,
            )
        }));
    }
    out
}
