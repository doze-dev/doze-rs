//! What doze-config knows of modules: only what a [`Modules`] says. Nothing module-specific lives
//! in this crate (no port table, no exceptions by name); the module catalog, or a test's fixture,
//! declares it.

use crate::model::ModuleBody;
use doze_hcl::Value;
use std::collections::BTreeMap;

/// What a module declares about itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModuleInfo {
    /// It takes no `version` (it is built in, or has one release line).
    pub versionless: bool,
    /// The port a block gets when it sets neither `port` nor `listen` (0: none).
    pub default_port: u16,
    /// doze does not sit in front of it, so it cannot sleep (Go's `Lifecycle.Supervised`).
    pub supervised: bool,
}

/// A module instance's address, as its attributes are computed for (Go's `engine.Endpoint`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Tcp(String),
    Unix(String),
}

/// The instance a module is asked about.
#[derive(Debug, Clone, Copy)]
pub struct Call<'a> {
    pub ty: &'a str,
    pub name: &'a str,
    pub version: &'a str,
    pub endpoint: &'a Endpoint,
    pub body: &'a ModuleBody,
}

/// What a module publishes besides the attributes every module has.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Attributes {
    /// The env var its clients read (`DATABASE_URL`), "" for none.
    pub env_var: String,
    pub url: String,
    /// Attributes of its own (they override the common ones of the same name).
    pub extras: BTreeMap<String, Value>,
}

/// A listener a module declares besides its main one (its port is held too).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    pub name: String,
    pub port: u16,
}

pub trait Modules {
    /// The module of this type, fetching it if it must (the full load).
    fn lookup(&self, ty: &str, version: &str) -> Option<ModuleInfo>;
    /// The module of this type if it is already on this machine (the shape pass: no network).
    fn cached(&self, ty: &str) -> Option<ModuleInfo>;
    /// Why a type cannot be looked up, when there is more to say than "no module called …".
    fn lookup_error(&self, _ty: &str) -> Option<String> {
        None
    }
    /// Every module type the catalog knows, for "did you mean".
    fn names(&self) -> Vec<String>;
    /// Whether the resolved module serves `version` (checked once every block is read).
    fn check_support(&self, _ty: &str, _version: &str) -> Result<(), String> {
        Ok(())
    }
    /// What an instance publishes to the blocks that reference it.
    fn attributes(&self, _call: &Call<'_>) -> Attributes {
        Attributes::default()
    }
    /// The listeners an instance has besides its main one.
    fn listeners(&self, _call: &Call<'_>, _port: u16) -> Vec<Listener> {
        Vec::new()
    }
}

/// No modules at all: every module block is refused (`Parse` with no catalog).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoModules;

impl Modules for NoModules {
    fn lookup(&self, _ty: &str, _version: &str) -> Option<ModuleInfo> {
        None
    }
    fn cached(&self, _ty: &str) -> Option<ModuleInfo> {
        None
    }
    fn names(&self) -> Vec<String> {
        Vec::new()
    }
}
