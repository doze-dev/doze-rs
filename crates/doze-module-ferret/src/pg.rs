//! The private PostgreSQL behind the gateway: finding both builds, making its cluster (a clone of the template the
//! `documentdb` build ships, or `initdb` when a hand-pointed build has none), its configuration, and its process.
//!
//! It listens on its unix socket only. Go also listened on 127.0.0.1 with `trust` for every local TCP client, and
//! picked its port, and the gateway's debug port, at random from 30000-40000 before binding them; with no TCP there is
//! nothing to pick and nothing to collide. The port number below only names the socket file. The extension connects
//! back to its own server for some work (building a unique index); `documentdb.localhost_connection_string` points
//! that connection at the socket too (Go's needed the loopback listener for it).

use crate::FerretSpec;
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Instance, Toolchain};
use doze_module::proto::msg::run::{Pin, Process, Provisioned, Ready, Resolved, StopHow};
use doze_module::{Cx, Result, help};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The DocumentDB extension build (PostgreSQL 18 and the extension chain), validated with the gateway; not the user's
/// `version`, which picks the gateway.
pub(crate) const DOCUMENTDB: &str = "0.112-0";
/// Names the socket file (`.s.PGSQL.5432`); nothing listens on a TCP port.
const PORT: &str = "5432";

/// The settings the extension needs, rewritten on every boot (Go's, less the loopback listener); then the socket the
/// extension connects back to.
const CONF: &str = "# Managed by doze; rewritten on every boot.
listen_addresses = ''
shared_preload_libraries = 'pg_cron,pg_documentdb_core,pg_documentdb'
cron.database_name = 'postgres'
cron.use_background_workers = on
max_worker_processes = 32
fsync = off
synchronous_commit = off
full_page_writes = off
";
const HBA: &str = "# Managed by doze: the socket only, in a directory only this user can open.
local all all trust
";
const INCLUDE: &str = "include = 'doze.conf'";

fn pgdata(data_dir: &str) -> PathBuf {
    Path::new(data_dir).join("pgdata")
}

fn pgsock(inst: &Instance) -> PathBuf {
    Path::new(&inst.socket_dir).join("pg")
}

fn path(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Both builds: the extension's (the toolchain's `bin_dir`) and the gateway's (`tools["ferretdb"]`), each with its pin.
pub(crate) async fn resolve(cx: &Cx<FerretSpec>, pins: &[Pin], target: &str) -> Result<Resolved> {
    let (pg_bin, _, pg_pin) = component(
        cx,
        pins,
        target,
        ("documentdb", DOCUMENTDB),
        "DOZE_DOCUMENTDB_BINDIR",
    )
    .await?;
    let version = cx.instance.version.clone();
    let (gw_bin, full, gw_pin) = component(
        cx,
        pins,
        target,
        ("ferretdb", &version),
        "DOZE_FERRET_BINDIR",
    )
    .await?;
    let gateway = Path::new(&gw_bin).join(format!("ferretdb{}", std::env::consts::EXE_SUFFIX));
    Ok(Resolved {
        toolchain: Toolchain {
            engine: "ferret".into(),
            full,
            bin_dir: pg_bin,
            tools: BTreeMap::from([("ferretdb".into(), path(&gateway))]),
        },
        pins: pg_pin.into_iter().chain(gw_pin).collect(),
    })
}

/// One build: a developer's own (`env`), else the lock's pin, else an exact version as given, else the major through
/// the host's mirror. Exact means three parts (`2.7.0`) or a dash (`0.112-0`), as Go read it.
async fn component(
    cx: &Cx<FerretSpec>,
    pins: &[Pin],
    target: &str,
    (engine, spec): (&str, &str),
    env: &str,
) -> Result<(String, String, Option<Pin>)> {
    if let Some(dir) = std::env::var_os(env).filter(|d| !d.is_empty()) {
        return Ok((dir.to_string_lossy().into_owned(), spec.to_string(), None));
    }
    let pin = pins.iter().find(|p| p.engine == engine && p.spec == spec);
    let full = match pin {
        Some(p) => p.resolved.clone(),
        None if help::is_exact(spec, 2) || spec.contains('-') => spec.to_string(),
        None => cx.fetch().resolve_major(engine, spec).await?,
    };
    let expected = pin.and_then(|p| p.hashes.get(target)).map(String::as_str);
    let got = cx.fetch().ensure(engine, &full, target, expected).await?;
    let pin = Pin {
        engine: engine.into(),
        spec: spec.into(),
        resolved: full.clone(),
        hashes: BTreeMap::from([(target.to_string(), got.digest)]),
    };
    Ok((got.bin_dir, full, Some(pin)))
}

