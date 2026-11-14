//! `doze logs [name…]` (Go cmd/doze/logs.go): the last lines of each service's newest run, oldest first; with several
//! services, merged by time with each line saying whose it is. Logs are kept on disk per run, so this works for a
//! service that is asleep or crashed, and with the daemon down (read from the files, [`doze_logstore`]). `-f` follows
//! as lines are written, until Ctrl-C; with `--json`, as NDJSON.

use crate::cli::args::Logs;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::stack::{self, Stack};
use crate::stream::{self, Ended};
use crate::view::View;
use doze_control::{Connection, LogFrame, LogQuery, Subscription, Timestamp, Topic};
use serde::Serialize;
use std::time::SystemTime;

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Logs) -> Outcome {
    let stack = Stack::find(ctx, mode)?;
    let conn = stack.read()?;
    let names = names(&stack, conn.as_ref(), &a.names)?;
    // `--since` is positive by its parser (`values::duration`).
    let since = match a.since {
        Some(d) => Some(ctx.now.checked_sub(d).unwrap_or(SystemTime::UNIX_EPOCH)),
        None => None,
    };
    let zone = zone(ctx);
    let page = Page {
        prefix: names.len() > 1,
        width: names.iter().map(|n| n.chars().count()).max().unwrap_or(0),
        zone,
    };
    if a.follow {
        if a.run != 0 {
            return Err(Diagnostic::new(
                ids::FLAGS_DISAGREE,
                "-f follows the newest run; it cannot be combined with --run",
            )
            .fix(format!("doze logs --run {} (no -f)", a.run)));
        }
        let conn = conn.ok_or_else(|| stack::down("doze logs -f"))?;
        let sub = Subscription {
            names: names.clone(),
            backlog: a.tail.map_or(0, i64::from),
            ..Subscription::to(&[Topic::Logs])
        };
        return follow(ctx, mode, &conn, &sub, since, &page);
    }
    let q = |instance: &str| LogQuery {
        instance: instance.to_owned(),
        run: a.run,
        tail: a.tail.map_or(0, i64::from),
        since: since.map_or_else(Timestamp::zero, Timestamp::from_system_time),
    };
    let mut lines: Vec<LogFrame> = Vec::new();
    for name in &names {
        let got = match &conn {
            Some(c) => c.logs(&q(name)).map_err(|e| stack::daemon_error(&e, None)),
            None => disk(&stack, &q(name)),
        };
        match got {
            Ok(mut l) => lines.append(&mut l),
            Err(d) if names.len() == 1 => return Err(d),
            Err(_) => {} // one service with no such run does not hide the others
        }
    }
    lines.sort_by_key(|l| l.time.unix_nanos().unwrap_or(0));
    Ok(Done::ok(View::Logs(Report {
        lines,
        alone: (names.len() == 1).then(|| names.join("")),
        page,
    })))
}

/// The named services (checked against what the stack declares when doze runs it), or every one.
fn names(
    stack: &Stack,
    conn: Option<&Connection>,
    asked: &[String],
) -> Result<Vec<String>, Diagnostic> {
    match conn {
        Some(c) => {
            let snap = stack::snapshot(c)?;
            if asked.is_empty() {
                Ok(stack::declared(&snap).map(|v| v.name.clone()).collect())
            } else {
                stack::check_names(&snap, asked)?;
                Ok(asked.to_vec())
            }
        }
        // With doze down, every service that has printed anything.
        None if asked.is_empty() => {
            let dir = doze_logstore::dir_of(stack.key.dir());
            let mut all: Vec<String> = std::fs::read_dir(dir)
                .map(|e| {
                    e.filter_map(Result::ok)
                        .filter_map(|e| e.file_name().into_string().ok())
                        .collect()
                })
                .unwrap_or_default();
            all.sort();
            Ok(all)
        }
        None => {
            stack::check_offline(stack, asked)?;
            Ok(asked.to_vec())
        }
    }
}

