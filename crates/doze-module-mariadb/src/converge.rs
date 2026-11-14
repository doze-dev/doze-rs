//! The data directory, the shared template, and keeping the instance's structure as declared: its database, users and
//! grants made or changed by `converge`, and taken away by `prune` when they leave doze.hcl. Every statement runs as
//! root over the server's own socket (root has no password there: the install's normal auth, local trust).

use crate::grants;
use crate::sql::{self, account, ident};
use crate::{Mariadb, MariadbSpec, every, host, tool};
use doze_module::caps::{Converge, Objects, Prune, Template};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Address, Instance, Object, Toolchain};
use doze_module::{Cx, Result, help};
use std::path::Path;

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

/// How root reaches the server: over its socket, or over TCP where the host granted TCP (the install makes
/// `root@127.0.0.1` and `root@::1` with no password, as `root@localhost`).
fn reach(inst: &Instance) -> Result<Vec<String>> {
    match inst.listeners.get("main").map(|l| &l.backend) {
        Some(b @ Address::Tcp(_)) => sql::client_args(b),
        Some(Address::Unix(p)) => Ok(vec![format!("--socket={p}")]),
        None => Ok(vec![format!(
            "--socket={}",
            Path::new(&inst.socket_dir)
                .join(crate::SOCKET)
                .to_string_lossy()
        )]),
    }
}

/// Runs `statements` in one client session as root, stopping at the first that fails. The session first drops
/// `NO_BACKSLASH_ESCAPES` from its sql_mode, so the quoting in [`sql::lit`] means what it says whatever the person set.
pub(crate) async fn exec(tc: &Toolchain, inst: &Instance, statements: &[String]) -> Result<String> {
    let mut script =
        vec!["SET SESSION sql_mode = REPLACE(@@sql_mode, 'NO_BACKSLASH_ESCAPES', '')".to_string()];
    script.extend(statements.iter().cloned());
    let script = script.join(";\n");
    let mut args = vec!["--no-defaults".to_string()];
    args.extend(reach(inst)?);
    args.extend(["--user=root", "--batch", "--skip-column-names", "-e"].map(String::from));
    args.push(script);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    help::run_tool(&tool(tc, "mariadb"), &args).await
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
        let mut q = vec![sql::create_database(&cx.instance.name, s)];
        q.extend(sql::alter_database(&cx.instance.name, s));
        for u in &s.user {
            q.extend(sql::user(u));
        }
        exec(tc, &cx.instance, &q).await?;
        for g in &s.grant {
            // Grant first, then take back only what is no longer declared: a refused GRANT takes nothing away.
            exec(tc, &cx.instance, &[sql::grant(g)]).await?;
            let now = exec(tc, &cx.instance, &[grants::current(g)]).await?;
            let revokes = grants::revokes(g, &now);
            if !revokes.is_empty() {
                exec(tc, &cx.instance, &revokes).await?;
            }
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
pub(crate) fn target_ok(t: &str) -> bool {
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

/// The statements that take a removed object away, each run on its own; `None` for a name this module could not have
/// made (prune then fails rather than leave it behind unsaid).
fn drop_statements(o: &Object) -> Option<Vec<String>> {
    match o.kind.as_str() {
        DATABASE => Some(vec![format!("DROP DATABASE IF EXISTS {}", ident(&o.name))]),
        USER => split_account(&o.name)
            .map(|(n, h)| vec![format!("DROP USER IF EXISTS {}", account(n, h))]),
        GRANT => {
            let (n, h, on) = grants::parse_name(&o.name, target_ok)?;
            let who = account(n, h);
            // REVOKE ALL leaves GRANT OPTION behind; it is taken on its own.
            Some(vec![
                format!("REVOKE ALL PRIVILEGES ON {on} FROM {who}"),
                format!("REVOKE GRANT OPTION ON {on} FROM {who}"),
            ])
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
        let failed = |o: &Object, why: String| {
            Error::new(
                ErrorKind::Unavailable,
                format!("dropping {} {:?}: {why}", o.kind, o.name),
            )
        };
        for o in objs {
            let Some(qs) = drop_statements(o) else {
                return Err(failed(o, "not a name this module makes".into()));
            };
            for q in qs {
                match exec(tc, &cx.instance, &[q]).await {
                    Ok(_) => {}
                    // A grant already gone (its user dropped by hand, say) is what prune wanted.
                    Err(e) if o.kind == GRANT && grants::no_such_grant(&e.to_string()) => {}
                    Err(e) => return Err(failed(o, e.to_string())),
                }
            }
        }
        Ok(())
    }
}

impl Template for Mariadb {
    /// The system tables made once, in a folder of its own beside the template that is renamed into place, so a reader
    /// never sees a half-made template; a failed install leaves nothing behind, and losing the race to another maker
    /// is fine.
    async fn ensure_template(&self, tc: &Toolchain, dir: &Path) -> Result<()> {
        if provisioned(dir) {
            return Ok(());
        }
        let parent = dir.parent().unwrap_or(Path::new("."));
        doze_os::fs::private_dir(parent).map_err(help::io_error("creating", parent))?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let staging = parent.join(format!("_tmpl-{}-{nonce}", std::process::id()));
        let made = install_db(tc, &staging).await;
        let placed = made
            .and_then(|()| std::fs::rename(&staging, dir).map_err(help::io_error("placing", dir)));
        let _ = std::fs::remove_dir_all(&staging);
        match placed {
            Err(_) if provisioned(dir) => Ok(()),
            other => other,
        }
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
            drop_statements(&o(USER, "a@b@%")),
            Some(vec!["DROP USER IF EXISTS 'a@b'@'%'".to_string()])
        );
        assert_eq!(
            drop_statements(&o(GRANT, "app@% on `app`.`t``x`")),
            Some(vec![
                "REVOKE ALL PRIVILEGES ON `app`.`t``x` FROM 'app'@'%'".to_string(),
                "REVOKE GRANT OPTION ON `app`.`t``x` FROM 'app'@'%'".to_string()
            ])
        );
        assert_eq!(
            drop_statements(&o(DATABASE, "a`b")),
            Some(vec!["DROP DATABASE IF EXISTS `a``b`".to_string()])
        );
        assert_eq!(drop_statements(&o(GRANT, "app@% on app.*; DROP x")), None);
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
