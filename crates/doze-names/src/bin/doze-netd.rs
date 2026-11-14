//! The network daemon on its own, for the end-to-end tests and a by-hand run; the doze core embeds the same code.
//!
//! macOS: `doze-netd <registry> <uid> <gid>`, started as root (it drops to uid/gid once the tunnel is up).
//! Linux: `doze-netd <registry>`, started as the user.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let translating = doze_names::Net::native().virtual_;
    let result = match (translating, args.as_slice()) {
        (true, [registry, uid, gid]) => match (uid.parse(), gid.parse()) {
            (Ok(uid), Ok(gid)) => {
                doze_names::netd::run_translating(&PathBuf::from(registry), uid, gid)
            }
            _ => return usage("the uid and gid to run as must be numbers"),
        },
        (false, [registry]) => doze_names::netd::run_zone(&PathBuf::from(registry)),
        (true, _) => return usage("usage: doze-netd <registry> <uid> <gid>"),
        (false, _) => return usage("usage: doze-netd <registry>"),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn usage(why: &str) -> ExitCode {
    eprintln!("doze-netd: {why}");
    ExitCode::from(2)
}
