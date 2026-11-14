//! `doze pin <name…>` (`keepawake`): keeps services awake past their idle time; `--off` lets them sleep again,
//! `--toggle` flips each (Go `runPin`). It says what it wants, so running it twice changes nothing the second time:
//! the daemon's verb is a toggle, so it is sent only to the ones that differ, and each result says whether it changed.

use crate::cli::args::Pin;
use crate::cmd::ask::Asker;
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
    let names = stack::unique(&a.names);
    let stack = Stack::find(ctx, mode)?;
    let Some(conn) = stack.open(mode.timeout)? else {
        if want == Want::Off {
            // Nothing runs, so nothing is pinned.
            let mut r = Report::new(mode);
            for n in &names {
                r.pin(ctx.out, n, false, false);
            }
            return Ok(Done::ok(View::Verbs(r)));
        }
        return Err(stack::down("doze pin"));
    };
    let snap = stack::snapshot(&conn)?;
    stack::check_names(&snap, &names)?;
    if a.dry.dry_run {
        return Ok(Done::ok(View::Plan(dry::pin(&snap, &names, want))));
    }
    let ask = Asker::new(conn, ctx, mode);
    let before = pinned(&ask.conn)?;
    let mut r = Report::new(mode);
    if let Err(d) = flip(ctx, &ask, &mut r, &names, want, &before) {
        return Ok(Done {
            view: View::Verbs(r),
            problem: Some(d),
        });
    }
    // What the daemon says now, not what was asked for.
    let now = pinned(&ask.conn)?;
    for name in &names {
        let was = before.get(name).copied().unwrap_or(false);
        let is = now.get(name).copied().unwrap_or(false);
        r.pin(ctx.out, name, is, is != was);
    }
    Ok(Done::ok(View::Verbs(r)))
}

/// Sends the daemon's toggle to each name not yet as wanted; on a failure, says which did change before it.
fn flip(
    ctx: &mut Ctx<'_>,
    ask: &Asker,
    r: &mut Report,
    names: &[String],
    want: Want,
    before: &HashMap<String, bool>,
) -> Result<(), crate::diag::Diagnostic> {
    let was = |n: &String| before.get(n).copied().unwrap_or(false);
    let flip: Vec<&String> = names.iter().filter(|n| want.flips(was(n))).collect();
    for (i, name) in flip.iter().enumerate() {
        if let Err(stop) = ask.verb(Verb::Pin, name) {
            for done in flip.iter().take(i) {
                r.pin(ctx.out, done, !was(done), true);
            }
            return Err(r.stopped(name, &stop));
        }
    }
    Ok(())
}

fn pinned(conn: &Connection) -> Result<HashMap<String, bool>, crate::diag::Diagnostic> {
    let snap = stack::snapshot(conn)?;
    Ok(snap
        .instances
        .into_iter()
        .map(|v| (v.name, v.keep_awake))
        .collect())
}
