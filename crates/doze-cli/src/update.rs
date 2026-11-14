//! The ambient update line (compat.md rule 4): at most one line a day, only on an interactive terminal (never in CI
//! or a pipe, never under `--json` or `-q`), and off with `DOZE_NO_UPDATE_NOTIFIER=1`. It never waits on the network:
//! what it says comes from a state file in the doze home, and the check that fills it runs in the background
//! (`doze __refresh --latest`, detached, at most once a day, within the 3 s budget).

use crate::ctx::{Ctx, VERSION};
use crate::install::Install;
use crate::output::Mode;
use crate::registry::latest::Latest;
use crate::registry::select::{compare_versions, is_doze_version};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The opt-out.
pub const OPT_OUT: &str = "DOZE_NO_UPDATE_NOTIFIER";
/// The state file, in the doze home.
pub const FILE: &str = "update.json";

const DAY: u64 = 86_400;
const HOUR: u64 = 3_600;

/// What the last check found and when the line was last shown (seconds since the epoch).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub checked_at: u64,
    #[serde(default)]
    pub attempted_at: u64,
    #[serde(default)]
    pub latest: Option<Latest>,
    #[serde(default)]
    pub shown_at: u64,
}

fn path(home: &Path) -> PathBuf {
    home.join(FILE)
}

/// The state, or a fresh one when there is none or it cannot be read (it is only a cache).
pub fn load(home: &Path) -> State {
    fs::read(path(home))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Writes the state whole or not at all.
pub fn save(home: &Path, s: &State) -> io::Result<()> {
    fs::create_dir_all(home)?;
    let tmp = home.join(format!("{FILE}.tmp{}", std::process::id()));
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(s).map_err(io::Error::other)?,
    )?;
    fs::rename(&tmp, path(home))
}

/// Whether `latest` is newer than this doze. A development build is older than every release.
pub fn newer(latest: &str, current: &str) -> bool {
    is_doze_version(latest)
        && (!is_doze_version(current)
            || compare_versions(latest, current.trim_start_matches('v')).is_gt())
}

/// The line for this run, if there is one to say; starts the background check when the last is a day old.
pub(crate) fn notice(ctx: &mut Ctx<'_>, mode: &Mode, allowed: bool) -> Option<String> {
    if !allowed || mode.json || mode.quiet || !ctx.interactive() {
        return None;
    }
    if ctx.var(OPT_OUT).is_some() || ctx.var("CI").is_some() {
        return None;
    }
    let now = ctx.unix_now();
    let mut state = load(&ctx.home);
    if now.saturating_sub(state.checked_at) >= DAY && now.saturating_sub(state.attempted_at) >= HOUR
    {
        state.attempted_at = now;
        // A home that cannot be written means no check (the next run would only try again): say nothing.
        if save(&ctx.home, &state).is_ok() {
            let _ = ctx.host.spawn_background(&["__refresh", "--latest"]);
        }
    }
    let latest = state.latest.clone()?;
    if !newer(&latest.version, VERSION) || now.saturating_sub(state.shown_at) < DAY {
        return None;
    }
    state.shown_at = now;
    save(&ctx.home, &state).ok()?;
    let how = Install::of(ctx.host.exe().as_deref()).how();
    Some(line(&latest, how))
}

/// The one line.
pub fn line(l: &Latest, how: &str) -> String {
    let what = if l.security {
        format!(
            "doze {} fixes a security problem (you have {VERSION})",
            l.version
        )
    } else {
        format!("doze {} is out (you have {VERSION})", l.version)
    };
    match l.news.first() {
        Some(news) if !l.security => format!("{what}: {news} · {how}"),
        _ => format!("{what} · {how}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_is_newer_than_an_older_one_and_than_a_dev_build() {
        assert!(newer("0.3.0", "0.2.9") && newer("0.10.0", "v0.9.0") && newer("0.3.0", "dev"));
        assert!(!newer("0.3.0", "0.3.0") && !newer("0.2.0", "0.3.0") && !newer("dev", "0.1.0"));
    }
}
