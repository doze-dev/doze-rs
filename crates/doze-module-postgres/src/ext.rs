//! Extensions, the part step 11 (core plan M1) covers: the extensions every PostgreSQL build ships (contrib), checked
//! by name and major at their line, created, updated in place to a declared version, and dropped when removed.
//!
//! The seams step 13 (M2, `pgext`) fills are here and nowhere else:
//! - doze's catalog (vector, pg_cron, postgis, …) is known by name, so an alias means its extension and a typo is
//!   answered, but its bundles are not fetched yet: a catalog extension is refused at its line unless it is `optional`;
//! - a `source` bundle is resolved against the folder of the file that declares it (`Instance.dir`, as Go's `baseDir`)
//!   and must exist; it is installed by M2's ledgered install (module bugs 3 and 4: Go unpacked any archive into the
//!   shared toolchain with no checksum and no record), so until then only a toolchain that already has it works;
//! - preload libraries, PG18 search paths and server environment come from fetched bundles, so with none they are the
//!   person's own `settings` (which `conf` writes as given).

use crate::spec::Ext;
use std::path::{Path, PathBuf};

/// Contrib extensions in every major doze builds (14 to 18).
const CONTRIB: [&str; 44] = [
    "amcheck",
    "autoinc",
    "bloom",
    "btree_gin",
    "btree_gist",
    "citext",
    "cube",
    "dblink",
    "dict_int",
    "dict_xsyn",
    "earthdistance",
    "file_fdw",
    "fuzzystrmatch",
    "hstore",
    "insert_username",
    "intagg",
    "intarray",
    "isn",
    "lo",
    "ltree",
    "moddatetime",
    "pageinspect",
    "pg_buffercache",
    "pg_freespacemap",
    "pg_prewarm",
    "pg_stat_statements",
    "pg_surgery",
    "pg_trgm",
    "pg_visibility",
    "pgcrypto",
    "pgrowlocks",
    "pgstattuple",
    "plpgsql",
    "postgres_fdw",
    "refint",
    "seg",
    "sslinfo",
    "tablefunc",
    "tcn",
    "tsm_system_rows",
    "tsm_system_time",
    "unaccent",
    "uuid-ossp",
    "xml2",
];

/// Contrib extensions that came or went: (name, first major, last major; 0 = open).
const CONTRIB_RANGE: [(&str, u32, u32); 4] = [
    ("adminpack", 0, 16),
    ("old_snapshot", 0, 16),
    ("pg_walinspect", 15, 0),
    ("pg_logicalinspect", 18, 0),
];

/// doze's catalog (Go's `catalog.go`, waves 1 and 2): CREATE EXTENSION name, aliases, names its bundle also provides,
/// and the majors it is built for.
/// (name, aliases, provided names, majors).
type Entry = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static [u32],
);

const CATALOG: [Entry; 18] = [
    ("vector", &["pgvector"], &[], ALL),
    ("pg_cron", &[], &[], ALL),
    ("pg_partman", &[], &[], ALL),
    ("pgmq", &[], &[], ALL),
    ("hypopg", &[], &[], ALL),
    ("pg_hint_plan", &[], &[], ALL),
    ("pg_repack", &[], &[], ALL),
    ("pgaudit", &[], &[], ALL),
    ("pg_ivm", &[], &[], ALL),
    ("pg_jsonschema", &[], &[], ALL),
    (
        "postgis",
        &[],
        &[
            "postgis_raster",
            "postgis_topology",
            "postgis_tiger_geocoder",
            "address_standardizer",
            "address_standardizer_data_us",
        ],
        ALL,
    ),
    ("h3", &[], &["h3_postgis"], ALL),
    ("pgrouting", &[], &[], ALL),
    ("timescaledb", &["timescale"], &[], &[15, 16, 17, 18]),
    ("pg_search", &[], &[], &[15, 16, 17, 18]),
    ("pg_duckdb", &[], &[], ALL),
    ("vectorscale", &["pgvectorscale"], &[], &[18]),
    ("age", &["apache_age"], &[], ALL),
];
const ALL: &[u32] = &[14, 15, 16, 17, 18];

/// Whether `name` ships with PostgreSQL `major` (0: not known, any contrib name).
pub(crate) fn is_contrib(name: &str, major: u32) -> bool {
    CONTRIB.contains(&name)
        || CONTRIB_RANGE.iter().any(|(n, from, to)| {
            *n == name
                && (major == 0 || ((*from == 0 || major >= *from) && (*to == 0 || major <= *to)))
        })
}

/// The catalog entry `name` (or an alias, or a name its bundle provides) belongs to: the CREATE EXTENSION name and
/// the majors it is built for.
fn catalog(name: &str) -> Option<(&'static str, &'static [u32])> {
    CATALOG.iter().find_map(|(n, aliases, provides, majors)| {
        if *n == name || aliases.contains(&name) {
            Some((*n, *majors))
        } else {
            provides.iter().find(|p| **p == name).map(|p| (*p, *majors))
        }
    })
}

