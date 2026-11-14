//! What MariaDB would refuse at converge, or what would put SQL of its own into doze's statements, refused before
//! boot at its line in doze.hcl (module bug 8: Go interpolated privileges into GRANT unchecked).

use crate::{MariadbSpec, host, sql};
use doze_module::help;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::value::PathSeg;

/// MariaDB 11's privileges (the GRANT reference: database, table, routine and global privileges).
const PRIVILEGES: &[&str] = &[
    "ALL",
    "ALL PRIVILEGES",
    "ALTER",
    "ALTER ROUTINE",
    "BINLOG ADMIN",
    "BINLOG MONITOR",
    "BINLOG REPLAY",
    "CONNECTION ADMIN",
    "CREATE",
    "CREATE ROUTINE",
    "CREATE TABLESPACE",
    "CREATE TEMPORARY TABLES",
    "CREATE USER",
    "CREATE VIEW",
    "DELETE",
    "DELETE HISTORY",
    "DROP",
    "EVENT",
    "EXECUTE",
    "FEDERATED ADMIN",
    "FILE",
    "GRANT OPTION",
    "INDEX",
    "INSERT",
    "LOCK TABLES",
    "PROCESS",
    "READ_ONLY ADMIN",
    "REFERENCES",
    "RELOAD",
    "REPLICA MONITOR",
    "REPLICATION CLIENT",
    "REPLICATION MASTER ADMIN",
    "REPLICATION SLAVE",
    "REPLICATION SLAVE ADMIN",
    "SELECT",
    "SET USER",
    "SHOW CREATE ROUTINE",
    "SHOW DATABASES",
    "SHOW VIEW",
    "SHUTDOWN",
    "SLAVE MONITOR",
    "SUPER",
    "TRIGGER",
    "UPDATE",
    "USAGE",
];

fn key(k: &str) -> PathSeg {
    PathSeg::Key(k.into())
}

fn at(path: Vec<PathSeg>, message: String, hint: Option<&str>) -> Diagnostic {
    Diagnostic {
        path,
        message,
        hint: hint.map(str::to_owned),
    }
}

fn index(i: usize) -> PathSeg {
    PathSeg::Index(u32::try_from(i).unwrap_or(u32::MAX))
}

/// A character set or collation name: MariaDB's are letters, digits and underscores.
fn charset_name(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A database or table name MariaDB takes quoted: 1 to 64 characters, no NUL, not ending in a space.
fn object_name(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 64 && !s.contains('\0') && !s.ends_with(' ')
}

/// Everything wrong with `s`, each at its path.
pub(crate) fn check(s: &MariadbSpec) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (k, v) in [
        ("character_set", &s.character_set),
        ("collation", &s.collation),
    ] {
        if let Some(v) = v.as_deref().filter(|v| !charset_name(v)) {
            out.push(at(
                vec![key(k)],
                format!("{v:?} is not a {} name", k.replace('_', " ")),
                Some("letters, digits and underscores, e.g. utf8mb4 or utf8mb4_unicode_ci"),
            ));
        }
    }
    for k in s.settings.keys() {
        if sql::OWNED.iter().any(|o| o.eq_ignore_ascii_case(k)) {
            out.push(at(
                vec![key("settings"), key(k)],
                format!(
                    "{k} is doze's to set: it decides where the server keeps its data and listens"
                ),
                Some("leave it out; doze url says where the server answers"),
            ));
        }
    }
    users(s, &mut out);
    grants(s, &mut out);
    out
}

fn users(s: &MariadbSpec, out: &mut Vec<Diagnostic>) {
    let mut seen = Vec::new();
    for (i, u) in s.user.iter().enumerate() {
        let who = format!("{}@{}", u.name, host(&u.host));
        if u.name.is_empty() || u.name.chars().count() > 80 {
            out.push(at(
                vec![key("user"), index(i)],
                format!("{:?} is not an account name MariaDB takes", u.name),
                Some("1 to 80 characters"),
            ));
        }
        if seen.contains(&who) {
            out.push(at(
                vec![key("user"), index(i)],
                format!("user {who:?} is declared more than once"),
                Some("keep one block per name and host"),
            ));
        }
        seen.push(who);
    }
}

fn grants(s: &MariadbSpec, out: &mut Vec<Diagnostic>) {
    for (i, g) in s.grant.iter().enumerate() {
        let here = |k: &str| vec![key("grant"), index(i), key(k)];
        if g.privileges.is_empty() {
            out.push(at(
                here("privileges"),
                "a grant needs at least one privilege".into(),
                Some("e.g. [\"SELECT\", \"INSERT\"] or [\"ALL PRIVILEGES\"]"),
            ));
        }
        for (j, p) in g.privileges.iter().enumerate() {
            let upper = p.trim().to_ascii_uppercase();
            if let Some(mut d) = help::one_of(
                here("privileges"),
                "a MariaDB privilege",
                &upper,
                PRIVILEGES,
            ) {
                d.path.push(index(j));
                out.push(d);
            }
        }
        for (k, v) in [("database", &g.database), ("table", &g.table)] {
            if let Some(v) = v
                .as_deref()
                .filter(|v| *v != "*" && !v.is_empty() && !object_name(v))
            {
                out.push(at(
                    here(k),
                    format!("{v:?} is not a {k} name MariaDB takes"),
                    Some("1 to 64 characters"),
                ));
            }
        }
        if g.table
            .as_deref()
            .is_some_and(|t| t != "*" && !t.is_empty())
            && g.database
                .as_deref()
                .is_none_or(|d| d == "*" || d.is_empty())
        {
            out.push(at(
                here("table"),
                "a table needs its database".into(),
                Some("set database too"),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Grant, User};
    use std::collections::BTreeMap;

    fn spec() -> MariadbSpec {
        MariadbSpec {
            character_set: Some("utf8mb4".into()),
            collation: None,
            settings: BTreeMap::from([("max_connections".into(), "200".into())]),
            user: vec![User {
                name: "app".into(),
                host: None,
                password: Some("secret".into()),
            }],
            grant: vec![Grant {
                user: "app".into(),
                host: None,
                privileges: vec!["select".into(), "ALL PRIVILEGES".into()],
                database: Some("app".into()),
                table: None,
            }],
        }
    }

    #[test]
    fn the_example_passes() {
        assert_eq!(check(&spec()), vec![]);
    }

    #[test]
    fn sql_in_a_privilege_is_refused_with_the_nearest_word() {
        let mut s = spec();
        s.grant[0].privileges = vec![
            "SELEC".into(),
            "SELECT ON *.* TO x; DROP DATABASE app; --".into(),
        ];
        let d = check(&s);
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(d[0].hint.as_deref(), Some("did you mean \"SELECT\"?"));
        assert_eq!(
            d[1].path,
            vec![key("grant"), index(0), key("privileges"), index(1)]
        );
    }

    #[test]
    fn owned_settings_bad_names_and_duplicates_are_refused() {
        let mut s = spec();
        s.settings.insert("Socket".into(), "/tmp/x".into());
        s.character_set = Some("utf8; DROP".into());
        s.user.push(User {
            name: "app".into(),
            host: Some("%".into()),
            password: None,
        });
        s.grant[0].table = Some("orders".into());
        s.grant[0].database = None;
        let d = check(&s);
        let msgs: Vec<&str> = d.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(d.len(), 4, "{msgs:?}");
        assert!(msgs.iter().any(|m| m.contains("Socket is doze's")));
        assert!(msgs.iter().any(|m| m.contains("declared more than once")));
        assert!(msgs.iter().any(|m| m.contains("needs its database")));
    }
}
