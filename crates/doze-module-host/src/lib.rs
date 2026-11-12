//! Running doze modules from the host (docs/port/modules/PLAN.md step 3): launch a module process, take its hello,
//! call it through typed methods, serve its callbacks (downloads during `resolve`, the backend during `wire.accept`),
//! and keep one per engine in a pool that restarts the dead and stops the idle.

pub mod pool;
pub mod running;
pub mod services;

pub use pool::{Locate, Pool};
pub use running::{Running, launch};
pub use services::{Backends, BoxFuture, Downloads, Notice, Notices, Services};
