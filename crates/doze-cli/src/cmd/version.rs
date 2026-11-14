//! `doze version`. The first line keeps Go's shape, `doze X.Y.Z (os/arch, …)`: the website's version picker parses
//! what people paste.

use crate::ctx::VERSION;
use crate::output::{Done, Outcome};
use crate::platform;
use crate::view::View;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Info {
    pub version: &'static str,
    pub os: String,
    pub arch: String,
    /// The module protocol this doze speaks.
    pub protocol: u16,
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
        os: os.to_string(),
        arch: arch.to_string(),
        protocol: doze_module_proto::version::CURRENT.major,
    })))
}
