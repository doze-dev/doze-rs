//! The mirror manifest as doze-binaries publishes it, and a build fetched from a `file://` mirror, checked against the
//! manifest, the lock's pin, and a tampered archive.

use super::*;
use crate::archive::tests::targz;

const MANIFEST: &str = "engines:\n  valkey:\n    versions:\n      \"9\": 9.1.1\n    artifacts:\n      9.1.1:\n        aarch64-apple-darwin:\n          url: valkey-9.1.1.tar.gz\n          sha256: SHA\n";

#[test]
fn reads_the_manifest() {
    let m = parse_manifest(MANIFEST).unwrap();
    let v = &m["valkey"];
    assert_eq!(v.versions["9"], "9.1.1");
    assert_eq!(
        v.artifacts["9.1.1"]["aarch64-apple-darwin"].url,
        "valkey-9.1.1.tar.gz"
    );
}

#[test]
fn a_build_is_fetched_verified_cached_and_held_to_its_pin() {
    let d = std::env::temp_dir().join(format!("doze-fetch-mirror-{}", std::process::id()));
    let root = d.join("mirror");
    std::fs::create_dir_all(root.join("valkey")).unwrap();
    let a = targz(&[("bin/valkey-server", b"#!", true)]);
    let sha = signed::sha256_hex(&a);
    std::fs::write(root.join("valkey/valkey-9.1.1.tar.gz"), &a).unwrap();
    std::fs::write(
        root.join("valkey/index.yaml"),
        MANIFEST.replace("SHA", &sha),
    )
    .unwrap();
    let m = Mirror {
        root: format!("file://{}", root.display()),
        home: d.join("home"),
        key: None,
    };
    assert_eq!(m.resolve_major("valkey", "9").unwrap(), "9.1.1");
    let (bin, digest) = m
        .ensure("valkey", "9.1.1", "aarch64-apple-darwin", None)
        .unwrap();
    assert!(bin.join("valkey-server").exists());
    assert_eq!(digest, format!("sha256:{sha}"));
    let err = m
        .ensure("valkey", "9.1.1", "aarch64-apple-darwin", Some("sha256:00"))
        .unwrap_err();
    assert!(err.contains("but the lock pins sha256:00"), "{err}");
    std::fs::write(
        root.join("valkey/valkey-9.1.1.tar.gz"),
        targz(&[("bin/evil", b"x", true)]),
    )
    .unwrap();
    let other = Mirror {
        home: d.join("home2"),
        ..m
    };
    let err = other
        .ensure("valkey", "9.1.1", "aarch64-apple-darwin", None)
        .unwrap_err();
    assert!(err.starts_with("checksum mismatch"), "{err}");
    let _ = std::fs::remove_dir_all(&d);
}

/// F1 audit 3, minor 3: an https manifest may not point at a file on this machine; a local one may.
#[test]
fn a_remote_manifest_may_not_name_a_local_file() {
    let err =
        local_only_from_local("file:///etc/passwd", "https://mirror.example/valkey").unwrap_err();
    assert!(err.contains("may not point at a file"), "{err}");
    for base in ["file:///srv/mirror/valkey", "/srv/mirror/valkey"] {
        assert_eq!(
            local_only_from_local("file:///srv/a.tgz", base).as_deref(),
            Ok("file:///srv/a.tgz")
        );
    }
    assert!(local_only_from_local("https://cdn.example/a.tgz", "https://mirror.example").is_ok());
}

/// F1 audit 3, minor 4: a tmp left beside a build that is already cached is swept when the build is used.
#[test]
fn a_tmp_beside_a_cached_build_is_swept() {
    let d = std::env::temp_dir().join(format!("doze-fetch-sweep-{}", std::process::id()));
    let content = d.join("valkey/9.1.1-x");
    std::fs::create_dir_all(content.join("bin")).unwrap();
    std::fs::write(content.join("bin/valkey-server"), b"#!").unwrap();
    let tmp = d.join("valkey/9.1.1-x.tmp4242");
    std::fs::create_dir_all(&tmp).unwrap();
    let (bin, _) = cached(&content, "valkey", None).unwrap();
    assert_eq!(bin, content.join("bin"));
    assert!(!tmp.exists());
    let _ = std::fs::remove_dir_all(&d);
}
