//! Finding: the empty palette, names, a name then words, and the rows that stand for a service or a stack (Go
//! `view/palette.go`: `atHand`, `byName`, `byObject`, `instanceRow`, `actsOf`).

use std::time::SystemTime;

use doze_display::{self as words, Instance, Kind, Snapshot};

use super::commands::{filter_rows, page, verb_on, with_text};
use super::instance::instance_commands;
use super::stack::{app_commands, stack_commands};
use super::verbs::alias;
use super::{
    Act, Ask, DoKind, Doing, Row, Scope, Section, Svc, asks, dot_of, first_of, key_of, web_url,
};
use crate::view::StackView;

/// A service in a stack, as the palette names it.
fn enter(st: &StackView, name: &str) -> Option<Scope> {
    Some(Scope {
        key: key_of(st),
        name: Some(name.to_owned()),
    })
}

/// A stack to tab into.
fn enter_stack(st: &StackView) -> Option<Scope> {
    Some(Scope {
        key: key_of(st),
        name: None,
    })
}

/// "Search api's logs for “timeout”": opens logs filtered to words.
fn log_search(st: &StackView, inst: &str, words: &str) -> Row {
    let whose = if inst.is_empty() {
        st.name.as_str()
    } else {
        inst
    };
    Row {
        glyph: "⌕",
        tone: "dim",
        label: format!("Search {whose}'s logs for “{words}”"),
        key: "⏎ search".into(),
        doing: Doing {
            text: words.to_owned(),
            ..page(&key_of(st), "logs", inst)
        },
        ..Row::default()
    }
}

/// "<service or stack> <what>": a command for a name ("api restart"), or words to look for in its logs ("api
/// timeout"). `None` when the first word names nothing, or nothing follows it (Go `byObject`).
pub(crate) fn by_object(
    stacks: &[&StackView],
    current: &str,
    first: &str,
    rest: &str,
    now: SystemTime,
) -> Option<Vec<Section>> {
    if rest.is_empty() {
        return None;
    }
    let mut ordered = stacks.to_vec();
    ordered.sort_by_key(|s| key_of(s) != current);
    for st in &ordered {
        if st.name.eq_ignore_ascii_case(first) {
            let rows = filter_rows(stack_commands(st), rest);
            let rows = if rows.is_empty() {
                vec![log_search(st, "", rest)]
            } else {
                rows
            };
            return Some(vec![Section {
                title: st.name.clone(),
                rows,
            }]);
        }
        if !st.running {
            continue;
        }
        let snap = st.words();
        if let Some(v) = snap.find(first) {
            let q = alias(&rest.to_lowercase()).to_owned();
            let rows = filter_rows(instance_commands(st, Svc::of(st, v), now), &q);
            let rows = if rows.is_empty() {
                vec![log_search(st, &v.name, rest)]
            } else {
                rows
            };
            let title = format!("{} in {}", v.name, st.name);
            return Some(vec![Section { title, rows }]);
        }
    }
    if first == "logs" {
        let st = ordered.iter().find(|s| s.running)?;
        return Some(vec![Section {
            title: "Logs".into(),
            rows: vec![log_search(st, "", rest)],
        }]);
    }
    None
}

/// The services needing you, across the running stacks.
fn needs(stacks: &[&StackView], now: SystemTime) -> Vec<Row> {
    let mut out = Vec::new();
    for st in stacks.iter().filter(|s| s.running) {
        let snap = st.words();
        for v in snap.instances.iter().filter(|v| words::troubled(v)) {
            let mut row = instance_row(st, &snap, v, now, "⏎ open");
            row.enter = enter(st, &v.name);
            out.push(row);
        }
    }
    out
}

/// The services opened lately, as rows, at most four.
fn recent_services(stacks: &[&StackView], ask: &Ask<'_>) -> Vec<Row> {
    let mut out = Vec::new();
    for r in ask.recent {
        for st in stacks.iter().filter(|s| s.running && key_of(s) == r.key) {
            let snap = st.words();
            if let Some(v) = snap.find(&r.name) {
                let mut row = recent_row(st, &snap, v, ask.now);
                row.enter = enter(st, &v.name);
                out.push(row);
            }
        }
        if out.len() == 4 {
            break;
        }
    }
    out
}

