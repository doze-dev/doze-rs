//! Keeping the declared structure over the gateway's own socket: databases and collections exist, indexes are as
//! declared (one whose keys or options changed is rebuilt; Go's create failed on it), empty collections get their seed.
//! Every step is safe to repeat. Objects (for `doze plan`) are databases,
//! collections and indexes, so an index removed from doze.hcl is dropped too (Go kept it).

use crate::bson::{Bson, Doc};
use crate::msg::Failure;
use crate::wire::Client;
use crate::{Collection, Ferret, FerretSpec, Index, check};
use doze_module::caps::{Converge, Objects, Prune};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::{Object, Toolchain};
use doze_module::{Cx, Result, help};
use std::time::Duration;

/// An index as its object sees it: name, keys in order, unique.
type IndexFacts = (String, Vec<(String, i64)>, bool);

const DATABASE: &str = "database";
const COLLECTION: &str = "collection";
const INDEX: &str = "index";

/// How long one command may take; a seed batch is the slowest.
const DEADLINE: Duration = Duration::from_secs(60);
/// Documents per insert: well under MongoDB's 100,000 and its 48 MB message.
const BATCH: usize = 1000;
const BATCH_BYTES: usize = 16 << 20;

/// MongoDB's codes for "it is already there" and "it is not there".
const NAMESPACE_EXISTS: i64 = 48;
const NAMESPACE_NOT_FOUND: i64 = 26;
const INDEX_NOT_FOUND: i64 = 27;
const INDEX_OPTIONS_CONFLICT: i64 = 85;
const INDEX_KEY_SPECS_CONFLICT: i64 = 86;

/// A refused command is the config's to fix; a socket that failed is the engine being down.
fn kind(e: &Failure) -> ErrorKind {
    match e {
        Failure::Command { .. } => ErrorKind::Invalid,
        Failure::Io(_) => ErrorKind::Unavailable,
        Failure::Malformed(_) => ErrorKind::Internal,
    }
}

fn failed(what: String, e: &Failure) -> Error {
    Error::new(kind(e), format!("{what}: {e}"))
}

async fn client(cx: &Cx<FerretSpec>) -> Result<Client> {
    Ok(Client::new(
        help::dial_backend(&cx.instance).await?,
        DEADLINE,
    ))
}

/// The index's name: the declared one, else MongoDB's own rule (`field_1_other_-1`), which the driver Go used made.
pub(crate) fn index_name(ix: &Index) -> String {
    ix.name.clone().unwrap_or_else(|| {
        let parts: Vec<String> = ix.keys.iter().map(|(f, d)| format!("{f}_{d}")).collect();
        parts.join("_")
    })
}

/// The `createIndexes` entry: keys in field order (doze.hcl maps have no order; Go sorted them the same way).
fn index_spec(ix: &Index) -> Doc {
    let keys = Doc(ix
        .keys
        .iter()
        .map(|(f, d)| (f.clone(), Bson::Int32(if *d < 0 { -1 } else { 1 })))
        .collect());
    let mut spec = Doc::new().with("key", keys).with("name", index_name(ix));
    if ix.unique {
        spec = spec.with("unique", true);
    }
    spec
}

impl Converge for Ferret {
    async fn converge(&self, cx: &Cx<FerretSpec>, _tc: &Toolchain) -> Result<()> {
        if cx.spec.database.is_empty() {
            return Ok(());
        }
        let mut c = client(cx).await?;
        for d in &cx.spec.database {
            for coll in &d.collection {
                collection(&mut c, &d.name, coll, &cx.instance.dir).await?;
            }
        }
        Ok(())
    }
}

async fn collection(c: &mut Client, db: &str, coll: &Collection, dir: &str) -> Result<()> {
    let ns = format!("{db}.{}", coll.name);
    match c
        .run(db, Doc::new().with("create", coll.name.as_str()))
        .await
    {
        Err(e) if e.code() != Some(NAMESPACE_EXISTS) => {
            return Err(failed(format!("creating collection {ns}"), &e));
        }
        _ => {}
    }
    for ix in &coll.index {
        index(c, db, &coll.name, ix).await?;
    }
    if let Some(seed) = &coll.seed {
        seed_collection(c, db, &coll.name, seed, dir).await?;
    }
    Ok(())
}

