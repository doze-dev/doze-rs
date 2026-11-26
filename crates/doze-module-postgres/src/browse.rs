//! The console's schema tree (protocol minor 2): databases, their schemas, their tables and views, and under each table
//! its first rows and its columns. Read from the catalogue with psql, one database at a time, a page at a time, so a
//! schema of ten thousand tables opens as quickly as one of ten. The leaves open in the value viewer (value.rs).

use crate::Postgres;
use crate::names::ident;
use crate::psql::{self, Psql};
use crate::spec::PostgresSpec;
use crate::sql::lit;
use doze_module::caps::Tree;
use doze_module::proto::msg::blocks::{Children, TreeNode};
use doze_module::{Cx, Result};
use std::collections::BTreeMap;

/// The tree's panel and block id.
pub(crate) const PANEL: &str = "browse";

/// Nodes one page carries.
const PAGE: usize = 200;

/// The most matches one database gives a filter.
const MATCHES: usize = 2000;

/// A node of the tree, as its id names it: each name escaped (`%` as `%25`, `/` as `%2F`) and joined by `/`, a table's
/// rows leaf `#rows` and a column leaf `.` and its name, so no name can read as another path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Id {
    Database(String),
    Schema(String, String),
    Table(String, String, String),
    /// A table's first rows.
    Rows(String, String, String),
    /// A table's column.
    Column(String, String, String, String),
}

/// A name as one part of an id.
fn esc(s: &str) -> String {
    s.replace('%', "%25").replace('/', "%2F")
}

/// One part of an id as the name it escapes; `None` for an escape [`esc`] never makes.
fn unesc(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(rest.get(..i)?);
        let code = rest.get(i + 1..i + 3)?;
        out.push(match code {
            "25" => '%',
            "2F" | "2f" => '/',
            _ => return None,
        });
        rest = rest.get(i + 3..)?;
    }
    out.push_str(rest);
    Some(out)
}

impl Id {
    pub(crate) fn parse(id: &str) -> Option<Id> {
        let parts: Vec<&str> = id.split('/').collect();
        if parts.iter().any(|p| p.is_empty()) {
            return None;
        }
        let name = |i: usize| parts.get(i).and_then(|p| unesc(p));
        match parts.len() {
            1 => Some(Id::Database(name(0)?)),
            2 => Some(Id::Schema(name(0)?, name(1)?)),
            3 => Some(Id::Table(name(0)?, name(1)?, name(2)?)),
            4 => {
                let (d, s, t) = (name(0)?, name(1)?, name(2)?);
                let last = parts.get(3)?;
                if *last == "#rows" {
                    Some(Id::Rows(d, s, t))
                } else {
                    let col = unesc(last.strip_prefix('.')?)?;
                    (!col.is_empty()).then_some(Id::Column(d, s, t, col))
                }
            }
            _ => None,
        }
    }

    pub(crate) fn text(&self) -> String {
        match self {
            Id::Database(d) => esc(d),
            Id::Schema(d, s) => format!("{}/{}", esc(d), esc(s)),
            Id::Table(d, s, t) => format!("{}/{}/{}", esc(d), esc(s), esc(t)),
            Id::Rows(d, s, t) => format!("{}/{}/{}/#rows", esc(d), esc(s), esc(t)),
            Id::Column(d, s, t, c) => format!("{}/{}/{}/.{}", esc(d), esc(s), esc(t), esc(c)),
        }
    }
}

fn node(id: Id, label: &str, leaf: bool, info: &[(&str, &str)]) -> TreeNode {
    TreeNode {
        id: id.text(),
        label: label.into(),
        leaf,
        info: info
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
    }
}

/// The schemas a person made: not the catalogue's, the toast's or a session's temporary ones. `col` is the column that
/// holds the schema's name.
fn shown_schema(col: &str) -> String {
    format!(
        "{col} NOT IN ('pg_catalog', 'information_schema') AND {col} NOT LIKE 'pg\\_toast%' AND {col} NOT LIKE 'pg\\_temp\\_%'"
    )
}

/// The relations the tree lists: tables, partitioned tables, views, materialized views and foreign tables.
const RELKINDS: &str = "c.relkind IN ('r', 'p', 'v', 'm', 'f')";

/// A case-blind match of `col` on `f` (already lower case), with no pattern characters to escape.
fn matches(col: &str, f: &str) -> String {
    format!("position({} IN lower({col})) > 0", lit(f))
}

/// A table or view from its catalogue row: kind (`relkind`) and estimated rows (`reltuples`, -1 if never counted).
fn relation(d: &str, s: &str, t: &str, relkind: &str, tuples: &str) -> TreeNode {
    let id = Id::Table(d.into(), s.into(), t.into());
    let kind = match relkind {
        "v" => Some("view"),
        "m" => Some("materialized view"),
        "f" => Some("foreign table"),
        _ => None,
    };
    let counted = tuples.parse::<i64>().is_ok_and(|n| n >= 0) && relkind != "v";
    let mut info = Vec::new();
    if counted {
        info.push(("rows", tuples));
    }
    if let Some(k) = kind {
        info.push(("kind", k));
    }
    node(id, t, false, &info)
}

