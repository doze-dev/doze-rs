//! `doze connect <name>` (new; the proposal's B6): the engine's own client on an instance, signed in, run on this
//! terminal. Everything about the client is the module's answer on the wire (`InstanceView.connect`: its program, the
//! arguments and variables with doze's address and the password filled in, its fallbacks), so the CLI knows no engine.
//!
//! The stack's daemon is started when it is not running (as `env` starts it), the instance is woken first (its
//! progress on stderr, as `up` shows it), then the client is looked for: in the engine's own build (`path`, once a
//! start has resolved it), else `program` on `PATH`, else each fallback on `PATH`. The client runs with Ctrl-C its own,
//! and doze exits with its code (DZ0710), as `doze run` does. Its secrets stay in its variables, never its arguments.

use crate::cli::args::Service;
use crate::cmd::ask::Asker;
use crate::cmd::progress::Boot;
use crate::cmd::report::Report;
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::stack;
use crate::view::View;
use doze_control::{ClientView, InstanceView, Verb};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Service) -> Outcome {
    let name = a.name.clone();
    let (stack, _, conn) =
        crate::cmd::env::opened(ctx, mode, std::slice::from_ref(&name), "doze connect")?;
    let snap = stack::snapshot(&conn)?;
    let view = stack::declared(&snap)
        .find(|v| v.name == name)
        .ok_or_else(|| stack::not_declared(&snap, &name))?;
    if view.connect.is_none() {
        return Err(no_client(view));
    }
    // Woken first, so a slow first start (a download) is shown as it goes rather than as a client that hangs.
    let ask = Asker::unwatched(conn, mode);
    let mut boot = Boot::begin(
        ctx.err,
        mode.err(),
        (stack.run_dir().log_file(), ctx.term.stderr),
        &name,
    );
    let woke = ask.verb_while(Verb::Wake, &name, &mut || boot.poll(ctx.err));
    boot.end(ctx.err);
    if let Err(stop) = woke {
        return Err(Report::new(mode).stopped(&name, &stop));
    }
    // Asked again: a start is what resolves the engine's build, and with it the client's place in it.
    let snap = stack::snapshot(&ask.conn)?;
    drop(ask);
    let view = stack::declared(&snap)
        .find(|v| v.name == name)
        .ok_or_else(|| stack::not_declared(&snap, &name))?;
    let Some(client) = view.connect.as_ref() else {
        return Err(no_client(view));
    };
    let path = ctx.host.var("PATH").unwrap_or_default();
    let pathext = ctx.host.var("PATHEXT");
    let program = find(
        client,
        OsStr::new(&path),
        pathext.as_deref().map(OsStr::new),
    )
    .ok_or_else(|| missing(client, &name))?;
    say(ctx, mode, client, &program, view);
    let env: Vec<(String, String)> = client.env.clone().into_iter().collect();
    let _ = ctx.out.flush();
    match ctx.host.run_env(&program, &client.args, &env) {
        Ok(0) => Ok(Done::ok(View::Silent)),
        Ok(code) => Err(Diagnostic::new(
            ids::command_exit(code),
            format!("{} exited with status {code}", file_name(&program)),
        )),
        Err(e) => Err(Diagnostic::new(
            ids::COMMAND_UNRUNNABLE,
            format!("could not run {}: {e}", program.display()),
        )
        .fix(format!(
            "doze url {name} gives the address for another client"
        ))),
    }
}

/// The client to run: the engine's own build's, else `program`, else a fallback, on `path`. A name the module gives
/// is a program's name, never a path of its own.
pub(crate) fn find(c: &ClientView, path: &OsStr, pathext: Option<&OsStr>) -> Option<PathBuf> {
    // The daemon's own answer: an absolute path that can be run, else nothing of it.
    let own = Some(Path::new(&c.path))
        .filter(|p| p.is_absolute())
        .and_then(|p| doze_os::which::which_in(&c.path, OsStr::new(""), None).filter(|f| f == p));
    own.or_else(|| {
        std::iter::once(&c.program)
            .chain(&c.fallbacks)
            .filter(|n| !n.is_empty() && !n.contains(['/', '\\']))
            .find_map(|n| doze_os::which::which_in(n, path, pathext))
    })
}

/// "→ valkey-cli on cache (127.0.0.1:6390)", on stderr: stdout is the client's.
fn say(ctx: &mut Ctx<'_>, mode: &Mode, c: &ClientView, program: &Path, v: &InstanceView) {
    let name = file_name(program);
    let at = if v.endpoint.is_empty() {
        String::new()
    } else {
        format!(" ({})", v.endpoint)
    };
    let mut line = format!("{} {name} on {}{at}", mode.err().dim("→"), v.name);
    let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy());
    if stem.as_deref() != Some(c.program.as_str()) {
        line.push_str(&format!(" · {} is not installed here", c.program));
    }
    let _ = writeln!(ctx.err, "{line}");
}

fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// An instance whose module declares no client (an app, a service, a module without one, or a Go daemon's answer).
fn no_client(v: &InstanceView) -> Diagnostic {
    let what = if v.kind == "module" {
        format!("the module that runs {} declares no client", v.name)
    } else {
        format!(
            "{} is {} {}: only a module's instance has a client",
            v.name,
            article(&v.kind),
            v.kind
        )
    };
    Diagnostic::new(
        ids::NOTHING_TO_DO,
        format!("nothing to connect with: {what}"),
    )
    .fix(format!("doze url {} gives its address", v.name))
}

pub(crate) fn article(kind: &str) -> &'static str {
    if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    }
}

/// None of the client's programs is here.
fn missing(c: &ClientView, name: &str) -> Diagnostic {
    let mut tried = vec![c.program.clone()];
    tried.extend(c.fallbacks.iter().cloned());
    let tried = tried.join(" or ");
    Diagnostic::new(
        ids::COMMAND_UNRUNNABLE,
        format!("the client for {name} is not installed here: no {tried} on PATH"),
    )
    .note("the engine's own build has none for this platform, or it has not been fetched")
    .fix(format!(
        "install {}, or use doze url {name} with another client",
        c.program
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engines_own_build_comes_first_then_path_then_the_fallbacks() {
        let dir = std::env::temp_dir().join(format!("dz-connect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok();
        // A runnable file stands for the engine build's client: this test's own program, on every system.
        let own = std::env::current_exe().unwrap_or_default();
        let c = ClientView {
            program: "dz-no-such-client".into(),
            path: own.display().to_string(),
            fallbacks: vec!["/bin/sh".into(), "dz-no-such-fallback".into()],
            ..ClientView::default()
        };
        assert_eq!(find(&c, OsStr::new(""), None), Some(own.clone()));
        // Its build not there, or not runnable, or relative: the PATH, where neither is; a fallback given as a path is not
        // taken.
        let plain = dir.join("plain");
        std::fs::write(&plain, "").ok();
        for path in [plain.display().to_string(), "sh".into()] {
            let c = ClientView { path, ..c.clone() };
            assert_eq!(find(&c, dir.as_os_str(), None), None);
        }
        let gone = ClientView {
            path: dir.join("gone").display().to_string(),
            ..c.clone()
        };
        assert_eq!(find(&gone, dir.as_os_str(), None), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
