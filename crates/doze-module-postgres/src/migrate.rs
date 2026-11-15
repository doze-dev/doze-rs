//! `migrate` (Go's `doze migrate`, `cmd/doze/migrate.go`): an instance's data moved to the major its block now
//! declares. PostgreSQL cannot read another major's data folder, so the data is dumped with the old major's server,
//! set aside (kept, never deleted), and restored into a fresh cluster made as the module makes one. The host has
//! stopped the instance; both majors' builds come through the host's downloads.
//!
//! Differences from Go: the servers are started and stopped with `pg_ctl` (it waits for them, on every system), and
//! the folder set aside is named for the major and the Unix time (`db.pg16-1794700000`) rather than a local date.

use crate::{Postgres, cluster, tool};
use doze_module::caps::Migrate;
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::Toolchain;
use doze_module::proto::msg::run::Pin;
use doze_module::{Cx, Module, Result, help};
use std::path::{Path, PathBuf};

fn invalid(msg: String) -> Error {
    Error::new(ErrorKind::Invalid, msg)
}

/// The majors to move between: the data's and the declared one.
fn majors(name: &str, data: &Path, version: &str) -> Result<(String, String)> {
    let raw = std::fs::read_to_string(data.join("PG_VERSION")).map_err(|_| {
        invalid(format!(
            "{name} has no data yet ({}): there is nothing to move; it starts on the declared version",
            data.display()
        ))
    })?;
    let from = raw.trim().to_string();
    let to = version.split('.').next().unwrap_or_default().to_string();
    if from == to {
        return Err(invalid(format!(
            "{name} is already on Postgres {from}, the version doze.hcl declares; nothing to move"
        )));
    }
    Ok((from, to))
}

/// A short folder for the servers' sockets (a socket path must stay short) and their logs.
fn work_dir() -> Result<PathBuf> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir =
        std::env::temp_dir().join(format!("dzm-{}-{}", std::process::id(), nonce % 1_000_000));
    doze_os::fs::private_dir(&dir.join("s")).map_err(help::io_error("creating", &dir))?;
    Ok(dir)
}

/// A server on `data`, listening only on a socket in `work/s`, until [`stop`].
async fn start(tc: &Toolchain, data: &Path, work: &Path) -> Result<()> {
    let opts = format!(
        "-k {} -c listen_addresses= -p 5432",
        work.join("s").to_string_lossy()
    );
    let (d, l) = (data.to_string_lossy(), work.join("server.log"));
    let l = l.to_string_lossy();
    help::run_tool(
        &tool(tc, "pg_ctl"),
        &["-D", &d, "-o", &opts, "-l", &l, "-w", "-t", "120", "start"],
    )
    .await
    .map(drop)
    .map_err(|e| {
        let log = std::fs::read_to_string(work.join("server.log")).unwrap_or_default();
        Error::new(
            e.kind.unwrap_or(ErrorKind::Unavailable),
            format!("postgres did not start on {d}: {}\n{log}", e.message),
        )
    })
}

async fn stop(tc: &Toolchain, data: &Path) {
    let d = data.to_string_lossy();
    let _ = help::run_tool(&tool(tc, "pg_ctl"), &["-D", &d, "-m", "fast", "-w", "stop"]).await;
}

/// Runs `program` with `args`; whether it exited 0, and what it wrote to both streams.
async fn output(program: &Path, args: &[&str]) -> Result<(bool, String)> {
    let out = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(help::io_error("running", program))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), text))
}

/// The builds of `major` (the old data's), fetched through the host if this machine does not have them.
async fn old_build(
    cx: &Cx<<Postgres as Module>::Spec>,
    major: &str,
    target: &str,
) -> Result<Toolchain> {
    let full = cx.fetch().resolve_major("postgres", major).await?;
    let got = cx.fetch().ensure("postgres", &full, target, None).await?;
    Ok(Toolchain {
        engine: "postgres".into(),
        full,
        bin_dir: got.bin_dir,
        tools: std::collections::BTreeMap::new(),
    })
}

/// Step 1: the old data dumped with the old server, read by the new `pg_dumpall` (the newer tools read older
/// servers; the reverse is not promised).
async fn dump(old: &Toolchain, new: &Toolchain, data: &Path, work: &Path, to: &Path) -> Result<()> {
    start(old, data, work).await?;
    let (s, f) = (work.join("s"), to.to_string_lossy().into_owned());
    let s = s.to_string_lossy();
    let args = ["-h", &*s, "-p", "5432", "-U", "postgres", "-f", &f];
    let dumped = output(&tool(new, "pg_dumpall"), &args).await;
    stop(old, data).await;
    match dumped? {
        (true, _) => Ok(()),
        (false, said) => Err(Error::new(
            ErrorKind::Unavailable,
            format!("dumping failed; nothing was changed: {}", said.trim()),
        )),
    }
}