/// The databases the tree opens with, as `databases()` in admin.rs lists them: no template, and not the server's own
/// `postgres` unless the instance is named after it; a database closed to connections cannot be read, so it is left
/// out.
fn databases_query(instance: &str) -> String {
    format!(
        "SELECT datname FROM pg_database WHERE NOT datistemplate AND datallowconn \
         AND (datname <> 'postgres' OR datname = {}) ORDER BY 1",
        lit(instance)
    )
}

/// One page of the tables and views of a schema, from `offset`, one more than a page to know if more follow.
fn tables_query(schema: &str, offset: usize) -> String {
    format!(
        "SELECT c.relname, c.relkind, c.reltuples::bigint FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = {} AND {RELKINDS} ORDER BY c.relname LIMIT {} OFFSET {offset}",
        lit(schema),
        PAGE + 1
    )
}

/// A table's columns, in their order.
fn columns_query(schema: &str, table: &str) -> String {
    format!(
        "SELECT a.attname, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull FROM pg_attribute a \
         WHERE a.attrelid = to_regclass({}) AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
        lit(&format!("{}.{}", ident(schema), ident(table)))
    )
}

/// What a filter finds in one database: the schemas whose name has it, and the tables and views whose name or
/// schema's name has it.
fn filter_query(f: &str) -> String {
    format!(
        "SELECT 's', nspname, '', '', '' FROM pg_namespace WHERE {} AND {}
         UNION ALL
         SELECT 't', n.nspname, c.relname, c.relkind, c.reltuples::bigint::text FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace WHERE {} AND {RELKINDS} AND ({} OR {})
         ORDER BY 2, 3 LIMIT {MATCHES}",
        shown_schema("nspname"),
        matches("nspname", f),
        shown_schema("n.nspname"),
        matches("c.relname", f),
        matches("n.nspname", f),
    )
}

/// Where a page starts: the cursor this module gave, or the first.
fn offset(cursor: Option<&str>) -> usize {
    cursor.and_then(|c| c.parse().ok()).unwrap_or(0)
}

/// A page of `rows` asked with one row more than [`PAGE`]: that row's presence says another page follows.
fn more(mut nodes: Vec<TreeNode>, from: usize) -> Children {
    let next = (nodes.len() > PAGE).then(|| (from + PAGE).to_string());
    nodes.truncate(PAGE);
    Children { nodes, next }
}

/// The databases of the instance's server.
async fn databases(p: &Psql, instance: &str) -> Result<Vec<String>> {
    p.column("postgres", &databases_query(instance)).await
}

async fn schemas(p: &Psql, d: &str) -> Result<Vec<TreeNode>> {
    let q = format!(
        "SELECT nspname FROM pg_namespace WHERE {} ORDER BY 1",
        shown_schema("nspname")
    );
    Ok(p.column(d, &q)
        .await?
        .iter()
        .map(|s| node(Id::Schema(d.into(), s.clone()), s, false, &[]))
        .collect())
}

/// Under a table: its rows, then its columns.
async fn table(p: &Psql, d: &str, s: &str, t: &str) -> Result<Vec<TreeNode>> {
    let rows = Id::Rows(d.into(), s.into(), t.into());
    let mut out = vec![node(rows, "rows", true, &[])];
    for r in p.rows(d, &columns_query(s, t), 3).await? {
        let [name, ty, nullable] = r.as_slice() else {
            continue;
        };
        let id = Id::Column(d.into(), s.into(), t.into(), name.clone());
        let mut info = vec![("type", ty.as_str())];
        if nullable == "t" {
            info.push(("null", "nullable"));
        }
        out.push(node(id, name, true, &info));
    }
    Ok(out)
}

/// What a filter finds in database `d`, under schema `only` if one is open.
async fn found(p: &Psql, d: &str, f: &str, only: Option<&str>) -> Result<Vec<TreeNode>> {
    let mut out = Vec::new();
    for r in p.rows(d, &filter_query(f), 5).await? {
        let [what, s, t, relkind, tuples] = r.as_slice() else {
            continue;
        };
        if only.is_some_and(|o| o != s) {
            continue;
        }
        match what.as_str() {
            "s" if only.is_none() => out.push(node(Id::Schema(d.into(), s.clone()), s, false, &[])),
            "t" => out.push(relation(d, s, t, relkind, tuples)),
            _ => {}
        }
    }
    Ok(out)
}

