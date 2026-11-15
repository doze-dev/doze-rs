//! Keeping the instance's structure as declared: roles, the database, schemas, extensions and grants made or changed
//! by `converge`, and listed by `objects` (prune is in `prune`). Every statement runs as `postgres` over the server's
//! socket ([`Psql`]).
//!
//! Better than Go (bug 10, "converge to the declared state"): a removed `member_of` is revoked, a removed password,
//! expiry, role setting, comment or schema owner is cleared; a privilege taken out of a grant is revoked (never an
//! owner's on what it owns), and a removed grant is too (grants are objects); an extension moved to another schema is
//! moved. Two converges racing on one server (a boot and a first connection) both succeed, as in Go.

use crate::psql::{Psql, wrap};
use crate::spec::{Ext, PostgresSpec, some};
use crate::{Postgres, ext, sql};
use doze_module::caps::{Converge, Objects};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Object, Toolchain};
use doze_module::{Cx, Result, help};

pub(crate) const ROLE: &str = "role";
pub(crate) const DATABASE: &str = "database";
pub(crate) const SCHEMA: &str = "schema";
pub(crate) const EXTENSION: &str = "extension";
pub(crate) const GRANT: &str = "grant";

/// Whether a failure is another session having made the same thing first.
fn raced(e: &Error) -> bool {
    e.message.contains("already exists")
}

async fn roles(p: &Psql, s: &PostgresSpec) -> Result<()> {
    for r in &s.role {
        let at = || wrap(format!("role {:?}", r.name));
        let q = format!(
            "SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = {})",
            sql::lit(&r.name)
        );
        let exists = p.yes("postgres", &q).await.map_err(at())?;
        match p.exec("postgres", &sql::role(r, exists)).await {
            Err(e) if !exists && raced(&e) => p
                .exec("postgres", &sql::role(r, true))
                .await
                .map_err(at())?,
            other => other.map_err(at())?,
        }
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
        match p.alone("postgres", &sql::create_database(name, s)).await {
            Err(e) if !raced(&e) => return Err(at()(e)),
            _ => {}
        }
    }
    p.exec("postgres", &sql::alter_database(name, s))
        .await
        .map_err(at())
}

/// One extension: created or updated, and moved if its declared schema is another.
async fn extension(p: &Psql, db: &str, e: &Ext) -> Result<()> {
    let q = format!(
        "SELECT EXISTS(SELECT 1 FROM pg_available_extensions WHERE name = {})",
        sql::lit(&e.name)
    );
    if !p.yes(db, &q).await? {
        return Err(Error::new(
            ErrorKind::Unavailable,
            "this PostgreSQL build does not ship it (bundles from `source` and doze's catalog are installed by a later \
             release of the module)",
        ));
    }
    p.exec(db, &ext::statements(e)).await?;
    let Some(want) = &e.schema else { return Ok(()) };
    let q = format!(
        "SELECT n.nspname FROM pg_extension x JOIN pg_namespace n ON n.oid = x.extnamespace WHERE x.extname = {}",
        sql::lit(&e.name)
    );
    if p.column(db, &q)
        .await?
        .first()
        .is_some_and(|now| now != want)
    {
        let moved = format!(
            "ALTER EXTENSION {} SET SCHEMA {}",
            sql::ident(&e.name),
            sql::ident(want)
        );
        p.exec(db, &[moved]).await?;
    }
    Ok(())
}

async fn extensions(cx: &Cx<PostgresSpec>, p: &Psql) -> Result<()> {
    let db = &cx.instance.name;
    for e in cx.spec.exts() {
        match extension(p, db, &e).await {
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

/// What `role` holds on `on` that a grant or its removal takes back: every privilege, except an owner's on what it
/// owns.
pub(crate) async fn revocations(
    p: &Psql,
    db: &str,
    on: &sql::On,
    role: &str,
) -> Result<Vec<String>> {
    let targets = p.column(db, &sql::revocable(on, role)).await?;
    Ok(sql::revoke(on, role, &targets))
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
            let db = on.runs_in(name);
            let at = || wrap(format!("grant to {:?}", g.role));
            let mut q = revocations(&p, db, &on, &g.role).await.map_err(at())?;
            q.extend(sql::grant(g, &on));
            p.exec(db, &q).await.map_err(at())?;
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
    /// Each object with a hash of what converge applies to it. The database's encoding, locale, template and tablespace
    /// are left out: they are fixed when it is made, so a change to them is nothing converge could do.
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
                let declared = vec![
                    sql::role_options(r),
                    r.member_of.join("\0"),
                    format!("{:?}", r.config),
                    o(&r.comment),
                ];
                obj(ROLE, r.name.clone(), declared)
            })
            .collect();
        let db = vec![
            o(&s.owner),
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