/// The CREATE EXTENSION name for what was declared: an alias becomes its extension, anything else is itself.
pub(crate) fn canonical(name: &str) -> String {
    catalog(name).map_or_else(|| name.to_string(), |(n, _)| n.to_string())
}

/// Every name a person could mean, for "did you mean".
pub(crate) fn known() -> Vec<&'static str> {
    let mut v: Vec<&str> = CONTRIB.to_vec();
    v.extend(CONTRIB_RANGE.iter().map(|(n, ..)| *n));
    v.extend(CATALOG.iter().map(|(n, ..)| *n));
    v
}

/// Why a declared extension cannot be had on `major`, `None` if it can (a contrib name, or one with a `source`), or
/// if it is `optional` (skipped at converge with a warning when it cannot be had).
pub(crate) fn refused(e: &Ext, major: u32, base: &str) -> Option<(String, String)> {
    if e.optional {
        return None;
    }
    if let Some(src) = &e.source {
        let path = source_path(base, src);
        return (src.contains("://") || !path.is_file()).then(|| {
            (
                format!("extension {:?}: no bundle at {}", e.name, path.display()),
                "a .tar.gz file on disk, relative to this file; URLs come with doze's checksummed bundles".into(),
            )
        });
    }
    if is_contrib(&e.name, major) {
        return None;
    }
    if let Some((n, majors)) = catalog(&e.name) {
        let why = if majors.contains(&major) || major == 0 {
            format!(
                "{n} is in doze's extension catalog, which this release of the module does not fetch yet"
            )
        } else {
            format!("{n} is built for PostgreSQL {majors:?}; this block declares {major}")
        };
        return Some((
            why,
            "contrib extensions work now; `optional = true` keeps it declared and skips it".into(),
        ));
    }
    let near = doze_module::spec::closest(&e.name, &known()).map_or_else(
        || "contrib extensions (pg_trgm, hstore, …), or a `source` bundle".to_string(),
        |n| format!("did you mean {n:?}?"),
    );
    Some((
        format!("PostgreSQL {major} has no extension {:?}", e.name),
        near,
    ))
}

/// A `source` relative to the declaring file's folder (Go's `resolveExtSource`); an absolute one as given.
pub(crate) fn source_path(base: &str, src: &str) -> PathBuf {
    let p = Path::new(src);
    if p.is_absolute() || base.is_empty() {
        p.to_path_buf()
    } else {
        Path::new(base).join(p)
    }
}

/// The statements for one extension: created if missing, then updated in place to a declared version (Go created at
/// a version only when absent, so a changed version did nothing).
pub(crate) fn statements(e: &Ext) -> Vec<String> {
    use crate::sql::{ident, lit};
    let mut create = format!("CREATE EXTENSION IF NOT EXISTS {}", ident(&e.name));
    if let Some(s) = &e.schema {
        create.push_str(&format!(" SCHEMA {}", ident(s)));
    }
    if let Some(v) = &e.version {
        create.push_str(&format!(" VERSION {}", lit(v)));
    }
    if e.cascade {
        create.push_str(" CASCADE");
    }
    let mut q = vec![create];
    if let Some(v) = &e.version {
        q.push(format!(
            "ALTER EXTENSION {} UPDATE TO {}",
            ident(&e.name),
            lit(v)
        ));
    }
    q
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
            optional: false,
            cascade: false,
        }
    }

    #[test]
    fn contrib_by_major_and_the_catalog_by_alias() {
        assert!(
            is_contrib("pg_trgm", 17)
                && is_contrib("pg_walinspect", 15)
                && !is_contrib("pg_walinspect", 14)
        );
        assert!(!is_contrib("adminpack", 17) && is_contrib("adminpack", 16));
        assert_eq!(canonical("pgvector"), "vector");
        assert_eq!(canonical("postgis_raster"), "postgis_raster");
        assert!(refused(&ext("pg_trgm"), 17, "").is_none());
        let typo = refused(&ext("pg_trgn"), 17, "").map(|r| r.1);
        assert_eq!(typo.as_deref(), Some("did you mean \"pg_trgm\"?"));
        assert!(refused(&ext("vector"), 17, "").is_some());
        let mut opt = ext("vector");
        opt.optional = true;
        assert!(refused(&opt, 17, "").is_none());
    }

    #[test]
    fn a_version_updates_in_place() {
        let mut e = ext("hstore");
        e.version = Some("1.8".into());
        e.schema = Some("x".into());
        e.cascade = true;
        assert_eq!(
            statements(&e),
            [
                "CREATE EXTENSION IF NOT EXISTS \"hstore\" SCHEMA \"x\" VERSION '1.8' CASCADE",
                "ALTER EXTENSION \"hstore\" UPDATE TO '1.8'"
            ]
        );
        assert_eq!(source_path("/p", "b.tgz"), PathBuf::from("/p/b.tgz"));
        assert_eq!(source_path("/p", "/a/b.tgz"), PathBuf::from("/a/b.tgz"));
    }
}
