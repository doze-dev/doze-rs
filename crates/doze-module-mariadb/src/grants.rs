//! Grants kept as declared: what each privilege may be granted on, what an account has now on a target (read from
//! information_schema), and the statements that move it to what doze.hcl says. A GRANT runs first and only what is
//! no longer declared is revoked after, so a refused statement never takes privileges away and a connected app is
//! never left with none.

use crate::sql::{account, lit, target};
use crate::{Grant, every, host};

/// Privileges that exist only on `*.*` (MariaDB 11's GRANT reference, global privileges).
pub(crate) const GLOBAL: &[&str] = &[
    "BINLOG ADMIN",
    "BINLOG MONITOR",
    "BINLOG REPLAY",
    "CONNECTION ADMIN",
    "CREATE TABLESPACE",
    "CREATE USER",
    "FEDERATED ADMIN",
    "FILE",
    "PROCESS",
    "READ_ONLY ADMIN",
    "RELOAD",
    "REPLICA MONITOR",
    "REPLICATION CLIENT",
    "REPLICATION MASTER ADMIN",
    "REPLICATION REPLICA",
    "REPLICATION SLAVE",
    "REPLICATION SLAVE ADMIN",
    "SET USER",
    "SHOW DATABASES",
    "SHUTDOWN",
    "SLAVE MONITOR",
    "SUPER",
];

/// Privileges a database (or `*.*`) takes but a single table does not.
pub(crate) const DATABASE: &[&str] = &[
    "ALTER ROUTINE",
    "CREATE ROUTINE",
    "CREATE TEMPORARY TABLES",
    "EVENT",
    "EXECUTE",
    "LOCK TABLES",
    "SHOW CREATE ROUTINE",
];

/// Privileges every level takes.
pub(crate) const TABLE: &[&str] = &[
    "ALL",
    "ALL PRIVILEGES",
    "ALTER",
    "CREATE",
    "CREATE VIEW",
    "DELETE",
    "DELETE HISTORY",
    "DROP",
    "GRANT OPTION",
    "INDEX",
    "INSERT",
    "REFERENCES",
    "SELECT",
    "SHOW VIEW",
    "TRIGGER",
    "UPDATE",
    "USAGE",
];

/// Where a grant applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    Global,
    Database,
    Table,
}

pub(crate) fn level(g: &Grant) -> Level {
    match (every(&g.database), every(&g.table)) {
        ("*", _) => Level::Global,
        (_, "*") => Level::Database,
        _ => Level::Table,
    }
}

/// The privileges a grant at `level` may name.
pub(crate) fn allowed(level: Level) -> Vec<&'static str> {
    let mut out: Vec<&str> = TABLE.to_vec();
    if level != Level::Table {
        out.extend(DATABASE);
    }
    if level == Level::Global {
        out.extend(GLOBAL);
    }
    out
}

/// The declared privileges in MariaDB's words, upper case.
pub(crate) fn declared(g: &Grant) -> Vec<String> {
    g.privileges
        .iter()
        .map(|p| p.trim().to_ascii_uppercase())
        .collect()
}

/// The query listing what the account has on the grant's target now: one `PRIVILEGE_TYPE IS_GRANTABLE` per line.
pub(crate) fn current(g: &Grant) -> String {
    let grantee = lit(&format!(
        "'{}'@'{}'",
        g.user.replace('\'', "''"),
        host(&g.host).replace('\'', "''")
    ));
    let (view, place) = match level(g) {
        Level::Global => ("USER_PRIVILEGES", String::new()),
        Level::Database => (
            "SCHEMA_PRIVILEGES",
            format!(" AND TABLE_SCHEMA = {}", lit(every(&g.database))),
        ),
        Level::Table => (
            "TABLE_PRIVILEGES",
            format!(
                " AND TABLE_SCHEMA = {} AND TABLE_NAME = {}",
                lit(every(&g.database)),
                lit(every(&g.table))
            ),
        ),
    };
    format!(
        "SELECT PRIVILEGE_TYPE, IS_GRANTABLE FROM information_schema.{view} WHERE GRANTEE = {grantee}{place}"
    )
}

/// The statements after the GRANT that take away what the account has on the target and is no longer declared,
/// GRANT OPTION included (REVOKE ALL never takes it).
pub(crate) fn revokes(g: &Grant, now: &str) -> Vec<String> {
    let want = declared(g);
    let all = want.iter().any(|p| p == "ALL" || p == "ALL PRIVILEGES");
    let on = target(every(&g.database), every(&g.table));
    let who = account(&g.user, host(&g.host));
    let mut out = Vec::new();
    let mut grantable = false;
    for line in now.lines() {
        let mut cols = line.split('\t');
        let (Some(p), grant) = (cols.next(), cols.next()) else {
            continue;
        };
        grantable |= grant == Some("YES");
        if !all && p != "USAGE" && !want.iter().any(|w| w == p) {
            out.push(format!("REVOKE {p} ON {on} FROM {who}"));
        }
    }
    if grantable && !want.iter().any(|w| w == "GRANT OPTION") {
        out.push(format!("REVOKE GRANT OPTION ON {on} FROM {who}"));
    }
    out
}

/// A grant object's name split back into account and target (`name@host on <target>`): at the first " on " whose right
/// side is a whole target `ok` accepts, so a target with " on " or "@" in its names still splits right.
pub(crate) fn parse_name(name: &str, ok: fn(&str) -> bool) -> Option<(&str, &str, &str)> {
    name.match_indices(" on ").find_map(|(i, _)| {
        let (who, on) = (name.get(..i)?, name.get(i + 4..)?);
        let (user, h) = who.rsplit_once('@')?;
        ok(on).then_some((user, h, on))
    })
}

/// Whether a client error is "no such grant" (1141, 1147): by code, so a server speaking another language is read
/// the same.
pub(crate) fn no_such_grant(e: &str) -> bool {
    e.contains("ERROR 1141") || e.contains("ERROR 1147")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(privs: &[&str], db: Option<&str>) -> Grant {
        Grant {
            user: "app".into(),
            host: None,
            privileges: privs.iter().map(|p| (*p).into()).collect(),
            database: db.map(Into::into),
            table: None,
        }
    }

    #[test]
    fn only_what_is_no_longer_declared_is_revoked() {
        let grant = g(&["select"], Some("shop"));
        let now = "SELECT\tNO\nINSERT\tYES\n";
        assert_eq!(
            revokes(&grant, now),
            [
                "REVOKE INSERT ON `shop`.* FROM 'app'@'%'",
                "REVOKE GRANT OPTION ON `shop`.* FROM 'app'@'%'"
            ]
        );
        assert!(revokes(&g(&["ALL PRIVILEGES"], Some("shop")), "SELECT\tNO\n").is_empty());
        assert!(current(&grant).contains("SCHEMA_PRIVILEGES"));
        assert!(current(&grant).contains("GRANTEE = '''app''@''%'''"));
    }

    #[test]
    fn names_split_from_the_right_parts() {
        assert_eq!(
            parse_name("a@b@% on `my on @db`.*", crate::converge::target_ok),
            Some(("a@b", "%", "`my on @db`.*"))
        );
        assert!(no_such_grant("ERROR 1141 (42000) at line 1: Es gibt keine"));
    }
}
