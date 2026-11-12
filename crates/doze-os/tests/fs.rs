//! Files and directories against the real OS: a lock held by another process, a directory only the user can open,
//! a clone that is a copy and not a link, and the doze home override.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use common::helper;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-os-fs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[tokio::test(flavor = "current_thread")]
async fn a_lock_held_by_another_process_is_refused_until_that_process_is_gone() {
    let dir = scratch("lock");
    let path = dir.join("daemon.lock");
    let mut holder = helper("hold-lock");
    holder.arg(&path).stdout(Stdio::piped());
    let mut holder = holder.spawn().unwrap();
    let mut line = String::new();
    BufReader::new(holder.stdout.take().unwrap())
        .read_line(&mut line)
        .await
        .unwrap();
    assert_eq!(line.trim(), "locked");
    assert!(
        doze_os::fs::try_lock(&path).unwrap().is_none(),
        "took a lock another process holds"
    );
    holder.start_kill().unwrap();
    holder.wait().await.unwrap();
    // The OS releases the lock when the holder dies, however it dies.
    assert!(
        doze_os::fs::try_lock(&path).unwrap().is_some(),
        "lock not released by the dead holder"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_private_directory_is_open_to_the_user_only() {
    let dir = scratch("private").join("state");
    doze_os::fs::private_dir(&dir).unwrap();
    std::fs::write(dir.join("x"), b"ok").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("icacls")
            .arg(&dir)
            .output()
            .unwrap();
        let acl = String::from_utf8_lossy(&out.stdout).to_string();
        for broad in [
            "BUILTIN\\Users",
            "Authenticated Users",
            "Everyone",
            "BUILTIN\\Administrators",
        ] {
            assert!(!acl.contains(broad), "{broad} can open it:\n{acl}");
        }
        assert!(acl.contains("NT AUTHORITY\\SYSTEM"), "{acl}");
    }
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

#[test]
fn a_clone_has_the_same_content_and_is_independent() {
    let base = scratch("clone");
    let src = base.join("template");
    std::fs::create_dir_all(src.join("base/1")).unwrap();
    std::fs::write(src.join("PG_VERSION"), b"17\n").unwrap();
    std::fs::write(src.join("base/1/rel"), vec![7u8; 64 << 10]).unwrap();
    let dst = base.join("instance");
    doze_os::fs::clone_dir(&src, &dst).unwrap();
    assert_eq!(
        std::fs::read(dst.join("base/1/rel")).unwrap(),
        vec![7u8; 64 << 10]
    );
    std::fs::write(dst.join("PG_VERSION"), b"18\n").unwrap();
    assert_eq!(
        std::fs::read(src.join("PG_VERSION")).unwrap(),
        b"17\n",
        "writing the clone changed the template"
    );
    assert!(
        doze_os::fs::clone_dir(&src, &dst).is_err(),
        "cloned over an existing directory"
    );
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test(flavor = "current_thread")]
async fn doze_home_follows_the_override() {
    let mut c = helper("home");
    c.env(doze_os::home::HOME_ENV, "/somewhere/else");
    let out = tokio::time::timeout(Duration::from_secs(10), c.output())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        PathBuf::from("/somewhere/else").display().to_string()
    );
    let mut c = helper("home");
    c.env_remove(doze_os::home::HOME_ENV);
    let out = c.output().await.unwrap();
    let home = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(
        home.ends_with(if cfg!(windows) { "doze" } else { ".doze" }),
        "{home}"
    );
}
