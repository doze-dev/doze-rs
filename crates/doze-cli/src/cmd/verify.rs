//! `doze modules verify`: every pin in `doze.lock` checked again against the signed index, fetched fresh: the index's
//! signature (with the key built into doze), each platform's sha256 the lock pins against the index's, each artifact's
//! own signature, and this machine's downloaded copy against the pin. Exits 0 all verified; 1 a mismatch (named, with
//! the fix); 10 the registry can't be reached and nothing could be checked; 11 some checked against this machine's
//! copy of the index only.

use super::fetch::{self, Origin, Prefer};
use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::lock::Pin;
use crate::output::{Done, Mode, Outcome};
use crate::paint::{Paint, pad, width};
use crate::platform;
use crate::registry::index::Release;
use crate::registry::signed;
use crate::registry::source::Source;
use crate::view::View;
use serde::Serialize;
use std::path::Path;

/// How one pin came out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Verified,
    /// Verified, against this machine's copy of the index (the registry did not answer).
    VerifiedOffline,
    Mismatch,
    NotChecked,
}

#[derive(Debug, Clone, Serialize)]
pub struct Checked {
    pub module: String,
    pub version: String,
    pub state: State,
    pub detail: String,
}

/// What `modules verify` shows.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub key: String,
    pub platform: String,
    pub modules: Vec<Checked>,
}

pub(crate) fn run(ctx: &mut Ctx<'_>, mode: &Mode) -> Outcome {
    let project = super::modules::find(ctx, mode)?;
    let lock = super::modules::read_lock(&project)?;
    let mut modules = Vec::new();
    for (source, pin) in &lock.modules {
        modules.push(check(ctx, source, pin));
    }
    let report = Report {
        key: signed::fingerprint(&ctx.registry.key),
        platform: platform::triple(),
        modules,
    };
    let problem = verdict(&report);
    Ok(Done {
        view: View::Verify(report),
        problem,
    })
}

fn check(ctx: &mut Ctx<'_>, source: &str, pin: &Pin) -> Checked {
    let out = |state, detail: String| Checked {
        module: Source::parse(source).map_or_else(|| source.to_string(), |s| s.short()),
        version: pin.version.clone(),
        state,
        detail,
    };
    let Some(src) = Source::parse(source) else {
        return out(
            State::Mismatch,
            format!("`{source}` is not a module source"),
        );
    };
    let m = match fetch::module(ctx, &src, false, Prefer::Network) {
        Ok(m) => m,
        Err(d) if d.id == ids::REGISTRY_UNREACHABLE => return out(State::NotChecked, d.message),
        Err(d) => return out(State::Mismatch, d.message),
    };
    let Some(rel) = m.index.releases.get(&pin.version) else {
        return out(
            State::Mismatch,
            format!("the signed index no longer lists {}", pin.version),
        );
    };
    if pin.hashes.is_empty() {
        return out(
            State::NotChecked,
            "doze.lock records no hashes for it".into(),
        );
    }
    if let Err(why) = builds(rel, pin, &ctx.registry.key) {
        return out(State::Mismatch, why);
    }
    let here = platform::triple();
    let copy_says = match this_copy(&ctx.home, &src, pin, &here) {
        Ok(says) => says,
        Err(why) => return out(State::Mismatch, why),
    };
    let sha = pin
        .hashes
        .get(&here)
        .map(|h| format!("sha256 {} ", short(h)))
        .unwrap_or_default();
    match m.origin {
        Origin::Fetched { .. } => out(
            State::Verified,
            format!("{sha}matches the signed index {copy_says}"),
        ),
        Origin::Cached { .. } => out(
            State::VerifiedOffline,
            format!("{sha}matches this machine's copy of the signed index {copy_says}"),
        ),
    }
}

/// `4d4c…7f13`.
fn short(h: &str) -> String {
    let h = h.strip_prefix("sha256:").unwrap_or(h);
    match (h.get(..4), h.get(h.len().saturating_sub(4)..)) {
        (Some(a), Some(b)) if h.len() > 8 => format!("{a}…{b}"),
        _ => h.to_string(),
    }
}

