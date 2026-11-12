//! Resolving, provisioning and running an instance: the arguments and answers of those calls, and the host's
//! fetch callbacks a module may make while `resolve` runs.

use super::instance::{Instance, Toolchain};
use minicbor::{Decode, Encode};
use std::collections::BTreeMap;

/// A pinned engine version from `doze.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Pin {
    #[n(0)]
    pub engine: String,
    /// The spec as declared ("17").
    #[n(1)]
    pub spec: String,
    #[n(2)]
    pub resolved: String,
    /// target triple -> "sha256:<hex>"
    #[n(3)]
    #[cbor(default)]
    pub hashes: BTreeMap<String, String>,
}

/// `resolve`: find (or have the host fetch) the toolchain.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct ResolveArgs {
    #[n(0)]
    pub instance: Instance,
    /// The host's target triple.
    #[n(1)]
    pub target: String,
    /// Pins from the lock relevant to this engine.
    #[n(2)]
    #[cbor(default)]
    pub pins: Vec<Pin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Resolved {
    #[n(0)]
    pub toolchain: Toolchain,
    /// Pins to record in the lock.
    #[n(1)]
    #[cbor(default)]
    pub pins: Vec<Pin>,
}

/// `fetch.resolve_major` (module to host, only while the `resolve` call `for_call` runs).
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ResolveMajor {
    #[n(0)]
    pub engine: String,
    #[n(1)]
    pub major: String,
}

/// `fetch.ensure` (module to host, only while `resolve` runs): make this exact build present and verified.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Ensure {
    #[n(0)]
    pub engine: String,
    #[n(1)]
    pub full: String,
    #[n(2)]
    pub target: String,
    /// From the lock, when pinned: the download must match.
    #[n(3)]
    pub expected_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Ensured {
    #[n(0)]
    pub bin_dir: String,
    #[n(1)]
    pub digest: String,
}

/// Arguments of the calls that act on an instance with its toolchain (provision, plan, converge, template).
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct WithToolchain {
    #[n(0)]
    pub instance: Instance,
    #[n(1)]
    pub toolchain: Toolchain,
}

/// What `provision` found or did.
/// Not `index_only`, so a variant added later reads as `None` in an `Option` (tests/compat.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum Provisioned {
    /// Just initialised: the host converges it.
    #[n(0)]
    Fresh,
    /// Already initialised.
    #[n(1)]
    Existing,
}

/// How to run an instance: the host starts and supervises these (protocol 1's SpawnPlan, without shell strings).
#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode)]
pub struct Plan {
    #[n(0)]
    pub processes: Vec<Process>,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Process {
    /// Unique within the plan.
    #[n(0)]
    pub name: String,
    #[n(1)]
    pub program: String,
    #[n(2)]
    #[cbor(default)]
    pub args: Vec<String>,
    /// Added to (and overriding) the host's environment for the process.
    #[n(3)]
    #[cbor(default)]
    pub env: BTreeMap<String, String>,
    #[n(4)]
    pub dir: Option<String>,
    /// Names of processes that must be ready first.
    #[n(5)]
    #[cbor(default)]
    pub after: Vec<String>,
    #[n(6)]
    pub ready: Option<Ready>,
    /// Commands (program and arguments, no shell) run once this process is ready.
    #[n(7)]
    #[cbor(default)]
    pub hooks: Vec<Vec<String>>,
    /// How to ask it to stop (PLAN.md D15); the host kills it after the grace period either way.
    #[n(8)]
    pub stop: Option<StopHow>,
}

/// A readiness check.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum Ready {
    #[n(0)]
    Tcp(#[n(0)] String),
    #[n(1)]
    Socket(#[n(0)] String),
    #[n(2)]
    Http(#[n(0)] String),
    /// A command (no shell) that exits 0 when ready.
    #[n(3)]
    Exec(#[n(0)] Vec<String>),
    /// A regular expression a line of the process's output must match.
    #[n(4)]
    LogLine(#[n(0)] String),
}

/// How a process is asked to stop.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub enum StopHow {
    #[n(0)]
    Terminate,
    #[n(1)]
    Interrupt,
    #[n(2)]
    Command(#[n(0)] Vec<String>),
}

/// How a supervised instance restarts after an unexpected exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct Restart {
    /// 0 never, 1 on failure, 2 always.
    #[n(0)]
    pub policy: u8,
    #[n(1)]
    pub backoff_ms: u32,
    #[n(2)]
    pub max_retries: u32,
}
