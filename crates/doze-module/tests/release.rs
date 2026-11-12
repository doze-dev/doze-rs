//! The release tool on the real echo module (dry run into a temporary directory): what it refuses, the archive's single
//! executable member, a reproducible sha256, the merged index and meta.yaml.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_module_release::{Args, release};
use std::path::PathBuf;

fn args(out: &std::path::Path, target: &str, version: &str, accepted: bool) -> Args {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_doze-module-echo"));
    Args {
        binary: bin.clone(),
        describe_with: bin,
        target: target.into(),
        version: version.into(),
        out: out.to_path_buf(),
        namespace: "doze".into(),
        accepted,
    }
}

#[test]
fn a_dry_run_release_of_the_echo_module() {
    let out = std::env::temp_dir().join(format!("doze-release-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let host = doze_module_test::target();
    // Refused: acceptance not passed, and a target the module does not declare.
    let e = release(&args(&out, &host, "0.1.0", false)).unwrap_err();
    assert!(e.contains("acceptance has not passed"), "{e}");
    let e = release(&args(&out, "riscv64gc-unknown-linux-gnu", "0.1.0", true)).unwrap_err();
    assert!(e.contains("does not declare"), "{e}");
    assert!(
        !out.join("echo").exists(),
        "a refused release must write nothing"
    );

    let first = release(&args(&out, &host, "0.1.0", true)).unwrap();
    // The same binary gives the same bytes: re-running a published version is not a change.
    assert_eq!(release(&args(&out, &host, "0.1.0", true)).unwrap(), first);
    let dir = out.join("echo");
    let archive = dir.join(format!("echo-plugin-0.1.0-{host}.tar.gz"));
    let listing = std::process::Command::new("tar")
        .arg("-tvzf")
        .arg(&archive)
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&listing.stdout).to_string();
    let exe = if cfg!(windows) { ".exe" } else { "" };
    assert_eq!(listing.lines().count(), 1, "{listing}");
    assert!(
        listing.contains(&format!("bin/echo-plugin{exe}")) && listing.contains("rwxr-xr-x"),
        "{listing}"
    );

    release(&args(&out, &host, "0.2.0", true)).unwrap();
    let index = std::fs::read_to_string(dir.join("index.yaml")).unwrap();
    for want in [
        "schema: 1",
        "module: echo",
        "namespace: doze",
        "protocol: 2",
        "stable: 0.2.0",
        "doze: 0.3.0",
    ] {
        assert!(index.contains(want), "index.yaml lacks {want:?}:\n{index}");
    }
    let meta = std::fs::read_to_string(dir.join("meta.yaml")).unwrap();
    for want in [
        "engine: echo",
        "name: greeting",
        "since: \"2\"",
        "name: channel",
        "label: name",
        "exampleLabel: e",
    ] {
        assert!(meta.contains(want), "meta.yaml lacks {want:?}:\n{meta}");
    }
    let _ = std::fs::remove_dir_all(&out);
}
