//! `doze pin <name…>` (`keepawake`): keeps services awake past their idle time; `--off` lets them sleep again,
//! `--toggle` flips each (Go `runPin`). It says what it wants, so running it twice changes nothing the second time:
//! the daemon's verb is a toggle, so it is sent only to the ones that differ, and each result says whether it changed.

use crate::cli::args::Pin;
use crate::cmd::dry;
use crate::cmd::report::Report;
use crate::ctx::Ctx;
use crate::output::{Done, Mode, Outcome};
use crate::stack::{self, Stack};
use crate::view::View;
use doze_control::{Connection, Verb};
use std::collections::HashMap;

/// What the run asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Want {
    On,
    Off,
    Toggle,
}

impl Want {
    fn of(a: &Pin) -> Want {
        match (a.off, a.toggle) {
            (true, _) => Want::Off,
            (_, true) => Want::Toggle,
            _ => Want::On,
        }
    }

    /// Whether a service pinned `now` must be flipped.
    pub(crate) fn flips(self, now: bool) -> bool {
        match self {
            Want::On => !now,
            Want::Off => now,
            Want::Toggle => true,
        }
    }
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Pin) -> Outcome {
    let want = Want::of(a);
    let stack = Stack::find(ctx, mode)?;
    let Some(conn) = stack.open(mode.timeout)? else {
        if want == Want::Off {
            // Nothing runs, so nothing is pinned.
            let mut r = Report::new(mode);
            for n in &a.names {
                r.pin(ctx.out, n, false, false);
            }
            return Ok(Done::ok(View::Verbs(r)));
        }
        return Err(stack::down("doze pin"));
    };
    let snap = stack::snapshot(&conn)?;
    stack::check_names(&snap, &a.names)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::pin(&snap, &a.names, want))));
    }
    let before = pinned(&conn)?;
    let mut r = Report::new(mode);
    let flip: Vec<&String> = a
        .names
        .iter()
        .filter(|n| want.flips(before.get(*n).copied().unwrap_or(false)))
        .collect();
    for (i, name) in flip.iter().enumerate() {
        if let Err(e) = conn.run_verb(Verb::Pin, name) {
            // The ones before it did change: say so before the failure.
            for done in flip.iter().take(i) {
                let was = before.get(*done).copied().unwrap_or(false);
                r.pin(ctx.out, done, !was, true);
            }
            let d = r.fail(name, &e);
            return Ok(Done {
                view: View::Verbs(r),
                problem: Some(d),
            });
        }
    }
    // What the daemon says now, not what was asked for.
    let now = pinned(&conn)?;
    for name in &a.names {
        let (b, n) = (before.get(name).copied(), now.get(name).copied());
        r.pin(ctx.out, name, n.unwrap_or(false), n != b);
    }
    Ok(Done::ok(View::Verbs(r)))
}

fn pinned(conn: &Connection) -> Result<HashMap<String, bool>, crate::diag::Diagnostic> {
    let snap = stack::snapshot(conn)?;
    Ok(snap
        .instances
        .into_iter()
        .map(|v| (v.name, v.keep_awake))
        .collect())
}
