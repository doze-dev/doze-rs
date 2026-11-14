//! The data directory, the shared template, and keeping the instance's structure as declared: its database, users and
//! grants made or changed by `converge`, and taken away by `prune` when they leave doze.hcl. Every statement runs as
//! root over the server's own socket (root has no password there: the install's normal auth, local trust).

use crate::sql::{self, account, ident};
use crate::{Mariadb, MariadbSpec, every, host, tool};
use doze_module::caps::{Converge, Objects, Prune, Template};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Address, Instance, Object, Toolchain};
use doze_module::{Cx, Result, help};
use std::path::{Path, PathBuf};

const DATABASE: &str = "database";
const USER: &str = "user";
const GRANT: &str = "grant";

/// Whether `data` holds MariaDB's system tables already.
pub(crate) fn provisioned(data: &Path) -> bool {
    data.join("mysql").is_dir()
}

/// The system tables, root with normal auth and no password (local trust). The install leaves anonymous accounts;
/// converge drops them ([`anonymous`]).
pub(crate) async fn install_db(tc: &Toolchain, data: &Path) -> Result<()> {
    doze_os::fs::private_dir(data).map_err(help::io_error("creating", data))?;
    let datadir = format!("--datadir={}", data.to_string_lossy());
    help::run_tool(
        &tool(tc, "mariadb-install-db"),
        &[
            "--no-defaults",
            &datadir,
            "--auth-root-authentication-method=normal",
            "--skip-test-db",
        ],
    )
    .await
    .map(drop)
}

/// The server's socket: the granted one, or the one doze keeps in the instance's folder when it listens on TCP.
fn socket(inst: &Instance) -> PathBuf {
    match inst.listeners.get("main").map(|l| &l.backend) {
        Some(Address::Unix(p)) => PathBuf::from(p),
        _ => Path::new(&inst.socket_dir).join(crate::SOCKET),
    }
}

/// Runs `statements` in one client session as root, stopping at the first that fails.
async fn exec(tc: &Toolchain, inst: &Instance, statements: &[String]) -> Result<String> {
    let sock = format!("--socket={}", socket(inst).to_string_lossy());
    let script = statements.join(";\n");
    help::run_tool(
        &tool(tc, "mariadb"),
        &[
            "--no-defaults",
            &sock,
            "--user=root",
            "--batch",
            "--skip-column-names",
            "-e",
            &script,
        ],
    )
    .await
}

/// Drops the anonymous accounts the install made. MariaDB matches `''@'localhost'` before `'app'@'%'` for a client on the
/// socket, so with one a declared user could never log in with its password (module bug 8, unseen in Go because its
/// test never logged in). Their hosts are read, not guessed (one is the machine's name).
async fn anonymous(tc: &Toolchain, inst: &Instance) -> Result<()> {
    let hosts = exec(
        tc,
        inst,
        &["SELECT Host FROM mysql.user WHERE User = ''".into()],
    )
    .await?;
    let drops: Vec<String> = hosts
        .lines()
        .filter(|h| !h.is_empty())
        .map(|h| format!("DROP USER IF EXISTS {}", account("", h)))
        .collect();
    if drops.is_empty() {
        return Ok(());
    }
    exec(tc, inst, &drops).await.map(drop)
}

fn grant_name(g: &crate::Grant) -> String {
    format!(
        "{}@{} on {}",
        g.user,
        host(&g.host),
        sql::target(every(&g.database), every(&g.table))
    )
}

impl Converge for Mariadb {
    async fn converge(&self, cx: &Cx<MariadbSpec>, tc: &Toolchain) -> Result<()> {
        let s = &cx.spec;
        anonymous(tc, &cx.instance).await?;
        let mut q = vec![sql::create_database(&cx.instance.name, s)];
        for u in &s.user {
            q.extend(sql::user(u));
        }
        exec(tc, &cx.instance, &q).await?;
        for g in &s.grant {
            // Revoke then grant, so a privilege dropped from the list goes; "no such grant" is a first converge.
            match exec(tc, &cx.instance, &[sql::revoke_all(g)]).await {
                Err(e) if !e.to_string().contains("no such grant") => return Err(e),
                _ => {}
            }
            exec(tc, &cx.instance, &[sql::grant(g)]).await?;
        }
        Ok(())
    }
}

impl Objects for Mariadb {
    fn objects(&self, cx: &Cx<MariadbSpec>) -> Result<Vec<Object>> {
        let s = &cx.spec;
        let obj = |kind: &str, name: String, declared: String| Object {
            kind: kind.into(),
            name,
            hash: help::object_hash(&declared),
        };
        let mut out = vec![obj(
            DATABASE,
            cx.instance.name.clone(),
            format!(
                "{}\0{}",
                s.character_set.as_deref().unwrap_or_default(),
                s.collation.as_deref().unwrap_or_default()
            ),
        )];
        for u in &s.user {
            let pass = u.password.as_deref().unwrap_or_default();
            out.push(obj(
                USER,
                format!("{}@{}", u.name, host(&u.host)),
                pass.into(),
            ));
        }
        for g in &s.grant {
            out.push(obj(
                GRANT,
                grant_name(g),
                g.privileges.join(",").to_ascii_uppercase(),
            ));
        }
        Ok(out)
    }
}

