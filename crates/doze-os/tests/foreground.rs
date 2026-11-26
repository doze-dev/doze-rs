//! A command run in the foreground (`doze run`, `doze connect`) hears each SIGINT once: one sent to doze alone, one
//! sent to doze's whole group, and Ctrl-C typed at the terminal. The command is `sh` counting its SIGINTs (each breaks
//! off the `wait` it is in) and exiting with 40 + the count; the helper plays doze (`doze_os::foreground::run`).
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod common;

use rustix::process::{Pid, Signal};
use std::io::{Read as _, Write as _};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The command: counts SIGINTs for three seconds, after saying it is ready (`mark`). It waits in the `wait` builtin,
/// which a trapped signal always breaks off (POSIX), on a `sleep` in the background (which ignores SIGINT, so it is
/// not ended by one). A loop of foreground commands would not do: bash 3.2 (macOS's `sh`) drops a trapped SIGINT that
/// lands after it has reaped one and before it puts its trap handler back (about 1 in 100 in a loop of them, with no
/// doze involved), which failed this test on CI with 40.
fn script(mark: &Path) -> String {
    format!(
        "n=0; trap 'n=$((n+1))' INT; sleep 3 & : > '{}'; until wait; do :; done; exit $((40+n))",
        mark.display()
    )
}

fn mark(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("doze-os-fg-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

/// The helper as doze (each test puts it in a group or session of its own, so the test's group is never signalled).
fn doze(mark: &Path) -> Command {
    let mut c = Command::new(common::HELPER);
    c.arg("foreground").arg(script(mark));
    c
}

/// Waits for the command's mark. That is enough: the command writes it once its trap is set, and doze's handlers are
/// in before the command starts.
fn ready(mark: &Path, child: &mut Child) {
    let t0 = Instant::now();
    while !mark.exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "the helper ended early"
        );
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the command never started"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn pid(child: &Child) -> Pid {
    Pid::from_raw(i32::try_from(child.id()).unwrap()).unwrap()
}

/// `kill -INT <doze>`: the command did not hear it before (it was taken by doze alone); now it is passed on, once.
#[test]
fn a_sigint_sent_to_doze_alone_reaches_the_command_once() {
    let m = mark("alone");
    let mut child = doze(&m)
        .process_group(0)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    ready(&m, &mut child);
    rustix::process::kill_process(pid(&child), Signal::INT).unwrap();
    let status = child.wait().unwrap();
    let _ = std::fs::remove_file(&m);
    assert_eq!(status.code(), Some(41), "{status}");
}

/// `kill -INT -<doze's group>` (an IDE stopping what it ran): the command hears it once, not from both sides.
#[test]
fn a_sigint_sent_to_dozes_group_reaches_the_command_once() {
    let m = mark("group");
    let mut child = doze(&m)
        .process_group(0)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    ready(&m, &mut child);
    rustix::process::kill_process_group(pid(&child), Signal::INT).unwrap();
    let status = child.wait().unwrap();
    let _ = std::fs::remove_file(&m);
    assert_eq!(status.code(), Some(41), "{status}");
}

/// Ctrl-C typed at the terminal doze runs on: the terminal is the command's, so it hears it once (doze does not pass
/// on a second), and doze reports how it ended.
#[test]
fn ctrl_c_at_the_terminal_reaches_the_command_once() {
    let m = mark("tty");
    let status = on_terminal(&m, &script(&m), b"\x03");
    assert_eq!(status.code(), Some(41), "{status}");
}

/// The command reads the terminal (a database shell's prompt): it is in the terminal's foreground, not stopped for
/// reading from the background.
#[test]
fn the_command_reads_the_terminal() {
    let m = mark("read");
    let read = format!(": > '{}'; read x; exit $((50 + ${{#x}}))", m.display());
    let status = on_terminal(&m, &read, b"abc\n");
    assert_eq!(status.code(), Some(53), "{status}");
}

/// Runs the helper as doze on a new pseudo-terminal that is its controlling terminal (a session of its own, as a
/// login shell's job has), types `keys` once the command is ready, and gives how doze ended.
#[allow(unsafe_code)]
fn on_terminal(mark: &Path, script: &str, keys: &[u8]) -> std::process::ExitStatus {
    let pty = doze_os::pty::open().unwrap();
    let mut cmd = Command::new(common::HELPER);
    cmd.arg("foreground")
        .arg(script)
        .stdin(pty.terminal.try_clone().unwrap())
        .stdout(pty.terminal.try_clone().unwrap())
        .stderr(pty.terminal.try_clone().unwrap());
    let hook = || {
        rustix::process::setsid()?;
        rustix::process::ioctl_tiocsctty(rustix::stdio::stdin())?;
        Ok(())
    };
    // SAFETY: the hook runs in the child between fork and exec and makes two syscalls through rustix: no allocation,
    // no locks, async-signal-safe.
    unsafe { cmd.pre_exec(hook) };
    let mut child = cmd.spawn().unwrap();
    drop(cmd);
    drop(pty.terminal);
    let mut controller = pty.controller;
    let mut writer = controller.try_clone().unwrap();
    // Drained, so nothing the command writes blocks it.
    let _reader = std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        while let Ok(n) = controller.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
    });
    ready(mark, &mut child);
    writer.write_all(keys).unwrap();
    let t0 = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if t0.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            panic!("doze did not end (the command stopped?)");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = std::fs::remove_file(mark);
    status
}