/// Steps 3 and 4: a fresh cluster on the new major, the dump restored into it; what psql said and how many statements
/// failed (the role postgres already exists, and that is expected).
async fn restore(
    new: &Toolchain,
    data: &Path,
    work: &Path,
    from: &Path,
) -> Result<(String, usize)> {
    cluster::initdb(new, data).await?;
    start(new, data, work).await?;
    let (s, f) = (work.join("s"), from.to_string_lossy().into_owned());
    let s = s.to_string_lossy();
    let args = [
        "-h", &*s, "-p", "5432", "-U", "postgres", "-d", "postgres", "-q", "-f", &f,
    ];
    let restored = output(&tool(new, "psql"), &args).await;
    stop(new, data).await;
    let (ok, said) = restored?;
    if !ok {
        return Err(Error::new(
            ErrorKind::Unavailable,
            format!("restoring failed: {}", said.trim()),
        ));
    }
    let errors = said
        .lines()
        .filter(|l| l.contains("ERROR:") && !l.contains("role \"postgres\" already exists"))
        .count();
    Ok((said, errors))
}

impl Migrate for Postgres {
    async fn migrate(&self, cx: &Cx<Self::Spec>, pins: &[Pin], target: &str) -> Result<String> {
        let (name, data) = (&cx.instance.name, PathBuf::from(&cx.instance.data_dir));
        let (from, to) = majors(name, &data, &cx.instance.version)?;
        cx.log(
            2,
            format!("fetching Postgres {from} and {to} if this machine does not have them"),
        );
        let old = old_build(cx, &from, target).await?;
        let new = self.resolve(cx, pins, target).await?.toolchain;
        let work = work_dir()?;
        let parent = data.parent().unwrap_or(Path::new("."));
        let dumped = parent.join(format!("{name}-pg{from}-dump.sql"));
        cx.log(2, format!("reading {name}'s data with Postgres {from}"));
        let moved = move_data(&old, &new, &data, &work, &dumped, &from).await;
        let _ = std::fs::remove_dir_all(&work);
        let (kept, errors, said) = moved?;
        let mut out = format!(
            "{name} is on Postgres {to} now. The Postgres {from} data is kept at {}, and the dump at {}; delete them when you are sure.",
            kept.display(),
            dumped.display()
        );
        if errors > 0 {
            out.push_str(&format!(
                "\n{errors} statements did not restore (often an extension the new major does not have yet). What psql said:\n{}",
                said.trim()
            ));
        }
        Ok(out)
    }
}

/// Dump, set aside, restore; on a failure after the set-aside the old data goes back where it was.
async fn move_data(
    old: &Toolchain,
    new: &Toolchain,
    data: &Path,
    work: &Path,
    dumped: &Path,
    from: &str,
) -> Result<(PathBuf, usize, String)> {
    dump(old, new, data, work, dumped).await?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let name = data
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let kept = data.with_file_name(format!("{name}.pg{from}-{secs}"));
    std::fs::rename(data, &kept).map_err(|e| {
        Error::new(
            ErrorKind::Unavailable,
            format!(
                "setting the old data aside: {e} (the dump is at {})",
                dumped.display()
            ),
        )
    })?;
    match restore(new, data, work, dumped).await {
        Ok((said, errors)) => Ok((kept, errors, said)),
        Err(e) => {
            let _ = std::fs::remove_dir_all(data);
            let _ = std::fs::rename(&kept, data);
            Err(Error::new(
                e.kind.unwrap_or(ErrorKind::Unavailable),
                format!("{}; the old data is back where it was", e.message),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::majors;

    #[test]
    fn the_majors_come_from_the_data_and_the_block() {
        let d = std::env::temp_dir().join(format!("dz-pg-majors-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::create_dir_all(&d);
        let err = |r: doze_module::Result<(String, String)>| {
            r.err().map(|e| e.message).unwrap_or_default()
        };
        assert!(err(majors("db", &d, "17")).starts_with("db has no data yet ("));
        let _ = std::fs::write(d.join("PG_VERSION"), "16\n");
        assert_eq!(
            err(majors("db", &d, "16.4")),
            "db is already on Postgres 16, the version doze.hcl declares; nothing to move"
        );
        assert_eq!(
            majors("db", &d, "17").ok(),
            Some(("16".to_string(), "17".to_string()))
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
