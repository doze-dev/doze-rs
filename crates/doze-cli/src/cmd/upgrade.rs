//! `doze modules upgrade [module…] [--check] [--dry-run]` (Go cmd/doze/modules.go): moves each module's pin in
//! doze.lock to the newest release that fits this doze and the engine versions doze.hcl declares, downloading and
//! verifying the build as it goes (doze-fetch's `Modules::upgrade`). The lock is written through doze-fetch's locked
//! writer, keyed by the file's canonical path, so a daemon's fetch children saving the same file lose nothing.
//!
//! The modules are the ones doze.hcl declares (its shape, read offline: the upgrade must run when a full load fails on
//! the very pin it fixes), or the ones named. `--check` reports and exits 1 if any would move; `--dry-run` shows each
//! pin from and to and exits 0; neither writes doze.lock. Exits 0; 1 a module failed or (`--check`) one would move;
//! 4 a named module doze.hcl does not declare (with "did you mean"); 5/6 the config.

use crate::cli::setup::Upgrade;
use crate::config;
use crate::ctx::{Ctx, VERSION};
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::paint::{Paint, pad, width};
use crate::plan::{Op, Plan};
use crate::registry::select::{Fit, Here, newest_that_fits};
use crate::registry::source::Source;
use crate::view::View;
use doze_fetch::modules::Modules;
use serde::Serialize;

/// One module's row.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub module: String,
    pub source: String,
    /// `-` when it was not pinned.
    pub pinned: String,
    /// `-` when none could be selected.
    pub selected: String,
    /// upgraded, up to date, upgrade available, failed.
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// What `modules upgrade` did, or found.
#[derive(Debug, Clone, Serialize)]
pub struct Upgraded {
    pub lock: String,
    pub check: bool,
    pub modules: Vec<Row>,
}

/// The engine major of a declared version (`8.2` is `8`).
fn major(v: &str) -> String {
    let v = v.trim().trim_start_matches('v');
    v.split('.').next().unwrap_or(v).to_owned()
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, u: &Upgrade) -> Outcome {
    let loaded = config::shape(ctx, mode)?;
    let mut declared: Vec<(String, Vec<String>)> = Vec::new();
    for i in loaded
        .config
        .instances
        .iter()
        .filter(|i| i.kind() == doze_config::Kind::Module)
    {
        let at = match declared.iter().position(|(t, _)| *t == i.ty) {
            Some(at) => at,
            None => {
                declared.push((i.ty.clone(), Vec::new()));
                declared.len().saturating_sub(1)
            }
        };
        if let Some((_, majors)) = declared.get_mut(at)
            && !i.version.is_empty()
            && !majors.contains(&major(&i.version))
        {
            majors.push(major(&i.version));
        }
    }
    let wanted = chosen(&declared, &u.modules)?;
    let lock_path = loaded.project.lock();
    let shown = crate::project::shown(&lock_path, &ctx.cwd);
    let lock = super::modules::read_lock(&loaded.project)?;
    let modules = Modules {
        registry: ctx.registry.clone(),
        home: ctx.home.clone(),
        triple: crate::platform::triple(),
        protocol: i64::from(doze_module_proto::version::CURRENT.major),
        doze: VERSION.into(),
    };
    let look = u.check || u.dry.dry_run;
    let mut rows = Vec::new();
    for (ty, majors) in &wanted {
        let Some(src) = Source::parse(ty) else {
            continue;
        };
        let from = lock
            .modules
            .get(&src.to_string())
            .map_or_else(|| "-".to_owned(), |p| p.version.clone());
        rows.push(if look {
            inspect(&modules, &src, majors, from)
        } else {
            upgrade(ctx, &modules, &src, majors, from, &lock_path)
        });
    }
    finish(
        Upgraded {
            lock: shown,
            check: u.check,
            modules: rows,
        },
        u,
    )
}

/// The modules to move: the ones named (each must be declared), or every one declared, in doze.hcl's order.
fn chosen(
    declared: &[(String, Vec<String>)],
    named: &[String],
) -> Result<Vec<(String, Vec<String>)>, Diagnostic> {
    if named.is_empty() {
        return Ok(declared.to_vec());
    }
    let types: Vec<String> = declared.iter().map(|(t, _)| t.clone()).collect();
    named
        .iter()
        .map(|n| {
            declared
                .iter()
                .find(|(t, _)| t == n)
                .cloned()
                .ok_or_else(|| {
                    let d = Diagnostic::new(
                        ids::NO_SUCH_SERVICE,
                        format!("doze.hcl declares no {n} module"),
                    );
                    match crate::suggest::nearest(n, &types) {
                        Some(near) => {
                            d.fix(format!("did you mean {near}? doze modules upgrade {near}"))
                        }
                        None => d.fix(format!(
                            "the modules doze.hcl declares: {}",
                            types.join(", ")
                        )),
                    }
                })
        })
        .collect()
}

fn row(src: &Source, pinned: String, selected: String, status: &str, error: String) -> Row {
    Row {
        module: src.name().to_owned(),
        source: src.to_string(),
        pinned,
        selected,
        status: status.into(),
        error,
    }
}

