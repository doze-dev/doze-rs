//! Small questions about the machine, asked of the tools it already has (no privileges, nothing installed): who listens on a port
//! (`lsof`), and how much disk is free (`df`). Each answers `None` where the tool is missing, which callers treat as "not known".

use std::path::Path;
use std::process::{Command, Stdio};

/// The process listening on TCP `port`: its pid and command name.
pub(crate) fn port_owner(port: u16) -> Option<(u32, String)> {
    let out = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fpc"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    parse_lsof(&String::from_utf8_lossy(&out.stdout))
}

fn parse_lsof(text: &str) -> Option<(u32, String)> {
    let pid = text
        .lines()
        .find_map(|l| l.strip_prefix('p')?.parse().ok())?;
    let cmd = text
        .lines()
        .find_map(|l| l.strip_prefix('c'))
        .unwrap_or("?");
    Some((pid, cmd.to_owned()))
}

/// Bytes free for an unprivileged user on the file system holding `path` (or its nearest existing parent).
pub(crate) fn free_bytes(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|p| p.exists())?;
    let out = Command::new("df")
        .arg("-Pk")
        .arg(existing)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    parse_df(&String::from_utf8_lossy(&out.stdout))
}

fn parse_df(text: &str) -> Option<u64> {
    let kb: u64 = text
        .lines()
        .nth(1)?
        .split_whitespace()
        .nth(3)?
        .parse()
        .ok()?;
    Some(kb * 1024)
}

/// Bytes as a person reads them.
pub(crate) fn human(bytes: u64) -> String {
    const GB: u64 = 1 << 30;
    const MB: u64 = 1 << 20;
    if bytes >= GB {
        format!("{} GB", bytes / GB)
    } else {
        format!("{} MB", bytes / MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsof_and_df_output_is_read() {
        assert_eq!(
            parse_lsof("p4242\ncpostgres\nf5\n"),
            Some((4242, "postgres".into()))
        );
        assert_eq!(parse_lsof(""), None);
        let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 100 50 2048 50% /\n";
        assert_eq!(parse_df(df), Some(2048 * 1024));
        assert_eq!(human(3 << 30), "3 GB");
    }
}
