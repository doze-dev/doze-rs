//! Extensions from bundles on the real server (modules plan step 13, core plan M2). Each test is a promise checked on
//! the engine: a `source` bundle installs (into the toolchain by the ledger on 14 to 17, from its own folder on 18),
//! is created, keeps its data across sleep and wake, is updated in place when a newer bundle is declared, preloads and
//! sets what its bundle.json asks for merged with the person's own, and is pinned by its hash, so a changed file is
//! refused. With `DOZE_PGEXT_BUNDLES` (a folder of doze-binaries' `pgext-*-pg<major>-*.tar.gz`), a catalog extension
//! (pgvector) comes through the host's fetch at a pinned release, then moves up to its line's newest, and a compiled
//! `source` bundle (hypopg) loads its library.
//!
//! On 14 to 17 the bundles are installed into the toolchain `DOZE_POSTGRES_BINDIR` names: point it at a copy.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::{graceful, harness, major, sql};
use doze_module_test::{Boot, Booted, LocalEngine, target};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const GRACE: Duration = Duration::from_secs(20);

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("dz-pgext-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `tar -czf` of `dir`'s contents (no AppleDouble files).
fn tar(dir: &Path, out: &Path) {
    let ok = Command::new("tar")
        .env("COPYFILE_DISABLE", "1")
        .args([
            "-czf",
            &out.to_string_lossy(),
            "-C",
            &dir.to_string_lossy(),
            ".",
        ])
        .status()
        .unwrap();
    assert!(ok.success());
}

/// A pure-SQL extension `doze_probe` whose newest version is `newest`, with every version up to it and the scripts
/// between them; its bundle.json preloads auto_explain (a library every build has) and sets one of its settings.
fn probe(at: &Path, newest: &str) -> PathBuf {
    let dir = at.join(format!("probe-{newest}"));
    let ext = dir.join("share/extension");
    std::fs::create_dir_all(&ext).unwrap();
    let control =
        format!("default_version = '{newest}'\nrelocatable = true\ncomment = 'doze probe'\n");
    std::fs::write(ext.join("doze_probe.control"), control).unwrap();
    let f = |v: &str| {
        format!(
            "CREATE OR REPLACE FUNCTION doze_probe() RETURNS text LANGUAGE sql AS $$ SELECT '{v}' $$;\n"
        )
    };
    std::fs::write(ext.join("doze_probe--1.0.sql"), f("1.0")).unwrap();
    if newest == "1.1" {
        std::fs::write(ext.join("doze_probe--1.1.sql"), f("1.1")).unwrap();
        std::fs::write(ext.join("doze_probe--1.0--1.1.sql"), f("1.1")).unwrap();
    }
    let manifest = r#"{"name":"doze_probe","version":"V","preload":["auto_explain"],
        "settings":{"auto_explain.log_min_duration":"250ms"}}"#;
    std::fs::write(dir.join("bundle.json"), manifest.replace('V', newest)).unwrap();
    let out = at.join(format!("doze_probe-{newest}.tar.gz"));
    tar(&dir, &out);
    out
}

fn block(major: &str, body: &str) -> String {
    format!(
        "postgres \"app\" {{\n  version = {major}\n  settings = {{ shared_preload_libraries = \"pg_stat_statements\" }}\n{body}}}\n"
    )
}

fn source(name: &str, path: &Path) -> String {
    format!(
        "  extension \"{name}\" {{\n    source = \"{}\"\n  }}\n",
        path.display()
    )
}

