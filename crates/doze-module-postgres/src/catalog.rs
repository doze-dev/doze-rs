//! Which extensions a PostgreSQL build ships (contrib), and doze's catalog of the ones it fetches as prebuilt bundles
//! (Go's `catalog.go`, waves 1 and 2). A catalog bundle is an engine in the binaries mirror, `pgext-<name>-pg<major>`,
//! whose index keys versions by line (`0.8` is the newest 0.8.x); doze-binaries' `versions.yaml` builds what this
//! table names, so the two must agree on names, majors and lines.

/// One extension doze can fetch by name.
#[derive(Debug)]
pub(crate) struct Entry {
    /// The CREATE EXTENSION name, and the bundle key's `<name>`.
    pub name: &'static str,
    /// Other names that mean it (`pgvector`).
    pub aliases: &'static [&'static str],
    /// The default version line, unless `lines` gives the major one of its own.
    pub line: &'static str,
    pub lines: &'static [(u32, &'static str)],
    /// The PostgreSQL majors it is built for.
    pub majors: &'static [u32],
    /// Libraries that must be in `shared_preload_libraries`.
    pub preload: &'static [&'static str],
    /// postgresql.conf settings it needs; `{db}` is the instance's database.
    pub settings: &'static [(&'static str, &'static str)],
    /// The server's environment; `{root}` is the bundle's folder.
    pub env: &'static [(&'static str, &'static str)],
    /// Extensions created before it (catalog or contrib).
    pub requires: &'static [&'static str],
    /// Further CREATE EXTENSION names in the same bundle, each with what it requires.
    pub provides: &'static [(&'static str, &'static [&'static str])],
}

const ALL: &[u32] = &[14, 15, 16, 17, 18];

const fn entry(name: &'static str, line: &'static str, majors: &'static [u32]) -> Entry {
    Entry {
        name,
        aliases: &[],
        line,
        lines: &[],
        majors,
        preload: &[],
        settings: &[],
        env: &[],
        requires: &[],
        provides: &[],
    }
}

/// Waves 1 and 2 of the design. The lines must be keys of the mirror's index for each bundle.
const CATALOG: [Entry; 18] = [
    Entry {
        aliases: &["pgvector"],
        ..entry("vector", "0.8", ALL)
    },
    Entry {
        preload: &["pg_cron"],
        // Background workers, not libpq connections: the server has no TCP.
        settings: &[
            ("cron.database_name", "{db}"),
            ("cron.use_background_workers", "on"),
        ],
        ..entry("pg_cron", "1.6", ALL)
    },
    // Not preloaded: 5.5's background worker runs as a role that may not exist and stops the server without it.
    entry("pg_partman", "5.5", ALL),
    entry("pgmq", "1.13", ALL),
    entry("hypopg", "1.4", ALL),
    Entry {
        preload: &["pg_hint_plan"],
        lines: &[
            (14, "1.4"),
            (15, "1.5"),
            (16, "1.6"),
            (17, "1.7"),
            (18, "1.8"),
        ],
        ..entry("pg_hint_plan", "", ALL)
    },
    entry("pg_repack", "1.5", ALL),
    Entry {
        preload: &["pgaudit"],
        lines: &[
            (14, "1.6"),
            (15, "1.7"),
            (16, "16.1"),
            (17, "17.1"),
            (18, "18.0"),
        ],
        ..entry("pgaudit", "", ALL)
    },
    Entry {
        preload: &["pg_ivm"],
        ..entry("pg_ivm", "1.17", ALL)
    },
    entry("pg_jsonschema", "0.3", ALL),
    Entry {
        provides: &[
            ("postgis_raster", &["postgis"]),
            ("postgis_topology", &["postgis"]),
            ("postgis_tiger_geocoder", &["postgis", "fuzzystrmatch"]),
            ("address_standardizer", &[]),
            ("address_standardizer_data_us", &["address_standardizer"]),
        ],
        env: &[
            ("PROJ_DATA", "{root}/share/proj"),
            ("GDAL_DATA", "{root}/share/gdal"),
        ],
        ..entry("postgis", "3.6", ALL)
    },
    Entry {
        provides: &[("h3_postgis", &["h3", "postgis"])],
        ..entry("h3", "4.2", ALL)
    },
    Entry {
        requires: &["postgis"],
        ..entry("pgrouting", "3.8", ALL)
    },
    Entry {
        aliases: &["timescale"],
        preload: &["timescaledb"],
        settings: &[
            ("timescaledb.telemetry_level", "off"),
            ("timescaledb.license", "apache"),
        ],
        ..entry("timescaledb", "2.31", &[15, 16, 17, 18])
    },
    Entry {
        preload: &["pg_search"],
        ..entry("pg_search", "0.20", &[15, 16, 17, 18])
    },
    Entry {
        preload: &["pg_duckdb"],
        ..entry("pg_duckdb", "1.1", ALL)
    },
    Entry {
        aliases: &["pgvectorscale"],
        requires: &["vector"],
        ..entry("vectorscale", "0.9", &[18])
    },
    Entry {
        aliases: &["apache_age"],
        lines: &[
            (14, "1.5"),
            (15, "1.5"),
            (16, "1.5"),
            (17, "1.6"),
            (18, "1.6"),
        ],
        ..entry("age", "", ALL)
    },
];

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

/// Whether `name` ships with PostgreSQL `major` (0: not known, any contrib name).
pub(crate) fn is_contrib(name: &str, major: u32) -> bool {
    CONTRIB.contains(&name)
        || CONTRIB_RANGE.iter().any(|(n, from, to)| {
            *n == name
                && (major == 0 || ((*from == 0 || major >= *from) && (*to == 0 || major <= *to)))
        })
}

