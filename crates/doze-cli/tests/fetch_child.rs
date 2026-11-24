//! `doze __fetch` as the daemon uses it: a child per request (`ChildDownloads`), one JSON line each way, against a
//! mirror on disk; a failure comes back as words, not as a dead child.

use doze_fetch::child::{ChildDownloads, Request};

const DOZE: &str = env!("CARGO_BIN_EXE_doze");

/// This machine's platform, as the child names it (a module may only ask for a real platform's build).
fn triple() -> String {
    use std::env::consts::{ARCH, OS};
    match OS {
        "macos" => format!("{ARCH}-apple-darwin"),
        "windows" => format!("{ARCH}-pc-windows-msvc"),
        _ => format!("{ARCH}-unknown-linux-gnu"),
    }
}

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
        "engines:\n  valkey:\n    versions:\n      \"9\": 9.1.1\n    artifacts:\n      9.1.1:\n        {}:\n          url: valkey-9.1.1.tar.gz\n          sha256: {sha}\n",
        triple()
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
            target: triple(),
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

/// One request to a `__fetch` child with its home, mirror and registry under `dir`: its reply and its log (stderr).
#[allow(clippy::unwrap_used)]
fn ask(dir: &std::path::Path, req: &str) -> (String, String) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(DOZE)
        .arg("__fetch")
        .env("DOZE_HOME", dir.join("home"))
        .env(
            "DOZE_MIRROR",
            format!("file://{}", dir.join("mirror").display()),
        )
        .env("DOZE_REGISTRY", dir.join("no-registry"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{req}\n").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A module not pinned and cached here: the registry is asked (here one with nothing in it), and that is said with its
/// end, because a daemon on a fresh doze home does it before it serves, and `up` waits for it while it goes on.
#[test]
fn a_child_says_a_module_fetch_on_its_log() {
    let d = Dir(std::env::temp_dir().join(format!("doze-cli-fetch-mod-{}", std::process::id())));
    std::fs::create_dir_all(&d.0).unwrap();
    let (reply, log) = ask(
        &d.0,
        r#"{"op":"module","source":"valkey","majors":["9"],"lock":null,"persist":false}"#,
    );
    assert!(reply.contains("error"), "{reply}");
    let (start, end) = log.split_once('\n').unwrap();
    assert_eq!(
        start,
        format!(
            "doze: fetching module doze/valkey from {}…",
            d.0.join("no-registry").display()
        )
    );
    assert!(end.starts_with("doze: download failed after "), "{log}");
    assert_eq!(doze_fetch::child::busy(start), Some(true));
    assert_eq!(doze_fetch::child::busy(end.trim_end()), Some(false));
}

/// The child says a download as it starts, on its stderr: the daemon's log, which `doze up` relays while a service
/// wakes (an engine build takes a minute and more; `up` said nothing meanwhile). Only what is not on this machine yet.
#[test]
fn a_child_says_an_engine_download_on_its_log() {
    let d = Dir(std::env::temp_dir().join(format!("doze-cli-fetch-log-{}", std::process::id())));
    let mirror = d.0.join("mirror/valkey");
    std::fs::create_dir_all(&mirror).unwrap();
    let junk = b"not an archive";
    let sha = doze_fetch::registry::signed::sha256_hex(junk);
    std::fs::write(mirror.join("valkey-9.1.1.tar.gz"), junk).unwrap();
    let manifest = format!(
        "engines:\n  valkey:\n    versions:\n      \"9\": 9.1.1\n    artifacts:\n      9.1.1:\n        {}:\n          url: valkey-9.1.1.tar.gz\n          sha256: {sha}\n",
        triple()
    );
    std::fs::write(mirror.join("index.yaml"), manifest).unwrap();
    let ask = |req: &str| ask(&d.0, req);
    let ensure = format!(
        r#"{{"op":"ensure","engine":"valkey","full":"9.1.1","target":"{}","expected_sha":null}}"#,
        triple()
    );
    let (reply, log) = ask(&ensure);
    assert!(reply.contains("extracting"), "{reply}");
    // The start and the end: a daemon not yet serving is waited for while a download is under way.
    let (start, end) = log.split_once('\n').unwrap();
    assert_eq!(
        start,
        format!(
            "doze: downloading valkey 9.1.1 ({}) from {}…",
            triple(),
            d.0.join("mirror/valkey").display()
        )
    );
    assert!(end.starts_with("doze: download failed after "), "{log}");
    assert_eq!(doze_fetch::child::busy(start), Some(true));
    assert_eq!(doze_fetch::child::busy(end.trim_end()), Some(false));
    // A request that downloads nothing says nothing.
    let (reply, log) = ask(r#"{"op":"resolve_major","engine":"valkey","major":"9"}"#);
    assert!(reply.contains("9.1.1"), "{reply}");
    assert_eq!(log, "");
}
