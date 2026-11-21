//! What a stack started last time (Go cmd/doze/remember.go), so a bare `doze up` can offer the same set again. `up`
//! records what it was asked for, joined with what was already running; `down` adds what was awake. Neither shrinks
//! the set: a service that slept while idle was still part of the stack. The record is the project's `project.json`
//! (doze-projects), the one the Go doze and the app read.
//!
//! Remembering is a convenience: a record that cannot be written is a notice, never the command's failure.

use crate::ctx::Ctx;
use crate::notice::Notices;
use crate::output::Mode;
use doze_config::Config;
use doze_control::Snapshot;
use doze_projects::Project;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

/// Every enabled instance, in declaration order.
pub(crate) fn enabled_names(cfg: &Config) -> Vec<String> {
    cfg.instances
        .iter()
        .filter(|i| i.enabled)
        .map(|i| i.name.clone())
        .collect()
}

/// The targets and everything they need, each after what it needs (Go `bootClosure`). A name doze.hcl does not
/// declare is left out.
pub(crate) fn closure(cfg: &Config, targets: &[String]) -> Vec<String> {
    fn visit(cfg: &Config, name: &str, seen: &mut Vec<String>, out: &mut Vec<String>) {
        if seen.iter().any(|s| s == name) {
            return;
        }
        seen.push(name.to_owned());
        let Some(i) = cfg.instance(name) else { return };
        for d in &i.deps {
            visit(cfg, &d.name, seen, out);
        }
        out.push(name.to_owned());
    }
    let (mut seen, mut out) = (Vec::new(), Vec::new());
    for t in targets {
        visit(cfg, t, &mut seen, &mut out);
    }
    out
}

/// Whether starting `names` starts every enabled instance.
fn is_everything(cfg: &Config, names: &[String]) -> bool {
    closure(cfg, names).len() == closure(cfg, &enabled_names(cfg)).len()
}

/// The names still declared and enabled, in their order.
fn declared(cfg: &Config, names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|n| cfg.instance(n).is_some_and(|i| i.enabled))
        .cloned()
        .collect()
}

/// The record's folder: the config's state folder (Go `ProjectDir`), else the stack key's.
pub(crate) fn record_dir(cfg: &Config, fallback: &Path) -> PathBuf {
    if cfg.data_dir.is_empty() {
        fallback.to_path_buf()
    } else {
        PathBuf::from(&cfg.data_dir)
    }
}

/// Last time's set, keeping only names still declared and enabled. `None` when nothing was recorded, everything was
/// started, or what it starts is everything anyway. A record that cannot be read is an error, so `--last` can say so
/// rather than claim nothing was started.
fn last_set(cfg: &Config, dir: &Path) -> Result<Option<Vec<String>>, doze_projects::Error> {
    let Some(p) = doze_projects::load(dir)? else {
        return Ok(None);
    };
    let keep = declared(cfg, &p.last_set);
    Ok((!keep.is_empty() && !is_everything(cfg, &keep)).then_some(keep))
}

/// What a bare `doze up` starts (Go `chooseUpTargets`): `--all` everything; `--last` last time's set; with nothing
/// smaller recorded, everything; on a terminal with `-y`, last time's set, said; on a terminal, it asks; otherwise
/// everything, with how to get last time's set (a script gets the same stack whatever someone ran before).
pub(crate) fn choose(
    ctx: &mut Ctx<'_>,
    mode: &Mode,
    notices: &mut Notices,
    (cfg, dir): (&Config, &Path),
    (last, all): (bool, bool),
) -> Vec<String> {
    let everything = enabled_names(cfg);
    let prev = match last_set(cfg, dir) {
        Ok(prev) => prev,
        Err(e) => {
            notices.add(format!(
                "could not read last time's set ({e}): starting everything"
            ));
            return everything;
        }
    };
    let can_prompt = !mode.json && ctx.interactive();
    match (prev, all, last) {
        (_, true, _) => everything,
        (None, _, true) => {
            notices.add("nothing smaller was started last time: starting everything");
            everything
        }
        (Some(prev), _, true) => prev,
        (None, _, _) => everything,
        (Some(prev), _, _) if can_prompt && mode.yes => {
            notices.add(format!("starting last time's set: {}", prev.join(", ")));
            prev
        }
        (Some(prev), _, _) if can_prompt => {
            if ask_same(ctx, mode, &prev) {
                prev
            } else {
                everything
            }
        }
        (Some(prev), _, _) => {
            notices.add(format!(
                "last time only {} started; doze up --last starts just those",
                prev.join(", ")
            ));
            everything
        }
    }
}

