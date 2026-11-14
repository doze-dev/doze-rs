//! doze.hcl (core plan phase H4): doze's schema over doze-hcl. It reads a stack's files the way
//! Go's `internal/config` does and gives the engine its typed stack, or the diagnostics Go gives,
//! at Go's positions. The oracle corpus in `testdata/oracle` holds Go's answer for 257 sources.
//!
//! What doze-config knows of the machine comes through a [`Host`]; what it knows of modules,
//! through [`Modules`]. Nothing module-specific is in this crate.
//!
//! The pipeline, in Go's order: files ([`files`]), old forms and the root schema ([`kinds`]),
//! overrides (`overrides`), variables and locals ([`vars`]), defaults and tls (`defaults`),
//! module and app blocks declared (`instance`, `foreach`), services and tasks declared
//! (`runnable`), bodies decoded in dependency order (`order`, `deps`, [`command`],
//! `module_body`, `shape`), cycles (`cycles`), outputs, warnings (`lint`), and the checks
//! across the stack ([`validate`]).

mod attrs;
mod body;
pub mod command;
mod cycles;
mod defaults;
mod deps;
pub mod error;
mod eval;
pub mod files;
mod fingerprint;
mod foreach;
mod gohcl;
pub mod gotime;
pub mod host;
mod instance;
pub mod kinds;
mod lint;
mod load;
pub mod model;
mod module_body;
pub mod modules;
mod order;
mod override_expr;
mod overrides;
pub mod paths;
pub mod psl;
mod refs;
mod render;
mod runcmd;
mod runnable;
pub mod runspec;
mod shape;
pub mod stack;
pub mod validate;
pub mod vars;
pub mod version;

pub use attrs::{instance_addr, join_host_port, split_host_port};
pub use error::{Error, Fault};
pub use fingerprint::BlockPrint;
pub use foreach::MAX_COUNT;
pub use host::{DomainChecks, Host, OsHost};
pub use load::{Options, load, load_shape, parse};
pub use model::{Config, Instance, Kind, Ref, Spec};
pub use modules::{ModuleInfo, Modules, NoModules};
pub use stack::{Wiring, ca_env, to_stack};
