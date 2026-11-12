//! Every operating-system difference doze has, behind one portable API (docs/port/modules/PLAN.md D11, `p1.md`).
//!
//! No other crate in the workspace contains `#[cfg(unix)]` or `#[cfg(windows)]`, and this is the only crate allowed
//! `unsafe`: Win32 calls and one async-signal-safe fork hook. Each `unsafe` block does one thing and says why it is
//! sound, and each is wrapped in a safe function.
//!
//! - [`channel`]: the control channel to a module process, including handing it a client connection.
//! - [`tree`]: a process and everything it starts, stopped gently or killed, and gone when doze is gone.
//! - [`privilege`]: never run engines with administrator rights.
//! - [`which`]: find a program on `PATH` without a shell.

pub mod channel;
pub mod privilege;
pub mod tree;
pub mod which;

#[cfg(windows)]
mod winsock;
