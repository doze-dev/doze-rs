//! doze-fetch: what doze downloads, and the proof it is what was published (docs/port/core/PLAN.md, phase F1).
//!
//! - [`registry`]: the signed module registry (moved here from doze-cli, PR #10): its hosts in turn within a deadline,
//!   the release index and its two signatures, release selection, module documentation, the doze release file.
//! - [`lock`]: `doze.lock`, read and written in the Go doze's exact format ([`yaml_out`]).
//! - [`archive`]: unpacking a verified archive into the cache, whole or not at all.
//! - [`mirror`]: engine builds from the doze-binaries mirror, checked against their checksum and signature.
//! - [`modules`]: module builds from the registry, verified, cached and pinned in the lock.
//! - [`child`]: `doze __fetch`, so a download (and its HTTPS client) never lives in the daemon, and the host side of
//!   the module protocol's `fetch.*` calls that runs it.

pub mod archive;
pub mod child;
pub mod download;
pub mod http;
pub mod lock;
pub mod mirror;
pub mod modules;
pub mod registry;
pub mod yaml_out;

/// The registry's artifact key for this machine (`aarch64-apple-darwin`).
pub fn triple() -> String {
    use std::env::consts::{ARCH, OS};
    match OS {
        "macos" => format!("{ARCH}-apple-darwin"),
        "windows" => format!("{ARCH}-pc-windows-msvc"),
        "linux" => format!("{ARCH}-unknown-linux-gnu"),
        other => format!("{ARCH}-unknown-{other}"),
    }
}