/// Creates the index; one of that name with other keys or options is dropped and made again.
async fn index(c: &mut Client, db: &str, coll: &str, ix: &Index) -> Result<()> {
    let name = index_name(ix);
    let create = || {
        Doc::new()
            .with("createIndexes", coll)
            .with("indexes", vec![Bson::Doc(index_spec(ix))])
    };
    let what = || format!("index {name} on {db}.{coll}");
    match c.run(db, create()).await {
        Ok(_) => Ok(()),
        Err(e)
            if matches!(
                e.code(),
                Some(INDEX_OPTIONS_CONFLICT | INDEX_KEY_SPECS_CONFLICT)
            ) =>
        {
            clear_way(c, db, coll, ix)
                .await
                .map_err(|e| failed(format!("rebuilding {}", what()), &e))?;
            c.run(db, create())
                .await
                .map(|_| ())
                .map_err(|e| failed(format!("rebuilding {}", what()), &e))
        }
        Err(e) => Err(failed(format!("creating {}", what()), &e)),
    }
}

/// Drops what stands in the index's way: an index of its name with other keys or options, or one with its keys under
/// another name (a renamed index; prune would only drop the old name after converge, so converge would never pass).
async fn clear_way(
    c: &mut Client,
    db: &str,
    coll: &str,
    ix: &Index,
) -> std::result::Result<(), Failure> {
    let name = index_name(ix);
    let want = index_spec(ix);
    let list = Doc::new().with("listIndexes", coll);
    for have in c.all(db, list, coll).await? {
        let same_name = have.str("name") == Some(name.as_str());
        let same_keys = match (have.doc("key"), want.doc("key")) {
            (Some(a), Some(b)) => same_key(a, b),
            _ => false,
        };
        if let (true, Some(old)) = (same_name || same_keys, have.str("name")) {
            c.run(db, Doc::new().with("dropIndexes", coll).with("index", old))
                .await?;
        }
    }
    Ok(())
}

/// Two key documents name the same fields in the same order with the same directions (a server may send 1 as 1.0).
fn same_key(a: &Doc, b: &Doc) -> bool {
    let dir = |v: &Bson| match v {
        Bson::Int32(n) => Some(i64::from(*n)),
        Bson::Int64(n) => Some(*n),
        Bson::Double(d) if d.fract() == 0.0 => Some(*d as i64),
        _ => None,
    };
    a.0.len() == b.0.len()
        && a.0
            .iter()
            .zip(&b.0)
            .all(|((ka, va), (kb, vb))| ka == kb && dir(va).is_some() && dir(va) == dir(vb))
}

/// Seed documents per insert: at most `BATCH` and about 16 MB, well under MongoDB's 48 MB message (Go's driver split
/// by size too).
fn batches(docs: &[Doc]) -> Vec<Vec<Bson>> {
    let mut out: Vec<Vec<Bson>> = Vec::new();
    let mut size = 0;
    for d in docs {
        let n = d.to_bytes().len();
        let full = out
            .last()
            .is_none_or(|b| b.len() >= BATCH || size + n > BATCH_BYTES);
        if full {
            out.push(Vec::new());
            size = 0;
        }
        size += n;
        if let Some(b) = out.last_mut() {
            b.push(Bson::Doc(d.clone()));
        }
    }
    out
}

/// Inserts the seed's documents while the collection is empty, so a second converge never doubles them.
async fn seed_collection(
    c: &mut Client,
    db: &str,
    coll: &str,
    seed: &str,
    dir: &str,
) -> Result<()> {
    let ns = format!("{db}.{coll}");
    let n = c
        .run(db, Doc::new().with("count", coll))
        .await
        .map_err(|e| failed(format!("counting {ns}"), &e))?
        .int("n")
        .unwrap_or(0);
    if n > 0 {
        return Ok(());
    }
    let docs = check::read_seed(seed, dir).map_err(|why| Error::new(ErrorKind::Invalid, why))?;
    for docs in batches(&docs) {
        let insert = Doc::new()
            .with("insert", coll)
            .with("documents", docs)
            .with("ordered", true);
        c.run(db, insert)
            .await
            .map_err(|e| failed(format!("seeding {ns}"), &e))?;
    }
    Ok(())
}

