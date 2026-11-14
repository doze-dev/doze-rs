//! The cluster on disk: made once (`initdb`, or a clone of the shared template), checked against the declared major,
//! configured on every boot, and a server left running found before a second one is started on it.

use crate::spec::PostgresSpec;
use crate::{Postgres, conf, tool};
use doze_module::caps::Template;
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Instance, Toolchain};
use doze_module::proto::msg::run::Provisioned;
use doze_module::{Result, help};
use std::path::Path;

/// Whether `dir` holds a cluster.
pub(crate) fn provisioned(dir: &Path) -> bool {
    dir.join("PG_VERSION").is_file()
}

/// `initdb` into `dir`: `postgres` as the superuser, trusted on the socket, UTF8, no fsync while it writes.
async fn initdb(tc: &Toolchain, dir: &Path) -> Result<()> {
    if let Some(parent) = dir.parent() {
        doze_os::fs::private_dir(parent).map_err(help::io_error("creating", parent))?;
    }
    let d = dir.to_string_lossy();
    let args = [
        "-D",
        &d,
        "-U",
        "postgres",
        "-A",
        "trust",
        "-E",
        "UTF8",
        "--no-sync",
    ];
    help::run_tool(&tool(tc, "initdb"), &args).await.map(drop)
}

/// The cluster made in a folder of its own beside `dir` and renamed into place, so a crash or a failed initdb never
/// leaves half a cluster that looks whole (Go ran initdb into the data directory itself); losing the race to another
/// maker is fine.
async fn make(tc: &Toolchain, dir: &Path) -> Result<()> {
    let parent = dir.parent().unwrap_or(Path::new("."));
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let staging = parent.join(format!(".{name}.part-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let made = initdb(tc, &staging).await;
    let placed = made.and_then(|()| {
        if !provisioned(dir) {
            // An empty or half-made folder (no PG_VERSION) holds no cluster.
            let _ = std::fs::remove_dir_all(dir);
        }
        std::fs::rename(&staging, dir).map_err(help::io_error("placing", dir))
    });
    let _ = std::fs::remove_dir_all(&staging);
    match placed {
        Err(_) if provisioned(dir) => Ok(()),
        other => other,
    }
}

/// The major the cluster's data belongs to, against the declared one: PostgreSQL cannot read another major's data.
fn check_version(inst: &Instance, data: &Path) -> Result<()> {
    let Ok(raw) = std::fs::read_to_string(data.join("PG_VERSION")) else {
        return Ok(());
    };
    let on_disk = raw.trim();
    let declared = inst.version.split('.').next().unwrap_or_default();
    if on_disk == declared {
        return Ok(());
    }
    let n = &inst.name;
    Err(Error::new(
        ErrorKind::Invalid,
        format!(
            "instance {n:?}: its data is Postgres {on_disk}'s, but doze.hcl now says version {declared}, and Postgres cannot \
             read another major's data. Bring the stack down and run `doze migrate {n}`: it dumps the {on_disk} data, \
             keeps it aside, and restores it into {declared}. Or keep version = {on_disk} for now"
        ),
    ))
}

/// Makes the cluster if there is none, checks its major, and writes doze's configuration (every boot: doze owns it).
pub(crate) async fn provision(
    inst: &Instance,
    tc: &Toolchain,
    s: &PostgresSpec,
) -> Result<Provisioned> {
    let data = Path::new(&inst.data_dir);
    let made = if provisioned(data) {
        Provisioned::Existing
    } else {
        make(tc, data).await?;
        Provisioned::Fresh
    };
    check_version(inst, data)?;
    conf::write(data, s)?;
    Ok(made)
}

/// A `postmaster.pid` whose process lives (and is this user's: another user's process with that number is not the
/// server that wrote it, as PostgreSQL's own check reads it) is refused; a dead one is removed, with the socket it
/// left.
pub(crate) fn stale(inst: &Instance, socket: &Path) -> Result<()> {
    let pidfile = Path::new(&inst.data_dir).join("postmaster.pid");
    let live = std::fs::read_to_string(&pidfile)
        .ok()
        .and_then(|t| t.lines().next().and_then(|l| l.trim().parse::<u32>().ok()))
        .filter(|p| doze_os::process::alive_own(*p));
    if let Some(p) = live {
        let msg = format!(
            "instance {:?} appears to already be running (pid {p}); remove {} if you are sure it is not",
            inst.name,
            pidfile.display()
        );
        return Err(Error::new(ErrorKind::Unavailable, msg));
    }
    help::clear_stale(&pidfile)?;
    help::clear_stale(socket)?;
    help::clear_stale(&socket.with_file_name(format!(
        "{}.lock",
        socket.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    )))
}

impl Template for Postgres {
    /// A cluster made once, which the host clones (copy-on-write where the disk can) for an instant first boot.
    async fn ensure_template(&self, tc: &Toolchain, dir: &Path) -> Result<()> {
        if provisioned(dir) {
            return Ok(());
        }
        make(tc, dir).await
    }
}
