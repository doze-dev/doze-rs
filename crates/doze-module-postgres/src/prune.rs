//! Taking away what left doze.hcl. Prune never loses data the person did not declare: a role that owns objects is
//! dropped after its objects pass to `postgres` (Go's bare DROP ROLE failed on any owner); an extension or a schema
//! something still depends on is not dropped with what depends on it (Go's CASCADE dropped the columns of an
//! extension's types and every table of a schema), the error saying what is in the way. A database closed to
//! connections is opened for prune and closed again.

use crate::converge::{DATABASE, EXTENSION, GRANT, ROLE, SCHEMA, revocations};
use crate::psql::{Psql, wrap};
use crate::spec::PostgresSpec;
use crate::{Postgres, names, sql};
use doze_module::caps::Prune;
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Object, Toolchain};
use doze_module::{Cx, Result};

fn allow(db: &str, on: bool) -> String {
    format!(
        "ALTER DATABASE {} WITH ALLOW_CONNECTIONS {on}",
        sql::ident(db)
    )
}

/// The databases a role may own objects in (every one but template0), each with whether it takes connections.
async fn databases(p: &Psql) -> Result<Vec<(String, bool)>> {
    let rows = p
        .rows(
            "postgres",
            "SELECT datname, datallowconn FROM pg_database WHERE datname <> 'template0' ORDER BY 1",
            2,
        )
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| match r.as_slice() {
            [n, a] => Some((n.clone(), a == "t")),
            _ => None,
        })
        .collect())
}

/// A role's objects pass to `postgres` and its privileges go, in every database (one closed to connections is opened
/// for it and closed again), then the role is dropped.
async fn drop_role(p: &Psql, name: &str) -> Result<()> {
    let q = format!(
        "SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = {})",
        sql::lit(name)
    );
    if !p.yes("postgres", &q).await? {
        return Ok(());
    }
    let r = sql::ident(name);
    for (db, open) in databases(p).await? {
        if !open {
            p.exec("postgres", &[allow(&db, true)]).await?;
        }
        let done = p
            .exec(
                &db,
                &[
                    format!("REASSIGN OWNED BY {r} TO postgres"),
                    format!("DROP OWNED BY {r}"),
                ],
            )
            .await;
        if !open {
            p.exec("postgres", &[allow(&db, false)]).await?;
        }
        done?;
    }
    p.exec("postgres", &[format!("DROP ROLE IF EXISTS {r}")])
        .await
}

/// What still depends on a dropped object, said plainly.
fn in_the_way(e: Error) -> Error {
    if e.message.contains("depend") {
        let mut e = e;
        e.message = format!(
            "{} (doze drops nothing that holds data: drop what depends on it first)",
            e.message
        );
        return e;
    }
    e
}

/// Takes one removed object away.
async fn drop_object(p: &Psql, db: &str, o: &Object) -> Result<()> {
    match o.kind.as_str() {
        EXTENSION => p
            .exec(
                db,
                &[format!("DROP EXTENSION IF EXISTS {}", sql::ident(&o.name))],
            )
            .await
            .map_err(in_the_way),
        SCHEMA => p
            .exec(
                db,
                &[format!("DROP SCHEMA IF EXISTS {}", sql::ident(&o.name))],
            )
            .await
            .map_err(in_the_way),
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
            let at = on.runs_in(db);
            let revoked = match revocations(p, at, &on, &role).await {
                Ok(q) => p.exec(at, &q).await,
                Err(e) => Err(e),
            };
            match revoked {
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
        let db = cx.instance.name.as_str();
        let closed = databases(&p)
            .await?
            .iter()
            .any(|(n, open)| n == db && !open);
        if closed {
            p.exec("postgres", &[allow(db, true)]).await?;
        }
        let mut result = Ok(());
        for o in objs {
            result = drop_object(&p, db, o)
                .await
                .map_err(wrap(format!("dropping {} {:?}", o.kind, o.name)));
            if result.is_err() {
                break;
            }
        }
        // Closed again as declared, if it is still there.
        let gone = removed.iter().any(|o| o.kind == DATABASE && o.name == db);
        if closed && !gone {
            p.exec("postgres", &[allow(db, cx.spec.allow_connections)])
                .await?;
        }
        result
    }
}