/// The empty palette: what is broken, what suits where you are, what you did lately, and every stack to tab into
/// (Go `atHand`).
pub(crate) fn at_hand(stacks: &[&StackView], ask: &Ask<'_>) -> Vec<Section> {
    let mut out = Vec::new();
    let mut push = |title: &str, rows: Vec<Row>| {
        if !rows.is_empty() {
            out.push(Section {
                title: title.to_owned(),
                rows,
            });
        }
    };
    push("Needs you", needs(stacks, ask.now));
    push(
        "Suggested",
        super::commands::suggested(stacks, ask.current, ask.inst, ask.now),
    );
    let cmds = super::recent::recent_commands(stacks, ask.cmds);
    if cmds.is_empty() {
        push("Recent", recent_services(stacks, ask));
    } else {
        push("Recent", cmds);
    }
    let mut sts: Vec<Row> = stacks
        .iter()
        .map(|st| {
            let (tone, note) = if st.running {
                ("g", stack_figure(st))
            } else {
                ("dim", crate::overview::Look::STOPPED.word.into())
            };
            Row {
                glyph: "dot",
                tone,
                label: st.name.clone(),
                note,
                key: "tab ›".into(),
                enter: enter_stack(st),
                doing: page(&key_of(st), "home", ""),
                ..Row::default()
            }
        })
        .collect();
    sts.push(open_folder());
    push("Stacks", sts);
    out
}

/// "Open folder…".
fn open_folder() -> Row {
    Row {
        glyph: "⌘O",
        tone: "dim",
        label: "Open folder…".into(),
        note: "add a project to Home".into(),
        key: "open".into(),
        doing: Doing {
            kind: DoKind::Folder,
            ..Doing::default()
        },
        ..Row::default()
    }
}

/// Services and stacks whose names match, best match first (Go `byName`).
pub(crate) fn by_name(stacks: &[&StackView], query: &str, now: SystemTime) -> Vec<Section> {
    let home = words::home();
    let mut rows = Vec::new();
    for st in stacks {
        let tilde = words::tilde(&st.dir, home.as_deref()).into_owned();
        let stack_row = |tone, note: String| Row {
            glyph: "dot",
            tone,
            label: st.name.clone(),
            note: format!("{tilde} · {note}"),
            key: "⏎ open".into(),
            enter: enter_stack(st),
            doing: page(&key_of(st), "home", ""),
            ..Row::default()
        };
        if !st.running {
            // A name under Open opens a stopped stack's page, where Up is; starting it is "up acme", said as a verb.
            rows.push(stack_row("dim", crate::overview::Look::STOPPED.word.into()));
            continue;
        }
        let snap = st.words();
        for r in words::rows(&snap) {
            let mut row = instance_row(st, &snap, r.v, now, "⏎ open");
            row.enter = enter(st, &r.v.name);
            rows.push(row);
        }
        rows.push(stack_row("g", stack_figure(st)));
        if awake_count(st) > 0 {
            rows.push(sleep_all_row(st, "wakes on connect; nothing is lost"));
        }
    }
    rows.push(open_folder());
    let rows = rank(rows, query);
    if rows.is_empty() {
        return Vec::new();
    }
    vec![Section {
        title: "Open".into(),
        rows,
    }]
}

/// The rows whose labels match `query`, best first, ties in order.
pub(crate) fn rank(rows: Vec<Row>, query: &str) -> Vec<Row> {
    let mut hits: Vec<(usize, usize, Row)> = rows
        .into_iter()
        .enumerate()
        .filter_map(|(i, mut r)| {
            let at = words::fuzzy_match(query, &r.label)?;
            let score = words::fuzzy_score(&at);
            r.at = at;
            Some((score, i, r))
        })
        .collect();
    hits.sort_by_key(|h| (h.0, h.1));
    hits.into_iter().map(|h| h.2).collect()
}

/// The commands for where you are: the service on screen, its stack, then the app's.
pub(crate) fn here(stacks: &[&StackView], ask: &Ask<'_>) -> Vec<Row> {
    let mut out = Vec::new();
    if let Some(st) = stacks.iter().find(|s| key_of(s) == ask.current) {
        if !ask.inst.is_empty() {
            let snap = st.words();
            if let Some(v) = snap.find(ask.inst) {
                out.extend(instance_commands(st, Svc::of(st, v), ask.now));
            }
        }
        out.extend(stack_commands(st));
    }
    out.extend(app_commands());
    let known: Vec<String> = out.iter().map(|r| r.label.to_lowercase()).collect();
    out.extend(
        ask.extra
            .iter()
            .filter(|r| !known.contains(&r.label.to_lowercase()))
            .cloned(),
    );
    out
}

