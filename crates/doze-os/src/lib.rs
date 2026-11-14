//! Every operating-system difference doze has, behind one portable API (docs/port/modules/PLAN.md D11, `p1.md`).
//!
//! No other crate in the workspace contains `#[cfg(unix)]` or `#[cfg(windows)]`, and this is the only crate allowed
//! `unsafe`: Win32 calls and one async-signal-safe fork hook. Each `unsafe` block does one thing and says why it is
//! sound, and each is wrapped in a safe function.
//!
//! - [`autostart`]: open the desktop app at login (a LaunchAgent, an XDG entry), in a folder the caller names.
//! - [`channel`]: the control channel to a module process, including handing it a client connection.
//! - [`tree`]: a process and everything it starts, stopped gently or killed, and gone when doze is gone.
//! - [`privilege`]: never run engines with administrator rights.
//! - [`process`]: whether a process is still there, by pid.
//! - [`which`]: find a program on `PATH` without a shell.
//! - [`fs`]: locks between processes, owner-only directories, cloning a data directory.
//! - [`home`]: where doze keeps its state.
//! - [`net`]: dial an engine's backend, a Unix socket or loopback TCP, as one stream type.
//! - [`detach`]: start the daemon so it outlives its starter (and can still stop engines gently).
//! - [`local`]: the daemon's control socket as blocking I/O, for clients that run no async runtime.
//! - `ui` (feature `ui`): the desktop app's menu-bar item and Dock badge.
//! - [`browse`]: open a web page in the person's browser.
//! - [`pty`]: a pseudo-terminal, for tests that must see what a person sees.
//! - [`interrupt`]: Ctrl-C and SIGTERM as a flag a blocking command checks, so a follow ends cleanly.

pub mod autostart;
pub mod browse;
pub mod channel;
pub mod detach;
pub mod fs;
pub mod home;
pub mod interrupt;
pub mod local;
pub mod net;
pub mod privilege;
pub mod process;
pub mod pty;
pub mod tree;
#[cfg(feature = "ui")]
pub mod ui;
pub mod which;

#[cfg(windows)]
mod winsec;
#[cfg(windows)]
mod winsock;
