//! `doze open [dir|doze://link] [--show service]` (Go cmd/doze/open.go) and bare `doze`: the app, at this project.
//!
//! `doze` is the CLI-only binary (core plan O1): the window lives in the app, which answers `open` itself (it hands
//! the arguments to a running app, or starts one on its own and says "doze is open"; Go's Doze.app answers the same
//! words). So this finds the app and hands it the command line, its folder made absolute: `$DOZE_APP_BUNDLE`, then
//! Doze.app in the Applications folders on a Mac, then `doze-app` on the PATH.
//!
//! With no display here (SSH, or Linux with no display server) there is no window to open: it prints the status
//! table instead, as Go does, and so does bare `doze` there or when piped (O2). `--console <name>` needs the module's
//! web address from the running stack, which comes with the Rust daemon (core plan D1): it says so.

use super::{status, stub};
use crate::cli::args::Status;
use crate::cli::setup::Open;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::notice::Notices;
use crate::output::{Done, Mode, Outcome};
use crate::view::View;
use std::path::{Path, PathBuf};

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Open, notices: &mut Notices) -> Outcome {
    if a.console.is_some() {
        return Err(stub::not_built("doze open --console", stub::Needs::Daemon)
            .note("the console's address is the running module's (its web UI port), which the Rust daemon gives")
            .fix("until then: doze url <name> prints the service's address"));
    }
    if !display(ctx) {
        let _ = writeln!(ctx.err, "no display here; showing the status table instead");
        return table(ctx, mode, notices);
    }
    let mut args = vec!["open".to_owned()];
    if let Some(t) = &a.target {
        args.push(if t.starts_with("doze://") {
            t.clone()
        } else {
            let p = ctx.cwd.join(t);
            if !p.is_dir() {
                return Err(Diagnostic::new(ids::NO_CONFIG, format!("there is no folder {t}"))
                    .fix("doze open <folder> opens a project's folder · doze open doze://<stack> opens a stack"));
            }
            p.display().to_string()
        });
    } else if let Some(p) = crate::project::find(&ctx.cwd, mode.config.as_deref()) {
        args.push(p.dir.display().to_string());
    }
    if let Some(s) = &a.show {
        check_show(ctx, mode, args.get(1).map(String::as_str), s)?;
        args.extend(["--show".to_owned(), s.clone()]);
    }
    hand_over(ctx, &args)
}

/// `--show <name>` names a service of the project being opened: a typo is said here, with the nearest name, rather
/// than by a window that opens on nothing. A link, or a project that does not read, is left to the app.
fn check_show(
    ctx: &Ctx<'_>,
    mode: &Mode,
    place: Option<&str>,
    name: &str,
) -> Result<(), Diagnostic> {
    if place.is_some_and(|p| p.starts_with("doze://")) {
        return Ok(());
    }
    let project = match place {
        Some(dir) => crate::project::find(Path::new(dir), None),
        None => crate::project::find(&ctx.cwd, mode.config.as_deref()),
    };
    let Some(cfg) = project.and_then(|p| crate::config::shape_of(ctx, mode, &p).ok()) else {
        return Ok(());
    };
    let names = super::declared::names(&cfg);
    if names.iter().any(|n| n == name) {
        return Ok(());
    }
    Err(crate::stack::not_in(&names, name))
}

/// Bare `doze`: the app on a terminal with a display, the status table over SSH or when piped (O2).
pub(crate) fn bare(ctx: &mut Ctx<'_>, mode: &Mode, notices: &mut Notices) -> Outcome {
    if display(ctx) && ctx.term.stdin && ctx.term.stdout && !mode.json {
        let mut args = vec!["open".to_owned()];
        if let Some(p) = crate::project::find(&ctx.cwd, mode.config.as_deref()) {
            args.push(p.dir.display().to_string());
        }
        return hand_over(ctx, &args);
    }
    table(ctx, mode, notices)
}

fn table(ctx: &mut Ctx<'_>, mode: &Mode, notices: &mut Notices) -> Outcome {
    status::run(
        ctx,
        mode,
        &Status {
            watch: false,
            graph: false,
        },
        notices,
    )
}

fn hand_over(ctx: &mut Ctx<'_>, args: &[String]) -> Outcome {
    let Some(app) = find_app(ctx) else {
        return Err(Diagnostic::new(ids::NO_APP, "the doze app is not on this machine")
            .note("looked for $DOZE_APP_BUNDLE, Doze.app in the Applications folders, and doze-app on the PATH")
            .fix("install the app (https://doze-dev.com/download) · doze status shows the stack here"));
    };
    let _ = ctx.out.flush();
    match ctx.host.run(&app, args) {
        Ok(0) => Ok(Done::ok(View::Silent)),
        Ok(code) => Err(
            Diagnostic::new(ids::NO_APP, format!("the app stopped with exit {code}"))
                .note(format!("{} {}", app.display(), args.join(" ")))
                .fix("doze bug writes a report with the app's log"),
        ),
        Err(e) => Err(
            Diagnostic::new(ids::NO_APP, format!("the app could not be started: {e}"))
                .note(app.display().to_string())
                .fix("reinstall the app (https://doze-dev.com/download)"),
        ),
    }
}

/// Whether a window can be shown here: not over SSH, and on Linux only with a display server.
fn display(ctx: &Ctx<'_>) -> bool {
    if ctx.var("SSH_CONNECTION").is_some() || ctx.var("SSH_TTY").is_some() {
        return false;
    }
    !crate::platform::os_arch().starts_with("linux")
        || ctx.var("DISPLAY").is_some()
        || ctx.var("WAYLAND_DISPLAY").is_some()
}

/// The app's program: in `$DOZE_APP_BUNDLE`, in a Doze.app in the Applications folders, or `doze-app` on the PATH.
fn find_app(ctx: &Ctx<'_>) -> Option<PathBuf> {
    let in_bundle = |b: &Path| {
        ["doze-app", "doze"]
            .iter()
            .map(|n| b.join("Contents/MacOS").join(n))
            .find(|p| p.is_file())
    };
    if let Some(b) = ctx.var("DOZE_APP_BUNDLE")
        && let Some(p) = in_bundle(Path::new(&b))
    {
        return Some(p);
    }
    if let Some(p) = ctx
        .host
        .applications()
        .iter()
        .find_map(|d| in_bundle(&d.join("Doze.app")))
    {
        return Some(p);
    }
    let path = ctx.var("PATH")?;
    let ext = ctx.var("PATHEXT");
    doze_os::which::which_in(
        "doze-app",
        path.as_ref(),
        ext.as_deref().map(std::ffi::OsStr::new),
    )
}