/// `--check` and `--dry-run`: the newest that fits, from the registry's signed index; nothing is downloaded or written.
fn inspect(m: &Modules, src: &Source, majors: &[String], from: String) -> Row {
    let here = Here {
        protocol: m.protocol,
        doze: &m.doze,
    };
    match m.index(src).map(|idx| newest_that_fits(&idx, here, majors)) {
        Ok(Fit::Release(v)) if v != from => row(src, from, v, "upgrade available", String::new()),
        Ok(Fit::Release(v)) => row(src, from, v, "up to date", String::new()),
        Ok(other) => row(
            src,
            from,
            "-".into(),
            "failed",
            m.no_fit(src, majors, &other),
        ),
        Err(e) => row(src, from, "-".into(), "failed", e),
    }
}

/// Resolves `src` again, downloads and verifies the build, and pins it, under the lock's lock.
fn upgrade(
    ctx: &Ctx<'_>,
    m: &Modules,
    src: &Source,
    majors: &[String],
    from: String,
    path: &std::path::Path,
) -> Row {
    let got = doze_fetch::lock::update(&ctx.home, path, Some(VERSION), |l| {
        m.upgrade(src, majors, l)
    });
    match got {
        Ok((old, r)) if old.as_deref() == Some(r.version.as_str()) => {
            row(src, from, r.version, "up to date", String::new())
        }
        Ok((_, r)) => row(src, from, r.version, "upgraded", String::new()),
        Err(e) => row(src, from, "-".into(), "failed", e),
    }
}

fn finish(done: Upgraded, u: &Upgrade) -> Outcome {
    let failed: Vec<&Row> = done
        .modules
        .iter()
        .filter(|r| r.status == "failed")
        .collect();
    let waiting = done
        .modules
        .iter()
        .filter(|r| r.status == "upgrade available")
        .count();
    let problem = if let [first, ..] = failed.as_slice() {
        let mut d = Diagnostic::new(
            ids::UPGRADE_FAILED,
            format!("{} module(s) failed", failed.len()),
        );
        for r in &failed {
            d = d.note(format!("{}: {}", r.module, r.error));
        }
        let network = [
            "fetching",
            "downloading",
            "can't be reached",
            "no answer",
            "no DNS",
        ]
        .iter()
        .any(|w| first.error.contains(w));
        let fix = if network {
            format!(
                "check the network, then: doze modules upgrade {}",
                first.module
            )
        } else {
            format!(
                "doze modules docs {} shows what each release supports",
                first.module
            )
        };
        Some(d.fix(fix))
    } else if u.check && waiting > 0 {
        Some(
            Diagnostic::new(
                ids::UPGRADES_WAITING,
                format!("{waiting} module upgrade(s) available"),
            )
            .fix("doze modules upgrade"),
        )
    } else {
        None
    };
    if u.dry.dry_run && !u.check {
        let mut plan = Plan::new("doze modules upgrade");
        for r in &done.modules {
            match r.status.as_str() {
                "upgrade available" => plan.step(
                    &r.module,
                    Op::Change,
                    format!("{} → {}", r.pinned, r.selected),
                    r.source.clone(),
                ),
                "failed" => plan.step(
                    &r.module,
                    Op::Do,
                    "not known: it would fail",
                    r.error.clone(),
                ),
                _ => plan.unchanged(&r.module),
            }
        }
        return Ok(Done {
            view: View::Plan(plan),
            problem,
        });
    }
    Ok(Done {
        view: View::Upgraded(done),
        problem,
    })
}

impl Upgraded {
    /// Go's table (ENGINE SOURCE PINNED SELECTED STATUS), then what to do next.
    pub(crate) fn human(&self, p: Paint) -> String {
        if self.modules.is_empty() {
            return "no modules declared\n".into();
        }
        let head = ["MODULE", "SOURCE", "PINNED", "SELECTED", "STATUS"];
        let cells = |r: &Row| {
            [
                r.module.clone(),
                r.source.clone(),
                r.pinned.clone(),
                r.selected.clone(),
                r.status.clone(),
            ]
        };
        let mut w = head.map(width);
        for r in &self.modules {
            for (i, c) in cells(r).iter().enumerate() {
                if let Some(x) = w.get_mut(i) {
                    *x = (*x).max(width(c));
                }
            }
        }
        let line = |c: [String; 5]| -> String {
            let mut s: String = c
                .iter()
                .zip(w)
                .take(4)
                .map(|(c, w)| pad(c, w + 2))
                .collect();
            s.push_str(c.get(4).map_or("", String::as_str));
            s.trim_end().to_owned() + "\n"
        };
        let mut out = format!("{}", p.dim(&line(head.map(String::from))));
        for r in &self.modules {
            out.push_str(&line(cells(r)));
        }
        let moved = self.modules.iter().any(|r| r.status == "upgraded");
        if self.check && !self.modules.iter().any(|r| r.status != "up to date") {
            out.push_str("\nall module pins are at their newest compatible releases\n");
        } else if moved {
            out.push_str(&format!(
                "\n{} updated — commit it so the team and CI pick up the same builds\n",
                self.lock
            ));
        }
        out
    }
}
