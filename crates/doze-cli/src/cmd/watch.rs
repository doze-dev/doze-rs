//! `doze status -w`: the table, drawn again whenever an instance changes or new meters arrive (Go `watchStatus`). On a
//! terminal each draw replaces the last; piped, each follows the one before with a blank line; with `--json`, one
//! `status` line per change (never a repeat) and an end line.

use crate::cmd::status::Report;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::stack::{self, Stack};
use crate::stream::{self, Ended};
use crate::view::View;
use doze_control::{Frame, InstanceView, Subscription, Topic};

/// Clears a terminal and puts the cursor home.
const CLEAR: &str = "\x1b[H\x1b[2J";

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, stack: &Stack, graph: bool) -> Outcome {
    if graph {
        return Err(Diagnostic::new(
            ids::FLAGS_DISAGREE,
            "-w redraws the table; it cannot be combined with --graph",
        )
        .fix("doze status -w, or doze status --graph"));
    }
    let conn = stack.read()?.ok_or_else(|| stack::down("doze status -w"))?;
    let snap = stack::snapshot(&conn)?;
    let mut report = Report::of(&snap, ctx.now);
    let stream = conn
        .subscribe(&Subscription::to(&[Topic::Instances, Topic::Stats]))
        .map_err(|e| stack::daemon_error(&e, None))?;
    let interrupt = ctx.host.watch_interrupt();
    let mut screen = Screen {
        mode,
        tty: ctx.term.stdout,
        last: String::new(),
    };
    let _ = screen.draw(ctx, &report);
    let ended = stream::follow(&stream, &interrupt, |f| {
        if apply(&mut report.views, f) {
            report.now = ctx.host.now();
            screen.draw(ctx, &report)?;
        }
        Ok(())
    });
    stream.cancel();
    // The stack stopping is how a watch ends, as Ctrl-C is.
    let problem = match ended {
        Ended::Lost(d) if stack.read().ok().flatten().is_some() => Some(d),
        Ended::Lost(_) | Ended::Done => None,
    };
    if mode.json {
        let _ = stream::end(ctx.out, problem.as_ref());
    }
    Ok(Done {
        view: View::Streamed,
        problem,
    })
}

struct Screen<'m> {
    mode: &'m Mode,
    tty: bool,
    /// The last JSON line written, so a change that changes nothing is not written again.
    last: String,
}

impl Screen<'_> {
    fn draw(&mut self, ctx: &mut Ctx<'_>, r: &Report) -> std::io::Result<()> {
        if self.mode.quiet {
            return Ok(());
        }
        if self.mode.json {
            let line = doze_control::go_line(&r.json()).map_err(std::io::Error::other)?;
            if line != self.last {
                self.last = line;
                stream::line(ctx.out, "status", &r.json())?;
            }
            return Ok(());
        }
        let p = self.mode.out();
        if self.tty {
            write!(ctx.out, "{CLEAR}")?;
        }
        write!(ctx.out, "{}", r.human(p))?;
        if !self.tty {
            writeln!(ctx.out)?;
        }
        ctx.out.flush()
    }
}

/// Folds one frame into the views; whether there is something new to draw. The opening snapshot repeats what the
/// status call gave, without the meters: the meters in hand are kept, and nothing is drawn for it.
fn apply(views: &mut Vec<InstanceView>, f: Frame) -> bool {
    match f.known_topic() {
        Some(Topic::Instances) => {
            for mut v in f.instances {
                if !v.declared {
                    continue;
                }
                match views.iter_mut().find(|o| o.name == v.name) {
                    Some(old) => {
                        if f.snapshot || old.pid == v.pid {
                            v.ram = old.ram;
                            v.cpu = old.cpu;
                            v.children = std::mem::take(&mut old.children);
                        }
                        *old = v;
                    }
                    None => views.push(v),
                }
            }
            views.retain(|v| !f.removed.contains(&v.name));
            !f.snapshot
        }
        Some(Topic::Stats) => {
            for s in f.stats {
                if let Some(v) = views.iter_mut().find(|v| v.name == s.name) {
                    v.ram = s.ram;
                    v.cpu = s.cpu;
                }
            }
            true
        }
        _ => false,
    }
}
