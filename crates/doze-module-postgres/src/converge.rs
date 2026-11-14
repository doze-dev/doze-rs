//! Keeping the instance's structure as declared: roles, the database, schemas, extensions and grants made or changed
//! by `converge`, listed by `objects`, and taken away by `prune` when they leave doze.hcl. Every statement runs as
//! `postgres` over the server's socket ([`Psql`]).
//!
//! Better than Go (bug 10, "converge to the declared state"): a removed `member_of` is revoked, a removed password,
//! expiry, role setting or comment is cleared; a privilege taken out of a grant is revoked and a removed grant is too
//! (grants are objects); a role that owns objects can be dropped (its objects pass to `postgres`, its privileges go).

use crate::psql::{Psql, wrap};
use crate::spec::{PostgresSpec, some};
use crate::{Postgres, ext, names, sql};
use doze_module::caps::{Converge, Objects, Prune};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Instance, Object, Toolchain};
use doze_module::{Cx, Result, help};

const ROLE: &str = "role";
const DATABASE: &str = "database";
const SCHEMA: &str = "schema";
const EXTENSION: &str = "extension";
const GRANT: &str = "grant";

async fn roles(p: &Psql, s: &PostgresSpec) -> Result<()> {
    for r in &s.role {
        let q = format!(
            "SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = {})",
            sql::lit(&r.name)
        );
        let exists = p
            .yes("postgres", &q)
            .await
            .map_err(wrap(format!("role {:?}", r.name)))?;
        p.exec("postgres", &sql::role(r, exists))
            .await
            .map_err(wrap(format!("role {:?}", r.name)))?;
    }
    // Memberships once every declared role exists, so one may name a role declared after it.
    for r in &s.role {
        let q = format!(
            "SELECT b.rolname FROM pg_auth_members m JOIN pg_roles b ON b.oid = m.roleid \
             JOIN pg_roles r ON r.oid = m.member WHERE r.rolname = {} GROUP BY b.rolname",
            sql::lit(&r.name)
        );
        let now = p
            .column("postgres", &q)
            .await
            .map_err(wrap(format!("role {:?}", r.name)))?;
        let change = sql::membership(r, &now);
        if !change.is_empty() {
            p.exec("postgres", &change)
                .await
                .map_err(wrap(format!("role {:?}: membership", r.name)))?;
        }
    }
    Ok(())
}

async fn database(p: &Psql, name: &str, s: &PostgresSpec) -> Result<()> {
    let at = || wrap(format!("database {name:?}"));
    let q = format!(
        "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = {})",
        sql::lit(name)
    );
    if !p.yes("postgres", &q).await.map_err(at())? {
        p.alone("postgres", &sql::create_database(name, s))
            .await
            .map_err(at())?;
    }
    p.exec("postgres", &sql::alter_database(name, s))
        .await
        .map_err(at())
}

async fn extensions(cx: &Cx<PostgresSpec>, p: &Psql) -> Result<()> {
    let db = &cx.instance.name;
    for e in cx.spec.exts() {
        let q = format!(
            "SELECT EXISTS(SELECT 1 FROM pg_available_extensions WHERE name = {})",
            sql::lit(&e.name)
        );
        let made = match p.yes(db, &q).await {
            Ok(true) => p.exec(db, &ext::statements(&e)).await,
            Ok(false) => Err(Error::new(
                ErrorKind::Unavailable,
                "this PostgreSQL build does not ship it (bundles from `source` and doze's catalog are installed by a later release of the module)",
            )),
            Err(e) => Err(e),
        };
        match made {
            Ok(()) => {}
            Err(why) if e.optional => cx.log(
                2,
                format!("extension {:?} skipped: {}", e.name, why.message),
            ),
            Err(why) => return Err(wrap(format!("extension {:?}", e.name))(why)),
        }
    }
    Ok(())
}

impl Converge for Postgres {
    async fn converge(&self, cx: &Cx<PostgresSpec>, tc: &Toolchain) -> Result<()> {
        let (s, name) = (&cx.spec, &cx.instance.name);
        let p = Psql::new(tc, &cx.instance)?;
        roles(&p, s).await?;
        database(&p, name, s).await?;
        let schemas = sql::schemas(s);
        if !schemas.is_empty() {
            p.exec(name, &schemas)
                .await
                .map_err(wrap("schemas".into()))?;
        }
        extensions(cx, &p).await?;
        for g in &s.grant {
            let Some(on) = sql::target(g) else { continue };
            p.exec(on.runs_in(name), &sql::grant(g, &on))
                .await
                .map_err(wrap(format!("grant to {:?}", g.role)))?;
        }
        // Last: a database closed to connections is closed once converge no longer needs to work in it.
        if let Some(close) = sql::close_database(name, s) {
            p.exec("postgres", &[close])
                .await
                .map_err(wrap(format!("database {name:?}")))?;
        }
        Ok(())
    }
}

