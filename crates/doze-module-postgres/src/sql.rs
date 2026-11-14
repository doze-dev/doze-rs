//! The SQL converge and prune send, built from the declaration alone (no I/O here, so every statement is tested as
//! text). Names reach SQL only as quoted identifiers and values only as quoted literals.

pub(crate) use crate::names::{On, ident};
use crate::spec::{Grant, PostgresSpec, Role, some};

/// The grant's target, `None` if it names none or two (refused by `check`).
pub(crate) fn target(g: &Grant) -> Option<On> {
    match (some(&g.database), some(&g.schema)) {
        (Some(d), None) => some(&g.objects).is_none().then(|| On::Database(d.into())),
        (None, Some(s)) => match some(&g.objects) {
            None => Some(On::Schema(s.into())),
            Some(o) => crate::names::kind(o).map(|k| On::All(k, s.into())),
        },
        _ => None,
    }
}

/// `'value'`, a single quote doubled inside (standard_conforming_strings is on: a backslash is a backslash).
pub(crate) fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn flag(on: bool, word: &str) -> String {
    if on { word.into() } else { format!("NO{word}") }
}

/// The role's attributes, every one stated, so ALTER puts back what was changed by hand or removed from doze.hcl: a
/// password or an expiry removed is cleared (Go left the old one: bug 10).
pub(crate) fn role_options(r: &Role) -> String {
    let mut o = vec![
        flag(r.login, "LOGIN"),
        flag(r.superuser, "SUPERUSER"),
        flag(r.createdb, "CREATEDB"),
        flag(r.createrole, "CREATEROLE"),
        flag(r.replication, "REPLICATION"),
        flag(r.inherit, "INHERIT"),
        flag(r.bypassrls, "BYPASSRLS"),
        format!("CONNECTION LIMIT {}", r.connection_limit.unwrap_or(-1)),
    ];
    o.push(match some(&r.password) {
        Some(p) => format!("PASSWORD {}", lit(p)),
        None => "PASSWORD NULL".into(),
    });
    o.push(format!(
        "VALID UNTIL {}",
        lit(some(&r.valid_until).unwrap_or("infinity"))
    ));
    o.join(" ")
}

/// The role made or changed, its settings exactly the declared ones (RESET ALL first), its comment set or cleared.
pub(crate) fn role(r: &Role, exists: bool) -> Vec<String> {
    let n = ident(&r.name);
    let verb = if exists { "ALTER" } else { "CREATE" };
    let mut q = vec![format!("{verb} ROLE {n} WITH {}", role_options(r))];
    q.push(format!("ALTER ROLE {n} RESET ALL"));
    for (k, v) in &r.config {
        q.push(format!("ALTER ROLE {n} SET {} = {}", ident(k), lit(v)));
    }
    q.push(comment("ROLE", &n, some(&r.comment)));
    q
}

fn comment(what: &str, n: &str, c: Option<&str>) -> String {
    let c = c.map_or_else(|| "NULL".to_string(), lit);
    format!("COMMENT ON {what} {n} IS {c}")
}

/// Memberships to grant and to revoke so the role is a member of exactly the declared roles.
pub(crate) fn membership(r: &Role, current: &[String]) -> Vec<String> {
    let me = ident(&r.name);
    let mut q: Vec<String> = r
        .member_of
        .iter()
        .filter(|p| !current.contains(p))
        .map(|p| format!("GRANT {} TO {me}", ident(p)))
        .collect();
    q.extend(
        current
            .iter()
            .filter(|c| !r.member_of.contains(c))
            .map(|c| format!("REVOKE {} FROM {me}", ident(c))),
    );
    q
}

/// `CREATE DATABASE` for the instance (Go's: a template0 when an encoding or locale is set and no template is).
pub(crate) fn create_database(name: &str, s: &PostgresSpec) -> String {
    let (collate, ctype) = s.collate_ctype();
    let encoding = some(&s.encoding);
    let template = some(&s.template)
        .or((encoding.is_some() || collate.is_some() || ctype.is_some()).then_some("template0"));
    let mut w = Vec::new();
    if let Some(o) = some(&s.owner) {
        w.push(format!("OWNER {}", ident(o)));
    }
    if let Some(t) = template {
        w.push(format!("TEMPLATE {}", ident(t)));
    }
    for (k, v) in [
        ("ENCODING", encoding),
        ("LC_COLLATE", collate),
        ("LC_CTYPE", ctype),
    ] {
        if let Some(v) = v {
            w.push(format!("{k} {}", lit(v)));
        }
    }
    if let Some(t) = some(&s.tablespace) {
        w.push(format!("TABLESPACE {}", ident(t)));
    }
    if let Some(n) = s.connection_limit.filter(|n| *n != -1) {
        w.push(format!("CONNECTION LIMIT {n}"));
    }
    let mut q = format!("CREATE DATABASE {}", ident(name));
    if !w.is_empty() {
        q.push_str(" WITH ");
        q.push_str(&w.join(" "));
    }
    q
}

/// The database's options as declared, every one stated (Go never put back `is_template = false`, a limit removed, an
/// owner or a comment removed), and connections allowed while converge works in it; [`close_database`] applies
/// `allow_connections = false` once it is done.
pub(crate) fn alter_database(name: &str, s: &PostgresSpec) -> Vec<String> {
    let n = ident(name);
    vec![
        format!(
            "ALTER DATABASE {n} OWNER TO {}",
            ident(some(&s.owner).unwrap_or("postgres"))
        ),
        format!(
            "ALTER DATABASE {n} WITH ALLOW_CONNECTIONS true CONNECTION LIMIT {} IS_TEMPLATE {}",
            s.connection_limit.unwrap_or(-1),
            s.is_template
        ),
        comment("DATABASE", &n, some(&s.comment)),
    ]
}

