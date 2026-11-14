//! Extension bundles, made present while the toolchain resolves (the one call that may fetch): doze's catalog through
//! the host's mirror (`pgext-<name>-pg<major>`, checked and pinned in doze.lock like the engine), and a `source`
//! archive from disk (checked against its declared `sha256` and its doze.lock pin, then unpacked once by its hash).
//! On PostgreSQL 14 to 17 each is installed into the toolchain by the ledger's rules; on 18 the server is pointed at
//! the bundle's own folder. Each bundle's folder rides to provision, plan and converge in `Toolchain.tools` as
//! `pgext:<name>`.
//!
//! Better than Go: the bundles are the instance's own, read from its decoded block in `resolve` (Go kept a
//! process-wide map of every block it had decoded, bug 2), in a fixed order (bug 5); a bundle's programs never stand
//! in for the toolchain's (`psql`), only add to it (bug 3); a `source` is pinned by its hash and installed by the
//! ledger (bug 4).

use crate::bundle_read::{Manifest, default_version};
use crate::catalog::{self, Entry};
use crate::ledger::{self, Tree};
use crate::spec::{Ext, PostgresSpec};
use crate::tool;
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::Toolchain;
use doze_module::proto::msg::run::Pin;
use doze_module::{Cx, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The key of a bundle's folder in `Toolchain.tools`.
pub(crate) fn key(name: &str) -> String {
    format!("pgext:{name}")
}

/// The folder of the bundle `name`, when the toolchain has one.
pub(crate) fn root(tc: &Toolchain, name: &str) -> Option<PathBuf> {
    tc.tools.get(&key(name)).map(PathBuf::from)
}

/// The bundle's `bundle.json`, or what a `source` bundle without one implies: its name, and the version its control
/// file makes.
pub(crate) fn manifest(root: &Path, name: &str) -> Manifest {
    let read = std::fs::read_to_string(root.join("bundle.json")).ok();
    let mut m = read
        .and_then(|t| Manifest::parse(&t).ok())
        .unwrap_or_default();
    if m.name.is_empty() {
        m.name = name.to_owned();
    }
    if m.version.is_empty() {
        m.version = control_version(root, name).unwrap_or_default();
    }
    m
}

/// The SQL version the bundle's control file for `create` makes.
pub(crate) fn control_version(root: &Path, create: &str) -> Option<String> {
    let control = root
        .join("share")
        .join("extension")
        .join(format!("{create}.control"));
    std::fs::read_to_string(control)
        .ok()
        .and_then(|t| default_version(&t))
}

pub(crate) fn failed(msg: String) -> Error {
    Error::new(ErrorKind::Unavailable, msg)
}

/// One bundle the instance needs: a catalog entry (at a pin, or its line) or a `source` archive.
#[derive(Debug)]
enum Want {
    Catalog {
        entry: &'static Entry,
        pin: Option<String>,
        optional: bool,
    },
    Source(Ext),
}

/// The bundles the block needs, in a fixed order: each catalog extension's requirements before it, then itself, each
/// bundle once; then the `source` bundles.
fn wanted(exts: Vec<Ext>, major: u32) -> Vec<Want> {
    let pin_of = |name: &str| {
        exts.iter()
            .find(|e| {
                e.source.is_none() && catalog::lookup(&e.name).is_some_and(|r| r.entry.name == name)
            })
            .and_then(|e| e.version.clone())
    };
    let mut out: Vec<Want> = Vec::new();
    let mut add = |entry: &'static Entry, optional: bool| {
        let had = out.iter_mut().find_map(|w| match w {
            Want::Catalog {
                entry: e,
                optional: o,
                ..
            } if e.name == entry.name => Some(o),
            _ => None,
        });
        match had {
            Some(o) => *o = *o && optional,
            None => out.push(Want::Catalog {
                entry,
                pin: pin_of(entry.name),
                optional,
            }),
        }
    };
    for e in exts
        .iter()
        .filter(|e| e.source.is_none() && !catalog::is_contrib(&e.name, major))
    {
        let Some(r) = catalog::lookup(&e.name) else {
            continue;
        };
        for req in catalog::requires_of(&r) {
            if let Some(rr) = catalog::lookup(req).filter(|_| !catalog::is_contrib(req, major)) {
                add(rr.entry, e.optional);
            }
        }
        add(r.entry, e.optional);
    }
    out.extend(
        exts.into_iter()
            .filter(|e| e.source.is_some())
            .map(Want::Source),
    );
    out
}

