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
            Error::Io(e) => write!(f, "the names registry: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}
