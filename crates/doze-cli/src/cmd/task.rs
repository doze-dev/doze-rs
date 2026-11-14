//! `doze task [name]` (Go cmd/doze/task.go): runs a task now, whatever its state, after waking what it needs, and
//! returns when it is over; with no name, lists the tasks and how each last ran. How a task ran is kept in the
//! project's `.doze/state.json` (Go's format), so the list reads it from there.

use crate::cli::args::Task;
use crate::cmd::{dry, report};
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::stack::{self, Stack};
use crate::view::View;
use doze_control::{Snapshot, Timestamp, Verb};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime};

pub(crate) fn run(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    a: &Task,
    notices: &mut crate::notice::Notices,
) -> Outcome {
    let stack = Stack::find(ctx, mode)?;
    let Some(name) = &a.name else {
        return list(ctx, &stack, notices);
    };
    let conn = stack.require(mode.timeout, "doze task")?;
    let snap = stack::snapshot(&conn)?;
    check(&snap, name)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::task(&snap, name))));
    }
    let mut r = report::Report::new(mode);
    if !mode.json && !mode.quiet {
        let _ = writeln!(ctx.err, "{} {name} running…", mode.err().dim("→"));
    }
    let start = Instant::now();
    if let Err(e) = conn.run_verb(Verb::Task, name) {
        let d = r.fail(name, &e);
        return Ok(Done {
            view: View::Verbs(r),
            problem: Some(d),
        });
    }
    let took =
        Duration::from_millis(u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX));
    r.ok(ctx.out, name, "done", "done", took);
    Ok(Done::ok(View::Verbs(r)))
}

/// `name` is a task of the stack; exit 4 saying what it is instead, or which tasks there are.
fn check(snap: &Snapshot, name: &str) -> Result<(), Diagnostic> {
    let tasks: Vec<&str> = stack::declared(snap)
        .filter(|v| v.kind == "task")
        .map(|v| v.name.as_str())
        .collect();
    if tasks.contains(&name) {
        return Ok(());
    }
    let d = if stack::declared(snap).any(|v| v.name == name) {
        Diagnostic::new(ids::NO_SUCH_SERVICE, format!("{name} is not a task"))
            .fix(format!("doze wake {name}"))
    } else if tasks.is_empty() {
        Diagnostic::new(
            ids::NO_SUCH_SERVICE,
            format!("no task called {name:?}: this stack declares no tasks"),
        )
    } else {
        Diagnostic::new(ids::NO_SUCH_SERVICE, format!("no task called {name:?}"))
            .note(format!("the tasks are {}", tasks.join(", ")))
            .fix("doze task")
    };
    Err(d)
}

/// How a task last ran (Go `state.TaskRecord`), as `.doze/state.json` has it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Record {
    result: String,
    exit_code: i64,
    took_ms: i64,
    ran_at: Timestamp,
    why: String,
    inputs: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct State {
    tasks: BTreeMap<String, Record>,
}

/// One task in the list.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Listed {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last: Option<Record>,
}

/// The tasks and how each last ran.
#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    tasks: Vec<Listed>,
    #[serde(skip)]
    now: Option<SystemTime>,
}

fn list(ctx: &Ctx<'_>, stack: &Stack, notices: &mut crate::notice::Notices) -> Outcome {
    let path = stack.project.dir.join(".doze").join("state.json");
    let state: State = match std::fs::read(&path) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| {
            Diagnostic::new(
                ids::DAEMON_UNREADABLE,
                format!("{} cannot be read: {e}", path.display()),
            )
            .fix("doze sync rewrites it")
        })?,
        Err(_) => State::default(),
    };
    let declared: Option<Vec<String>> = match stack.read()? {
        Some(c) => Some(
            stack::declared(&stack::snapshot(&c)?)
                .filter(|v| v.kind == "task")
                .map(|v| v.name.clone())
                .collect(),
        ),
        None => None,
    };
    let names: Vec<String> = match declared {
        Some(mut d) => {
            d.sort();
            d
        }
        None => {
            notices.add("doze is not running: only the tasks that have run are listed");
            state.tasks.keys().cloned().collect()
        }
    };
    let tasks = names
        .into_iter()
        .map(|name| Listed {
            last: state.tasks.get(&name).cloned(),
            name,
        })
        .collect();
    Ok(Done::ok(View::Tasks(Listing {
        tasks,
        now: Some(ctx.now),
    })))
}

impl Listing {
    pub(crate) fn human(&self, p: Paint) -> String {
        if self.tasks.is_empty() {
            return format!("{}\n", p.dim("no tasks declared"));
        }
        let now = self.now.unwrap_or(SystemTime::UNIX_EPOCH);
        let mut out = String::new();
        for t in &self.tasks {
            let said = match &t.last {
                None => p.dim("never ran").into_owned(),
                Some(r) => {
                    let ago = since(r.ran_at.to_system_time(), now);
                    if r.result == "done" {
                        let took = Duration::from_millis(u64::try_from(r.took_ms).unwrap_or(0));
                        let tail = format!(
                            "{ago} ago in {} — {}",
                            doze_control::format_go_duration(took),
                            r.why
                        );
                        format!("{} {}", p.ok("done"), p.dim(&tail))
                    } else {
                        let tail = format!("{ago} ago, exit {} — {}", r.exit_code, r.why);
                        format!("{} {}", p.fail("failed"), p.dim(&tail))
                    }
                }
            };
            out.push_str(&format!("{:<14} {said}\n", t.name));
        }
        out
    }
}

/// How long ago, as the task list says it (Go `since`): 42s, 5m, 3h, 2d.
fn since(t: Option<SystemTime>, now: SystemTime) -> String {
    let d = t
        .and_then(|t| now.duration_since(t).ok())
        .unwrap_or_default();
    let secs = (d.as_millis() + 500) / 1000;
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 48 * 3600 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}
