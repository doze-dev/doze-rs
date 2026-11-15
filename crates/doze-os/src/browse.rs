//! Opening a web page in the person's browser: `open` on macOS, `xdg-open` elsewhere on Unix, the shell's `start` on
//! Windows (through `explorer`, which hands a URL to the default browser without a console window or a shell parsing
//! the URL). Nothing waits for the browser; only a launcher that could not start is an error.

use std::io;
use std::process::{Command, Stdio};

/// Opens `url` (an `http`/`https` URL) in the default browser.
pub fn open(url: &str) -> io::Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "only http and https URLs are opened",
        ));
    }
    let mut cmd = Command::new(launcher());
    cmd.arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn().map(drop)
}

/// The program that hands a URL to the browser here.
pub fn launcher() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    }
}
