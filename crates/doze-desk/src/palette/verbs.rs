//! A verb then a name: what the verb applies to, "Did you mean" under it, and the closest name when nothing matched
//! (Go `view/palette.go`: `forVerb`, `verbRow`, `nothing`, `editDistance`).

use std::time::SystemTime;

use doze_display::{self as words, Instance, Kind};

use super::commands::{page, stack_do, verb_on, with_text};
use super::find::{awake_count, instance_row, sleep_all_row};
use super::{DoKind, Doing, Nothing, Row, Section, Svc, asks, first_of, key_of, web_url};
use crate::view::StackView;

/// The words a person types first.
pub const VERBS: [&str; 14] = [
    "sleep", "wake", "restart", "pin", "unpin", "reset", "off", "on", "logs", "copy", "open",
    "run", "up", "down",
];

/// Words people type for a verb doze names differently.
pub const ALIASES: [(&str, &str); 8] = [
    ("start", "wake"),
    ("stop", "sleep"),
    ("kill", "restart"),
    ("reboot", "restart"),
    ("url", "copy"),
    ("browse", "open"),
    ("log", "logs"),
    ("tail", "logs"),
];

/// The verb a word means.
pub(crate) fn alias(w: &str) -> &str {
    ALIASES.iter().find(|(a, _)| *a == w).map_or(w, |(_, v)| v)
}

/// A stack's row under `up`, `down` or `restart`, if the verb names stacks.
fn stack_row(st: &StackView, verb: &str) -> Option<Row> {
    let key = key_of(st);
    let n = st.name.as_str();
    let dim = |glyph, label: String, note: String, key: String, cli: &str, doing| Row {
        glyph,
        tone: "dim",
        label,
        note,
        key,
        cli: cli.to_owned(),
        doing,
        ..Row::default()
    };
    let open = |tone, label: String, note: &str| Row {
        glyph: "dot",
        tone,
        label,
        note: note.to_owned(),
        key: "⏎ open".into(),
        doing: page(&key, "home", ""),
        ..Row::default()
    };
    match (verb, st.running) {
        ("up", false) => {
            let set = if st.last_set.is_empty() {
                "everything".to_owned()
            } else {
                st.last_set.join(", ")
            };
            Some(dim(
                "↑",
                format!("Up {n}"),
                format!("stopped · as last time: {set}"),
                format!("up {n}"),
                "doze up --last",
                stack_do(&key, "up-last"),
            ))
        }
        // Said, not missing: "up shop" on a running shop.
        ("up", true) => Some(open(
            "g",
            format!("{n} is up"),
            "already running; ⏎ opens it",
        )),
        ("down", true) => Some(dim(
            "↓",
            format!("Down {n}"),
            "stops everything; the set is remembered".into(),
            format!("down {n}"),
            "doze down",
            stack_do(&key, "down"),
        )),
        ("down", false) => Some(open(
            "dim",
            format!("{n} is stopped"),
            "already down; ⏎ opens it",
        )),
        // A stack's name after restart restarts the stack: down, then up as last time.
        ("restart", true) => Some(dim(
            "↻",
            format!("Restart {n}"),
            "the whole stack: down, then up as last time".into(),
            format!("restart {n}"),
            "doze down && doze up --last",
            stack_do(&key, "restart"),
        )),
        _ => None,
    }
}

/// What a verb can apply to, narrowed by the name after it, then "Did you mean" (Go `forVerb`).
pub(crate) fn for_verb(
    stacks: &[&StackView],
    verb: &str,
    name: &str,
    now: SystemTime,
) -> Vec<Section> {
    let mut rows: Vec<(String, Row)> = Vec::new();
    for st in stacks {
        if let Some(r) = stack_row(st, verb) {
            rows.push((st.name.clone(), r));
        }
        if matches!(verb, "up" | "down") || !st.running {
            continue;
        }
        let snap = st.words();
        for r in words::rows(&snap) {
            if let Some(row) = verb_row(st, &snap, r.v, verb, now) {
                rows.push((r.v.name.clone(), row));
            }
        }
    }
    let rows: Vec<Row> = if name.is_empty() {
        rows.into_iter().map(|(_, r)| r).collect()
    } else {
        rank_by_name(rows, name)
    };
    if rows.is_empty() {
        return Vec::new();
    }
    let title: String = verb
        .chars()
        .take(1)
        .flat_map(char::to_uppercase)
        .chain(verb.chars().skip(1))
        .collect();
    let mut out = vec![Section { title, rows }];
    let meant = did_you_mean(stacks, verb);
    if !meant.is_empty() {
        out.push(Section {
            title: "Did you mean".into(),
            rows: meant,
        });
    }
    out
}

