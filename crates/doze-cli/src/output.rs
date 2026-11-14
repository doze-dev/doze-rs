//! The output contract, for people, scripts and agents (as the Go doze published it):
//!
//! - stdout carries what a command produced; stderr carries progress, notices and errors.
//! - `--json` (or `DOZE_JSON=1`) makes stdout one envelope, `doze/v1`, on success and on failure alike, for every
//!   command that has a result to give; a command without one refuses `--json`.
//! - `-q` prints nothing on stdout: the exit code is the answer.
//! - colour only on a terminal, never under `NO_COLOR` or `--no-color`.
//!
//! The update line, at most once a day on an interactive terminal, comes last ([`crate::update`]).

use crate::cli::{Cmd, Global};
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::exit::Exit;
use crate::notice::Notices;
use crate::paint::Paint;
use crate::view::View;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

/// How this run prints, from the global flags and the environment.
#[derive(Debug, Clone)]
pub struct Mode {
    pub json: bool,
    pub quiet: bool,
    pub color_out: bool,
    pub color_err: bool,
    pub yes: bool,
    pub timeout: Duration,
    pub config: Option<PathBuf>,
}

impl Mode {
    /// Before the command line is understood (a usage error): plain, JSON only if asked for in so many words.
    pub(crate) fn fallback(ctx: &Ctx<'_>, json: bool) -> Mode {
        Mode {
            json,
            quiet: false,
            color_out: false,
            color_err: color(ctx, ctx.term.stderr, false),
            yes: false,
            timeout: Duration::from_secs(300),
            config: None,
        }
    }

    pub(crate) fn new(g: &Global, cmd: Option<&Cmd>, ctx: &Ctx<'_>) -> Result<Mode, Diagnostic> {
        let capable = json_capable(cmd);
        let json = g.json || (!g.quiet && capable && ctx.var("DOZE_JSON").as_deref() == Some("1"));
        let mode = Mode {
            json,
            quiet: g.quiet,
            color_out: color(ctx, ctx.term.stdout, g.no_color),
            color_err: color(ctx, ctx.term.stderr, g.no_color),
            yes: g.yes || ctx.var("DOZE_YES").as_deref() == Some("1"),
            timeout: g.timeout,
            config: g.config.clone(),
        };
        if g.json && g.quiet {
            return Err(Diagnostic::new(
                ids::JSON_AND_QUIET,
                "--json and -q ask for opposite things",
            )
            .fix("pick one: --json for a program, -q for the exit code alone"));
        }
        if g.json && !capable {
            return Err(
                Diagnostic::new(ids::NO_JSON, "this command has no JSON result").fix("drop --json"),
            );
        }
        Ok(mode)
    }

    pub(crate) fn out(&self) -> Paint {
        Paint::new(self.color_out)
    }

    pub(crate) fn err(&self) -> Paint {
        Paint::new(self.color_err)
    }
}

fn color(ctx: &Ctx<'_>, terminal: bool, no_color: bool) -> bool {
    terminal
        && !no_color
        && ctx.var("NO_COLOR").is_none()
        && ctx.var("TERM").as_deref() != Some("dumb")
}

/// Commands with a result to give as JSON: all but those that hand the terminal to something else or print a script.
pub(crate) fn json_capable(cmd: Option<&Cmd>) -> bool {
    !matches!(
        cmd,
        None | Some(
            Cmd::Run(_) | Cmd::Connect(_) | Cmd::Open(_) | Cmd::Completion(_) | Cmd::Refresh(_)
        )
    )
}

/// What a command produced: its result, and what kept it from being the whole answer (a partial check, a failure
/// found) when something did.
#[derive(Debug)]
pub struct Done {
    pub view: View,
    pub problem: Option<Diagnostic>,
}

impl Done {
    pub(crate) fn ok(view: View) -> Done {
        Done {
            view,
            problem: None,
        }
    }
}

/// A command's outcome: a result (perhaps with a problem), or only a problem.
pub type Outcome = Result<Done, Diagnostic>;

#[derive(Serialize)]
struct Envelope<'a> {
    schema: &'static str,
    command: &'a str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    warnings: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<crate::diag::ErrorBody<'a>>,
}

/// Prints the outcome as the mode says and gives the exit class. Write errors are ignored: a reader that went away
/// (`| head`) is not a failure of the command.
pub(crate) fn finish(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    command: &str,
    outcome: Outcome,
    notices: &Notices,
) -> Exit {
    let (view, problem) = match outcome {
        Ok(d) => (Some(d.view), d.problem),
        Err(d) => (None, Some(d)),
    };
    let exit = problem.as_ref().map_or(Exit::Ok, Diagnostic::exit);
    if mode.json {
        let env = Envelope {
            schema: "doze/v1",
            command,
            ok: exit == Exit::Ok,
            data: view.as_ref().map(View::json),
            warnings: notices.lines(),
            error: problem.as_ref().map(Diagnostic::json),
        };
        let text = serde_json::to_string_pretty(&env).unwrap_or_default();
        let _ = writeln!(ctx.out, "{text}");
        return exit;
    }
    if let Some(v) = &view
        && !mode.quiet
    {
        let _ = write!(ctx.out, "{}", v.human(mode.out()));
    }
    // The result first, then what is said about it: stdout is buffered, stderr is not.
    let _ = ctx.out.flush();
    if let Some(d) = &problem {
        let _ = write!(ctx.err, "{}", d.render(mode.err()));
    }
    for n in notices.lines() {
        let _ = writeln!(ctx.err, "{} {n}", mode.err().warn("!"));
    }
    exit
}
