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
        q.push(format!(
            "ALTER ROLE {n} SET {} = {}",
            ident(k),
            setting_value(k, v)
        ));
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
            None => {
                q.push(format!("CREATE SCHEMA IF NOT EXISTS {n}"));
                q.push(format!("ALTER SCHEMA {n} OWNER TO postgres"));
            }
        }
    }
    q
}

/// The query listing what a revocation on `on` from `role` may touch, each named as SQL takes it: the target itself
/// unless `role` owns it, or each object of the kind in the schema that `role` does not own. An owner's privileges on
/// what it owns are never taken (REVOKE ALL on a schema's tables would take an owner's rights on its own tables).
pub(crate) fn revocable(on: &On, role: &str) -> String {
    let me = format!("(SELECT oid FROM pg_roles WHERE rolname = {})", lit(role));
    match on {
        On::Database(d) => format!(
            "SELECT 'DATABASE ' || quote_ident(datname) FROM pg_database WHERE datname = {} AND datdba IS DISTINCT FROM {me}",
            lit(d)
        ),
        On::Schema(sc) => format!(
            "SELECT 'SCHEMA ' || quote_ident(nspname) FROM pg_namespace WHERE nspname = {} AND nspowner IS DISTINCT FROM {me}",
            lit(sc)
        ),
        On::All("FUNCTIONS", sc) => format!(
            "SELECT 'ROUTINE ' || p.oid::regprocedure::text FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = {} AND p.proowner IS DISTINCT FROM {me} ORDER BY 1",
            lit(sc)
        ),
        On::All(k, sc) => {
            let (word, kinds) = if *k == "SEQUENCES" {
                ("SEQUENCE", "'S'")
            } else {
                ("TABLE", "'r', 'p', 'v', 'm', 'f'")
            };
            format!(
                "SELECT '{word} ' || format('%I.%I', n.nspname, c.relname) FROM pg_class c \
                 JOIN pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = {} AND c.relkind IN ({kinds}) \
                 AND c.relowner IS DISTINCT FROM {me} ORDER BY 1",
                lit(sc)
            )
        }
    }
}

/// Every privilege of `role` on `targets` (rows of [`revocable`]) taken, with what it granted on from them (CASCADE:
/// the declared state wins over a re-grant the role made with its grant option), and for a schema's objects the
/// default privileges too.
pub(crate) fn revoke(on: &On, role: &str, targets: &[String]) -> Vec<String> {
    let r = ident(role);
    let mut q: Vec<String> = targets
        .iter()
        .map(|t| format!("REVOKE ALL ON {t} FROM {r} CASCADE"))
        .collect();
    if let On::All(k, s) = on {
        q.push(format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA {} REVOKE ALL ON {k} FROM {r} CASCADE",
            ident(s)
        ));
    }
    q
}

/// The grant as declared; run after [`revoke`] in the same transaction, so a privilege taken out of the list is taken
/// away (Go only ever added).
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
    let mut q = vec![format!("GRANT {privs} ON {} TO {r}{wgo}", on.sql())];
    if let On::All(k, s) = on {
        q.push(format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA {} GRANT {privs} ON {k} TO {r}{wgo}",
            ident(s)
        ));
    }
    q
}

/// Settings PostgreSQL reads as a list: a role's value for one is set item by item (`'a', 'b'`), not as one quoted
/// string naming one item (Go's `SET search_path = 'app, public'` named a schema "app, public").
const LIST_SETTINGS: [&str; 5] = [
    "search_path",
    "temp_tablespaces",
    "session_preload_libraries",
    "local_preload_libraries",
    "datestyle",
];

/// A role setting's value as SQL.
pub(crate) fn setting_value(k: &str, v: &str) -> String {
    if LIST_SETTINGS.iter().any(|l| l.eq_ignore_ascii_case(k)) {
        let items: Vec<String> = v
            .split(',')
            .map(str::trim)
            .filter(|i| !i.is_empty())
            .map(lit)
            .collect();
        if !items.is_empty() {
            return items.join(", ");
        }
    }
    lit(v)
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
                "ALTER ROLE \"app\" SET \"search_path\" = 'app', 'public'",
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
            revoke(&on, "app", &["TABLE t".to_string()]),
            [
                "REVOKE ALL ON TABLE t FROM \"app\" CASCADE",
                "ALTER DEFAULT PRIVILEGES IN SCHEMA \"an\"\"x\" REVOKE ALL ON TABLES FROM \"app\" CASCADE",
            ]
        );
        assert!(revocable(&on, "a'b").contains("rolname = 'a''b'"));
        assert_eq!(setting_value("work_mem", "8MB"), "'8MB'");
        assert_eq!(
            grant(&g, &on),
            [
                "GRANT SELECT, INSERT ON ALL TABLES IN SCHEMA \"an\"\"x\" TO \"app\" WITH GRANT OPTION",
                "ALTER DEFAULT PRIVILEGES IN SCHEMA \"an\"\"x\" GRANT SELECT, INSERT ON TABLES TO \"app\" WITH GRANT OPTION",
            ]
        );
    }
}
