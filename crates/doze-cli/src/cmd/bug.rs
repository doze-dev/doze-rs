//! `doze bug` (new, CLI proposal rule 1): writes a report to a file in the doze home and says where: this doze's
//! version, platform and how it was installed; the stack's shape (kinds, names, engines, versions and ports, never a
//! value: no env, no password, no argument); the pins in doze.lock; the last lines of the daemon's own log. Nothing is
//! sent anywhere: the person reads it and attaches it. Exits 0 with the path; 1 when it cannot be written.

use crate::ctx::{Ctx, VERSION};
use crate::diag::{Diagnostic, ids};
use crate::install::Install;
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::view::View;
use serde::Serialize;
use std::fmt::Write as _;
use std::path::PathBuf;

/// Where reports go to be attached.
pub(crate) const ISSUES: &str = "https://github.com/doze-dev/doze/issues/new";
/// How much of the daemon's log a report carries.
const LOG_LINES: usize = 40;

/// The report written.
#[derive(Debug, Serialize)]
pub struct Report {
    pub report: String,
    pub issues: &'static str,
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode) -> Outcome {
    let text = report(ctx, mode);
    let dir = ctx.home.join("reports");
    let file = dir.join(format!("doze-bug-{}.md", ctx.unix_now()));
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&file, text))
        .map_err(|e| {
            Diagnostic::new(
                ids::REPORT_UNWRITABLE,
                format!("the report can't be written to {}: {e}", file.display()),
            )
            .fix("set DOZE_HOME to a folder doze can write, or check the free space")
        })?;
    Ok(Done::ok(View::Bug(Report {
        report: file.display().to_string(),
        issues: ISSUES,
    })))
}

fn report(ctx: &Ctx<'_>, mode: &Mode) -> String {
    let os_arch = crate::platform::os_arch();
    let install = Install::of(ctx.host.exe().as_deref());
    let mut r = format!(
        "# doze bug report\n\nWritten by `doze bug` at {} (seconds since 1970). Nothing was sent: read it, then attach it to an issue at {ISSUES}\n\n## doze\n\n- doze {VERSION} ({os_arch}, module protocol {})\n- installed through {}\n",
        ctx.unix_now(),
        doze_module_proto::version::CURRENT.major,
        install.name()
    );
    let Some(project) = crate::project::find(&ctx.cwd, mode.config.as_deref()) else {
        r.push_str("\n## the stack\n\nno doze.hcl here or in any parent\n");
        return r;
    };
    r.push_str("\n## the stack (its shape: no values)\n\n");
    match crate::config::shape_of(ctx, mode, &project) {
        Ok(cfg) => {
            for i in &cfg.instances {
                let _ = write!(r, "- {} {}", i.kind().as_str(), i.name);
                let module = i.kind() == doze_config::Kind::Module;
                for (what, v) in [("", &i.ty), ("version ", &i.version)]
                    .into_iter()
                    .filter(|_| module)
                {
                    if !v.is_empty() {
                        let _ = write!(r, " {what}{v}");
                    }
                }
                if i.port > 0 {
                    let _ = write!(r, " port {}", i.port);
                }
                r.push('\n');
            }
        }
        Err(d) => {
            let _ = writeln!(r, "the config does not read: [{}] {}", d.id.code, d.message);
        }
    }
    lock(&mut r, &project);
    log(&mut r, ctx, &project);
    r
}

fn lock(r: &mut String, project: &crate::project::Project) {
    r.push_str("\n## doze.lock\n\n");
    match crate::lock::Lock::load(&project.lock()) {
        Ok(l) if l.modules.is_empty() => r.push_str("no module pinned\n"),
        Ok(l) => {
            for (src, pin) in &l.modules {
                let _ = writeln!(r, "- {src} {} (protocol {})", pin.version, pin.protocol);
            }
        }
        Err(e) => {
            let _ = writeln!(r, "it does not read: {e}");
        }
    }
}

fn log(r: &mut String, ctx: &Ctx<'_>, project: &crate::project::Project) {
    let key = doze_projects::StackKey::for_config(
        &ctx.home,
        project.source.as_ref().unwrap_or(&project.config),
    );
    let path: PathBuf = key.log_file();
    let _ = write!(r, "\n## the daemon's log (its last {LOG_LINES} lines)\n\n");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let lines: Vec<&str> = text.lines().collect();
            r.push_str("```\n");
            for l in lines.iter().skip(lines.len().saturating_sub(LOG_LINES)) {
                r.push_str(l);
                r.push('\n');
            }
            r.push_str("```\n");
        }
        Err(_) => r.push_str("no log: the stack has not run on this machine\n"),
    }
}

impl Report {
    pub(crate) fn human(&self, p: Paint) -> String {
        format!(
            "{} wrote {}\n{} nothing was sent · read it, then attach it to an issue: {}\n",
            p.ok("✓"),
            self.report,
            p.dim("  next:"),
            self.issues
        )
    }
}