fn verdict(r: &Report) -> Option<Diagnostic> {
    let named = |s: State| -> Vec<&str> {
        r.modules
            .iter()
            .filter(|m| m.state == s)
            .map(|m| m.module.as_str())
            .collect()
    };
    let bad = named(State::Mismatch);
    if !bad.is_empty() {
        return Some(
            Diagnostic::new(
                ids::VERIFY_MISMATCH,
                format!(
                    "{}: doze.lock does not match the signed registry",
                    bad.join(", ")
                ),
            )
            .fix(format!(
                "doze modules upgrade {} pins again from the index, or restore doze.lock from git",
                bad.join(" ")
            )),
        );
    }
    let unchecked = named(State::NotChecked);
    let verified = r.modules.len() - unchecked.len();
    if !unchecked.is_empty() && verified == 0 {
        return Some(
            Diagnostic::new(
                ids::REGISTRY_UNREACHABLE,
                "nothing could be verified: the module registry can't be reached",
            )
            .fix("check the network and run it again · doze doctor checks the registry"),
        );
    }
    let offline = named(State::VerifiedOffline);
    let mut short = Vec::new();
    if !offline.is_empty() {
        short.push(format!(
            "{} checked only against this machine's copy of the index",
            offline.join(", ")
        ));
    }
    if !unchecked.is_empty() {
        short.push(format!("{} not checked", unchecked.join(", ")));
    }
    (!short.is_empty()).then(|| {
        Diagnostic::new(
            ids::NOT_ALL_CHECKED,
            format!("the registry can't be reached: {}", short.join("; ")),
        )
        .fix("run it again when the registry can be reached")
    })
}

impl Report {
    pub(crate) fn human(&self, p: Paint) -> String {
        let mut out = format!(
            "registry key ed25519 {} {}\n",
            self.key,
            p.dim("(built into this doze)")
        );
        if self.modules.is_empty() {
            out.push_str(&format!(
                "nothing pinned yet · {} pins the modules doze.hcl uses\n",
                p.cmd("doze up")
            ));
            return out;
        }
        let w = self
            .modules
            .iter()
            .map(|m| width(&m.module))
            .max()
            .unwrap_or(0)
            + 1;
        let v = self
            .modules
            .iter()
            .map(|m| width(&m.version))
            .max()
            .unwrap_or(0)
            + 2;
        for m in &self.modules {
            let mark = match m.state {
                State::Verified => p.ok("✓"),
                State::VerifiedOffline | State::NotChecked => p.warn("!"),
                State::Mismatch => p.fail("✗"),
            };
            out.push_str(&format!(
                "{mark} {}{}{}\n",
                pad(&m.module, w),
                pad(&m.version, v),
                m.detail
            ));
        }
        let ok = self
            .modules
            .iter()
            .filter(|m| matches!(m.state, State::Verified | State::VerifiedOffline))
            .count();
        out.push_str(&format!(
            "  {ok} of {} verified on {}\n",
            self.modules.len(),
            self.platform
        ));
        out
    }
}

/// Each platform's build the lock pins: the same sha256 as the signed index, and the build's own signature good.
fn builds(rel: &Release, pin: &Pin, key: &ed25519_dalek::VerifyingKey) -> Result<(), String> {
    for (triple, pinned) in &pin.hashes {
        let pinned = pinned.strip_prefix("sha256:").unwrap_or(pinned);
        let Some(art) = rel.artifacts.get(triple) else {
            return Err(format!(
                "{triple}: pinned, but the signed index has no build for it"
            ));
        };
        if !art.sha256.eq_ignore_ascii_case(pinned) {
            let (a, b) = (short(pinned), short(&art.sha256));
            return Err(format!(
                "{triple}: doze.lock pins sha256 {a}, the signed index says {b}"
            ));
        }
        signed::verify_hex(key, &art.sha256, &art.sig)
            .map_err(|why| format!("{triple}: the build is refused: {why}"))?;
    }
    Ok(())
}

/// This machine's download of the pinned build (the digest the Go doze records beside it), against the pin.
fn this_copy(home: &Path, src: &Source, pin: &Pin, here: &str) -> Result<&'static str, String> {
    let copy = home
        .join("modules")
        .join(src.ns())
        .join(src.name())
        .join(format!("{}-{here}", pin.version));
    let Ok(digest) = std::fs::read_to_string(copy.join(".doze-archive-sha")) else {
        return Ok("(not downloaded here)");
    };
    match pin.hashes.get(here) {
        Some(p) if digest.trim() != p.trim() => Err(format!(
            "this machine's copy is {}, the lock pins {} · delete {} and doze fetches it again",
            short(digest.trim()),
            short(p),
            copy.display()
        )),
        _ => Ok("and this machine's copy"),
    }
}
