//! `doze init [--wizard] [--force] [--dry-run]` (Go cmd/doze/init.go): writes a doze.hcl from what the project says
//! (doze-detect), every block with the reason it was found; with `--wizard` on a terminal, asks instead.
//!
//! The modules it can recognise are the ones that declare how (`detect` in their `meta.yaml`, through doze-fetch):
//! there is no engine table. Better than Go (proposal F6): it never invents. Finding nothing, it writes a commented
//! starter that says how to add what is needed, where Go wrote a postgres block; the wizard has no default pick. With
//! no module's declaration to be had (offline, nothing cached) it says so and exits 11: the file it wrote could not
//! have any module in it.

use super::{add, decl};
use crate::cli::setup::Init;
use crate::ctx::{Ctx, VERSION};
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::Paint;
use crate::view::View;
use doze_detect::{Declared, Found};
use serde::Serialize;

/// What `init` wrote, or would write.
#[derive(Debug, Default, Serialize)]
pub struct Inited {
    pub file: String,
    pub written: bool,
    /// Each block found and why, `kind name why`.
    pub found: Vec<Finding>,
    pub notes: Vec<String>,
    /// The file's text, with `--dry-run`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Nothing was found, so the file is a commented starter.
    pub starter: bool,
    /// The table was already shown (before the question).
    #[serde(skip)]
    shown: bool,
    /// The wizard's last check, when asked for.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub checked: String,
    /// The person said no.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub declined: bool,
}

/// One block found.
#[derive(Debug, Serialize)]
pub struct Finding {
    pub kind: String,
    pub name: String,
    pub why: String,
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, a: &Init) -> Outcome {
    let path = ctx.cwd.join(
        mode.config
            .as_deref()
            .unwrap_or(std::path::Path::new(crate::project::CONFIG)),
    );
    let shown = crate::project::shown(&path, &ctx.cwd);
    if path.exists() && !a.force {
        return Err(
            Diagnostic::new(ids::CONFIG_EXISTS, format!("{shown} already exists"))
                .fix("doze init --force overwrites it · doze add <module> adds to it"),
        );
    }
    let known = decl::all(ctx);
    let dir = path.parent().unwrap_or(&ctx.cwd).to_path_buf();
    let found = if a.wizard {
        Found::default()
    } else {
        doze_detect::scan(&dir, &known.declared)
    };
    let mut done = Inited {
        file: shown.clone(),
        ..Inited::default()
    };
    let text = if !found.is_empty() {
        detected(ctx, &found, &mut done)
    } else if a.wizard && ctx.interactive() && !mode.yes {
        match wizard(ctx, &known.declared, &mut done) {
            Some(t) => t,
            None => return Ok(Done::ok(View::Inited(done))),
        }
    } else {
        done.starter = true;
        starter()
    };
    add::must_read(ctx, mode, &path, text.as_bytes(), &shown)?;
    if !done.shown && !a.dry.dry_run && !mode.yes && ctx.interactive() && !found.is_empty() {
        let _ = write!(ctx.out, "{}", done.table(mode.out()));
        done.shown = true;
        if !confirm(ctx, &format!("Write {shown} with this?")) {
            done.declined = true;
            return Ok(Done::ok(View::Inited(done)));
        }
    }
    if a.dry.dry_run {
        done.text = text;
    } else {
        add::write(&path, text.as_bytes(), &shown)?;
        done.written = true;
    }
    // Rule 3: with no module's declaration to be had, no module could be guessed; the run says so and is partial.
    let problem = known.none.then(|| {
        Diagnostic::new(ids::NOT_ALL_CHECKED, known.missing.clone())
            .note("the file was written from what the project says, without modules")
            .fix("check the network and run doze init --force again · doze add <module> adds one by hand")
    });
    if !known.none && !known.missing.is_empty() {
        done.notes.push(known.missing);
    }
    Ok(Done {
        view: View::Inited(done),
        problem,
    })
}

