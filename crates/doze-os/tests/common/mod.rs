//! Shared by the integration tests: the helper binary, its frame format, and a portable "is this pid alive".
#![allow(dead_code, unreachable_pub)]

use std::process::Stdio;
use std::time::{Duration, Instant};

pub const HELPER: &str = env!("CARGO_BIN_EXE_doze-os-helper");

pub fn helper(mode: &str) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(HELPER);
    c.arg(mode).stdin(Stdio::null()).kill_on_drop(true);
    c
}

pub fn frame(flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = (payload.len() as u32).to_le_bytes().to_vec();
    v.push(flags);
    v.extend_from_slice(payload);
    v
}

/// Pops one complete frame's payload off the front of `buf`.
pub fn take_frame(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    let len = u32::from_le_bytes(buf.get(..4)?.try_into().ok()?) as usize;
    let payload = buf.get(5..5 + len)?.to_vec();
    buf.drain(..5 + len);
    Some(payload)
}

/// Whether a process with this pid exists, asked of the OS's own tool (the tests do not link platform code).
pub fn alive(pid: u32) -> bool {
    if cfg!(windows) {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output();
        out.map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    } else {
        // `ps` sees zombies too; ask for the state and treat Z as gone.
        let out = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output();
        out.map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            let s = s.trim();
            !s.is_empty() && !s.starts_with('Z')
        })
        .unwrap_or(false)
    }
}

/// Polls until the pid is gone; returns how long it took, or None after `limit`.
pub async fn gone_within(pid: u32, limit: Duration) -> Option<Duration> {
    let t0 = Instant::now();
    while t0.elapsed() < limit {
        if !alive(pid) {
            return Some(t0.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

/// Kills a stray test process by its exact pid (cleanup only).
pub fn kill_pid(pid: u32) {
    if cfg!(windows) {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output();
    } else {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }
}
