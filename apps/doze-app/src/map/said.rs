//! A small map's detail card in Go's words (Go `flow.card`): the head ("Go · 84 MB · awake"), then what it needs
//! ("api, auth · and db, cache below"), who needs it ("web · it keeps it awake"), and what runs before it ("migrate ✓
//! · seed not run", toned, with how long ago). The calls, callers and uses are the layout's; what each instance is
//! comes from the snapshot, kept on the page ([`Said`]).

use std::collections::HashMap;
use std::time::SystemTime;

use doze_display::{self as words, Holder, Kind, Outcome, State};
use doze_map::{Card, CardId, Layout, Role};

/// What the detail card says of one instance, beyond the layout.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Said {
    /// "Go · 84 MB · awake".
    pub(crate) head: String,
    /// Its dependants that hold it awake.
    pub(crate) held_by: Vec<String>,
    /// The tasks it needs, each as the row says it: name, how it stands, when it last ran.
    pub(crate) tasks: Vec<(String, TaskIs, Option<SystemTime>)>,
    /// A task: its own last run, "migrate ✓ 16m ago".
    pub(crate) last_run: Option<(String, &'static str)>,
    /// A task: the apps it runs before.
    pub(crate) task: bool,
}

/// How a setup task stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskIs {
    Running,
    NotRun,
    Failed,
    Done,
}

/// One row: its key, its words, their tone (`ok`, `cool`, `dim`, `bad`, or plain) and a dim tail.
pub(crate) type Line = (&'static str, String, &'static str, String);

fn lang_name(run: &str) -> &'static str {
    match doze_map::Lang::of(run) {
        Some(doze_map::Lang::Go) => "Go",
        Some(doze_map::Lang::Py) => "Python",
        Some(doze_map::Lang::Ts) => "TypeScript",
        Some(doze_map::Lang::Js) => "JavaScript",
        _ => "",
    }
}

fn task_is(t: &words::Instance) -> TaskIs {
    match (&t.state, &t.task) {
        (State::Booting, _) => TaskIs::Running,
        (_, None) => TaskIs::NotRun,
        (_, Some(r)) if r.outcome == Outcome::Failed => TaskIs::Failed,
        _ => TaskIs::Done,
    }
}

/// What the detail card says of every instance on the stack.
pub(crate) fn said_of(snap: &words::Snapshot, now: SystemTime) -> HashMap<String, Said> {
    let mut out = HashMap::new();
    for v in &snap.instances {
        let mut head: Vec<String> = Vec::new();
        let lang = lang_name(&v.run);
        if !lang.is_empty() {
            head.push(lang.to_owned());
        }
        if v.kind == Kind::Task {
            head.push("task".to_owned());
        }
        if words::awake_now(v) && v.ram > 0 {
            head.push(words::mb(i64::try_from(v.ram).unwrap_or(i64::MAX)));
        }
        head.push(words::look(v).word().to_owned());
        let tasks = v
            .needs
            .iter()
            .filter_map(|n| snap.find(n))
            .filter(|t| t.kind == Kind::Task)
            .map(|t| {
                (
                    t.name.clone(),
                    task_is(t),
                    t.task.as_ref().and_then(|r| r.ran_at),
                )
            })
            .collect();
        let last_run = (v.kind == Kind::Task).then(|| {
            let ago = |at: Option<SystemTime>| {
                at.and_then(|a| now.duration_since(a).ok())
                    .map(words::ago)
                    .unwrap_or_default()
            };
            let at = v.task.as_ref().and_then(|r| r.ran_at);
            match task_is(v) {
                TaskIs::Running => (format!("{} running", v.name), "cool"),
                TaskIs::NotRun => (format!("{} not run yet", v.name), "bad"),
                TaskIs::Failed => (format!("{} failed {} ago", v.name, ago(at)), "bad"),
                TaskIs::Done => (format!("{} ✓ {} ago", v.name, ago(at)), "bad"),
            }
        });
        let held_by = v
            .holders
            .iter()
            .filter_map(|h| match h {
                Holder::Dependant(n) => Some(n.clone()),
                _ => None,
            })
            .collect();
        out.insert(
            v.name.clone(),
            Said {
                head: head.join(" · "),
                held_by,
                tasks,
                last_run,
                task: v.kind == Kind::Task,
            },
        );
    }
    out
}

fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The rows of a small map's detail card for `c` (Go `card`, and `share` for a band card).
pub(crate) fn lines(l: &Layout, c: &Card, s: &Said, now: SystemTime) -> Vec<Line> {
    let names = |ids: &[CardId], only_apps: bool| -> Vec<String> {
        ids.iter()
            .filter_map(|i| l.card(*i))
            .filter(|c| !only_apps || c.kind == doze_map::Kind::App)
            .map(|c| c.name.clone())
            .collect()
    };
    if c.role == Role::Shared {
        let users = names(&c.uses, false);
        let v = if users.is_empty() {
            "nothing uses it".to_owned()
        } else {
            users.join(", ")
        };
        return vec![("used by", v, "", String::new())];
    }
    if s.task {
        let before = and_list(&names(&c.callers, false));
        let (run, tone) = s.last_run.clone().unwrap_or_default();
        return vec![
            ("before", before, "", String::new()),
            ("last run", run, tone, String::new()),
        ];
    }
    let calls = names(&c.calls, true);
    let uses = names(&c.uses, false);
    let mut needs = calls.join(", ");
    if !uses.is_empty() {
        if !needs.is_empty() {
            needs.push_str(" · and ");
        }
        needs.push_str(&format!("{} below", uses.join(", ")));
    }
    if needs.is_empty() {
        needs = "nothing".into();
    }
    let callers = names(&c.callers, false);
    let by = if callers.is_empty() {
        "nobody calls it".to_owned()
    } else {
        let holding: Vec<String> = callers
            .iter()
            .filter(|c| s.held_by.contains(c))
            .cloned()
            .collect();
        let mut by = callers.join(", ");
        if holding.len() == callers.len() && callers.len() > 1 {
            by.push_str(" · they keep it awake");
        } else if holding.len() == 1 && callers.len() == 1 {
            by.push_str(" · it keeps it awake");
        } else if !holding.is_empty() {
            by.push_str(&format!(" · {} keep it awake", and_list(&holding)));
        }
        by
    };
    vec![
        ("needs", needs, "", String::new()),
        ("needed by", by, "", String::new()),
        before_it(s, now),
    ]
}

/// "before it": the setup tasks it needs, toned by the worst of them, and how long ago the last one ran.
fn before_it(s: &Said, now: SystemTime) -> Line {
    if s.tasks.is_empty() {
        return ("before it", "nothing".into(), "dim", String::new());
    }
    let mut tone = "ok";
    let mut last: Option<SystemTime> = None;
    let mut said = Vec::new();
    for (name, is, at) in &s.tasks {
        match is {
            TaskIs::Running => {
                said.push(format!("{name} running"));
                if tone == "ok" {
                    tone = "cool";
                }
            }
            TaskIs::NotRun => {
                said.push(format!("{name} not run"));
                if tone == "ok" {
                    tone = "dim";
                }
            }
            TaskIs::Failed => {
                said.push(format!("{name} ✗"));
                tone = "bad";
            }
            TaskIs::Done => said.push(format!("{name} ✓")),
        }
        if let Some(a) = at
            && last.is_none_or(|l| *a > l)
        {
            last = Some(*a);
        }
    }
    let dim = last
        .and_then(|l| now.duration_since(l).ok())
        .map(|d| format!("{} ago", words::ago(d)))
        .unwrap_or_default();
    ("before it", said.join(" · "), tone, dim)
}
