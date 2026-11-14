//! The engine's own `psql`, run as `postgres` over the instance's socket (local trust), with the SQL on its standard
//! input: a password in `CREATE ROLE … PASSWORD` never sits on a command line where `ps` shows it (Go passed every
//! statement with `-c`). The machine's psqlrc (`-X`) and every `PG*` variable of the environment are left out, so
//! nothing of the person's shell sends it to another server or changes how it reads the script.

use doze_module::Result;
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Address, Instance, Toolchain};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

/// Separates fields in rows read back (psql's `-F`): the unit separator, which no catalog value holds.
pub(crate) const SEP: char = '\x1f';

/// psql for one instance.
#[derive(Debug, Clone)]
pub(crate) struct Psql {
    bin: PathBuf,
    /// The socket's directory and the port number that names it.
    dir: String,
    port: String,
}

/// The socket directory and port of `.s.PGSQL.<port>`: PostgreSQL names its socket itself, so a granted socket must
/// have that name.
pub(crate) fn socket(path: &str) -> Result<(String, String)> {
    let p = Path::new(path);
    let port = p
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix(".s.PGSQL."))
        .filter(|n| n.parse::<u16>().is_ok());
    match (p.parent(), port) {
        (Some(dir), Some(port)) => Ok((dir.to_string_lossy().into_owned(), port.into())),
        _ => Err(Error::new(
            ErrorKind::Invalid,
            format!(
                "{path} cannot be a PostgreSQL socket: PostgreSQL names its socket .s.PGSQL.<port>"
            ),
        )),
    }
}

/// Where the server listens: the socket the host granted, or `.s.PGSQL.5432` in the instance's socket directory.
/// PostgreSQL stays on its socket only (the module's design: the core proxies TCP to it); a TCP grant is refused, as no
/// PostgreSQL build for a system without sockets exists yet.
pub(crate) fn backend(inst: &Instance) -> Result<(String, String)> {
    match inst.listeners.get("main").map(|l| &l.backend) {
        Some(Address::Unix(p)) => socket(p),
        Some(Address::Tcp(a)) => Err(Error::new(
            ErrorKind::Unsupported,
            format!("postgres listens on its unix socket only; this host granted it TCP ({a})"),
        )),
        None => Ok((inst.socket_dir.clone(), crate::PORT.into())),
    }
}

impl Psql {
    pub(crate) fn new(tc: &Toolchain, inst: &Instance) -> Result<Psql> {
        let (dir, port) = backend(inst)?;
        Ok(Psql {
            bin: crate::tool(tc, "psql"),
            dir,
            port,
        })
    }