/// The last statement of a converge, if the database is declared closed to connections.
pub(crate) fn close_database(name: &str, s: &PostgresSpec) -> Option<String> {
    (!s.allow_connections).then(|| {
        format!(
            "ALTER DATABASE {} WITH ALLOW_CONNECTIONS false",
            ident(name)
        )
    })
}

/// Schemas made, and their owner kept as declared.
pub(crate) fn schemas(s: &PostgresSpec) -> Vec<String> {
    let mut q = Vec::new();
    for sc in &s.schema {
        let n = ident(&sc.name);
        match some(&sc.owner) {
            Some(o) => {
                q.push(format!(
                    "CREATE SCHEMA IF NOT EXISTS {n} AUTHORIZATION {}",
                    ident(o)
                ));
                q.push(format!("ALTER SCHEMA {n} OWNER TO {}", ident(o)));
            }
            None => q.push(format!("CREATE SCHEMA IF NOT EXISTS {n}")),
        }
    }
    q
}

/// The statements that take every privilege on `on` from `role`: what a grant removed from doze.hcl, or changed,
/// leaves behind.
pub(crate) fn revoke(on: &On, role: &str) -> Vec<String> {
    let r = ident(role);
    let mut q = vec![format!("REVOKE ALL ON {} FROM {r}", on.sql())];
    if let On::All(k, s) = on {
        q.push(format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA {} REVOKE ALL ON {k} FROM {r}",
            ident(s)
        ));
    }
    q
}

/// A grant made exactly as declared: what the role had on the target is revoked first, in the same transaction, so a
/// privilege taken out of the list is taken away (Go only ever added).
pub(crate) fn grant(g: &Grant, on: &On) -> Vec<String> {
    let privs = g
        .privileges
        .iter()
        .map(|p| p.trim().to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(", ");
    let wgo = if g.with_grant_option {
        " WITH GRANT OPTION"
    } else {
        ""
    };
    let r = ident(&g.role);
    let mut q = revoke(on, &g.role);
    q.push(format!("GRANT {privs} ON {} TO {r}{wgo}", on.sql()));
    if let On::All(k, s) = on {
        q.push(format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA {} GRANT {privs} ON {k} TO {r}{wgo}",
            ident(s)
        ));
    }
    q
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn role_named(n: &str) -> Role {
        Role {
            name: n.into(),
            password: None,
            login: true,
            superuser: false,
            createdb: false,
            createrole: false,
            replication: false,
            inherit: true,
            bypassrls: false,
            connection_limit: None,
            valid_until: None,
            member_of: vec![],
            comment: None,
            config: BTreeMap::new(),
        }
    }

    #[test]
    fn quoting_doubles_the_quote_and_nothing_else() {
        assert_eq!(ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(lit("it's \\n"), "'it''s \\n'");
    }

    #[test]
    fn a_role_states_every_attribute_and_clears_what_was_removed() {
        let mut r = role_named("app");
        assert_eq!(
            role_options(&r),
            "LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION INHERIT NOBYPASSRLS CONNECTION LIMIT -1 PASSWORD NULL VALID UNTIL 'infinity'"
        );
        r.password = Some("p'w".into());
        r.config = BTreeMap::from([("search_path".into(), "app, public".into())]);
        r.comment = Some("c".into());
        assert_eq!(
            role(&r, true),
            [
                "ALTER ROLE \"app\" WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION INHERIT NOBYPASSRLS CONNECTION LIMIT -1 PASSWORD 'p''w' VALID UNTIL 'infinity'",
                "ALTER ROLE \"app\" RESET ALL",
                "ALTER ROLE \"app\" SET \"search_path\" = 'app, public'",
                "COMMENT ON ROLE \"app\" IS 'c'",
            ]
        );
    }

    #[test]
    fn membership_grants_the_new_and_revokes_the_removed() {
        let mut r = role_named("app");
        r.member_of = vec!["readers".into(), "writers".into()];
        let now = vec!["writers".to_string(), "admins".to_string()];
        assert_eq!(
            membership(&r, &now),
            [
                "GRANT \"readers\" TO \"app\"",
                "REVOKE \"admins\" FROM \"app\""
            ]
        );
    }

    #[test]
    fn grant_targets_name_and_statements() {
        let g = Grant {
            role: "app".into(),
            privileges: vec!["select".into(), " insert".into()],
            database: None,
            schema: Some("an\"x".into()),
            objects: Some("tables".into()),
            with_grant_option: true,
        };
        let on = target(&g).unwrap_or(On::Schema(String::new()));
        assert_eq!(on.name("app"), "\"app\" on tables in schema \"an\"\"x\"");
        assert_eq!(
            grant(&g, &on),
            [
                "REVOKE ALL ON ALL TABLES IN SCHEMA \"an\"\"x\" FROM \"app\"",
                "ALTER DEFAULT PRIVILEGES IN SCHEMA \"an\"\"x\" REVOKE ALL ON TABLES FROM \"app\"",
                "GRANT SELECT, INSERT ON ALL TABLES IN SCHEMA \"an\"\"x\" TO \"app\" WITH GRANT OPTION",
                "ALTER DEFAULT PRIVILEGES IN SCHEMA \"an\"\"x\" GRANT SELECT, INSERT ON TABLES TO \"app\" WITH GRANT OPTION",
            ]
        );
    }
}
