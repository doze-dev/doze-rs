//! What is wrong with a stack now (Go `StackItems`): its files gone, a failed action, its daemon ended unasked or not
//! answering, its instances in trouble, its doze.hcl not loading or changed.

use std::time::{Duration, SystemTime};

use doze_display as words;

use super::trouble::instance_item;
use super::{Fix, FixKind, Item, Place};
use crate::conn::Conn;
use crate::overview::{ConfigChanged, ConfigError};
use crate::ports::Holder;
use crate::view::StackView;

/// A stack action that failed (Go `view.StackFailure`): what was asked, the CLI's words, and the config error when
/// the file was the reason.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StackFailure {
    /// "Restarting shop failed".
    pub what: String,
    pub problem: String,
    pub config: Option<ConfigError>,
    /// The CLI's last lines.
    pub lines: Vec<String>,
}

/// What the app knows about a stack beyond the hub (Go `view.StackFacts`).
#[derive(Default)]
#[allow(missing_debug_implementations)] // WHY: `held` is a lookup function, which has no Debug
pub struct StackFacts<'a> {
    pub failed: Option<&'a StackFailure>,
    pub config: Option<&'a ConfigError>,
    pub changed: Option<&'a ConfigChanged>,
    /// The saved file's time: what "changed" says for Not now (the words, "4m ago", move on their own).
    pub changed_at: Option<SystemTime>,
    /// "the folder is gone", "doze.hcl is gone", or empty.
    pub missing: &'a str,
    /// The process on a port a crash named, when known.
    pub held: Option<&'a dyn Fn(u16) -> Option<Holder>>,
}

pub(crate) fn time_print(t: Option<SystemTime>) -> String {
    t.and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or_else(String::new, |d| d.as_nanos().to_string())
}

fn ago(now: SystemTime, then: Option<SystemTime>) -> String {
    words::ago(then.map_or_else(Duration::default, |t| {
        now.duration_since(t).unwrap_or_default()
    }))
}

/// The base name of a folder.
fn base(dir: &str) -> &str {
    dir.trim_end_matches('/').rsplit('/').next().unwrap_or(dir)
}

/// A stack's items: its instances in trouble, then the stack itself (Go `StackItems`).
#[must_use]
pub fn stack_items(st: &StackView, f: &StackFacts<'_>, now: SystemTime) -> Vec<Item> {
    let name = if st.name.is_empty() {
        base(&st.dir).to_owned()
    } else {
        st.name.clone()
    };
    let key = st.key.dir().to_string_lossy().into_owned();
    let base_item = |id: &str, kind| Item {
        id: format!("{id}:{key}"),
        kind,
        key: key.clone(),
        stack: name.clone(),
        show: Some(Place {
            key: key.clone(),
            page: "home",
            ..Place::default()
        }),
        ..Item::default()
    };
    if !f.missing.is_empty() {
        // Nothing else about a stack whose files are gone is worth saying.
        let what = if f.missing == "doze.hcl is gone" {
            "doze.hcl is gone"
        } else {
            "folder is gone"
        };
        let home = words::home();
        return vec![Item {
            print: f.missing.to_owned(),
            title: format!("{name}'s {what}"),
            detail: format!(
                "{} was moved or deleted",
                words::tilde(&st.dir, home.as_deref())
            ),
            fixes: vec![Fix::new("Remove…", FixKind::Stack).verb("remove").primary()],
            show: None,
            ..base_item("missing", "missing")
        }];
    }
    let mut out = Vec::new();
    if let Some(fail) = f.failed {
        out.push(failed_item(base_item("failed", "failed"), fail, st.running));
    }
    out.extend(ended_item(base_item("ended", "ended"), st, &name, now));
    out.extend(daemon_item(base_item("daemon", "daemon"), st, &name, now));
    if st.running && st.link.conn == Conn::Live {
        let snap = st.words();
        out.extend(
            snap.instances
                .iter()
                .filter_map(|v| instance_item(st, &name, &key, v, f, now)),
        );
    }
    out.extend(config_item(
        base_item("config", "config"),
        base_item("changed", "changed"),
        st,
        &name,
        f,
    ));
    out
}

