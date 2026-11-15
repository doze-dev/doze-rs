//! What can go wrong claiming a name, each saying who is involved: "address in use" says nothing about who has it.

use std::fmt;

/// A refused or failed registry operation.
#[derive(Debug)]
pub enum Error {
    /// The name belongs to another live process. Not a failure to retry: the holder keeps it until it exits, which
    /// is the point of first-come.
    Held {
        host: String,
        pid: u32,
        owner: String,
    },
    /// The address asked for is another live name's.
    AddrTaken {
        ip: String,
        host: String,
        pid: u32,
        owner: String,
    },
    /// The registry was written by a newer doze; this build does not know what it would be throwing away.
    NewerFormat { path: String, have: u32, want: u32 },
    /// The stack name is another project's.
    StackTaken {
        stack: String,
        dir: String,
        pid: u32,
    },
    /// An apex name with no reserved address: it cannot be invented at runtime (Linux writes apex addresses into
    /// /etc/hosts at setup).
    NoApexAddress(String),
    /// Every address in the block belongs to a running service.
    PoolFull(String),
    /// The name is not this process's to change.
    NotHeld(String),
    /// The certificate authority could not be made, read or used.
    Ca(String),
    /// The doze network is not up, so a translated name cannot be listened on: setup has not run, or its daemon is
    /// not running. Says what was tried.
    NetworkDown { host: String, why: std::io::Error },
    /// An address that is none of this process's names, asked to listen on as one.
    NotMine(String),
    /// The privileged script of a setup or trust run did not run, or failed: `what` is what it was for (the prompt's
    /// words), `why` the runner's or the shell's error, kept as it came.
    Privileged { what: String, why: std::io::Error },
    /// Setup cannot be done here, or cannot start: says why and changes nothing.
    Setup(String),
    /// Reading or writing the registry failed.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Held { host, pid, owner } => write!(f, "{host} is held by pid {pid} ({owner})"),
            Error::AddrTaken {
                ip,
                host,
                pid,
                owner,
            } => {
                write!(f, "{ip} is the address of {host} (pid {pid}, {owner})")
            }
            Error::NewerFormat { path, have, want } => write!(
                f,
                "{path} is format {have}, written by a newer doze than this one (which understands up to {want}): \
                 upgrade this program, or stop every doze program and remove the file"
            ),
            Error::StackTaken { stack, dir, pid } => {
                write!(
                    f,
                    "stack name {stack:?} is already in use by {dir} (pid {pid})"
                )
            }
            Error::NoApexAddress(host) => write!(
                f,
                "no reserved address for apex name {host:?} (add it to the apex table: the value is a \
                 compatibility commitment)"
            ),
            Error::PoolFull(cidr) => write!(
                f,
                "every address in {cidr} is in use by a running doze service"
            ),
            Error::NotHeld(host) => write!(f, "{host} is not held by this process"),
            Error::Ca(why) => write!(f, "the doze certificate authority: {why}"),
            Error::NetworkDown { host, why } => write!(
                f,
                "{host}: the doze network is not running (run `doze dns-setup`): {why}"
            ),
            Error::NotMine(addr) => {
                write!(f, "listen {addr}: no name of this program has that address")
            }
            Error::Privileged { what, why } => write!(f, "setup ({what}): {why}"),
            Error::Setup(why) => write!(f, "setup: {why}"),
            Error::Io(e) => write!(f, "the names registry: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Privileged { why, .. } | Error::NetworkDown { why, .. } | Error::Io(why) => {
                Some(why)
            }
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}
