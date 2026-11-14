//! The text the module hands MariaDB: its my.cnf, the listening arguments, and SQL. Names reach SQL only quoted, and
//! only after `check` has refused anything MariaDB's own grammar would not take, so nothing a person writes in
//! doze.hcl can become SQL of its own (module bug 8).

use crate::{Grant, MariadbSpec, User, every, host};
use doze_module::proto::msg::instance::Address;
use doze_module::{Result, help};
use std::fmt::Write as _;
use std::path::Path;

/// my.cnf keys doze decides: where the server keeps its data and where it listens.
pub(crate) const OWNED: &[&str] = &[
    "datadir",
    "socket",
    "pid-file",
    "pid_file",
    "skip-networking",
    "skip_networking",
    "port",
    "bind-address",
    "bind_address",
];

/// doze's my.cnf: development tuning (durability off for speed, as the postgres profile), the instance database's
/// character set, then the person's settings in order. Rewritten on every provision.
pub(crate) fn conf(s: &MariadbSpec) -> String {
    let mut b = String::from("# Managed by doze, rewritten on every boot.\n[mysqld]\n");
    b.push_str("innodb_flush_log_at_trx_commit = 0\nsync_binlog = 0\nskip-name-resolve\n");
    if let Some(c) = s.character_set.as_deref().filter(|c| !c.is_empty()) {
        let _ = writeln!(b, "character-set-server = {c}");
    }
    if let Some(c) = s.collation.as_deref().filter(|c| !c.is_empty()) {
        let _ = writeln!(b, "collation-server = {c}");
    }
    for (k, v) in help::settings(&s.settings, OWNED) {
        let _ = writeln!(b, "{k} = {v}");
    }
    b
}

/// Where mariadbd listens. On a socket, that socket alone (`--skip-networking` is added by the plan); on TCP, the
/// granted address, and its socket kept in the instance's own folder so it never meets another server's
/// `/tmp/mysql.sock`.
pub(crate) fn listen_args(backend: &Address, socket_dir: &str) -> Result<Vec<String>> {
    Ok(match backend {
        Address::Unix(p) => vec![format!("--socket={p}")],
        Address::Tcp(a) => {
            let (h, p) = help::host_port(a)?;
            let sock = Path::new(socket_dir).join(crate::SOCKET);
            vec![
                format!("--socket={}", sock.to_string_lossy()),
                format!("--port={p}"),
                format!("--bind-address={h}"),
            ]
        }
    })
}

/// How a client of the engine's own reaches it.
pub(crate) fn client_args(backend: &Address) -> Result<Vec<String>> {
    Ok(match backend {
        Address::Unix(p) => vec![format!("--socket={p}")],
        Address::Tcp(a) => {
            let (h, p) = help::host_port(a)?;
            vec![
                format!("--host={h}"),
                format!("--port={p}"),
                "--protocol=TCP".into(),
            ]
        }
    })
}

/// An identifier in backquotes.
pub(crate) fn ident(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

/// A string literal in single quotes; backslashes are escapes in MariaDB's default mode, so they are doubled too.
pub(crate) fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
}

/// `'name'@'host'`.
pub(crate) fn account(name: &str, host: &str) -> String {
    format!("{}@{}", lit(name), lit(host))
}

/// `db`.`table`, with MariaDB's unquoted `*` for every one.
pub(crate) fn target(db: &str, table: &str) -> String {
    let part = |s: &str| if s == "*" { "*".into() } else { ident(s) };
    format!("{}.{}", part(db), part(table))
}

/// The instance database, with its character set and collation.
pub(crate) fn create_database(name: &str, s: &MariadbSpec) -> String {
    let mut q = format!("CREATE DATABASE IF NOT EXISTS {}", ident(name));
    if let Some(c) = s.character_set.as_deref().filter(|c| !c.is_empty()) {
        let _ = write!(q, " CHARACTER SET {}", ident(c));
    }
    if let Some(c) = s.collation.as_deref().filter(|c| !c.is_empty()) {
        let _ = write!(q, " COLLATE {}", ident(c));
    }
    q
}

/// The account, made if missing and its password set to what is declared, none included: a password removed from
/// doze.hcl is cleared (Go kept the old one).
pub(crate) fn user(u: &User) -> [String; 2] {
    let who = account(&u.name, host(&u.host));
    let pass = lit(u.password.as_deref().unwrap_or_default());
    [
        format!("CREATE USER IF NOT EXISTS {who}"),
        format!("ALTER USER {who} IDENTIFIED BY {pass}"),
    ]
}

/// The grant, privileges as `check` allowed them (upper case, MariaDB's words).
pub(crate) fn grant(g: &Grant) -> String {
    format!(
        "GRANT {} ON {} TO {}",
        privileges(&g.privileges),
        target(every(&g.database), every(&g.table)),
        account(&g.user, host(&g.host))
    )
}

/// The instance database's character set and collation made what is declared, for a database that already exists
/// (`CREATE … IF NOT EXISTS` leaves an old one as it was).
pub(crate) fn alter_database(name: &str, s: &MariadbSpec) -> Option<String> {
    let create = create_database(name, s);
    let rest = create.split_once(&ident(name))?.1;
    (!rest.is_empty()).then(|| format!("ALTER DATABASE {}{rest}", ident(name)))
}

fn privileges(p: &[String]) -> String {
    p.iter()
        .map(|p| p.trim().to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_reach_sql_only_quoted() {
        assert_eq!(ident("a`b"), "`a``b`");
        assert_eq!(lit(r"it's \ ok"), r"'it''s \\ ok'");
        assert_eq!(target("*", "*"), "*.*");
        assert_eq!(target("app", "*"), "`app`.*");
        assert_eq!(target("app", "orders"), "`app`.`orders`");
        assert_eq!(account("app", "%"), "'app'@'%'");
    }

    #[test]
    fn a_user_without_a_password_has_it_cleared() {
        let u = User {
            name: "app".into(),
            host: None,
            password: None,
        };
        assert_eq!(
            user(&u),
            [
                "CREATE USER IF NOT EXISTS 'app'@'%'",
                "ALTER USER 'app'@'%' IDENTIFIED BY ''"
            ]
        );
    }

    #[test]
    fn listening_on_tcp_keeps_the_socket_in_the_instance() {
        let a = listen_args(&Address::Tcp("127.0.0.1:3307".into()), "/s/app").unwrap();
        let sock = Path::new("/s/app").join("mysqld.sock");
        assert_eq!(
            a,
            [
                format!("--socket={}", sock.to_string_lossy()),
                "--port=3307".into(),
                "--bind-address=127.0.0.1".into()
            ]
        );
        let c = client_args(&Address::Tcp("127.0.0.1:3307".into())).unwrap();
        assert_eq!(c, ["--host=127.0.0.1", "--port=3307", "--protocol=TCP"]);
    }
}
