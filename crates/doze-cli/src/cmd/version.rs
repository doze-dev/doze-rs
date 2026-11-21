//! `doze version`. The first line keeps Go's shape, `doze X.Y.Z (os/arch, …)`: the website's version picker parses
//! what people paste.
//!
//! `doze --json version` is the machine-readable answer a program that talks to doze reads (the desktop app checks a
//! `doze` on the PATH with it, to say when that doze is too old). Its `data` is stable; fields are only ever added:
//!
//! - `version`: the release (`DOZE_VERSION` at build time), `"dev"` for a build that set none.
//! - `commit`: the source commit (`DOZE_COMMIT` at build time), `null` for a build that set none.
//! - `os`, `arch`: the platform, as Go's `GOOS`/`GOARCH` name it.
//! - `protocol`: the module protocol's major (what a module must speak to run under this doze).
//! - `control_protocol`: the control protocol (CLI and app to the stack's daemon) this doze speaks and serves.

use crate::ctx::{COMMIT, VERSION};
use crate::output::{Done, Outcome};
use crate::platform;
use crate::view::View;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Info {
    pub version: &'static str,
    /// The source commit this doze was built from, when the build said it.
    pub commit: Option<&'static str>,
    pub os: String,
    pub arch: String,
    /// The module protocol this doze speaks.
    pub protocol: u16,
    /// The control protocol this doze's CLI and daemon speak.
    pub control_protocol: i64,
}

impl Info {
    pub(crate) fn human(&self) -> String {
        format!(
            "doze {} ({}/{}, module protocol {})\n",
            self.version, self.os, self.arch, self.protocol
        )
    }
}

pub(crate) fn run() -> Outcome {
    let os_arch = platform::os_arch();
    let (os, arch) = os_arch.split_once('/').unwrap_or((&os_arch, ""));
    Ok(Done::ok(View::Version(Info {
        version: VERSION,
        commit: COMMIT,
        os: os.to_string(),
        arch: arch.to_string(),
        protocol: doze_module_proto::version::CURRENT.major,
        control_protocol: doze_control::PROTO_VERSION,
    })))
}