fn detected(ctx: &Ctx<'_>, found: &Found, done: &mut Inited) -> String {
    let rows = [
        (
            "app",
            found
                .apps
                .iter()
                .map(|x| (&x.name, &x.why))
                .collect::<Vec<_>>(),
        ),
        (
            "module",
            found.modules.iter().map(|x| (&x.module, &x.why)).collect(),
        ),
        (
            "service",
            found.services.iter().map(|x| (&x.name, &x.why)).collect(),
        ),
        (
            "task",
            found.tasks.iter().map(|x| (&x.name, &x.why)).collect(),
        ),
    ];
    for (kind, list) in rows {
        for (name, why) in list {
            done.found.push(Finding {
                kind: kind.into(),
                name: name.clone(),
                why: why.clone(),
            });
        }
    }
    done.notes.extend(found.notes.iter().cloned());
    let path = ctx.var("PATH").unwrap_or_default();
    let ext = ctx.var("PATHEXT");
    doze_detect::scaffold(found, &|bin| {
        doze_os::which::which_in(bin, path.as_ref(), ext.as_deref().map(std::ffi::OsStr::new))
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    })
}

/// Asks a question on stdout where Enter means yes.
fn confirm(ctx: &mut Ctx<'_>, question: &str) -> bool {
    let _ = write!(ctx.out, "{question} [Y/n]: ");
    let _ = ctx.out.flush();
    let line = ctx
        .host
        .read_line()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    matches!(line.as_str(), "" | "y" | "yes")
}

fn ask(ctx: &mut Ctx<'_>, question: &str) -> Option<String> {
    let _ = write!(ctx.out, "{question}");
    let _ = ctx.out.flush();
    ctx.host.read_line().map(|l| l.trim().to_owned())
}

/// The wizard (Go's `runWizard`): which modules, an app command, and whether to check it. No default pick: Enter
/// on the first question picks nothing. `None` when nothing was chosen.
fn wizard(ctx: &mut Ctx<'_>, offers: &[Declared], done: &mut Inited) -> Option<String> {
    let _ = writeln!(ctx.out, "Let's scaffold a doze stack. (Ctrl-C to bail)\n");
    for (i, d) in offers.iter().enumerate() {
        let _ = writeln!(ctx.out, "  {}  {:<12} {}", i + 1, d.module, d.about);
    }
    let _ = writeln!(ctx.out);
    let line = ask(ctx, "Services — numbers or names, space-separated: ").unwrap_or_default();
    let picks = picks(&line, offers);
    let app = ask(ctx, "App command — e.g. 'go run . serve' (Enter to skip): ").unwrap_or_default();
    if picks.is_empty() && app.is_empty() {
        done.notes
            .push("nothing selected — run doze init again to pick services".into());
        return None;
    }
    let lint = ask(ctx, "Validate it now with doze lint? [y/N]: ")
        .is_some_and(|a| matches!(a.to_lowercase().as_str(), "y" | "yes"));
    for d in &picks {
        done.found.push(Finding {
            kind: "module".into(),
            name: d.module.clone(),
            why: "picked".into(),
        });
    }
    done.shown = true;
    if lint {
        done.checked = format!(
            "looks good — {} service(s)",
            picks.len() + usize::from(!app.is_empty())
        );
    }
    Some(scaffold(&picks, &app))
}

/// "1 3 valkey": the offers named, each once, in the order given.
fn picks<'a>(line: &str, offers: &'a [Declared]) -> Vec<&'a Declared> {
    let mut out: Vec<&Declared> = Vec::new();
    for tok in line.split_whitespace() {
        let hit = offers
            .iter()
            .enumerate()
            .find(|(i, d)| d.module == tok || (i + 1).to_string() == tok)
            .map(|(_, d)| d);
        if let Some(d) = hit
            && !out.iter().any(|o| o.module == d.module)
        {
            out.push(d);
        }
    }
    out
}

const HEADER: &str = "# doze.hcl — declarative local services, no Docker.\n\
# Each instance pins its own port; doze boots it on first connect, reaps when idle.\n\
# Edit freely, then: doze lint · doze up · doze status\n\n";

