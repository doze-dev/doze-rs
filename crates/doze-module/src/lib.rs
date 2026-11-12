//! The SDK doze modules are written against (docs/port/modules/sdk.md).
//!
//! A module is a type implementing [`Module`] with a config type deriving [`Spec`]; its binary's `main` is
//! `doze_module::serve(MyModule).release(env!("CARGO_PKG_VERSION")).run()`, adding capability registrations
//! (`.converge()`, `.admin()` …) for the [`caps`] traits it implements.

pub mod caps;
pub mod fetch;
mod fields;
pub mod help;
pub mod module;
mod register;
mod register_io;
mod route;
pub mod server;
pub mod spec;

pub use doze_module_derive::Spec;
pub use doze_module_proto as proto;
pub use module::{About, Cx, Module, Result};
pub use server::{Server, serve};

/// Prints why the module cannot run and returns a failing exit code (the host shows stderr when a module dies).
pub(crate) fn exit_with(message: &str) -> std::process::ExitCode {
    let mut err = std::io::stderr();
    let _ = std::io::Write::write_all(&mut err, format!("doze module: {message}\n").as_bytes());
    std::process::ExitCode::FAILURE
}
