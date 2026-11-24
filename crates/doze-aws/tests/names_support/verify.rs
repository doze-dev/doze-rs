//! The front door's certificates checked by TLS stacks other than rustls: curl trusting only the test's root, and on
//! macOS the system's own verifier (SecTrust, through `security verify-cert` with the root as the only anchor and no
//! keychain search list, so nothing on the machine is read or changed).

use std::path::{Path, PathBuf};
use std::process::Command;

/// `https://<host>/` through the front door at `tls` with curl, trusting only `root_pem`: the handshake must verify
/// (any HTTP status will do).
pub(crate) fn by_curl(tls: &str, root_pem: &Path, host: &str) {
    let port = tls.rsplit_once(':').unwrap().1;
    let out = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "--max-time", "20", "--cacert"])
        .arg(root_pem)
        .arg("--resolve")
        .arg(format!("{host}:{port}:127.0.0.1"))
        .arg(format!("https://{host}:{port}/"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "curl {host} ({} chars): {}",
        host.len(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The leaf the front door minted for `host` under `ca_dir`: kept as up to 64 characters of the name, a dash and a hash.
fn leaf_file(ca_dir: &Path, host: &str) -> PathBuf {
    let prefix: String = host.chars().take(64).collect();
    let prefix = format!("{}-", prefix.trim_end_matches('.'));
    std::fs::read_dir(ca_dir.join("certs/under"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|f| f.starts_with(&prefix) && f.ends_with(".crt"))
        })
        .unwrap_or_else(|| panic!("no leaf kept for {host}"))
}

/// The leaf for `host` evaluated by SecTrust for an SSL server of that name, anchored at the test's root only.
#[cfg(target_os = "macos")]
pub(crate) fn by_sectrust(ca_dir: &Path, host: &str) {
    let out = Command::new("/usr/bin/security")
        .arg("verify-cert")
        .arg("-c")
        .arg(leaf_file(ca_dir, host))
        .arg("-r")
        .arg(ca_dir.join("root.crt"))
        .args(["-p", "ssl", "-n", host, "-N", "-L"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "SecTrust {host} ({} chars): {}{}",
        host.len(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn by_sectrust(ca_dir: &Path, host: &str) {
    let _ = leaf_file(ca_dir, host);
}