/// Fetches, checks and installs every bundle the instance needs; the pins for doze.lock.
pub(crate) async fn resolve(
    cx: &Cx<PostgresSpec>,
    tc: &mut Toolchain,
    pins: &[Pin],
    target: &str,
) -> Result<Vec<Pin>> {
    let major = crate::check::major(&cx.instance.version);
    let mut out = Vec::new();
    let mut tree = None;
    for want in wanted(cx.spec.exts(), major) {
        let (name, optional) = match &want {
            Want::Catalog {
                entry, optional, ..
            } => (entry.name.to_owned(), *optional),
            Want::Source(e) => (e.name.clone(), e.optional),
        };
        let got = match &want {
            Want::Catalog { entry, pin, .. } => {
                catalog_bundle(cx, entry, pin.as_deref(), major, pins, target).await
            }
            Want::Source(e) => crate::source::source_bundle(cx, e, pins, target),
        };
        let wins_ties = matches!(want, Want::Source(_));
        let placed = match got {
            Ok((root, pin, m)) => match crate::preload::refused(&m) {
                Some(why) => Err(failed(format!("extension bundle {name}: {why}"))),
                None => install(tc, &mut tree, major, &root, &m, wins_ties)
                    .await
                    .map(|notes| (root, pin, notes)),
            },
            Err(e) => Err(e),
        };
        match placed {
            Ok((root, pin, notes)) => {
                for n in notes {
                    cx.log(2, format!("extension {name}: placed {n} in the toolchain"));
                }
                add_tools(cx, tc, &root);
                tc.tools
                    .insert(key(&name), root.to_string_lossy().into_owned());
                out.push(pin);
            }
            Err(e) if optional => cx.log(1, format!("extension {name:?} skipped: {}", e.message)),
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// A catalog bundle through the host: the lock's pin, else an exact release as given, else the line's newest.
async fn catalog_bundle(
    cx: &Cx<PostgresSpec>,
    entry: &'static Entry,
    pin: Option<&str>,
    major: u32,
    pins: &[Pin],
    target: &str,
) -> Result<(PathBuf, Pin, Manifest)> {
    let spec = pin.unwrap_or_else(|| entry.line(major)).to_owned();
    let engine = entry.key(major);
    let locked = pins.iter().find(|p| p.engine == engine && p.spec == spec);
    let full = match locked {
        Some(p) => p.resolved.clone(),
        None if spec.matches('.').count() == 2 => spec.clone(),
        None => cx
            .fetch()
            .resolve_major(&engine, &spec)
            .await
            .map_err(|e| {
                failed(format!(
                    "extension {} {spec} for PostgreSQL {major} ({engine}): {}",
                    entry.name, e.message
                ))
            })?,
    };
    let expected = locked
        .and_then(|p| p.hashes.get(target))
        .map(String::as_str);
    let got = cx
        .fetch()
        .ensure(&engine, &full, target, expected)
        .await
        .map_err(|e| {
            failed(format!(
                "fetching extension {} {full} for PostgreSQL {major} ({engine}): {}",
                entry.name, e.message
            ))
        })?;
    let root = Path::new(&got.bin_dir)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let m = catalog_manifest(&root, &engine, &full, entry, major)?;
    let pin = Pin {
        engine,
        spec,
        resolved: full,
        hashes: BTreeMap::from([(target.to_owned(), got.digest)]),
    };
    Ok((root, pin, m))
}

/// A catalog bundle's bundle.json, which must be there and readable (Go refused it otherwise), for this extension and
/// this major, with its control file.
fn catalog_manifest(
    root: &Path,
    engine: &str,
    full: &str,
    entry: &Entry,
    major: u32,
) -> Result<Manifest> {
    let text = std::fs::read_to_string(root.join("bundle.json")).map_err(|e| {
        failed(format!(
            "extension bundle {engine} {full} has no bundle.json: {e}"
        ))
    })?;
    let mut m = Manifest::parse(&text)
        .map_err(|e| failed(format!("extension bundle {engine} {full}: {e}")))?;
    // The release, as Go recorded it: a control file's version is only the SQL one (pg_cron 1.6.4 makes 1.6).
    if m.version.is_empty() {
        full.clone_into(&mut m.version);
    }
    if m.name.is_empty() {
        m.name = entry.name.to_owned();
    }
    let wrong = if m.name != entry.name {
        Some(format!("holds {:?}, not {}", m.name, entry.name))
    } else if m.pg_major != 0 && m.pg_major != u64::from(major) {
        Some(format!(
            "is built for PostgreSQL {}, not {major}",
            m.pg_major
        ))
    } else {
        control_version(root, entry.name)
            .is_none()
            .then(|| format!("has no share/extension/{}.control", entry.name))
    };
    if let Some(w) = wrong {
        return Err(failed(format!("extension bundle {engine} {full} {w}")));
    }
    Ok(m)
}

/// On 14 to 17, the bundle copied into the toolchain (its folders asked of `pg_config` once).
async fn install(
    tc: &Toolchain,
    tree: &mut Option<Tree>,
    major: u32,
    root: &Path,
    m: &Manifest,
    wins_ties: bool,
) -> Result<Vec<String>> {
    if major >= 18 {
        return Ok(vec![]);
    }
    let t = match tree {
        Some(t) => t.clone(),
        None => {
            let pg_config = tool(tc, "pg_config");
            let dir = |flag: &'static str| {
                let p = pg_config.clone();
                async move {
                    doze_module::help::run_tool(&p, &[flag])
                        .await
                        .map(|o| PathBuf::from(o.trim()))
                }
            };
            let t = Tree {
                share: dir("--sharedir").await?,
                pkglib: dir("--pkglibdir").await?,
            };
            *tree = Some(t.clone());
            t
        }
    };
    ledger::install(&t, root, m, wins_ties).map_err(failed)
}

/// A bundle's own programs (pg_repack's client) as tools, unless the toolchain has one of that name: a bundle adds to
/// the toolchain and never stands in for its `psql` (Go let any bundle's `bin/` override any tool, bug 3).
fn add_tools(cx: &Cx<PostgresSpec>, tc: &mut Toolchain, root: &Path) {
    let Ok(list) = std::fs::read_dir(root.join("bin")) else {
        return;
    };
    let mut names: Vec<(String, PathBuf)> = list
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, _)| !n.starts_with('.'))
        .collect();
    names.sort();
    for (n, p) in names {
        let program = n
            .strip_suffix(std::env::consts::EXE_SUFFIX)
            .filter(|s| !s.is_empty())
            .unwrap_or(&n);
        if tool(tc, program).exists() || tc.tools.contains_key(program) {
            cx.log(
                1,
                format!(
                    "{}: a bundle's {program} does not replace the toolchain's",
                    root.display()
                ),
            );
            continue;
        }
        tc.tools
            .insert(program.to_owned(), p.to_string_lossy().into_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn ext(name: &str, version: Option<&str>, optional: bool) -> Ext {
        Ext {
            name: name.into(),
            version: version.map(Into::into),
            schema: None,
            source: None,
            sha256: None,
            optional,
            cascade: false,
        }
    }

    fn names(w: &[Want]) -> Vec<String> {
        w.iter()
            .map(|w| match w {
                Want::Catalog {
                    entry,
                    pin,
                    optional,
                } => format!("{}@{pin:?}/{optional}", entry.name),
                Want::Source(e) => format!("source {}", e.name),
            })
            .collect()
    }

    #[test]
    fn requirements_first_each_bundle_once_in_declared_order() {
        let mut src = ext("mine", None, false);
        src.source = Some("m.tar.gz".into());
        let exts = vec![
            ext("vectorscale", None, false),
            src,
            ext("pg_trgm", None, false),
            ext("postgis_tiger_geocoder", None, true),
            ext("postgis_raster", None, false),
            ext("pgvector", Some("0.8.1"), false),
        ];
        assert_eq!(
            names(&wanted(exts, 18)),
            [
                "vector@Some(\"0.8.1\")/false",
                "vectorscale@None/false",
                "postgis@None/false",
                "source mine"
            ]
        );
    }

    #[test]
    fn keys_and_lock_engines() {
        assert_eq!(key("vector"), "pgext:vector");
        assert_eq!(crate::source::source_engine("mine"), "pgext-source-mine");
    }
}
