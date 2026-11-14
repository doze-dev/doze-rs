//! What MariaDB would refuse at converge, or what would put SQL or my.cnf lines of its own into what doze writes,
//! refused before boot at its line in doze.hcl (module bug 8: Go interpolated privileges into GRANT unchecked).

use crate::grants::{self, Level};
use crate::{MariadbSpec, every, host, sql};
use doze_module::help;
use doze_module::proto::msg::error::Diagnostic;
use doze_module::proto::msg::value::PathSeg;

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
    s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A database or table name MariaDB takes quoted: 1 to 64 characters, no NUL, not ending in a space.
fn object_name(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 64 && !s.contains('\0') && !s.ends_with(' ')
}

/// An account name MariaDB takes: 1 to 80 characters.
fn account_name(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= 80
}

/// A my.cnf key as mariadbd reads it: prefixes that change nothing about which option it is dropped, `_` as `-`.
fn option(k: &str) -> String {
    let mut k = k.trim().to_ascii_lowercase().replace('_', "-");
    while let Some(rest) = ["loose-", "enable-", "disable-", "maximum-", "skip-"]
        .iter()
        .find_map(|p| k.strip_prefix(p))
    {
        k = rest.to_owned();
    }
    k
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
    settings(s, &mut out);
    users(s, &mut out);
    for (i, g) in s.grant.iter().enumerate() {
        grant(g, i, &mut out);
    }
    duplicate_grants(s, &mut out);
    out
}

/// A setting goes into my.cnf as `key = value`: a key doze owns (with any of mariadbd's prefixes) is refused, and so
/// is anything that would start a line or a section of its own.
fn settings(s: &MariadbSpec, out: &mut Vec<Diagnostic>) {
    let owned: Vec<String> = sql::OWNED.iter().map(|o| option(o)).collect();
    for (k, v) in &s.settings {
        let here = vec![key("settings"), key(k)];
        if owned.contains(&option(k)) {
            out.push(at(
                here.clone(),
                format!(
                    "{k} is doze's to set: it decides where the server keeps its data and listens"
                ),
                Some("leave it out; doze url says where the server answers"),
            ));
        }
        let bad_key = k.is_empty()
            || k.chars()
                .any(|c| c.is_whitespace() || c.is_control() || "=[]#".contains(c));
        if bad_key || v.chars().any(char::is_control) {
            out.push(at(
                here,
                format!("{k:?} = {v:?} cannot be one my.cnf line"),
                Some("an option name, and a value without line breaks or control characters"),
            ));
        }
    }
}

