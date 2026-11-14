//! Extensions at their line: a contrib name (checked by major), doze's catalog (checked by major, its pin a line or a
//! release), or a `source` bundle on disk (a URL is refused: doze's checksummed downloads are the catalog's), and the
//! statements that create one and move it to a declared version. Fetching and installing bundles is `bundle`; their
//! server settings are `preload`.

use crate::catalog;
use crate::spec::Ext;
use crate::sql::{ident, lit};
use std::path::{Path, PathBuf};

/// Why a declared extension cannot be had on `major`, `None` if it can. An `optional` one is skipped at converge with
/// a warning when it cannot be had, so only its pin is checked here (as Go did).
pub(crate) fn refused(e: &Ext, major: u32, base: &str) -> Option<(String, String)> {
    let r = catalog::lookup(&e.name)
        .filter(|_| e.source.is_none() && !catalog::is_contrib(&e.name, major));
    if let (Some(r), Some(v)) = (&r, &e.version)
        && !catalog::pin_ok(v)
    {
        let line = r.entry.line(major);
        return Some((
            format!(
                "extension {}: version {v:?} is not a bundle line or release",
                r.entry.name
            ),
            format!("a line like {line:?} (its newest release) or a release like \"{line}.0\""),
        ));
    }
    if e.optional {
        return None;
    }
    if let Some(src) = &e.source {
        return source_refused(e, src, base);
    }
    if e.name == "plpgsql" {
        // Always installed; declaring it and then removing it would drop every PL/pgSQL function.
        return Some((
            "plpgsql is built into every database".into(),
            "leave it out".into(),
        ));
    }
    if catalog::is_contrib(&e.name, major) {
        return None;
    }
    if let Some(r) = r {
        return (!r.entry.supports(major)).then(|| {
            let needs = catalog::majors_phrase(r.entry.majors);
            (
                format!("{} needs PostgreSQL {needs}; this block declares {major}", r.entry.name),
                "change the block's version, or set `optional = true` to keep it declared and skip it".into(),
            )
        });
    }
    let near = doze_module::spec::closest(&e.name, &catalog::known(major)).map_or_else(
        || {
            format!(
                "doze's catalog: {}; contrib extensions (pg_trgm, hstore, …) work too; or give `source` a bundle",
                catalog::names(major)
            )
        },
        |n| format!("did you mean {:?}?", catalog::canonical(n)),
    );
    Some((
        format!("PostgreSQL {major} has no extension {:?}", e.name),
        near,
    ))
}

/// A `source` must be an archive on disk, and a declared `sha256` a SHA-256.
fn source_refused(e: &Ext, src: &str, base: &str) -> Option<(String, String)> {
    let path = source_path(base, src);
    if src.contains("://") || !path.is_file() {
        return Some((
            format!("extension {:?}: no bundle at {}", e.name, path.display()),
            "a .tar.gz file on disk, relative to this file; URLs come with doze's checksummed catalog".into(),
        ));
    }
    let bad = e
        .sha256
        .as_ref()
        .filter(|h| h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()));
    bad.map(|h| {
        (
            format!("extension {:?}: sha256 {h:?} is not a SHA-256", e.name),
            "64 hex digits, as `shasum -a 256` prints them".into(),
        )
    })
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

fn create(name: &str, e: &Ext, version: Option<&str>) -> String {
    let mut create = format!("CREATE EXTENSION IF NOT EXISTS {}", ident(name));
    if let Some(s) = &e.schema {
        create.push_str(&format!(" SCHEMA {}", ident(s)));
    }
    if let Some(v) = version {
        create.push_str(&format!(" VERSION {}", lit(v)));
    }
    if e.cascade {
        create.push_str(" CASCADE");
    }
    create
}

/// The statements for one contrib extension: created if missing, then updated in place to a declared version (Go
/// created at a version only when absent, so a changed version did nothing).
pub(crate) fn statements(e: &Ext) -> Vec<String> {
    let mut q = vec![create(&e.name, e, e.version.as_deref())];
    if let Some(v) = &e.version {
        q.push(format!(
            "ALTER EXTENSION {} UPDATE TO {}",
            ident(&e.name),
            lit(v)
        ));
    }
    q
}

/// A bundle's extension created at the version its bundle makes (`target`); moving an existing one up is converge's.
pub(crate) fn create_at(e: &Ext, target: &str) -> String {
    create(&e.name, e, Some(target))
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
    fn contrib_by_major_and_the_catalog_by_major_and_pin() {
        assert!(refused(&ext("pg_trgm"), 17, "").is_none());
        let typo = refused(&ext("pg_trgn"), 17, "").map(|r| r.1);
        assert_eq!(typo.as_deref(), Some("did you mean \"pg_trgm\"?"));
        let alias = refused(&ext("vectr"), 17, "").map(|r| r.1);
        assert_eq!(alias.as_deref(), Some("did you mean \"vector\"?"));
        assert!(refused(&ext("vector"), 17, "").is_none());
        let scale = refused(&ext("vectorscale"), 17, "").map(|r| r.0);
        assert_eq!(
            scale.as_deref(),
            Some("vectorscale needs PostgreSQL 18; this block declares 17")
        );
        assert!(refused(&ext("vectorscale"), 18, "").is_none());
        let mut pinned = ext("vector");
        pinned.version = Some("0.8".into());
        assert!(refused(&pinned, 17, "").is_none());
        pinned.version = Some("latest".into());
        pinned.optional = true;
        assert!(
            refused(&pinned, 17, "").is_some(),
            "an optional pin is checked too"
        );
        let mut opt = ext("nothing_like_it");
        opt.optional = true;
        assert!(refused(&opt, 17, "").is_none());
    }

    #[test]
    fn a_source_is_a_file_with_a_well_formed_hash() {
        let dir = std::env::temp_dir().join(format!("dz-ext-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.tar.gz"), "x").unwrap();
        let base = dir.to_string_lossy();
        let mut e = ext("mine");
        e.source = Some("b.tar.gz".into());
        assert!(refused(&e, 17, &base).is_none());
        e.sha256 = Some("abc".into());
        assert!(refused(&e, 17, &base).is_some_and(|r| r.0.contains("not a SHA-256")));
        e.sha256 = None;
        e.source = Some("https://example.com/b.tar.gz".into());
        assert!(refused(&e, 17, &base).is_some());
        e.source = Some("gone.tar.gz".into());
        assert!(refused(&e, 17, &base).is_some_and(|r| r.0.contains("no bundle")));
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(
            create_at(&ext("vector"), "0.8.7"),
            "CREATE EXTENSION IF NOT EXISTS \"vector\" VERSION '0.8.7'"
        );
        assert_eq!(source_path("/p", "b.tgz"), PathBuf::from("/p/b.tgz"));
        assert_eq!(source_path("/p", "/a/b.tgz"), PathBuf::from("/a/b.tgz"));
    }
}