/// Ranks rows by the name each was found under ("Down shop" is found by "shop"), best first, ties in order. The
/// marks fall on the name where it sits in the label (Go marked the name's positions from the label's start).
fn rank_by_name(rows: Vec<(String, Row)>, name: &str) -> Vec<Row> {
    let mut hits: Vec<(usize, usize, Row)> = rows
        .into_iter()
        .enumerate()
        .filter_map(|(i, (n, mut r))| {
            let at = words::fuzzy_match(name, &n)?;
            let score = words::fuzzy_score(&at);
            let shift = r
                .label
                .find(n.as_str())
                .map_or(0, |b| r.label.get(..b).map_or(0, |p| p.chars().count()));
            r.at = at.into_iter().map(|a| a + shift).collect();
            Some((score, i, r))
        })
        .collect();
    hits.sort_by_key(|h| (h.0, h.1));
    hits.into_iter().map(|h| h.2).collect()
}

/// Did you mean: the whole-stack form of sleep and wake.
fn did_you_mean(stacks: &[&StackView], verb: &str) -> Vec<Row> {
    let mut out = Vec::new();
    for st in stacks.iter().filter(|s| s.running) {
        match verb {
            "sleep" => {
                // What it puts to sleep, in the strip's words: the awake and the dozing apart.
                if awake_count(st) > 0 {
                    out.push(sleep_all_row(st, &up_figure(st)));
                }
            }
            "wake" => out.push(Row {
                glyph: "↑",
                tone: "dim",
                label: format!("Wake everything in {}", st.name),
                note: "every service, with what it needs".into(),
                key: "wake all".into(),
                cli: "doze wake".into(),
                doing: verb_on(&key_of(st), "wake", "", false),
                ..Row::default()
            }),
            _ => {}
        }
    }
    out
}

/// One instance under a verb, when the verb applies to it (Go `verbRow`).
pub(crate) fn verb_row(
    st: &StackView,
    snap: &doze_display::Snapshot,
    v: &Instance,
    verb: &str,
    now: SystemTime,
) -> Option<Row> {
    let (doing, key, cli) = verb_doing(&key_of(st), Svc::of(st, v), verb)?;
    let mut row = instance_row(st, snap, v, now, "");
    row.bad = verb == "reset";
    (row.doing, row.key, row.cli) = (doing, key, cli);
    Some(row)
}

/// What `verb` does to the service, the key's words and its command line; `None` when it does not apply.
#[rustfmt::skip]
fn verb_doing(key: &str, s: Svc<'_>, verb: &str) -> Option<(Doing, String, String)> {
    let (v, n) = (s.v, s.v.name.as_str());
    let (task, awake) = (v.kind == Kind::Task, words::awake_now(v));
    let plain = |verb: &str, said: &str, cli: String| Some((verb_on(key, verb, n, false), said.to_owned(), cli));
    let asked = |verb: &str, said: &str| {
        let ask = asks(v, verb);
        Some((verb_on(key, verb, n, ask), if ask { "⏎ asks first" } else { said }.to_owned(), format!("doze {verb} {n}")))
    };
    match verb {
        "sleep" if !task && awake => asked("sleep", "⏎ sleep now"),
        "restart" if !task => asked("restart", "⏎ restart"),
        "off" if !task && !v.disabled => asked("off", "⏎ off"),
        "wake" if !task && !awake => plain("wake", "⏎ wake now", format!("doze wake {n}")),
        "pin" if !task && !v.keep_awake => plain("pin", "⏎ pin awake", format!("doze pin {n}")),
        "unpin" if !task && v.keep_awake => plain("pin", "⏎ unpin", format!("doze pin --off {n}")),
        "on" if !task && v.disabled => plain("on", "⏎ on", format!("doze on {n}")),
        "run" if task => plain("task", "⏎ run now", format!("doze task {n}")),
        "reset" if v.kind == Kind::Module => Some((verb_on(key, "reset", n, true), "⏎ asks first · wipes its data".into(), format!("doze reset {n}"))),
        "logs" => Some((page(key, "logs", n), "⏎ logs".into(), format!("doze logs {n}"))),
        "copy" if !task => {
            let line = first_of(&[s.https, &v.url, &v.endpoint]);
            (!line.is_empty()).then(|| (with_text(DoKind::Copy, key, line, n), format!("⏎ copy {line}"), format!("doze url {n}")))
        }
        "open" => {
            let url = first_of(&[s.https, web_url(v)]);
            (!url.is_empty()).then(|| (with_text(DoKind::Open, key, url, n), format!("⏎ open {url}"), format!("open {url}")))
        }
        _ => None,
    }
}

