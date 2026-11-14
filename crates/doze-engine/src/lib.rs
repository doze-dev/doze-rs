//! doze-engine: a doze stack at run time (docs/port/core/PLAN.md, phase E1).
//!
//! - [`lifecycle`]: every instance's state machine, with no I/O (Go's `registry` and the crash decision).
//! - [`Engine`]: wake on demand with one shared start per cold instance, needs woken and held first, the reaper, holders,
//!   pin and off, stop and shutdown, the crash watcher and restart policy.
//! - [`exec`] and [`supervise`]: a plan's processes started in order as doze-os trees, gated on [`ready`], then owned
//!   by a task that reports the first exit; nothing a unit started outlives it.
//! - [`process`]: the built-in process engine (`app`, `service`): argument lists, `$PORT`/`$HOST`/`$DATA`, restart
//!   policy, `setup`/`on_ready`/`stop` [`steps`].
//! - [`stack`]: what the engine runs, built in code ([`Engine::builder`]) until doze-config fills it from doze.hcl.
//!
//! Go is read for *what* (the behaviours are catalogued in docs/port/core/behaviours/engine.md), never for *how*.

pub mod applied;
pub mod error;
pub mod events;
pub mod exec;
pub mod godur;
pub mod graph;
pub mod lifecycle;
pub mod logs;
pub mod machine;
pub mod pidfile;
pub mod porttrap;
pub mod process;
pub mod ready;
pub mod reaper;
pub mod restart;
pub mod stack;
pub mod state;
pub mod steps;
pub mod supervise;
pub mod tasks;
pub mod watch;

mod boot;
mod engine;
mod short;
mod stop;

pub use engine::{Builder, Engine};
pub use error::{BuildError, Error};
pub use events::{Cause, Event};
pub use process::{Probe, ProcessSpec, Step};
pub use reaper::Background;
pub use restart::{Policy, Restart};
pub use stack::{Endpoint, Need, PlanCtx, Planned, Planner, Service, Sleep, Stack, When};
pub use state::{Holder, Instance, State};