/// "Last time: …. Start the same?": yes unless the answer says otherwise; the input ending is no.
fn ask_same(ctx: &mut Ctx<'_>, mode: &Mode, prev: &[String]) -> bool {
    let _ = write!(
        ctx.err,
        "Last time: {}. Start the same? {} ",
        prev.join(", "),
        mode.err().dim("[Y/n, a = everything]")
    );
    let _ = ctx.err.flush();
    ctx.host
        .read_line()
        .is_some_and(|line| matches!(line.trim().to_lowercase().as_str(), "" | "y" | "yes"))
}

/// What a snapshot shows running: in any state but asleep.
pub(crate) fn awake_names(snap: Option<&Snapshot>) -> Vec<String> {
    snap.map(|s| {
        s.instances
            .iter()
            .filter(|v| {
                !matches!(
                    v.state.as_str(),
                    "" | "reaped" | "asleep" | "disabled" | "done"
                )
            })
            .map(|v| v.name.clone())
            .collect()
    })
    .unwrap_or_default()
}

/// The names no other of them needs, in declaration order: what to ask for to start them all again.
fn roots_of(cfg: &Config, names: &[String]) -> Vec<String> {
    let needed: Vec<&str> = names
        .iter()
        .filter_map(|n| cfg.instance(n))
        .flat_map(|i| i.deps.iter().map(|d| d.name.as_str()))
        .filter(|d| names.iter().any(|n| n == d))
        .collect();
    enabled_names(cfg)
        .into_iter()
        .filter(|n| names.contains(n) && !needed.contains(&n.as_str()))
        .collect()
}

/// A set that is everything is recorded as empty.
fn as_recorded(cfg: &Config, set: Vec<String>) -> Vec<String> {
    if is_everything(cfg, &set) {
        Vec::new()
    } else {
        set
    }
}

/// Records what `up` was asked to start, joined with what was already running (`snap`), so adding a service to a
/// running stack keeps the rest.
pub(crate) fn remember_up(
    ctx: &Ctx<'_>,
    notices: &mut Notices,
    (cfg, dir): (&Config, &Path),
    names: &[String],
    snap: Option<&Snapshot>,
) {
    let mut all = names.to_vec();
    all.extend(awake_names(snap));
    let set = as_recorded(cfg, roots_of(cfg, &all));
    let now = now(ctx);
    remember(ctx, notices, (cfg, dir), |p| {
        p.last_up = Some(now);
        p.last_set = set;
    });
}

/// Adds what was awake when `down` ran to what `up` recorded: a service woken since counts, and one that slept while
/// idle is not dropped.
pub(crate) fn remember_down(
    ctx: &Ctx<'_>,
    notices: &mut Notices,
    (cfg, dir): (&Config, &Path),
    snap: Option<&Snapshot>,
) {
    let now = now(ctx);
    let awake = awake_names(snap);
    remember(ctx, notices, (cfg, dir), |p| {
        p.last_down = Some(now);
        if awake.is_empty() || p.last_set.is_empty() {
            return; // nothing to add, or everything was started: keep what up recorded
        }
        let mut all = declared(cfg, &p.last_set);
        all.extend(awake);
        p.last_set = as_recorded(cfg, roots_of(cfg, &all));
    });
}

fn remember(
    ctx: &Ctx<'_>,
    notices: &mut Notices,
    (cfg, dir): (&Config, &Path),
    change: impl FnOnce(&mut Project),
) {
    let config = doze_config::paths::clean(&ctx.cwd.join(&cfg.path)); // as Go's `filepath.Abs`
    let written = doze_projects::update(dir, |p| {
        p.config = config.to_string_lossy().into_owned();
        p.name = cfg.stack();
        change(p);
    });
    if let Err(e) = written {
        notices.add(format!("could not remember this run: {e}"));
    }
}

/// The run's clock in this machine's zone, as Go's `time.Now()` is written.
fn now(ctx: &Ctx<'_>) -> OffsetDateTime {
    let utc = OffsetDateTime::from(ctx.now);
    let zone = crate::cmd::logs::zone(ctx);
    let offset = jiff::Timestamp::try_from(ctx.now)
        .map(|t| zone.to_offset(t).seconds())
        .ok()
        .filter(|s| s % 60 == 0)
        .and_then(|s| time::UtcOffset::from_whole_seconds(s).ok());
    offset.map_or(utc, |o| utc.to_offset(o))
}