impl Objects for Ferret {
    /// Databases, collections (with their seed and indexes in the hash) and indexes; created in that order.
    fn objects(&self, cx: &Cx<FerretSpec>) -> Result<Vec<Object>> {
        let mut out = Vec::new();
        for d in &cx.spec.database {
            out.push(Object {
                kind: DATABASE.into(),
                name: d.name.clone(),
                hash: help::object_hash(&d.name),
            });
            for c in &d.collection {
                let ixs: Vec<IndexFacts> = c
                    .index
                    .iter()
                    .map(|ix| {
                        (
                            index_name(ix),
                            ix.keys.clone().into_iter().collect(),
                            ix.unique,
                        )
                    })
                    .collect();
                let hash = help::object_hash(&(&c.name, &c.seed, &ixs));
                out.push(Object {
                    kind: COLLECTION.into(),
                    name: format!("{}.{}", d.name, c.name),
                    hash,
                });
                for (name, keys, unique) in &ixs {
                    out.push(Object {
                        kind: INDEX.into(),
                        name: format!("{}.{}${name}", d.name, c.name),
                        hash: help::object_hash(&(keys, unique)),
                    });
                }
            }
        }
        Ok(out)
    }
}

impl Prune for Ferret {
    /// Drops what is no longer declared: an index, a collection, or a database whole. What is already gone is fine.
    async fn prune(&self, cx: &Cx<FerretSpec>, _tc: &Toolchain, removed: &[Object]) -> Result<()> {
        if removed.is_empty() {
            return Ok(());
        }
        let mut c = client(cx).await?;
        for o in removed {
            let Some((db, cmd)) = drop_command(o) else {
                continue;
            };
            match c.run(&db, cmd).await {
                Err(e) if !matches!(e.code(), Some(NAMESPACE_NOT_FOUND | INDEX_NOT_FOUND)) => {
                    return Err(failed(format!("dropping {} {}", o.kind, o.name), &e));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// The database and command that drop a removed object; `None` for a kind this module never listed.
fn drop_command(o: &Object) -> Option<(String, Doc)> {
    match o.kind.as_str() {
        DATABASE => Some((o.name.clone(), Doc::new().with("dropDatabase", 1))),
        COLLECTION => {
            let (db, coll) = o.name.split_once('.')?;
            Some((db.into(), Doc::new().with("drop", coll)))
        }
        INDEX => {
            // `$` cannot be in a database or collection name, so it divides the namespace from the index's name.
            let (ns, name) = o.name.split_once('$')?;
            let (db, coll) = ns.split_once('.')?;
            Some((
                db.into(),
                Doc::new().with("dropIndexes", coll).with("index", name),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn ix(name: Option<&str>, keys: &[(&str, i64)]) -> Index {
        Index {
            name: name.map(str::to_owned),
            keys: keys
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect::<BTreeMap<_, _>>(),
            unique: false,
        }
    }

    #[test]
    fn an_unnamed_index_is_named_as_mongodb_names_it() {
        assert_eq!(index_name(&ix(None, &[("sku", 1)])), "sku_1");
        assert_eq!(index_name(&ix(None, &[("b", -1), ("a", 1)])), "a_1_b_-1");
        assert_eq!(index_name(&ix(Some("by_sku"), &[("sku", 1)])), "by_sku");
    }

    #[test]
    fn removed_objects_become_their_drop_commands() {
        let o = |kind: &str, name: &str| Object {
            kind: kind.into(),
            name: name.into(),
            hash: String::new(),
        };
        let (db, cmd) = drop_command(&o(INDEX, "shop.my orders$by_day")).unwrap();
        assert_eq!(db, "shop");
        assert_eq!(
            cmd,
            Doc::new()
                .with("dropIndexes", "my orders")
                .with("index", "by_day")
        );
        let (db, cmd) = drop_command(&o(COLLECTION, "shop.a.b")).unwrap();
        assert_eq!((db.as_str(), cmd), ("shop", Doc::new().with("drop", "a.b")));
        assert_eq!(
            drop_command(&o(DATABASE, "shop")).unwrap().1,
            Doc::new().with("dropDatabase", 1)
        );
        assert!(drop_command(&o("user", "x")).is_none());
    }
}
