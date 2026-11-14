//! Extensions at converge: each declared one created, requirements first; one from a bundle at the version its bundle
//! makes, and an older one in the database moved up to it (`ALTER EXTENSION … UPDATE`, PostGIS's own upgrade call); a
//! contrib one at its declared version; each moved to its declared schema. A server running without a library the
//! bundles need preloaded is told to restart, rather than left to fail obscurely at CREATE EXTENSION.

use crate::bundle_read::compare;
use crate::psql::{Psql, wrap};
use crate::spec::{Ext, PostgresSpec};
use crate::{bundle, catalog, check, ext, preload, sql};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::Toolchain;
use doze_module::{Cx, Result};
use std::cmp::Ordering;

fn unavailable(msg: String) -> Error {
    Error::new(ErrorKind::Unavailable, msg)
}

/// The declared extensions with what each requires before it (a requirement nobody declared is created as is), each
/// once.
fn ordered(exts: &[Ext], major: u32) -> Vec<Ext> {
    let mut out: Vec<Ext> = Vec::new();
    for e in exts {
        let r = catalog::lookup(&e.name)
            .filter(|_| e.source.is_none() && !catalog::is_contrib(&e.name, major));
        let reqs = r.map(|r| catalog::requires_of(&r)).unwrap_or_default();
        for req in reqs {
            if !out.iter().any(|o| o.name == req) {
                let declared = exts.iter().find(|d| d.name == req).cloned();
                out.push(declared.unwrap_or_else(|| Ext {
                    name: req.to_owned(),
                    version: None,
                    schema: None,
                    source: None,
                    sha256: None,
                    optional: e.optional,
                    cascade: false,
                }));
            }
        }
        if !out.iter().any(|o| o.name == e.name) {
            out.push(e.clone());
        }
    }
    out
}

/// The version a bundled extension is created at: a `source` one's declared version or its control file's, a catalog
/// one's control file's (its declared version picks the bundle). `None` for contrib.
fn target(tc: &Toolchain, e: &Ext, major: u32) -> Result<Option<String>> {
    let (bundle_name, create) = match (&e.source, catalog::lookup(&e.name)) {
        (Some(_), _) => (e.name.clone(), e.name.clone()),
        (None, Some(r)) if !catalog::is_contrib(&e.name, major) => {
            (r.entry.name.to_owned(), r.create.to_owned())
        }
        _ => return Ok(None),
    };
    let Some(root) = bundle::root(tc, &bundle_name) else {
        return Err(unavailable(format!(
            "its bundle was not fetched for PostgreSQL {major}: doze fetches bundles when it resolves the toolchain, so \
             restart the instance (doze down, then doze up)"
        )));
    };
    if let (Some(_), Some(v)) = (&e.source, &e.version) {
        return Ok(Some(v.clone()));
    }
    bundle::control_version(&root, &create)
        .map(Some)
        .ok_or_else(|| unavailable(format!("its bundle has no default_version for {create}")))
}

/// Created at `target`, or moved up to it; never down.
async fn at_version(
    cx: &Cx<PostgresSpec>,
    p: &Psql,
    db: &str,
    e: &Ext,
    target: &str,
) -> Result<()> {
    p.exec(db, &[ext::create_at(e, target)]).await?;
    let q = format!(
        "SELECT extversion FROM pg_extension WHERE extname = {}",
        sql::lit(&e.name)
    );
    let now = p
        .column(db, &q)
        .await?
        .into_iter()
        .next()
        .unwrap_or_default();
    match compare(target, &now) {
        Ordering::Equal => return Ok(()),
        Ordering::Less => {
            return Err(unavailable(format!(
                "it is at {now} in database {db}, newer than the {target} its bundle makes; PostgreSQL cannot move an \
                 extension down, so pin a version at or above {now}"
            )));
        }
        Ordering::Greater => {}
    }
    if e.name == "postgis" {
        // One call moves postgis and every postgis_* extension of the database.
        p.exec(db, &["SELECT postgis_extensions_upgrade()".to_owned()])
            .await?;
        if p.column(db, &q).await?.first().is_some_and(|v| v == target) {
            cx.log(
                2,
                format!("extension {} in {db}: {now} to {target}", e.name),
            );
            return Ok(());
        }
    }
    let update = format!(
        "ALTER EXTENSION {} UPDATE TO {}",
        sql::ident(&e.name),
        sql::lit(target)
    );
    p.exec(db, &[update]).await?;
    cx.log(
        2,
        format!("extension {} in {db}: {now} to {target}", e.name),
    );
    Ok(())
}