/// `name@host` split at its last `@`.
fn split_account(s: &str) -> Option<(&str, &str)> {
    s.rsplit_once('@')
}

/// A grant's target as [`sql::target`] wrote it: `*.*`, `` `db`.* `` or `` `db`.`table` ``, backquotes doubled inside.
/// Anything else is refused: an object's name comes back from the core's records, and only what this module made may
/// reach SQL.
fn target_ok(t: &str) -> bool {
    fn quoted(s: &str) -> Option<&str> {
        let inner = s.strip_prefix('`')?;
        let mut chars = inner.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '`' {
                if chars.peek().map(|(_, n)| *n) == Some('`') {
                    chars.next();
                } else {
                    return inner.get(i + 1..);
                }
            }
        }
        None
    }
    if t == "*.*" {
        return true;
    }
    let Some(rest) = quoted(t).and_then(|r| r.strip_prefix('.')) else {
        return false;
    };
    rest == "*" || quoted(rest) == Some("")
}

/// The statement that takes a removed object away; `None` for a name this module could not have made.
fn drop_statement(o: &Object) -> Option<String> {
    match o.kind.as_str() {
        DATABASE => Some(format!("DROP DATABASE IF EXISTS {}", ident(&o.name))),
        USER => {
            split_account(&o.name).map(|(n, h)| format!("DROP USER IF EXISTS {}", account(n, h)))
        }
        GRANT => {
            let (who, on) = o.name.rsplit_once(" on ")?;
            let (n, h) = split_account(who)?;
            target_ok(on).then(|| format!("REVOKE ALL PRIVILEGES ON {on} FROM {}", account(n, h)))
        }
        _ => None,
    }
}

impl Prune for Mariadb {
    async fn prune(&self, cx: &Cx<MariadbSpec>, tc: &Toolchain, removed: &[Object]) -> Result<()> {
        // Grants first, then users, then the database: a grant whose user is gone has nothing left to revoke.
        let rank = |k: &str| match k {
            GRANT => 0,
            USER => 1,
            _ => 2,
        };
        let mut objs: Vec<&Object> = removed.iter().collect();
        objs.sort_by_key(|o| rank(&o.kind));
        for o in objs {
            let Some(q) = drop_statement(o) else { continue };
            match exec(tc, &cx.instance, &[q]).await {
                Ok(_) => {}
                // A grant already gone (its user dropped by hand, say) is what prune wanted.
                Err(e) if o.kind == GRANT && e.to_string().contains("no such grant") => {}
                Err(e) => {
                    return Err(Error::new(
                        ErrorKind::Unavailable,
                        format!("dropping {} {:?}: {e}", o.kind, o.name),
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Template for Mariadb {
    /// The system tables made once, into a folder beside the template that is renamed into place, so a reader never
    /// sees a half-made template.
    async fn ensure_template(&self, tc: &Toolchain, dir: &Path) -> Result<()> {
        if provisioned(dir) {
            return Ok(());
        }
        let staging = dir.with_extension(format!("making-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&staging);
        install_db(tc, &staging).await?;
        std::fs::rename(&staging, dir).map_err(help::io_error("placing", dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(kind: &str, name: &str) -> Object {
        Object {
            kind: kind.into(),
            name: name.into(),
            hash: String::new(),
        }
    }

    #[test]
    fn removed_objects_become_their_statements() {
        assert_eq!(
            drop_statement(&o(USER, "a@b@%")).as_deref(),
            Some("DROP USER IF EXISTS 'a@b'@'%'")
        );
        assert_eq!(
            drop_statement(&o(GRANT, "app@% on `app`.`t``x`")).as_deref(),
            Some("REVOKE ALL PRIVILEGES ON `app`.`t``x` FROM 'app'@'%'")
        );
        assert_eq!(
            drop_statement(&o(DATABASE, "a`b")).as_deref(),
            Some("DROP DATABASE IF EXISTS `a``b`")
        );
    }

    #[test]
    fn only_targets_this_module_writes_reach_sql() {
        for ok in ["*.*", "`app`.*", "`app`.`orders`", "`a``b`.`c`"] {
            assert!(target_ok(ok), "{ok}");
        }
        for bad in [
            "app.*",
            "`app`.* ; DROP DATABASE x",
            "`app`",
            "`a`b`.*",
            "*.`x`",
        ] {
            assert!(!target_ok(bad), "{bad}");
        }
    }
}
