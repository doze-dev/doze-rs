//! `doze modules docs <module> [--web]`: a module's description, its example and every argument (type, required or
//! default, which module version brought it), from the registry's `meta.yaml`; the version is the project's pin when
//! there is one, else the stable head of the signed index. `--web` opens the module's page on the registry site.
//! Exits 0; 4 no module by that name; 10 not on this machine and the registry can't be reached.

use super::fetch::{self, Origin, Prefer};
use crate::cli::setup::Docs;
use crate::ctx::{Ctx, VERSION};
use crate::diag::{Diagnostic, ids};
use crate::notice::Notices;
use crate::output::{Done, Mode, Outcome};
use crate::registry::meta::Meta;
use crate::registry::select::{Fit, Here, newest_that_fits};
use crate::registry::source::Source;
use crate::view::View;
use crate::{install::Install, lock, project};
use serde::Serialize;

/// What `modules docs` shows.
#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub module: String,
    pub version: String,
    /// The project pins this version.
    pub pinned: bool,
    /// The engine majors this version runs.
    pub engines: Vec<String>,
    #[serde(flatten)]
    pub meta: Meta,
    pub web: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_seconds_ago: Option<u64>,
}

/// The module's page on the registry site.
pub(crate) fn web_url(src: &Source) -> String {
    format!("{}/{}/{}/", crate::registry::WEB, src.ns(), src.name())
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode, args: &Docs, notices: &mut Notices) -> Outcome {
    let Some(src) = Source::parse(&args.module) else {
        return Err(Diagnostic::new(
            ids::USAGE,
            format!("`{}` is not a module name", args.module),
        )
        .fix("a module is named like valkey, or namespace/module"));
    };
    if args.web {
        let url = web_url(&src);
        return match ctx.host.open_url(&url) {
            Ok(()) => Ok(Done::ok(View::Opened(url))),
            Err(e) => Err(Diagnostic::new(
                ids::NO_BROWSER,
                format!("the browser could not be opened ({e})"),
            )
            .fix(format!("open it yourself: {url}"))),
        };
    }
    let m = fetch::module(ctx, &src, true, Prefer::Cache)?;
    let pin = project::find(&ctx.cwd, mode.config.as_deref())
        .and_then(|p| std::fs::read_to_string(p.lock()).ok())
        .and_then(|t| lock::parse(&t).ok())
        .and_then(|l| l.modules.get(&src.to_string()).cloned());
    let (version, pinned) = match &pin {
        Some(p) if m.index.releases.contains_key(&p.version) => (p.version.clone(), true),
        _ => (m.index.stable().unwrap_or_default().to_string(), false),
    };
    let engines = m
        .index
        .releases
        .get(&version)
        .map(|r| r.engines.clone())
        .unwrap_or_default();
    tell_fit(ctx, &src, &m.index, &version, &engines, notices);
    let (fetched_from, cached_seconds_ago) = match m.origin {
        Origin::Fetched { from } => (Some(from), None),
        Origin::Cached { age } => (None, Some(age.as_secs())),
    };
    Ok(Done::ok(View::Docs(Box::new(Page {
        module: src.to_string(),
        version,
        pinned,
        engines,
        meta: m.meta.unwrap_or_default(),
        web: web_url(&src),
        fetched_from,
        cached_seconds_ago,
    }))))
}

/// The second level of telling (compat.md rule 4): only when what they reach for will not run on this doze.
fn tell_fit(
    ctx: &Ctx<'_>,
    src: &Source,
    idx: &crate::registry::index::Index,
    shown: &str,
    engines: &[String],
    notices: &mut Notices,
) {
    let here = Here {
        protocol: i64::from(doze_module_proto::version::CURRENT.major),
        doze: VERSION,
    };
    let name = src.short();
    match newest_that_fits(idx, here, engines) {
        Fit::Release(v) if v == shown => {}
        Fit::Release(v) => notices.add(format!("this doze would run {name} {v}")),
        Fit::NeedsDoze(need) => {
            let how = Install::of(ctx.host.exe().as_deref()).how();
            notices.add(format!(
                "{name} {shown} needs doze {need} (you have {VERSION}) · {how}"
            ));
        }
        Fit::Protocol { min, max } => {
            let speaks = if min == max {
                min.to_string()
            } else {
                format!("{min} to {max}")
            };
            notices.add(format!(
                "no {name} release runs on this doze yet: they speak module protocol {speaks}, this doze speaks {}",
                here.protocol
            ));
        }
        Fit::Engines | Fit::Empty => {}
    }
}
