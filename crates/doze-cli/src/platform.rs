//! This machine, named two ways: the target triple the registry keys artifacts by (`aarch64-apple-darwin`), and the
//! `os/arch` the `doze version` line has always printed (`darwin/arm64`), which the website's version picker parses
//! from what people paste.

use std::env::consts::{ARCH, OS};

/// The registry's artifact key for this machine.
pub fn triple() -> String {
    match OS {
        "macos" => format!("{ARCH}-apple-darwin"),
        "windows" => format!("{ARCH}-pc-windows-msvc"),
        "linux" => format!("{ARCH}-unknown-linux-gnu"),
        other => format!("{ARCH}-unknown-{other}"),
    }
}

/// `darwin/arm64`, as Go's `GOOS/GOARCH`.
pub fn os_arch() -> String {
    let os = match OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        "x86" => "386",
        other => other,
    };
    format!("{os}/{arch}")
}
