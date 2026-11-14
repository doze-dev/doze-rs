//! `doze __fetch` as the daemon uses it: a child per request (`ChildDownloads`), one JSON line each way, against a
//! mirror on disk; a failure comes back as words, not as a dead child.

use doze_fetch::child::{ChildDownloads, Request};

const DOZE: &str = env!("CARGO_BIN_EXE_doze");

struct Dir(std::path::PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn a_child_resolves_from_the_mirror_and_says_why_it_failed() {
    let d = Dir(std::env::temp_dir().join(format!("doze-cli-fetch-{}", std::process::id())));
    let mirror = d.0.join("mirror/valkey");
    std::fs::create_dir_all(&mirror).unwrap();
    let junk = b"not an archive";
    let sha = doze_fetch::registry::signed::sha256_hex(junk);
    std::fs::write(mirror.join("valkey-9.1.1.tar.gz"), junk).unwrap();
    let manifest = format!(
        "engines:\n  valkey:\n    versions:\n      \"9\": 9.1.1\n    artifacts:\n      9.1.1:\n        t:\n          url: valkey-9.1.1.tar.gz\n          sha256: {sha}\n"
    );
    std::fs::write(mirror.join("index.yaml"), manifest).unwrap();
    let child = ChildDownloads {
        program: DOZE.into(),
        args: vec![],
        env: vec![
            ("DOZE_HOME".into(), d.0.join("home").display().to_string()),
            (
                "DOZE_MIRROR".into(),
                format!("file://{}", d.0.join("mirror").display()),
            ),
            (
                "DOZE_REGISTRY".into(),
                d.0.join("no-registry").display().to_string(),
            ),
        ],
    };
    let r = child
        .ask(&Request::ResolveMajor {
            engine: "valkey".into(),
            major: "9".into(),
        })
        .await
        .unwrap();
    assert_eq!((r.version.as_str(), r.error), ("9.1.1", None));
    let r = child
        .ask(&Request::Ensure {
            engine: "valkey".into(),
            full: "9.1.1".into(),
            target: "t".into(),
            expected_sha: None,
        })
        .await
        .unwrap();
    let err = r.error.unwrap_or_default();
    assert!(err.starts_with("extracting"), "{err}");
    let r = child
        .ask(&Request::Module {
            source: "doze/valkey".into(),
            majors: vec![],
            lock: None,
            persist: false,
        })
        .await
        .unwrap();
    let err = r.error.unwrap_or_default();
    assert!(err.starts_with("fetching module index"), "{err}");
}