fn users(s: &MariadbSpec, out: &mut Vec<Diagnostic>) {
    let mut seen = Vec::new();
    for (i, u) in s.user.iter().enumerate() {
        let who = format!("{}@{}", u.name, host(&u.host));
        if !account_name(&u.name) {
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

fn grant(g: &crate::Grant, i: usize, out: &mut Vec<Diagnostic>) {
    let here = |k: &str| vec![key("grant"), index(i), key(k)];
    if !account_name(&g.user) {
        out.push(at(
            here("user"),
            format!("{:?} is not an account name MariaDB takes", g.user),
            Some("1 to 80 characters"),
        ));
    }
    let privs = grants::declared(g);
    if privs.is_empty() {
        out.push(at(
            here("privileges"),
            "a grant needs at least one privilege".into(),
            Some("e.g. [\"SELECT\", \"INSERT\"] or [\"ALL PRIVILEGES\"]"),
        ));
    }
    if privs.len() > 1 && privs.iter().any(|p| p == "ALL" || p == "ALL PRIVILEGES") {
        out.push(at(
            here("privileges"),
            "ALL PRIVILEGES goes alone".into(),
            Some("[\"ALL PRIVILEGES\"], or the privileges one by one"),
        ));
    }
    let level = grants::level(g);
    let allowed = grants::allowed(level);
    let every_privilege: Vec<&str> = grants::allowed(Level::Global);
    for (j, p) in privs.iter().enumerate() {
        let d = if every_privilege.contains(&p.as_str()) && !allowed.contains(&p.as_str()) {
            let on = if level == Level::Table {
                "a table"
            } else {
                "a database"
            };
            Some(at(
                here("privileges"),
                format!("{p} cannot be granted on {on}"),
                Some("grant it on every database: leave database out"),
            ))
        } else {
            help::one_of(
                here("privileges"),
                "a MariaDB privilege",
                p,
                &every_privilege,
            )
        };
        if let Some(mut d) = d {
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
    if every(&g.table) != "*" && every(&g.database) == "*" {
        out.push(at(
            here("table"),
            "a table needs its database".into(),
            Some("set database too"),
        ));
    }
}

/// Two grants on one account and target would overwrite each other's privileges: one block per target.
fn duplicate_grants(s: &MariadbSpec, out: &mut Vec<Diagnostic>) {
    let mut seen = Vec::new();
    for (i, g) in s.grant.iter().enumerate() {
        let k = (
            g.user.as_str(),
            host(&g.host),
            every(&g.database),
            every(&g.table),
        );
        if seen.contains(&k) {
            out.push(at(
                vec![key("grant"), index(i)],
                format!(
                    "a second grant to {}@{} on {}",
                    k.0,
                    k.1,
                    sql::target(k.2, k.3)
                ),
                Some("list every privilege in one grant block"),
            ));
        }
        seen.push(k);
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
                privileges: vec!["select".into(), "insert".into()],
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

#[cfg(test)]
mod levels {
    use super::*;
    use crate::Grant;
    use std::collections::BTreeMap;

    fn with(grants: Vec<Grant>, settings: &[(&str, &str)]) -> MariadbSpec {
        MariadbSpec {
            character_set: Some(String::new()),
            collation: None,
            settings: settings
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect::<BTreeMap<_, _>>(),
            user: vec![],
            grant: grants,
        }
    }

    fn g(privs: &[&str], db: Option<&str>, table: Option<&str>) -> Grant {
        Grant {
            user: "app".into(),
            host: None,
            privileges: privs.iter().map(|p| (*p).into()).collect(),
            database: db.map(Into::into),
            table: table.map(Into::into),
        }
    }

    fn messages(s: &MariadbSpec) -> Vec<String> {
        check(s).into_iter().map(|d| d.message).collect()
    }

    /// Each of these MariaDB 11.4 refuses at GRANT (1064, 1221, 1144); each is refused here at its line.
    #[test]
    fn what_mariadb_refuses_at_grant_is_refused_before() {
        let m = messages(&with(
            vec![g(&["SELECT", "ALL PRIVILEGES"], Some("app"), None)],
            &[],
        ));
        assert_eq!(m, ["ALL PRIVILEGES goes alone"]);
        let m = messages(&with(vec![g(&["SHUTDOWN"], Some("app"), None)], &[]));
        assert_eq!(m, ["SHUTDOWN cannot be granted on a database"]);
        let m = messages(&with(
            vec![g(&["CREATE ROUTINE"], Some("app"), Some("t"))],
            &[],
        ));
        assert_eq!(m, ["CREATE ROUTINE cannot be granted on a table"]);
        assert!(
            messages(&with(
                vec![g(&["SHUTDOWN", "REPLICATION REPLICA"], None, None)],
                &[]
            ))
            .is_empty()
        );
        let mut empty = g(&["SELECT"], None, None);
        empty.user = String::new();
        assert_eq!(messages(&with(vec![empty], &[])).len(), 1);
    }

    #[test]
    fn two_grants_on_one_target_and_settings_that_break_lines_are_refused() {
        let m = messages(&with(
            vec![
                g(&["SELECT"], Some("app"), None),
                g(&["INSERT"], Some("app"), None),
            ],
            &[
                ("max_connections", "1\nskip-networking"),
                ("loose_skip_networking", "1"),
                ("bind_address", "0.0.0.0"),
            ],
        ));
        assert_eq!(m.len(), 4, "{m:?}");
        assert!(m.iter().any(|x| x.starts_with("a second grant")));
        assert!(m.iter().any(|x| x.contains("cannot be one my.cnf line")));
        assert_eq!(
            m.iter().filter(|x| x.contains("is doze's to set")).count(),
            2
        );
    }
}