/// A declared name resolved against the catalog: the entry whose bundle holds it, the CREATE EXTENSION name, and what
/// is created before it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ref {
    pub entry: &'static Entry,
    pub create: &'static str,
    pub requires: &'static [&'static str],
}

/// The catalog entry `name` (or an alias, or a name its bundle provides) belongs to.
pub(crate) fn lookup(name: &str) -> Option<Ref> {
    CATALOG.iter().find_map(|e| {
        if e.name == name || e.aliases.contains(&name) {
            return Some(Ref {
                entry: e,
                create: e.name,
                requires: e.requires,
            });
        }
        e.provides
            .iter()
            .find(|(p, _)| *p == name)
            .map(|(p, r)| Ref {
                entry: e,
                create: p,
                requires: r,
            })
    })
}

/// The catalog entry named exactly `name` (a bundle's own name).
pub(crate) fn bundle(name: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.name == name)
}

/// The CREATE EXTENSION name for what was declared: an alias becomes its extension, anything else is itself.
pub(crate) fn canonical(name: &str) -> String {
    lookup(name).map_or_else(|| name.to_string(), |r| r.create.to_string())
}

/// Everything `r` needs created before it, transitively and in creation order, without itself.
pub(crate) fn requires_of(r: &Ref) -> Vec<&'static str> {
    fn walk(names: &[&'static str], seen: &mut Vec<&'static str>, out: &mut Vec<&'static str>) {
        for n in names {
            if seen.contains(n) {
                continue;
            }
            seen.push(n);
            if let Some(r) = lookup(n) {
                walk(r.requires, seen, out);
            }
            out.push(n);
        }
    }
    let (mut seen, mut out) = (vec![r.create], Vec::new());
    walk(r.requires, &mut seen, &mut out);
    out
}

impl Entry {
    /// Whether it is built for `major` (0: not known, so yes).
    pub(crate) fn supports(&self, major: u32) -> bool {
        major == 0 || self.majors.contains(&major)
    }

    /// The default version line on `major`.
    pub(crate) fn line(&self, major: u32) -> &'static str {
        self.lines
            .iter()
            .find(|(m, _)| *m == major)
            .map_or(self.line, |(_, l)| l)
    }

    /// The mirror engine its bundle for `major` is published as.
    pub(crate) fn key(&self, major: u32) -> String {
        format!("pgext-{}-pg{major}", self.name)
    }
}

/// Every name a person could mean on `major`, for "did you mean".
pub(crate) fn known(major: u32) -> Vec<&'static str> {
    let mut v: Vec<&str> = CONTRIB.to_vec();
    v.extend(CONTRIB_RANGE.iter().map(|(n, ..)| *n));
    for e in CATALOG.iter().filter(|e| e.supports(major)) {
        v.push(e.name);
        v.extend(e.aliases);
        v.extend(e.provides.iter().map(|(p, _)| *p));
    }
    v
}

/// The catalog's names on `major`, for a message that lists them.
pub(crate) fn names(major: u32) -> String {
    let v: Vec<&str> = CATALOG
        .iter()
        .filter(|e| e.supports(major))
        .map(|e| e.name)
        .collect();
    v.join(", ")
}

/// A support list as a person reads it: "18", "15 or newer", "14, 16".
pub(crate) fn majors_phrase(ms: &[u32]) -> String {
    let (Some(lo), Some(hi)) = (ms.first(), ms.last()) else {
        return "no major".into();
    };
    let contiguous = usize::try_from(hi - lo + 1).is_ok_and(|n| n == ms.len());
    match ms.len() {
        1 => lo.to_string(),
        _ if contiguous && *hi == 18 => format!("{lo} or newer"),
        _ if contiguous => format!("{lo} to {hi}"),
        _ => ms.iter().map(u32::to_string).collect::<Vec<_>>().join(", "),
    }
}

/// A catalog pin: a line ("0.8") or a release ("0.8.1"), numbers only.
pub(crate) fn pin_ok(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_aliases_and_what_a_bundle_provides() {
        assert!(is_contrib("pg_trgm", 17) && is_contrib("pg_walinspect", 15));
        assert!(!is_contrib("pg_walinspect", 14) && !is_contrib("adminpack", 17));
        assert_eq!(canonical("pgvector"), "vector");
        let r = lookup("postgis_tiger_geocoder").map(|r| (r.entry.name, r.create));
        assert_eq!(r, Some(("postgis", "postgis_tiger_geocoder")));
        let geo = lookup("postgis_tiger_geocoder").map(|r| requires_of(&r));
        assert_eq!(geo, Some(vec!["postgis", "fuzzystrmatch"]));
        let h3 = lookup("h3_postgis").map(|r| requires_of(&r));
        assert_eq!(h3, Some(vec!["h3", "postgis"]));
        assert_eq!(bundle("pgaudit").map(|e| e.line(16)), Some("16.1"));
        assert_eq!(
            bundle("vector").map(|e| e.key(17)).as_deref(),
            Some("pgext-vector-pg17")
        );
        assert!(bundle("vectorscale").is_some_and(|e| !e.supports(17) && e.supports(18)));
    }

    #[test]
    fn pins_and_phrases() {
        assert!(pin_ok("0.8") && pin_ok("0.8.1") && pin_ok("16.1"));
        assert!(!pin_ok("0") && !pin_ok("0.8.1.2") && !pin_ok("v0.8") && !pin_ok("0..8"));
        assert_eq!(majors_phrase(&[18]), "18");
        assert_eq!(majors_phrase(&[15, 16, 17, 18]), "15 or newer");
        assert_eq!(majors_phrase(&[14, 15, 16]), "14 to 16");
        assert_eq!(majors_phrase(&[14, 16]), "14, 16");
        assert!(names(17).contains("vector") && !names(17).contains("vectorscale"));
    }
}