const DEFAULTS: &str = "defaults {\n  sleep = \"2m\"\n  names = true # DNS names per service: <name>.<stack>.doze, e.g. app.myproject.doze:5432\n}\n";

/// The line a new doze.hcl starts with: the oldest doze the stack needs (this one's major and minor); none for a
/// development build (Go's `dozeversion.Marker`).
fn marker() -> String {
    let v = VERSION.trim_start_matches('v');
    if !crate::registry::select::is_doze_version(VERSION) {
        return String::new();
    }
    let mm: Vec<&str> = v.split('.').take(2).collect();
    format!(
        "doze = \">= {}\" # the oldest doze this stack needs\n\n",
        mm.join(".")
    )
}

/// The wizard's file (Go's `scaffoldFor`): each pick's block, and the app wired to what it picked.
fn scaffold(picks: &[&Declared], app: &str) -> String {
    let mut b = format!("{HEADER}{}{DEFAULTS}", marker());
    for d in picks {
        b.push_str(&format!(
            "\n{}\n",
            doze_detect::render_module(&d.module, &d.label, &d.version, d.port)
        ));
    }
    if !app.is_empty() {
        b.push_str(&format!(
            "\napp \"api\" {{\n  run  = {}\n  port = 8080 # doze listens here; your app listens on $PORT\n",
            doze_detect::quote(app)
        ));
        let env: Vec<String> = picks
            .iter()
            .filter_map(|d| {
                doze_detect::wiring(d).map(|(e, a)| format!("    {e} = module.{}.{a}", d.label))
            })
            .collect();
        if !env.is_empty() {
            b.push_str(&format!("\n  env = {{\n{}\n  }}\n", env.join("\n")));
        }
        b.push_str("}\n");
    }
    String::from_utf8_lossy(&doze_hcl::fmt::format(b.as_bytes())).into_owned()
}

/// What `init` writes when it found nothing: never an invented block, but the way to add one.
fn starter() -> String {
    format!(
        "{HEADER}{}{DEFAULTS}\n\
         # doze found nothing it runs in this folder. Add what the project needs:\n\
         #   doze add <module>    a module (doze modules docs <module> describes one: valkey, postgres, …)\n\
         #   doze add <name>      anything else, run from your PATH, a container or a download\n",
        marker()
    )
}

impl Inited {
    fn table(&self, p: Paint) -> String {
        if self.found.is_empty() {
            return String::new();
        }
        let mut out = format!("{}\n", p.cmd("Found in this project:"));
        for f in &self.found {
            out.push_str(&format!(
                "  {:<8} {:<14} {}\n",
                f.kind,
                f.name,
                p.dim(&f.why)
            ));
        }
        for n in &self.notes {
            out.push_str(&format!("  {}\n", p.dim(n)));
        }
        out.push('\n');
        out
    }

    pub(crate) fn human(&self, p: Paint) -> String {
        let mut out = if self.shown || self.starter {
            String::new()
        } else {
            self.table(p)
        };
        if self.declined {
            out.push_str("nothing written — doze init --wizard asks instead\n");
            return out;
        }
        if !self.text.is_empty() {
            out.push_str(&self.text);
            out.push_str(&format!(
                "{}\n",
                p.dim("  dry run: nothing was written · doze init writes this")
            ));
            return out;
        }
        if !self.written {
            for n in &self.notes {
                out.push_str(&format!("{n}\n"));
            }
            return out;
        }
        if self.starter {
            out.push_str(&format!(
                "{} wrote {} · found nothing doze runs here, so it is a commented starter\n{} doze add <module> fills it\n",
                p.ok("✓"),
                self.file,
                p.dim("  next:")
            ));
        } else {
            out.push_str(&format!(
                "{} wrote {}\n{} read it   ·   doze lint   ·   doze up   ·   doze add <module> for more\n",
                p.ok("✓"),
                self.file,
                p.dim("  next:")
            ));
        }
        if !self.checked.is_empty() {
            out.push_str(&format!("{} {}\n", p.ok("✓"), self.checked));
        }
        out
    }
}
