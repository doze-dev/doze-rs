//! `doze modules`: each module pinned in `doze.lock`, and the newest release that fits this doze, from this machine's
//! copies of the signed indexes (a read command never waits on the network, rule 5: a missing or day-old copy is
//! fetched in the background for next time, and the answer says it is partial). Exits 0; 6 no config; 11 partial.
//!
//! "Fits" here is what can be known without the core: the release speaks this doze's module protocol, this doze is
//! new enough for it, and it keeps every engine major the pin runs. The engine versions doze.hcl itself declares come
//! with the core, and with them the "used by" column.

use crate::ctx::{Ctx, VERSION};
use crate::diag::{Diagnostic, Span, ids};
use crate::lock::{self, Lock};
use crate::output::{Done, Mode, Outcome};
use crate::paint::{Paint, pad, width};
use crate::project::{self, Project};
use crate::registry::Cache;
use crate::registry::index;
use crate::registry::select::{Fit, Here, compare_versions, newest_that_fits};
use crate::registry::source::Source;
use crate::view::View;
use serde::Serialize;
use std::time::Duration;

/// One pinned module.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub module: String,
    pub pinned: String,
    /// The newest release that fits, when it is known and there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newest: Option<String>,
    /// Said instead when there is none, or it is not known.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// What `doze modules` shows.
#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    pub lock: String,
    pub modules: Vec<Row>,
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode) -> Outcome {
    let project = find(ctx, mode)?;
    let lock = read_lock(&project)?;
    let cache = Cache::new(&ctx.home);
    let here = Here {
        protocol: i64::from(doze_module_proto::version::CURRENT.major),
        doze: VERSION,
    };
    let (mut rows, mut refresh, mut unknown) = (Vec::new(), Vec::new(), Vec::new());
    for (source, pin) in &lock.modules {
        // A key that is not a module source (a hand edit) names no file: it is listed as not known.
        let src = Source::parse(source);
        let short = src.as_ref().map_or_else(|| source.clone(), Source::short);
        let copy = src
            .as_ref()
            .and_then(|s| cache.read(&s.file("index.yaml"), ctx.now));
        if copy
            .as_ref()
            .is_none_or(|(_, age)| *age > Duration::from_secs(86_400))
        {
            refresh.extend(src.as_ref().map(ToString::to_string));
        }
        let idx = copy
            .and_then(|(text, _)| index::parse(&text).ok())
            .filter(|i| index::verify(i, &ctx.registry.key).is_ok());
        let (newest, note) = match idx.map(|i| newest_that_fits(&i, here, &pin.engines)) {
            Some(Fit::Release(v)) => (Some(v), String::new()),
            Some(Fit::NeedsDoze(need)) => (None, format!("newer ones need doze {need}")),
            Some(Fit::Protocol { .. }) => (None, "none for this doze yet (module protocol)".into()),
            Some(Fit::Engines | Fit::Empty) => (None, "none".into()),
            None => {
                unknown.push(short.clone());
                (None, "not known yet".into())
            }
        };
        rows.push(Row {
            module: short,
            pinned: pin.version.clone(),
            newest,
            note,
        });
    }
    if !refresh.is_empty() {
        let mut args = vec!["__refresh"];
        for s in &refresh {
            args.extend(["--module", s.as_str()]);
        }
        let _ = ctx.host.spawn_background(&args);
    }
    let listing = Listing {
        lock: project.lock().display().to_string(),
        modules: rows,
    };
    let problem = (!unknown.is_empty()).then(|| {
        Diagnostic::new(
            ids::NOT_ALL_CHECKED,
            format!(
                "the newest release of {} is not known: this machine has no copy of its index",
                unknown.join(", ")
            ),
        )
        .fix("it is being fetched in the background: run doze modules again in a moment")
    });
    Ok(Done {
        view: View::Modules(listing),
        problem,
    })
}

/// The project, or "no doze.hcl" (exit 6).
pub(crate) fn find(ctx: &Ctx<'_>, mode: &Mode) -> Result<Project, Diagnostic> {
    project::find(&ctx.cwd, mode.config.as_deref()).ok_or_else(|| match &mode.config {
        Some(c) => Diagnostic::new(
            ids::NO_CONFIG,
            format!("there is no doze.hcl at {}", c.display()),
        )
        .fix("check the path given to -c"),
        None => Diagnostic::new(
            ids::NO_CONFIG,
            "there is no doze.hcl here or in any parent directory",
        )
        .fix("doze init writes one · -c <path> points at one elsewhere"),
    })
}

/// The project's lock: empty when there is none, a pointed error when it cannot be read.
pub(crate) fn read_lock(p: &Project) -> Result<Lock, Diagnostic> {
    let path = p.lock();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Lock::default());
    };
    lock::parse(&text).map_err(|e| {
        let d = Diagnostic::new(
            ids::LOCK_UNREADABLE,
            format!("doze.lock can't be read: {}", e.message),
        )
        .fix("resolve it by hand, or delete doze.lock and let doze up pin again");
        match e.at {
            Some(at) => d.span(Span::of(lock::FILE, &text, at, at + 1)),
            None => d,
        }
    })
}

impl Listing {
    pub(crate) fn human(&self, p: Paint) -> String {
        if self.modules.is_empty() {
            return format!(
                "nothing pinned yet · {} pins the modules doze.hcl uses\n",
                p.cmd("doze up")
            );
        }
        let col = |f: fn(&Row) -> &str, title: &str| {
            self.modules
                .iter()
                .map(|r| width(f(r)))
                .chain([width(title)])
                .max()
                .unwrap_or(0)
                + 3
        };
        let (w0, w1) = (col(|r| &r.module, "module"), col(|r| &r.pinned, "pinned"));
        let mut out = format!(
            "{}\n",
            p.dim(&format!(
                "{}{}newest that fits",
                pad("module", w0),
                pad("pinned", w1)
            ))
        );
        let mut waiting = Vec::new();
        for r in &self.modules {
            let newest = match &r.newest {
                Some(v) if compare_versions(v, &r.pinned).is_gt() => {
                    waiting.push(r.module.as_str());
                    p.warn(v).into_owned()
                }
                Some(v) => v.clone(),
                None => p.dim(&r.note).into_owned(),
            };
            out.push_str(&format!(
                "{}{}{newest}\n",
                pad(&r.module, w0),
                pad(&r.pinned, w1)
            ));
        }
        match waiting.len() {
            0 => {}
            n => out.push_str(&format!(
                "  {n} upgrade{} waiting · {}\n",
                if n == 1 { "" } else { "s" },
                p.cmd(&format!("doze modules upgrade {}", waiting.join(" ")))
            )),
        }
        out
    }
}