/// Up was the last thing asked of a stack, and nothing runs: its daemon ended without doze down. The 30 s are a
/// start's own moment before the daemon is seen.
fn ended_item(base: Item, st: &StackView, name: &str, now: SystemTime) -> Option<Item> {
    let up = st.last_up?;
    if st.running
        || st.last_down.is_some_and(|d| up <= d)
        || now.duration_since(up).unwrap_or_default() <= Duration::from_secs(30)
    {
        return None;
    }
    {
        Some(Item {
            print: time_print(Some(up)),
            red: true,
            title: format!("{name} stopped without being asked"),
            detail: format!(
                "it was up {} ago and nothing ran doze down; its daemon ended (a crash, a kill, or a restart of this Mac)",
                ago(now, Some(up))
            ),
            since: Some(up),
            fixes: vec![
                Fix::new("Up as last time", FixKind::Stack)
                    .verb("up-last")
                    .primary(),
            ],
            ..base
        })
    }
}

/// A running stack whose daemon stopped answering.
fn daemon_item(base: Item, st: &StackView, name: &str, now: SystemTime) -> Option<Item> {
    (st.running && st.link.conn == Conn::Down).then(|| Item {
        print: "down".into(),
        red: true,
        title: format!("{name}'s daemon is not answering"),
        detail: format!(
            "what it shows is from {} ago; doze keeps trying",
            ago(now, st.link.last_frame)
        ),
        fixes: vec![
            Fix::new(format!("Restart {name}"), FixKind::Stack)
                .verb("restart")
                .primary(),
        ],
        ..base
    })
}

/// A failed stack action.
fn failed_item(base: Item, fail: &StackFailure, running: bool) -> Item {
    let mut it = Item {
        red: true,
        title: fail.what.clone(),
        detail: fail.problem.clone(),
        ..base
    };
    if let Some(c) = &fail.config {
        let file = base_of_path(&c.file);
        if c.line > 0 {
            it.detail = format!("{file} line {}: {}", c.line, c.summary);
        }
        it.fixes.push(Fix {
            path: c.file.clone(),
            ..Fix::new(format!("Edit {file}"), FixKind::Edit).primary()
        });
    }
    if it.detail.is_empty()
        && let Some(last) = fail.lines.last()
    {
        (it.detail, it.mono) = (last.clone(), true);
    }
    it.print = format!("{}|{}", it.title, it.detail);
    if !running {
        let mut up = Fix::new("Try again", FixKind::Stack).verb("up-last");
        up.primary = fail.config.is_none();
        it.fixes.push(up);
    }
    it.fixes.push(Fix::new("Dismiss", FixKind::Dismiss));
    it
}

fn base_of_path(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

/// doze.hcl does not load, or changed since the stack read it.
fn config_item(
    base: Item,
    changed: Item,
    st: &StackView,
    name: &str,
    f: &StackFacts<'_>,
) -> Option<Item> {
    if let Some(c) = f.config {
        let mut it = Item {
            title: format!("{name}'s doze.hcl does not load"),
            detail: c.summary.clone(),
            fixes: vec![Fix {
                path: c.file.clone(),
                ..Fix::new("Edit doze.hcl", FixKind::Edit).primary()
            }],
            ..base
        };
        if c.line > 0 {
            it.detail = format!("line {} · {}", c.line, c.summary);
        }
        it.print.clone_from(&it.detail);
        if st.running {
            it.detail.push_str(" · the file it started with runs on");
        }
        return Some(it);
    }
    let ch = f.changed.filter(|_| st.running)?;
    Some(Item {
        print: time_print(f.changed_at),
        title: "doze.hcl changed".into(),
        detail: format!("{}; restart {name} to run it", ch.text),
        fixes: vec![
            Fix::new(format!("Restart {name}"), FixKind::Stack)
                .verb("restart")
                .primary(),
        ],
        ..changed
    })
}