/// The gateway's program: the toolchain's `ferretdb` tool, or beside the extension's programs.
pub(crate) fn gateway(tc: &Toolchain) -> String {
    tc.tools
        .get("ferretdb")
        .cloned()
        .unwrap_or_else(|| path(&tool(tc, "ferretdb")))
}

fn tool(tc: &Toolchain, name: &str) -> PathBuf {
    Path::new(&tc.bin_dir).join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// The libpq URL the gateway reaches the cluster with: its socket directory.
pub(crate) fn url(inst: &Instance) -> String {
    format!(
        "postgres://postgres@/postgres?host={}&port={PORT}",
        path(&pgsock(inst))
    )
}

/// `documentdb.localhost_connection_string`: the socket directory as a libpq `host`, quoted for libpq and then for the
/// configuration file.
fn self_connection(sock: &Path) -> String {
    let libpq = format!(
        "host='{}'",
        path(sock).replace('\\', "\\\\").replace('\'', "\\'")
    );
    format!(
        "documentdb.localhost_connection_string = '{}'\n",
        libpq.replace('\'', "''")
    )
}

/// Makes the cluster once, then writes its configuration (every boot: doze owns it).
pub(crate) async fn provision(inst: &Instance, tc: &Toolchain) -> Result<Provisioned> {
    let data = Path::new(&inst.data_dir);
    let pgdata = pgdata(&path(data));
    let made = if pgdata.join("PG_VERSION").is_file() {
        Provisioned::Existing
    } else {
        make(data, &pgdata, tc).await?;
        Provisioned::Fresh
    };
    let conf = pgdata.join("doze.conf");
    let text = format!("{CONF}{}", self_connection(&pgsock(inst)));
    doze_os::fs::write_private(&conf, text.as_bytes()).map_err(help::io_error("writing", &conf))?;
    let hba = pgdata.join("pg_hba.conf");
    doze_os::fs::write_private(&hba, HBA.as_bytes()).map_err(help::io_error("writing", &hba))?;
    include(&pgdata.join("postgresql.conf"))?;
    Ok(made)
}

/// The cluster, made in a folder of its own and renamed into place, so a crash never leaves half a cluster that
/// looks whole. A folder left with no `PG_VERSION` holds no cluster and is replaced.
async fn make(data: &Path, pgdata: &Path, tc: &Toolchain) -> Result<()> {
    doze_os::fs::private_dir(data).map_err(help::io_error("creating", data))?;
    let staging = data.join(format!("pgdata.part-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let template = Path::new(&tc.bin_dir)
        .parent()
        .map(|p| p.join("share").join("documentdb-template"));
    let built = match template.filter(|t| t.join("PG_VERSION").is_file()) {
        // Copy-on-write where the disk can; every folder private, as PostgreSQL requires and the archive did not keep.
        Some(t) => doze_os::fs::clone_dir(&t, &staging)
            .and_then(|()| private_tree(&staging))
            .map_err(help::io_error("cloning the template into", &staging)),
        None => initdb(tc, &staging).await,
    };
    let placed = built.and_then(|()| {
        let _ = std::fs::remove_dir_all(pgdata);
        std::fs::rename(&staging, pgdata).map_err(help::io_error("placing", pgdata))
    });
    let _ = std::fs::remove_dir_all(&staging);
    placed
}

async fn initdb(tc: &Toolchain, dir: &Path) -> Result<()> {
    let d = path(dir);
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

/// Every folder under `dir` open to this user only.
fn private_tree(dir: &Path) -> std::io::Result<()> {
    doze_os::fs::private_dir(dir)?;
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        if e.file_type()?.is_dir() {
            private_tree(&e.path())?;
        }
    }
    Ok(())
}

/// `postgresql.conf` includes doze's file (once; the rest of it is the cluster's own).
fn include(conf: &Path) -> Result<()> {
    let text = std::fs::read_to_string(conf).map_err(help::io_error("reading", conf))?;
    if text.lines().any(|l| l.trim() == INCLUDE) {
        return Ok(());
    }
    let joined = format!("{text}\n# Added by doze\n{INCLUDE}\n");
    doze_os::fs::write_private(conf, joined.as_bytes()).map_err(help::io_error("writing", conf))
}

/// The cluster's process: ready when `pg_isready` says so, the extension created once it is, stopped with a fast
/// shutdown (SIGINT: open connections are closed, not waited for).
pub(crate) fn postgres(inst: &Instance, tc: &Toolchain) -> Result<Process> {
    let pgdata = pgdata(&inst.data_dir);
    let sock = pgsock(inst);
    doze_os::fs::private_dir(&sock).map_err(help::io_error("creating", &sock))?;
    stale(inst, &pgdata, &sock)?;
    let (s, d) = (path(&sock), path(&pgdata));
    let ready = vec![
        path(&tool(tc, "pg_isready")),
        "-h".into(),
        s.clone(),
        "-p".into(),
        PORT.into(),
    ];
    let mut ready = ready;
    ready.extend(["-d", "postgres", "-U", "postgres"].map(String::from));
    let create = "CREATE EXTENSION IF NOT EXISTS documentdb CASCADE;";
    let mut hook = vec![
        path(&tool(tc, "psql")),
        "-h".into(),
        s.clone(),
        "-p".into(),
        PORT.into(),
    ];
    hook.extend(
        [
            "-U",
            "postgres",
            "-d",
            "postgres",
            "-v",
            "ON_ERROR_STOP=1",
            "-X",
            "-q",
            "-c",
            create,
        ]
        .map(String::from),
    );
    Ok(Process {
        name: "postgres".into(),
        program: path(&tool(tc, "postgres")),
        args: vec!["-D".into(), d, "-k".into(), s, "-p".into(), PORT.into()],
        env: BTreeMap::new(),
        dir: None,
        after: vec![],
        ready: Some(Ready::Exec(ready)),
        hooks: vec![hook],
        stop: Some(StopHow::Interrupt),
    })
}

/// A `postmaster.pid` whose process lives is refused (a second server would fail on the cluster's lock, and the first
/// could no longer be reached); a dead one is removed, with any socket files it left (Go's `ClearStaleLock`).
fn stale(inst: &Instance, pgdata: &Path, sock: &Path) -> Result<()> {
    let pidfile = pgdata.join("postmaster.pid");
    let live = std::fs::read_to_string(&pidfile)
        .ok()
        .and_then(|t| t.lines().next().and_then(|l| l.trim().parse::<u32>().ok()))
        .filter(|p| doze_os::process::alive(*p));
    if let Some(p) = live {
        let msg = format!(
            "ferret {:?} appears to be running already (pid {p})",
            inst.name
        );
        return Err(Error::new(ErrorKind::Unavailable, msg));
    }
    help::clear_stale(&pidfile)?;
    for e in std::fs::read_dir(sock).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with(".s.PGSQL.") {
            help::clear_stale(&e.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_self_connection_names_the_socket_quoted_twice() {
        assert_eq!(
            self_connection(Path::new("/tmp/s/pg")),
            "documentdb.localhost_connection_string = 'host=''/tmp/s/pg'''\n"
        );
        // A quote in the path is escaped for libpq, then doubled for the configuration file.
        assert_eq!(
            self_connection(Path::new("/tmp/o'k")),
            "documentdb.localhost_connection_string = 'host=''/tmp/o\\''k'''\n"
        );
    }
}
