//! A pseudo-terminal, so a test can run a program the way a person does: its output on a terminal, not a pipe. The
//! CLI draws differently on a terminal (colour, the once-a-day update line), and only a real terminal proves the
//! detection. Unix only; Windows answers `Unsupported` (its ConPTY rewrites output into screen updates, so a byte
//! transcript there would test ConPTY, not doze).

use std::fs::File;
use std::io;

/// The two ends: the test reads `controller`; the program gets `terminal` as its stdin, stdout and stderr.
#[derive(Debug)]
pub struct Pty {
    pub controller: File,
    pub terminal: File,
}

/// Opens a new pseudo-terminal pair.
pub fn open() -> io::Result<Pty> {
    imp::open()
}

#[cfg(unix)]
mod imp {
    use super::Pty;
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    use std::fs::{File, OpenOptions};
    use std::io;

    pub(super) fn open() -> io::Result<Pty> {
        let controller = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)?;
        grantpt(&controller)?;
        unlockpt(&controller)?;
        let name = ptsname(&controller, Vec::new())?;
        let path = name
            .into_string()
            .map_err(|_| io::Error::other("the terminal's name is not UTF-8"))?;
        let terminal = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Pty {
            controller: File::from(controller),
            terminal,
        })
    }
}

#[cfg(windows)]
mod imp {
    use super::Pty;
    use std::io;

    pub(super) fn open() -> io::Result<Pty> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no pseudo-terminal on Windows",
        ))
    }
}
