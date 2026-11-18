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

pub(crate) use parse::parse;

/// A query that does not parse: what is wrong, and where (characters into the query).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueryError {
    pub(crate) message: String,
    pub(crate) start: usize,
}

impl QueryError {
    /// A refusal at byte `pos` of `src`, worded as AWS words one: `... at line L and position P`. (memory)
    pub(crate) fn at(src: &str, pos: usize, what: &str) -> Self {
        let before = src.get(..pos).unwrap_or(src);
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        Self {
            message: format!("{what} at line {line} and position {column}"),
            start: before.chars().count(),
        }
    }
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
        ptr: String::new(),
        lambda: true,
    };
    let mut engine = run::Engine::new(&q, 100);
    engine.push(&event);
    !engine.finish().rows.is_empty()
}
