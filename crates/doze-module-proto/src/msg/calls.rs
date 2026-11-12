//! Small argument and answer types of calls that have no other home.

use super::instance::{Instance, Object, Toolchain};
use minicbor::{Decode, Encode};

/// `template.ensure`: provision a shared template directory once.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct TemplateEnsure {
    #[n(0)]
    pub toolchain: Toolchain,
    #[n(1)]
    pub template_dir: String,
}

/// `template.clone`: make `dest_dir` a copy of `template_dir`.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct TemplateClone {
    #[n(0)]
    pub template_dir: String,
    #[n(1)]
    pub dest_dir: String,
}

/// `prune`: drop objects no longer declared.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct PruneArgs {
    #[n(0)]
    pub instance: Instance,
    #[n(1)]
    pub toolchain: Toolchain,
    #[n(2)]
    pub removed: Vec<Object>,
}

/// `run_action`: run an admin action on a resource.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct RunAction {
    #[n(0)]
    pub instance: Instance,
    #[n(1)]
    pub action: String,
    #[n(2)]
    pub resource: String,
    #[n(3)]
    pub input: Option<String>,
}

/// `shutdown`: finish what is in flight within the deadline, then exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct Shutdown {
    #[n(0)]
    pub grace_ms: u32,
}

/// An empty argument or answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct Unit {}