/// One extension: created or updated, and moved if its declared schema is another.
async fn extension(cx: &Cx<PostgresSpec>, p: &Psql, tc: &Toolchain, e: &Ext) -> Result<()> {
    let (db, major) = (&cx.instance.name, check::major(&cx.instance.version));
    let version = target(tc, e, major)?;
    let q = format!(
        "SELECT EXISTS(SELECT 1 FROM pg_available_extensions WHERE name = {})",
        sql::lit(&e.name)
    );
    if !p.yes(db, &q).await? {
        return Err(unavailable(
            "neither this PostgreSQL build nor its bundle has it".into(),
        ));
    }
    match version {
        Some(v) => at_version(cx, p, db, e, &v).await?,
        None => p.exec(db, &ext::statements(e)).await?,
    }
    let Some(want) = &e.schema else { return Ok(()) };
    let q = format!(
        "SELECT n.nspname FROM pg_extension x JOIN pg_namespace n ON n.oid = x.extnamespace WHERE x.extname = {}",
        sql::lit(&e.name)
    );
    if p.column(db, &q)
        .await?
        .first()
        .is_some_and(|now| now != want)
    {
        let moved = format!(
            "ALTER EXTENSION {} SET SCHEMA {}",
            sql::ident(&e.name),
            sql::ident(want)
        );
        p.exec(db, &[moved]).await?;
    }
    Ok(())
}

/// The libraries the bundles need preloaded must be loaded in the running server: they load at start only.
async fn preloaded(cx: &Cx<PostgresSpec>, p: &Psql, tc: &Toolchain) -> Result<()> {
    let (name, major) = (&cx.instance.name, check::major(&cx.instance.version));
    let want = preload::ext_conf(&cx.spec, tc, major, name);
    if want.preload.is_empty() {
        return Ok(());
    }
    let have = p
        .column("postgres", "SHOW shared_preload_libraries")
        .await?;
    let missing = want.missing(&have.join(","));
    if missing.is_empty() {
        return Ok(());
    }
    Err(unavailable(format!(
        "instance {name:?} is running without {} in shared_preload_libraries, which its extensions need; doze has \
         written it to doze.conf, so restart the instance (doze down, then doze up) to load it",
        missing.join(", ")
    )))
}

/// Every declared extension, requirements first; an `optional` one that fails is skipped with a warning.
pub(crate) async fn extensions(cx: &Cx<PostgresSpec>, p: &Psql, tc: &Toolchain) -> Result<()> {
    preloaded(cx, p, tc).await?;
    let major = check::major(&cx.instance.version);
    for e in ordered(&cx.spec.exts(), major) {
        match extension(cx, p, tc, &e).await {
            Ok(()) => {}
            Err(why) if e.optional => cx.log(
                1,
                format!("extension {:?} skipped: {}", e.name, why.message),
            ),
            Err(why) => return Err(wrap(format!("extension {:?}", e.name))(why)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext(name: &str) -> Ext {
        Ext {
            name: name.into(),
            version: None,
            schema: None,
            source: None,
            sha256: None,
            optional: false,
            cascade: false,
        }
    }

    #[test]
    fn requirements_are_created_first_and_a_declared_one_keeps_its_arguments() {
        let mut fuzzy = ext("fuzzystrmatch");
        fuzzy.schema = Some("x".into());
        let got = ordered(&[ext("postgis_tiger_geocoder"), fuzzy, ext("hstore")], 18);
        let names: Vec<&str> = got.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "postgis",
                "fuzzystrmatch",
                "postgis_tiger_geocoder",
                "hstore"
            ]
        );
        assert_eq!(got[1].schema.as_deref(), Some("x"));
    }
}