#[tokio::test(flavor = "current_thread")]
async fn a_source_bundle_installs_preloads_updates_survives_sleep_and_is_pinned() {
    let Some(h) = harness() else { return };
    let m = major(&h);
    let d = scratch("probe");
    let (v10, v11) = (probe(&d, "1.0"), probe(&d, "1.1"));
    let b = h
        .boot(&block(&m, &source("doze_probe", &v10)))
        .await
        .unwrap();
    let q = |b: &Booted, q: &str| sql(b, "postgres", "app", q).unwrap();
    assert_eq!(q(&b, "SELECT doze_probe()"), "1.0");
    assert_eq!(
        q(&b, "SHOW shared_preload_libraries"),
        "auto_explain,pg_stat_statements"
    );
    assert_eq!(q(&b, "SHOW auto_explain.log_min_duration"), "250ms");
    let pin = b
        .pins
        .iter()
        .find(|p| p.engine == "pgext-source-doze_probe")
        .unwrap();
    assert!(pin.resolved.starts_with("sha256:") && pin.hashes.contains_key(&target()));
    q(&b, "CREATE TABLE kept (n int); INSERT INTO kept VALUES (7)");
    if m.parse::<u32>().unwrap() < 18 {
        let share = Path::new(&b.toolchain.bin_dir).join("../share/extension");
        let ledger = std::fs::read_to_string(share.join(".doze-pgext.json")).unwrap();
        assert!(ledger.contains("extension/doze_probe--1.0.sql"), "{ledger}");
    } else {
        // 18 finds the bundle in its own folder: the shared toolchain is left alone.
        let share = Path::new(&b.toolchain.bin_dir).join("../share/extension");
        assert!(!share.join("doze_probe.control").exists());
        assert!(!share.join(".doze-pgext.json").exists());
        let conf =
            std::fs::read_to_string(Path::new(&b.instance.data_dir).join("doze.conf")).unwrap();
        assert!(
            conf.contains("extension_control_path") && conf.contains("$system"),
            "{conf}"
        );
    }
    // Sleep and wake: the extension and the data are there.
    let (stops, b) = b.sleep_and_wake(GRACE).await.unwrap();
    graceful(&stops);
    assert_eq!(q(&b, "SELECT doze_probe() || (SELECT n FROM kept)"), "1.07");
    // A newer bundle declared: updated in place, data kept.
    let (stops, b) = b
        .redeclare(&block(&m, &source("doze_probe", &v11)), GRACE)
        .await
        .unwrap();
    graceful(&stops);
    let ext = "SELECT extversion FROM pg_extension WHERE extname = 'doze_probe'";
    assert_eq!(q(&b, ext), "1.1");
    assert_eq!(q(&b, "SELECT doze_probe() || (SELECT n FROM kept)"), "1.17");
    // The file under a pinned name changed: refused, naming the lock.
    std::fs::copy(&v10, &v11).unwrap();
    let e = b
        .redeclare(&block(&m, &source("doze_probe", &v11)), GRACE)
        .await
        .unwrap_err();
    assert!(e.contains("changed since doze.lock pinned it"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

#[tokio::test(flavor = "current_thread")]
async fn a_declared_sha256_must_match() {
    let Some(h) = harness() else { return };
    let d = scratch("sha");
    let v10 = probe(&d, "1.0");
    let wrong = "0".repeat(64);
    let body = format!(
        "  extension \"doze_probe\" {{\n    source = \"{}\"\n    sha256 = \"{wrong}\"\n  }}\n",
        v10.display()
    );
    let e = h.boot(&block(&major(&h), &body)).await.unwrap_err();
    assert!(e.contains("but doze.hcl declares"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// doze-binaries' bundles of `name` for this major in `DOZE_PGEXT_BUNDLES`, unpacked, oldest first: (version, root,
/// archive).
fn bundles(name: &str, major: &str, into: &Path) -> Vec<(String, PathBuf, PathBuf)> {
    let Some(dir) = std::env::var_os("DOZE_PGEXT_BUNDLES") else {
        return vec![];
    };
    let (prefix, suffix) = (
        format!("pgext-{name}-pg{major}-"),
        format!("-{}.tar.gz", target()),
    );
    let mut out: Vec<(String, PathBuf, PathBuf)> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let file = e.unwrap().file_name().to_string_lossy().into_owned();
            let v = file
                .strip_prefix(&prefix)?
                .strip_suffix(&suffix)?
                .to_string();
            let archive = Path::new(&dir).join(&file);
            let ok = Command::new("tar")
                .args([
                    "-xzf",
                    &archive.to_string_lossy(),
                    "-C",
                    &into.to_string_lossy(),
                ])
                .status()
                .unwrap();
            assert!(ok.success());
            let root = into.join(file.strip_suffix(".tar.gz").unwrap());
            Some((v, root, archive))
        })
        .collect();
    let key = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    out.sort_by_key(|a| key(&a.0));
    out
}

#[tokio::test(flavor = "current_thread")]
async fn a_catalog_extension_comes_at_its_pin_then_moves_up_and_a_compiled_source_loads() {
    let Some(h) = harness() else { return };
    let m = major(&h);
    let d = scratch("catalog");
    let (vector, hypopg) = (bundles("vector", &m, &d), bundles("hypopg", &m, &d));
    if vector.len() < 2 || hypopg.len() < 2 {
        eprintln!(
            "skipped: DOZE_PGEXT_BUNDLES needs two pgext-vector-pg{m} and two pgext-hypopg-pg{m} bundles"
        );
        return;
    }
    let engine = format!("pgext-vector-pg{m}");
    let extra: Vec<LocalEngine> = vector
        .iter()
        .map(|(v, root, _)| LocalEngine {
            engine: engine.clone(),
            line: "0.8".into(),
            full: v.clone(),
            root: root.clone(),
        })
        .collect();
    let (old, new) = (&vector[0].0, &vector[vector.len() - 1].0);
    let first = format!(
        "  extension \"pgvector\" {{\n    version = \"{old}\"\n  }}\n{}",
        source("hypopg", &hypopg[0].2)
    );
    let b = h
        .boot_extra(&block(&m, &first), Boot::default(), &extra)
        .await
        .unwrap();
    let q = |b: &Booted, q: &str| sql(b, "postgres", "app", q).unwrap();
    let version = "SELECT extversion FROM pg_extension WHERE extname = 'vector'";
    assert_eq!(&q(&b, version), old);
    let pin = b.pins.iter().find(|p| p.engine == engine).unwrap();
    assert_eq!(
        (pin.spec.as_str(), pin.resolved.as_str()),
        (old.as_str(), old.as_str())
    );
    q(
        &b,
        "CREATE TABLE items (id int, v vector(3)); INSERT INTO items VALUES (1, '[1,2,3]'), (2, '[3,2,1]')",
    );
    q(&b, "CREATE INDEX ON items USING hnsw (v vector_l2_ops)");
    let near = "SELECT id FROM items ORDER BY v <-> '[3,2,2]' LIMIT 1";
    assert_eq!(q(&b, near), "2");
    let hypo = "SELECT count(*) FROM hypopg_create_index('CREATE INDEX ON items (id)')";
    assert_eq!(q(&b, hypo), "1");
    let (stops, b) = b.sleep_and_wake(GRACE).await.unwrap();
    graceful(&stops);
    assert_eq!(q(&b, near), "2");
    // Unpinned: the line's newest, and the database's extension moved up to it; the newer hypopg too.
    let second = format!(
        "  extensions = [\"vector\"]\n{}",
        source("hypopg", &hypopg[hypopg.len() - 1].2)
    );
    let (stops, b) = b.redeclare(&block(&m, &second), GRACE).await.unwrap();
    graceful(&stops);
    assert_eq!(&q(&b, version), new);
    assert_eq!(q(&b, near), "2");
    let hv = "SELECT extversion FROM pg_extension WHERE extname = 'hypopg'";
    assert_eq!(&q(&b, hv), &hypopg[hypopg.len() - 1].0);
    graceful(&b.stop(GRACE).await);
    let _ = std::fs::remove_dir_all(&d);
}