/// One page of the children of `parent`, or of what `filter` finds under it.
pub(crate) async fn children(
    p: &Psql,
    instance: &str,
    parent: &str,
    filter: &str,
    cursor: Option<&str>,
) -> Result<Children> {
    let f = filter.trim().to_lowercase();
    let parent = if parent.is_empty() {
        None
    } else {
        match Id::parse(parent) {
            Some(id) => Some(id),
            None => return Ok(Children::default()),
        }
    };
    if !f.is_empty() {
        let all = match &parent {
            None => {
                let mut all = Vec::new();
                for d in databases(p, instance).await? {
                    all.extend(found(p, &d, &f, None).await?);
                }
                all
            }
            Some(Id::Database(d)) => found(p, d, &f, None).await?,
            Some(Id::Schema(d, s)) => found(p, d, &f, Some(s)).await?,
            Some(Id::Table(d, s, t)) => table(p, d, s, t)
                .await?
                .into_iter()
                .filter(|n| n.label.to_lowercase().contains(&f))
                .collect(),
            Some(_) => vec![],
        };
        return Ok(Children::page(all, cursor, PAGE));
    }
    match &parent {
        None => {
            let all = databases(p, instance)
                .await?
                .iter()
                .map(|d| node(Id::Database(d.clone()), d, false, &[]))
                .collect();
            Ok(Children::page(all, cursor, PAGE))
        }
        Some(Id::Database(d)) => Ok(Children::page(schemas(p, d).await?, cursor, PAGE)),
        Some(Id::Schema(d, s)) => {
            let from = offset(cursor);
            let nodes = p
                .rows(d, &tables_query(s, from), 3)
                .await?
                .iter()
                .filter_map(|r| match r.as_slice() {
                    [t, relkind, tuples] => Some(relation(d, s, t, relkind, tuples)),
                    _ => None,
                })
                .collect();
            Ok(more(nodes, from))
        }
        Some(Id::Table(d, s, t)) => Ok(Children::page(table(p, d, s, t).await?, cursor, PAGE)),
        Some(_) => Ok(Children::default()),
    }
}

impl Tree for Postgres {
    async fn children(
        &self,
        cx: &Cx<PostgresSpec>,
        _panel: &str,
        parent: &str,
        filter: &str,
        cursor: Option<&str>,
    ) -> Result<Children> {
        let p = psql::admin(&cx.instance)?;
        children(&p, &cx.instance.name, parent, filter, cursor).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn ids_read_back_as_the_names_they_were_made_of() {
        let s = |v: &str| v.to_string();
        let all = [
            Id::Database(s("app")),
            Id::Schema(s("app"), s("a/b")),
            Id::Table(s("a%2Fb"), s("public"), s("x/y/z")),
            Id::Rows(s("app"), s("public"), s("orders")),
            Id::Column(s("app"), s("public"), s("orders"), s("#rows")),
            Id::Column(s("app"), s("public"), s("o/rders"), s(".total%")),
        ];
        for id in all {
            assert_eq!(Id::parse(&id.text()), Some(id.clone()), "{}", id.text());
        }
        // A table named "a/b" is one part, not two.
        assert_eq!(
            Id::Schema(s("d"), s("a/b")).text(),
            "d/a%2Fb",
            "the slash is escaped"
        );
        assert_eq!(Id::parse("d/a/b"), Some(Id::Table(s("d"), s("a"), s("b"))));
        // A column named "#rows" is not the rows leaf.
        assert_eq!(
            Id::Column(s("d"), s("s"), s("t"), s("#rows")).text(),
            "d/s/t/.#rows"
        );
        assert_eq!(
            Id::parse("d/s/t/#rows"),
            Some(Id::Rows(s("d"), s("s"), s("t")))
        );
    }

    #[test]
    fn a_malformed_id_names_nothing() {
        for bad in [
            "",
            "a//b",
            "a/b/c/d/e",
            "a/b/c/rows",
            "a/b/c/.",
            "a%2",
            "a%41",
            "/a",
        ] {
            assert_eq!(Id::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_filter_is_case_blind_and_quoted() {
        let q = filter_query("o'k");
        assert!(q.contains("position('o''k' IN lower(c.relname))"), "{q}");
        assert!(q.contains("LIMIT 2000"));
    }

    #[test]
    fn a_schema_pages_by_offset() {
        let n = |i: usize| TreeNode {
            id: i.to_string(),
            ..TreeNode::default()
        };
        let full = more((0..=PAGE).map(n).collect(), 400);
        assert_eq!(
            (full.nodes.len(), full.next.as_deref()),
            (PAGE, Some("600"))
        );
        let last = more((0..3).map(n).collect(), 600);
        assert_eq!((last.nodes.len(), last.next), (3, None));
        assert_eq!(offset(Some("600")), 600);
        assert_eq!(offset(None), 0);
        assert!(tables_query("s", 600).contains("LIMIT 201 OFFSET 600"));
    }

    #[test]
    fn a_view_is_named_and_a_table_counted() {
        let v = relation("d", "s", "v", "v", "-1");
        assert_eq!(v.info.get("kind").map(String::as_str), Some("view"));
        assert!(!v.info.contains_key("rows"));
        let t = relation("d", "s", "t", "r", "42");
        assert_eq!(t.info.get("rows").map(String::as_str), Some("42"));
        let fresh = relation("d", "s", "t", "r", "-1");
        assert!(fresh.info.is_empty(), "never counted: no guess");
    }

    #[test]
    fn identifiers_in_the_catalogue_queries_are_quoted() {
        let q = columns_query("my\"s", "t'x");
        assert!(q.contains(r#"to_regclass('"my""s"."t''x"')"#), "{q}");
    }
}