    /// Runs `script` in database `db`, stopping at the first error; one transaction when `atomic`. Its output (rows,
    /// tuples only, fields split by [`SEP`]).
    pub(crate) async fn run(&self, db: &str, script: &str, atomic: bool) -> Result<String> {
        // standard_conforming_strings forced on: with it off (a person's setting), a backslash in a literal would end it,
        // and the rest would run as SQL or as a psql command. A lock held by a client cannot stall converge for ever.
        let conninfo = format!(
            "dbname='{}' options='-c standard_conforming_strings=on -c lock_timeout=30s'",
            db.replace('\\', "\\\\").replace('\'', "\\'")
        );
        let mut c = tokio::process::Command::new(&self.bin);
        c.args([
            "-h", &self.dir, "-p", &self.port, "-U", "postgres", "-d", &conninfo,
        ])
        .args(["-v", "ON_ERROR_STOP=1", "-X", "-q", "-t", "-A", "-F"])
        .arg(SEP.to_string())
        .args(atomic.then_some("-1"))
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("PG") {
                c.env_remove(k);
            }
        }
        let mut child = c.spawn().map_err(|e| {
            Error::new(
                ErrorKind::Unavailable,
                format!("{}: {e}", self.bin.display()),
            )
        })?;
        let stdin = child.stdin.take();
        let feed = async move {
            if let Some(mut i) = stdin {
                // A failed write shows as psql's own error below.
                let _ = i.write_all(script.as_bytes()).await;
            }
        };
        let (_, out) = tokio::join!(feed, child.wait_with_output());
        let out = out.map_err(|e| Error::new(ErrorKind::Unavailable, format!("psql: {e}")))?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        Err(Error::new(ErrorKind::Unavailable, clean(err.trim())))
    }

    /// Statements, each ended, in one transaction, retried a few times on the errors two concurrent sessions changing
    /// the same catalog rows give (Go's `execRetry`).
    pub(crate) async fn exec(&self, db: &str, statements: &[String]) -> Result<()> {
        let script = script(statements);
        let mut wait = std::time::Duration::from_millis(50);
        for _ in 0..5 {
            match self.run(db, &script, true).await {
                Err(e) if transient(&e.message) => tokio::time::sleep(wait).await,
                other => return other.map(drop),
            }
            wait = (wait * 2).min(std::time::Duration::from_millis(800));
        }
        self.run(db, &script, true).await.map(drop)
    }

    /// One statement outside a transaction (CREATE and DROP DATABASE cannot run in one).
    pub(crate) async fn alone(&self, db: &str, statement: &str) -> Result<()> {
        self.run(db, &script(&[statement.to_string()]), false)
            .await
            .map(drop)
    }

    /// A query's rows, each split into its fields; a row with another count of fields is dropped (Go's `splitRows`).
    pub(crate) async fn rows(&self, db: &str, query: &str, n: usize) -> Result<Vec<Vec<String>>> {
        let out = self.run(db, &format!("{query};\n"), false).await?;
        Ok(split(&out, n))
    }

    /// The first field of each row.
    pub(crate) async fn column(&self, db: &str, query: &str) -> Result<Vec<String>> {
        Ok(self
            .rows(db, query, 1)
            .await?
            .into_iter()
            .flatten()
            .collect())
    }

    /// A yes-or-no query.
    pub(crate) async fn yes(&self, db: &str, query: &str) -> Result<bool> {
        Ok(self
            .column(db, query)
            .await?
            .first()
            .is_some_and(|v| v == "t"))
    }
}

fn script(statements: &[String]) -> String {
    statements.iter().map(|s| format!("{s};\n")).collect()
}

fn transient(e: &str) -> bool {
    [
        "tuple concurrently updated",
        "tuple concurrently deleted",
        "deadlock detected",
    ]
    .iter()
    .any(|t| e.contains(t))
}

/// psql's error without its script position (`psql:<stdin>:3: ERROR:  …` → `ERROR:  …`).
fn clean(e: &str) -> String {
    e.lines()
        .map(|l| match l.strip_prefix("psql:<stdin>:") {
            Some(rest) => rest.split_once(": ").map_or(rest, |(_, m)| m),
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rows of `n` fields from psql's unaligned output.
pub(crate) fn split(out: &str, n: usize) -> Vec<Vec<String>> {
    out.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.split(SEP).map(str::to_owned).collect::<Vec<_>>())
        .filter(|f| f.len() == n)
        .collect()
}

/// For an admin call: psql for the running instance, or why there is none.
pub(crate) fn admin(inst: &Instance) -> Result<Psql> {
    let Some(tc) = &inst.toolchain else {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "postgres is not running, so there is nothing to read",
        ));
    };
    Psql::new(tc, inst)
}

/// A failure of `what` on `name`, as converge reports it.
pub(crate) fn wrap(what: String) -> impl FnOnce(Error) -> Error {
    move |mut e| {
        e.message = format!("{what}: {}", e.message);
        e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_names_its_directory_and_port() {
        assert_eq!(
            socket("/s/app/.s.PGSQL.5432").ok(),
            Some(("/s/app".to_string(), "5432".to_string()))
        );
        assert!(socket("/s/app/main.sock").is_err());
        assert!(socket("/s/app/.s.PGSQL.x").is_err());
    }

    #[test]
    fn errors_lose_their_script_position() {
        assert_eq!(
            clean("psql:<stdin>:3: ERROR:  role \"x\" does not exist"),
            "ERROR:  role \"x\" does not exist"
        );
        assert_eq!(
            split("a\x1fb\nc\n\nd\x1fe\n", 2),
            vec![vec!["a", "b"], vec!["d", "e"]]
        );
    }
}