impl Objects for Postgres {
    fn objects(&self, cx: &Cx<PostgresSpec>) -> Result<Vec<Object>> {
        let s = &cx.spec;
        let obj = |kind: &str, name: String, declared: Vec<String>| Object {
            kind: kind.into(),
            name,
            hash: help::object_hash(&declared),
        };
        let o = |v: &Option<String>| some(v).unwrap_or_default().to_string();
        let mut out: Vec<Object> = s
            .role
            .iter()
            .map(|r| {
                obj(
                    ROLE,
                    r.name.clone(),
                    vec![
                        sql::role_options(r),
                        r.member_of.join("\0"),
                        format!("{:?}", r.config),
                        o(&r.comment),
                    ],
                )
            })
            .collect();
        let (collate, ctype) = s.collate_ctype();
        let db = vec![
            o(&s.owner),
            o(&s.encoding),
            collate.unwrap_or_default().into(),
            ctype.unwrap_or_default().into(),
            o(&s.template),
            o(&s.tablespace),
            o(&s.comment),
            s.connection_limit.unwrap_or(-1).to_string(),
            s.is_template.to_string(),
            s.allow_connections.to_string(),
        ];
        out.push(obj(DATABASE, cx.instance.name.clone(), db));
        out.extend(
            s.schema
                .iter()
                .map(|sc| obj(SCHEMA, sc.name.clone(), vec![o(&sc.owner)])),
        );
        out.extend(s.exts().into_iter().map(|e| {
            let declared = vec![
                o(&e.version),
                o(&e.schema),
                o(&e.source),
                e.optional.to_string(),
                e.cascade.to_string(),
            ];
            obj(EXTENSION, e.name, declared)
        }));
        for g in &s.grant {
            if let Some(on) = sql::target(g) {
                out.push(obj(GRANT, on.name(&g.role), sql::grant(g, &on)));
            }
        }
        Ok(out)
    }
}

/// A role's objects pass to `postgres` and its privileges go, in every database that takes connections, then the
/// role is dropped (Go's bare DROP ROLE failed on any role that owned something).
async fn drop_role(p: &Psql, name: &str) -> Result<()> {
    let q = format!(
        "SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = {})",
        sql::lit(name)
    );
    if !p.yes("postgres", &q).await? {
        return Ok(());
    }
    let r = sql::ident(name);
    for db in p
        .column(
            "postgres",
            "SELECT datname FROM pg_database WHERE datallowconn ORDER BY 1",
        )
        .await?
    {
        p.exec(
            &db,
            &[
                format!("REASSIGN OWNED BY {r} TO postgres"),
                format!("DROP OWNED BY {r}"),
            ],
        )
        .await?;
    }
    p.exec("postgres", &[format!("DROP ROLE IF EXISTS {r}")])
        .await
}

/// Takes one removed object away.
async fn drop_object(p: &Psql, inst: &Instance, o: &Object) -> Result<()> {
    let db = &inst.name;
    match o.kind.as_str() {
        EXTENSION => {
            p.exec(
                db,
                &[format!(
                    "DROP EXTENSION IF EXISTS {} CASCADE",
                    sql::ident(&o.name)
                )],
            )
            .await
        }
        SCHEMA => {
            p.exec(
                db,
                &[format!(
                    "DROP SCHEMA IF EXISTS {} CASCADE",
                    sql::ident(&o.name)
                )],
            )
            .await
        }
        DATABASE => {
            p.alone(
                "postgres",
                &format!(
                    "DROP DATABASE IF EXISTS {} WITH (FORCE)",
                    sql::ident(&o.name)
                ),
            )
            .await
        }
        ROLE => drop_role(p, &o.name).await,
        GRANT => {
            let Some((role, on)) = names::grant(&o.name) else {
                return Err(Error::new(
                    ErrorKind::Invalid,
                    "not a name this module makes",
                ));
            };
            match p.exec(on.runs_in(db), &sql::revoke(&on, &role)).await {
                // Its role, schema or database already gone: nothing is left to revoke.
                Err(e) if e.message.contains("does not exist") => Ok(()),
                other => other,
            }
        }
        // Unknown kinds are skipped, as Go did.
        _ => Ok(()),
    }
}

impl Prune for Postgres {
    async fn prune(&self, cx: &Cx<PostgresSpec>, tc: &Toolchain, removed: &[Object]) -> Result<()> {
        // Grants, then what lives in the database, then the database, then roles (which may own any of it).
        let rank = |k: &str| match k {
            GRANT => 0,
            EXTENSION => 1,
            SCHEMA => 2,
            DATABASE => 3,
            _ => 4,
        };
        let mut objs: Vec<&Object> = removed.iter().collect();
        objs.sort_by_key(|o| rank(&o.kind));
        let p = Psql::new(tc, &cx.instance)?;
        for o in objs {
            drop_object(&p, &cx.instance, o)
                .await
                .map_err(wrap(format!("dropping {} {:?}", o.kind, o.name)))?;
        }
        Ok(())
    }
}