/// The no-match line: where it looked, and the closest name by edit distance over services (and their engines) and
/// stacks (Go `nothing`).
pub(crate) fn nothing(stacks: &[&StackView], verb: &str, name: &str, now: SystemTime) -> Nothing {
    let names: Vec<&str> = stacks.iter().map(|s| s.name.as_str()).collect();
    let mut out = Nothing {
        name: name.to_owned(),
        r#where: or_list(&names),
        ..Nothing::default()
    };
    let lower = name.to_lowercase();
    let mut best: Option<(usize, &StackView, Instance)> = None;
    for st in stacks {
        for v in st.words().instances {
            for cand in [v.name.as_str(), v.engine.as_str()] {
                if cand.is_empty() {
                    continue;
                }
                let d = edit_distance(&lower, &cand.to_lowercase());
                if best.as_ref().is_none_or(|b| d < b.0) {
                    best = Some((d, st, v.clone()));
                }
            }
        }
    }
    let Some((d, st, v)) = best else { return out };
    if d > 3.max(name.chars().count() / 2) {
        return out;
    }
    out.closest.clone_from(&v.name);
    out.why = if !v.engine.is_empty() && !v.engine.eq_ignore_ascii_case(&v.name) {
        format!("{}, in {}", v.engine, st.name)
    } else {
        format!("in {}", st.name)
    };
    if !verb.is_empty() && verb_row(st, &st.words(), &v, verb, now).is_some() {
        out.key = format!("⏎ use {}", v.name);
        out.fill = format!("{verb} {}", v.name);
        return out;
    }
    out.key = format!("⏎ open {}", v.name);
    out.doing = Some(page(&key_of(st), "instance", &v.name));
    out
}

/// "shop, data or acme".
fn or_list(names: &[&str]) -> String {
    match names {
        [] => "any stack".into(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// The Levenshtein distance between two strings, by characters.
#[must_use]
pub fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = Vec::with_capacity(b.len() + 1);
        cur.push(i + 1);
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            let del = prev.get(j + 1).copied().unwrap_or(0).saturating_add(1);
            let ins = cur.last().copied().unwrap_or(0).saturating_add(1);
            let sub = prev.get(j).copied().unwrap_or(0).saturating_add(cost);
            cur.push(del.min(ins).min(sub));
        }
        prev = cur;
    }
    prev.last().copied().unwrap_or(0)
}

/// "4 awake · 1 dozing": what is up, in the strip's words; the dozing part only when there is one.
fn up_figure(st: &StackView) -> String {
    let t = words::tally(&st.words().instances);
    let mut out = format!("{} {}", t.awake, words::Look::Awake.word());
    if t.dozing > 0 {
        out.push_str(&format!(" · {} {}", t.dozing, words::Look::Dozing.word()));
    }
    out
}

/// A running stack's dot: red when a service in it is in trouble, as its tab, the Library and the bell have it.
pub(super) fn stack_tone(st: &StackView) -> &'static str {
    let troubled = st
        .words()
        .instances
        .iter()
        .any(|v| v.kind != Kind::Task && words::troubled(v));
    if troubled { "r" } else { "g" }
}
