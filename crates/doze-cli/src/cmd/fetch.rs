//! A module's registry files for a command, by rule 5: a read command uses this machine's copy and refreshes it in
//! the background when it is a day old; only a module this machine has never seen goes out, within the 3 s budget and
//! with a progress line on a terminal. Every index is checked against the registry key, the cached copy included.

use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::registry::index::{self, Index};
use crate::registry::meta::{self, Meta};
use crate::registry::source::Source;
use crate::registry::{Cache, FetchError};
use std::time::Duration;

const STALE: Duration = Duration::from_secs(86_400);

/// Where a file came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    Fetched { from: String },
    Cached { age: Duration },
}

/// What to do when this machine has a copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Prefer {
    /// Use it (read commands).
    Cache,
    /// Ask the registry first; the copy is the fallback (`modules verify`).
    Network,
}

/// A module's verified index and its documentation.
#[derive(Debug)]
pub(crate) struct Module {
    pub index: Index,
    pub meta: Option<Meta>,
    pub origin: Origin,
}

/// Loads `src`'s index (and, with `docs`, its documentation).
pub(crate) fn module(
    ctx: &mut Ctx<'_>,
    src: &Source,
    docs: bool,
    prefer: Prefer,
) -> Result<Module, Diagnostic> {
    let cache = Cache::new(&ctx.home);
    if prefer == Prefer::Cache
        && let Some((m, age)) = cached(ctx, &cache, src, docs)
    {
        if age > STALE {
            let _ = ctx
                .host
                .spawn_background(&["__refresh", "--module", &src.to_string()]);
        }
        return Ok(m);
    }
    match fetch(ctx, &cache, src, docs) {
        Ok(m) => Ok(m),
        Err(Failure::Unreachable(why)) => match cached(ctx, &cache, src, docs) {
            Some((m, _)) => Ok(m),
            None => Err(unreachable(src, &why)),
        },
        Err(Failure::Refused(d)) => Err(d),
    }
}

enum Failure {
    Unreachable(FetchError),
    Refused(Diagnostic),
}

/// This machine's copy, if it has a whole one that still verifies.
fn cached(ctx: &Ctx<'_>, cache: &Cache, src: &Source, docs: bool) -> Option<(Module, Duration)> {
    let (text, age) = cache.read(&src.file("index.yaml"), ctx.now)?;
    let idx = index::parse(&text).ok()?;
    index::verify(&idx, &ctx.registry.key).ok()?;
    let meta = if docs {
        Some(meta::parse(&cache.read(&src.file("meta.yaml"), ctx.now)?.0).ok()?)
    } else {
        None
    };
    Some((
        Module {
            index: idx,
            meta,
            origin: Origin::Cached { age },
        },
        age,
    ))
}

fn fetch(ctx: &mut Ctx<'_>, cache: &Cache, src: &Source, docs: bool) -> Result<Module, Failure> {
    let progress = ctx.term.stderr;
    if progress {
        let _ = write!(ctx.err, "fetching {} from the registry…", src.short());
        let _ = ctx.err.flush();
    }
    let got = fetch_files(ctx, cache, src, docs);
    if progress {
        let _ = write!(ctx.err, "\r\x1b[K");
    }
    got
}

fn fetch_files(ctx: &Ctx<'_>, cache: &Cache, src: &Source, docs: bool) -> Result<Module, Failure> {
    let got = match ctx.registry.get(&src.file("index.yaml")) {
        Ok(f) => f,
        Err(FetchError::Missing { .. }) => return Err(Failure::Refused(no_such_module(ctx, src))),
        Err(e) => return Err(Failure::Unreachable(e)),
    };
    let text = String::from_utf8_lossy(&got.body);
    let idx = index::parse(&text).map_err(|e| {
        Failure::Refused(
            Diagnostic::new(
                ids::REGISTRY_UNREADABLE,
                format!(
                    "the registry's index for {} can't be read: {}",
                    src.short(),
                    e.message
                ),
            )
            .note(format!("from {}", got.from))
            .fix("doze bug (this is the registry's problem or doze's, not yours)"),
        )
    })?;
    index::verify(&idx, &ctx.registry.key).map_err(|why| {
        Failure::Refused(
            Diagnostic::new(
                ids::REGISTRY_UNSIGNED,
                format!("the registry's index for {} is refused: {why}", src.short()),
            )
            .note(format!("from {}; nothing from it was used", got.from))
            .fix("do not work around this: report it with doze bug"),
        )
    })?;
    let meta = if docs {
        let m = match ctx.registry.get(&src.file("meta.yaml")) {
            Ok(m) => m,
            Err(FetchError::Missing { from }) => {
                let d = Diagnostic::new(
                    ids::REGISTRY_UNREADABLE,
                    format!("the registry has no documentation for {}", src.short()),
                );
                return Err(Failure::Refused(
                    d.note(format!("{from} serves its index but no meta.yaml")),
                ));
            }
            Err(e) => return Err(Failure::Unreachable(e)),
        };
        let parsed = meta::parse(&String::from_utf8_lossy(&m.body)).map_err(|e| {
            Failure::Refused(Diagnostic::new(
                ids::REGISTRY_UNREADABLE,
                format!(
                    "the registry's documentation for {} can't be read: {}",
                    src.short(),
                    e.message
                ),
            ))
        })?;
        let _ = cache.write(&src.file("meta.yaml"), &m.body);
        Some(parsed)
    } else {
        None
    };
    // Kept only once it verified, so the cache never holds an index the key did not sign.
    let _ = cache.write(&src.file("index.yaml"), &got.body);
    Ok(Module {
        index: idx,
        meta,
        origin: Origin::Fetched { from: got.from },
    })
}

fn unreachable(src: &Source, why: &FetchError) -> Diagnostic {
    Diagnostic::new(
        ids::REGISTRY_UNREACHABLE,
        format!(
            "the module registry can't be reached, and this machine has no copy of {}",
            src.short()
        ),
    )
    .note(why.reasons())
    .fix("check the network and run it again · doze doctor checks the registry")
}

/// "No module called X", with the nearest name the registry (or this machine) knows.
fn no_such_module(ctx: &Ctx<'_>, src: &Source) -> Diagnostic {
    let mut known = Cache::new(&ctx.home).modules(src.ns());
    if let Ok(cat) = ctx.registry.get("index.json")
        && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&cat.body)
        && let Some(mods) = v
            .pointer(&format!("/namespaces/{}/modules", src.ns()))
            .and_then(|m| m.as_object())
    {
        known.extend(mods.keys().cloned());
    }
    let d = Diagnostic::new(
        ids::NO_SUCH_MODULE,
        format!("there is no module called {} in the registry", src.short()),
    );
    match crate::suggest::nearest(src.name(), &known) {
        Some(near) => d.fix(format!("did you mean {near}? doze modules docs {near}")),
        None => d.fix(format!(
            "the registry lists every module: {}/",
            crate::registry::WEB
        )),
    }
}
