//! CloudWatch Logs Insights: the query language (written here by hand, fuzzed: `fuzz_insights_query`) and the engine that runs a query
//! over a group's events.
//!
//! The language as AWS documents it in the Logs Insights query syntax pages, which are not part of the model; everything below that
//! is not in the model's own text is (memory) until a recording says otherwise. Commands: `fields`, `display`, `filter`, `stats` (with
//! `by`, `bin()`, and aggregates `count`, `count_distinct`, `sum`, `avg`, `min`, `max`, `stddev`, `pct`, `earliest`, `latest`,
//! `sortsFirst`, `sortsLast`, which may be combined arithmetically), `sort`, `limit`, `parse` (glob or regular expression), `dedup`
//! and `unmask`. Expressions: arithmetic, comparisons, `and`/`or`/`not`, `like` and `=~` with a string or a regular expression, `in`,
//! and the string, number, time, IP and general functions (`funcs`). Fields: `@timestamp`, `@message`, `@logStream`, `@log`,
//! `@ingestionTime`, `@ptr`, the members of a JSON event (flattened with dots), and for a Lambda function's group the fields of its
//! `START`, `END` and `REPORT` lines (`@requestId`, `@duration`, `@billedDuration`, `@memorySize`, `@maxMemoryUsed`, `@initDuration`).

pub(crate) mod ast;
mod eval;
mod funcs;
pub(crate) mod job;
mod lex;
mod parse;
pub(crate) mod record;
pub(crate) mod run;
mod stats;
pub(crate) mod value;

#[cfg(test)]
mod tests;

pub(crate) use parse::{parse, without_groups};

/// A query that does not parse: what is wrong, and where (bytes into the query).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueryError {
    pub(crate) what: String,
    pub(crate) start: usize,
    /// Where the refused text ends (exclusive), for a refusal AWS words with the range it covers.
    pub(crate) end: Option<usize>,
}

impl QueryError {
    /// A refusal at byte `pos`, worded `... at line L and position P`.
    pub(crate) fn at(pos: usize, what: &str) -> Self {
        Self {
            what: what.to_owned(),
            start: pos,
            end: None,
        }
    }

    /// A refusal of the text from byte `start` to `end` (exclusive), worded `... ([S,E])`.
    pub(crate) fn span(start: usize, end: usize, what: &str) -> Self {
        Self {
            what: what.to_owned(),
            start,
            end: Some(end),
        }
    }

    /// The refusal as AWS words it for the query `src`, which AWS reads behind a `prefix` of `prefix` characters (the `SOURCE ...
    /// | ` it writes in front of a query; see [`source_prefix`]). Recorded 2026-11-19: positions count characters from 0 in that
    /// rewritten query (`unexpected symbol found | at line 1 and position 83`), and a range ends on its last character
    /// (`Unrecognized function name: nosuch ([83,98])`).
    pub(crate) fn message(&self, src: &str, prefix: usize) -> String {
        let chars = |to: usize| src.get(..to).unwrap_or(src).chars().count();
        if let Some(end) = self.end {
            let (s, e) = (prefix + chars(self.start), prefix + chars(end));
            return format!("{} ([{s},{}])", self.what, e.saturating_sub(1).max(s));
        }
        let before = src.get(..self.start).unwrap_or(src);
        let line = before.matches('\n').count() + 1;
        let mut column = before.rsplit('\n').next().map_or(0, |l| l.chars().count());
        if line == 1 {
            column += prefix;
        }
        format!("{} at line {line} and position {column}", self.what)
    }
}

/// What AWS writes in front of a `StartQuery`'s query before it reads it (recorded 2026-11-19, `DescribeQueries` shows it:
/// `SOURCE "/group" START=1795099290000 END=1795099444999 | fields @message`): the groups, and the range in milliseconds, its
/// end the last millisecond of `endTime`. How several groups are written is (memory).
pub(crate) fn source_prefix(groups: &[String], (start_ms, end_ms): (i64, i64)) -> String {
    let names: Vec<String> = groups.iter().map(|g| format!("\"{g}\"")).collect();
    format!(
        "SOURCE {} START={start_ms} END={end_ms} | ",
        names.join(", ")
    )
}

/// Whether `query` parses, and if it does, runs it over `message` as one event. Exists so the parser and the evaluator can be fuzzed.
#[doc(hidden)]
#[must_use]
pub(crate) fn check(query: &str, message: &str) -> bool {
    let Ok(q) = parse(query) else {
        return false;
    };
    let event = record::Event {
        ts: 1_700_000_000_000,
        ingested: 1_700_000_000_001,
        message,
        transformed: None,
        stream: "s",
        group: "g",
        account: "000000000000",
        region: "us-east-1",
        ptr: String::new(),
        lambda: true,
    };
    let mut engine = run::Engine::new(&q, 100);
    engine.push(&event);
    !engine.finish().rows.is_empty()
}
