//! Executable lookup against a real directory, and running what was found with arguments that need quoting.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use doze_os::which::which_in;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("doze-os-which-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[cfg(unix)]
fn write_script(dir: &std::path::Path, executable: bool) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("hello");
    std::fs::write(&p, "#!/bin/sh\necho \"[$1] [$2]\"\n").unwrap();
    std::fs::set_permissions(
        &p,
        std::fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
    )
    .unwrap();
    p
}

#[cfg(windows)]
fn write_script(dir: &std::path::Path, _executable: bool) -> PathBuf {
    let p = dir.join("hello.cmd");
    std::fs::write(&p, "@echo off\r\necho [%~1] [%~2]\r\n").unwrap();
    p
}

#[test]
fn finds_a_script_and_runs_it_with_arguments_intact() {
    let dir = scratch("found");
    let script = write_script(&dir, true);
    let path = std::env::join_paths([dir.clone(), PathBuf::from("/nonexistent")]).unwrap();
    let found =
        which_in("hello", &path, Some(std::ffi::OsStr::new(".EXE;.CMD"))).expect("not found");
    assert_eq!(found, script);
    let out = std::process::Command::new(&found)
        .args(["a b", "c"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "[a b] [c]");
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn a_file_without_the_execute_bit_is_not_a_program() {
    let dir = scratch("noexec");
    write_script(&dir, false);
    assert_eq!(which_in("hello", dir.as_os_str(), None), None);
    let _ = std::fs::remove_dir_all(dir);
}