/// An instance as a palette line: its dot, name, where it is and how it is, with ⏎ opening its page and its actions
/// on tab (Go `instanceRow`).
pub(crate) fn instance_row(
    st: &StackView,
    snap: &Snapshot,
    v: &Instance,
    now: SystemTime,
    key: &str,
) -> Row {
    let k = key_of(st);
    let look = words::look(v);
    let mut row = Row {
        glyph: "dot",
        tone: dot_of(look),
        label: v.name.clone(),
        key: key.to_owned(),
        doing: page(&k, "instance", &v.name),
        ..Row::default()
    };
    let mut parts = vec![format!("in {}", st.name)];
    if v.kind == Kind::Task {
        (row.glyph, row.tone, row.label) = ("▸", "acc", format!("Run {}", v.name));
        row.doing = verb_on(&k, "task", &v.name, false);
        parts.push(match v.task.as_ref().and_then(|t| t.ran_at) {
            Some(at) => format!(
                "last run {} ago",
                words::ago(now.duration_since(at).unwrap_or_default())
            ),
            None => "never run".into(),
        });
    } else {
        let mut state = look.word().to_owned();
        if words::awake_now(v) && v.ram > 0 {
            state = format!(
                "{state}, {}",
                words::mb(i64::try_from(v.ram).unwrap_or(i64::MAX))
            );
        }
        parts.push(state);
        let b = words::because(v, snap, now);
        if !b.is_empty() {
            parts.push(b);
        }
    }
    row.note = parts.join(" · ");
    row.acts = acts_of(st, Svc::of(st, v), &k);
    if v.kind != Kind::Task && words::troubled(v) {
        // Needs you's first fix for a service in trouble (`attention::trouble`): restart it.
        row.first = Some(Act {
            label: "restart",
            doing: verb_on(&k, "restart", &v.name, asks(v, "restart")),
        });
    }
    row
}

/// A recent service: the same line, its actions named after the stack (Go `recentRow`).
fn recent_row(st: &StackView, snap: &Snapshot, v: &Instance, now: SystemTime) -> Row {
    let mut row = instance_row(st, snap, v, now, "⏎");
    let https = Svc::of(st, v).https;
    if v.kind == Kind::Task {
    } else if !https.is_empty() {
        row.note = format!("in {} · {https}", st.name);
    } else {
        let names: Vec<&str> = row.acts.iter().take(3).map(|a| a.label).collect();
        if !names.is_empty() {
            row.note = format!("in {} · {}", st.name, names.join(", "));
        }
    }
    row
}

/// What a row can do, the most likely first (Go `actsOf`).
fn acts_of(_st: &StackView, s: Svc<'_>, key: &str) -> Vec<Act> {
    let (v, n) = (s.v, s.v.name.as_str());
    let logs = Act {
        label: "logs",
        doing: page(key, "logs", n),
    };
    if v.kind == Kind::Task {
        return vec![
            Act {
                label: "run",
                doing: verb_on(key, "task", n, false),
            },
            logs,
        ];
    }
    let mut acts = Vec::new();
    let line = first_of(&[s.https, &v.url, &v.endpoint]);
    if !line.is_empty() {
        acts.push(Act {
            label: "copy URL",
            doing: with_text(DoKind::Copy, key, line, n),
        });
    }
    let url = first_of(&[s.https, web_url(v)]);
    if !url.is_empty() {
        acts.push(Act {
            label: "open",
            doing: with_text(DoKind::Open, key, url, n),
        });
    }
    if words::awake_now(v) {
        acts.push(Act {
            label: "sleep",
            doing: verb_on(key, "sleep", n, asks(v, "sleep")),
        });
        acts.push(Act {
            label: "restart",
            doing: verb_on(key, "restart", n, asks(v, "restart")),
        });
    } else if !v.disabled {
        acts.push(Act {
            label: "wake",
            doing: verb_on(key, "wake", n, false),
        });
    }
    acts.push(logs);
    acts
}

/// "Sleep everything in shop".
pub(crate) fn sleep_all_row(st: &StackView, note: &str) -> Row {
    Row {
        glyph: "↓",
        tone: "dim",
        label: format!("Sleep everything in {}", st.name),
        note: note.to_owned(),
        key: "sleep all".into(),
        cli: "doze sleep".into(),
        doing: verb_on(&key_of(st), "sleep", "", false),
        ..Row::default()
    }
}

/// How many are up, tasks aside.
pub(crate) fn awake_count(st: &StackView) -> usize {
    st.words()
        .instances
        .iter()
        .filter(|v| v.kind != Kind::Task && words::awake_now(v))
        .count()
}

/// "5 awake", or "stopped". Awake as the strip, the Library and the menu bar count it: up and held, dozing apart (Go
/// counted the dozing too, so ⌘K said 6 where the stack's page said 5).
pub(crate) fn stack_figure(st: &StackView) -> String {
    if st.running {
        format!(
            "{} {}",
            words::tally(&st.words().instances).awake,
            words::Look::Awake.word()
        )
    } else {
        crate::overview::Look::STOPPED.word.into()
    }
}