/// The lines from the files, as the daemon would give them.
fn disk(stack: &Stack, q: &LogQuery) -> Result<Vec<LogFrame>, Diagnostic> {
    let store = doze_logstore::Store::open(doze_logstore::dir_of(stack.key.dir()));
    let tail = usize::try_from(q.tail)
        .ok()
        .filter(|n| *n > 0)
        .unwrap_or(doze_logstore::BACKLOG);
    let lines = store.tail(&q.instance, q.run, tail).map_err(|e| {
        Diagnostic::new(ids::NO_SUCH_RUN, e.to_string()).fix(format!("doze logs {}", q.instance))
    })?;
    let since = q.since.unix_nanos().filter(|_| !q.since.is_zero());
    Ok(lines
        .into_iter()
        .filter_map(|l| {
            let time = Timestamp::parse(&l.stamp)?;
            let keep = since.is_none_or(|s| time.unix_nanos().is_some_and(|t| t >= s));
            keep.then(|| LogFrame {
                instance: l.instance,
                line: l.text,
                stream: l.stream,
                run: i64::from(l.run),
                time,
            })
        })
        .collect())
}

/// `logs -f`: the backlog and then every line as it is written, from one subscription so none falls between.
fn follow(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    conn: &Connection,
    sub: &Subscription,
    since: Option<SystemTime>,
    page: &Page,
) -> Outcome {
    let s = conn
        .subscribe(sub)
        .map_err(|e| stack::daemon_error(&e, None))?;
    let interrupt = ctx.host.watch_interrupt();
    let since = since.map(|t| Timestamp::from_system_time(t).unix_nanos().unwrap_or(0));
    let p = mode.out();
    let ended = stream::follow(&s, &interrupt, |f| {
        let Some(l) = f.log else { return Ok(()) };
        if since.is_some_and(|s| l.time.unix_nanos().unwrap_or(0) < s) || mode.quiet {
            return Ok(());
        }
        if mode.json {
            return stream::line(ctx.out, "log", &l);
        }
        writeln!(ctx.out, "{}", page.line(&l, p))?;
        ctx.out.flush()
    });
    s.cancel();
    let problem = match ended {
        Ended::Lost(d) => Some(d),
        Ended::Done => None,
    };
    if mode.json {
        let _ = stream::end(ctx.out, problem.as_ref());
    }
    Ok(Done {
        view: View::Streamed,
        problem,
    })
}

/// How lines are drawn.
#[derive(Debug, Clone)]
pub(crate) struct Page {
    /// Several services: each line says whose it is.
    prefix: bool,
    width: usize,
    zone: jiff::tz::TimeZone,
}

impl Page {
    /// One line (Go `formatLogLine`): the time on this machine's clock, whose it is when several are shown, a mark for
    /// stderr or a step, and the text as the service printed it.
    pub(crate) fn line(&self, l: &LogFrame, p: Paint) -> String {
        let mut b = p.dim(&self.clock(&l.time)).into_owned();
        b.push(' ');
        if self.prefix {
            let pad = self.width.saturating_sub(l.instance.chars().count());
            b.push_str(&p.dim(&format!("{}{} │ ", l.instance, " ".repeat(pad))));
        }
        let (stream, mut part) = l.stream.split_once(':').unwrap_or((l.stream.as_str(), ""));
        match stream {
            "e" => {
                b.push_str(&p.warn("err"));
                b.push(' ');
            }
            "s" => {
                b.push_str(&p.dim(&format!("[{part}]")));
                b.push(' ');
                part = "";
            }
            _ => {}
        }
        if !part.is_empty() {
            // one process of a service that runs several
            b.push_str(&p.dim(&format!("{part}:")));
            b.push(' ');
        }
        b.push_str(&l.line);
        b
    }

    fn clock(&self, t: &Timestamp) -> String {
        crate::clock::wall(t, &self.zone)
    }
}

/// This machine's time zone (`TZ`, read through the run's host).
pub(crate) fn zone(ctx: &Ctx<'_>) -> jiff::tz::TimeZone {
    crate::clock::zone(ctx.var("TZ").as_deref())
}

/// The lines read, and how to draw them.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    lines: Vec<LogFrame>,
    /// The one service asked about, for "has not printed anything yet".
    #[serde(skip)]
    alone: Option<String>,
    #[serde(skip)]
    page: Page,
}

impl Report {
    pub(crate) fn human(&self, p: Paint) -> String {
        if self.lines.is_empty()
            && let Some(name) = &self.alone
        {
            return format!(
                "{}\n",
                p.dim(&format!("{name} has not printed anything yet"))
            );
        }
        self.lines
            .iter()
            .map(|l| self.page.line(l, p) + "\n")
            .collect()
    }
}
