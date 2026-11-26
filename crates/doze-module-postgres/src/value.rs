//! The console's value viewer (protocol minor 2): a tree leaf in full (a table's first rows as JSON, a column's
//! definition as JSON) and a session's whole query (the sessions table cuts it at 600 characters). Each is made to fit
//! one answer ([`VALUE_CHUNK`]): the rows leaf takes as many of the first rows as fit, so a value is read once, whole,
//! never re-run for a part that could have changed. A read gives up after [`psql::READ_TIMEOUT`].

use crate::Postgres;
use crate::browse::{self, Id};
use crate::names::ident;
use crate::psql::{self, Psql};
use crate::spec::PostgresSpec;
use crate::sql::lit;
use doze_module::caps::Values;
use doze_module::proto::msg::blocks::{VALUE_CHUNK, ValueAnswer, media};
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::{Cx, Result};
use std::collections::BTreeMap;

/// The sessions table's panel, whose rows name a session by its `pid`.
pub(crate) const SESSIONS: &str = "sessions";

/// How many rows a table's rows leaf shows.
const FIRST_ROWS: u32 = 100;

/// Marks a value read back, so a value that is empty and one that is not there read apart.
const MARK: char = 'v';

fn gone(what: String) -> Error {
    Error::new(ErrorKind::NotFound, what)
}

/// The most bytes of rows the rows leaf gathers: under one answer, with room for the array around them.
const ROWS_BYTES: usize = VALUE_CHUNK as usize - 4096;

/// The first rows of a table as one JSON array: of the first [`FIRST_ROWS`], as many as fit in [`ROWS_BYTES`].
fn rows_query(schema: &str, table: &str) -> String {
    format!(
        "SELECT '{MARK}' || coalesce(json_agg(r ORDER BY n), '[]'::json)::text FROM (
           SELECT r, n, sum(octet_length(r::text)) OVER (ORDER BY n) AS upto FROM (
             SELECT row_to_json(t) AS r, row_number() OVER () AS n FROM (SELECT * FROM {}.{} LIMIT {FIRST_ROWS}) t
           ) numbered
         ) sized WHERE upto <= {ROWS_BYTES}",
        ident(schema),
        ident(table)
    )
}

/// A column's definition as one JSON object.
fn column_query(schema: &str, table: &str, column: &str) -> String {
    format!(
        "SELECT '{MARK}' || json_build_object(
           'column', a.attname, 'type', format_type(a.atttypid, a.atttypmod), 'nullable', NOT a.attnotnull,
           'default', pg_get_expr(d.adbin, d.adrelid), 'position', a.attnum,
           'comment', col_description(a.attrelid, a.attnum))::text
         FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
         WHERE a.attrelid = to_regclass({}) AND a.attname = {} AND a.attnum > 0 AND NOT a.attisdropped",
        lit(&format!("{}.{}", ident(schema), ident(table))),
        lit(column)
    )
}

/// A session's whole query.
fn session_query(pid: u32) -> String {
    format!("SELECT '{MARK}' || coalesce(query, '') FROM pg_stat_activity WHERE pid = {pid}")
}

/// The one value `query` reads, whole (it may hold line breaks); `None` if it found no row.
async fn one(p: &Psql, db: &str, query: &str) -> Result<Option<String>> {
    let out = p
        .read(db, &format!("{query};\n"), VALUE_CHUNK as usize + 64)
        .await?;
    Ok(read_back(&out))
}

/// psql's output of one marked value: the mark taken off, and the line break psql ends it with.
fn read_back(out: &str) -> Option<String> {
    let v = out.strip_prefix(MARK)?;
    Some(v.strip_suffix('\n').unwrap_or(v).to_string())
}

/// A tree leaf's value: its JSON, or why there is none.
async fn leaf(p: &Psql, id: &str) -> Result<(String, &'static str)> {
    let missing = |e: Error, what: String| {
        if e.message.contains("does not exist") {
            gone(format!("{what} no longer exists"))
        } else {
            e
        }
    };
    match Id::parse(id) {
        Some(Id::Rows(d, s, t)) => {
            let what = format!("table {s}.{t} in {d}");
            let v = one(p, &d, &rows_query(&s, &t))
                .await
                .map_err(|e| missing(e, what.clone()))?;
            v.map(|v| (v, media::JSON))
                .ok_or_else(|| gone(format!("{what} no longer exists")))
        }
        Some(Id::Column(d, s, t, c)) => {
            let what = format!("column {c} of {s}.{t} in {d}");
            let v = one(p, &d, &column_query(&s, &t, &c))
                .await
                .map_err(|e| missing(e, what.clone()))?;
            v.map(|v| (v, media::JSON))
                .ok_or_else(|| gone(format!("{what} no longer exists")))
        }
        _ => Err(Error::new(
            ErrorKind::Invalid,
            format!("{id:?} is not a table's rows or a column: open one of those"),
        )),
    }
}

/// A session's whole query, by the pid its row shows.
async fn session(p: &Psql, row: &BTreeMap<String, String>) -> Result<String> {
    let cell = row
        .get("pid")
        .or_else(|| row.get("name"))
        .map(String::as_str)
        .unwrap_or_default();
    let Some(pid) = cell.trim().parse::<u32>().ok().filter(|p| *p > 0) else {
        return Err(Error::new(
            ErrorKind::Invalid,
            format!("{cell:?} is not a session"),
        ));
    };
    one(p, "postgres", &session_query(pid))
        .await?
        .ok_or_else(|| gone(format!("session {pid} has ended")))
}

impl Values for Postgres {
    async fn value(
        &self,
        cx: &Cx<PostgresSpec>,
        panel: &str,
        row: &BTreeMap<String, String>,
        offset: u64,
        max: u32,
    ) -> Result<ValueAnswer> {
        let p = psql::admin(&cx.instance)?;
        let (whole, m) = match panel {
            browse::PANEL => {
                let id = row.get("id").map(String::as_str).unwrap_or_default();
                leaf(&p, id).await?
            }
            SESSIONS => (session(&p, row).await?, media::TEXT),
            other => {
                return Err(gone(format!("postgres shows no values in {other:?}")));
            }
        };
        Ok(ValueAnswer::slice(m, whole.as_bytes(), offset, max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_read_back_keeps_its_line_breaks_and_an_absent_one_is_none() {
        assert_eq!(
            read_back("vSELECT 1\nFROM t\n").as_deref(),
            Some("SELECT 1\nFROM t")
        );
        assert_eq!(read_back("v\n").as_deref(), Some(""));
        assert_eq!(read_back(""), None);
    }

    #[test]
    fn names_reach_the_queries_quoted() {
        let q = rows_query("a\"b", "c");
        assert!(q.contains(r#"FROM "a""b"."c" LIMIT 100"#), "{q}");
        assert!(
            q.contains(&format!("upto <= {ROWS_BYTES}")),
            "only what fits one answer"
        );
        let q = column_query("s", "t", "it's");
        assert!(q.contains("a.attname = 'it''s'"), "{q}");
        assert!(q.contains(r#"to_regclass('"s"."t"')"#), "{q}");
    }
}
