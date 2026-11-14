//! What a start that cannot go ahead says: who is in the way, and the commands that get past it.

use std::net::SocketAddr;
use std::path::Path;

use crate::instance::endpoint_of;

/// The data directory is held by another process: most often a doze-aws already running here, which is named.
pub(crate) fn data_dir_held(dir: &Path) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let shown = crate::config::display_path(dir, &cwd);
    match crate::instance::read(dir) {
        Some(other) => format!(
            "doze-aws is already running on {shown} (pid {}, {}).\n\n  \
             Use it:            eval \"$(doze-aws env)\"\n  \
             Or stop it:        kill {}\n  \
             Or run a second:   doze-aws --data-dir <another directory> --listen 127.0.0.1:0",
            other.pid, other.endpoint, other.pid
        ),
        None => format!(
            "another process holds the data directory {shown}.\n\n  \
             Run with another:  doze-aws --data-dir <another directory>"
        ),
    }
}

/// The address is taken: by a doze-aws (asked over `/_doze/health`), by a program `lsof` can name, or by something.
pub(crate) fn port_taken(addr: SocketAddr) -> String {
    let next = SocketAddr::new(addr.ip(), addr.port().wrapping_add(1));
    let who = match crate::locate::health(&endpoint_of(addr)) {
        Some(other) => format!(
            "another doze-aws (pid {}, data {})",
            other.pid, other.data_dir
        ),
        None => crate::probe::port_owner(addr.port()).map_or_else(
            || "another program".to_owned(),
            |(pid, cmd)| format!("{cmd} (pid {pid})"),
        ),
    };
    format!(
        "doze-aws cannot listen on {addr}: it is taken by {who}.\n\n  \
         Any free port:     doze-aws --listen {}:0\n  \
         Another port:      doze-aws --listen {next}",
        addr.ip()
    )
}
