//! Running a parsed query over events. The commands before the first `stats` or `sort` run event by event as the events are read
//! (newest first, so a query with no `sort` answers newest first and a `limit` stops the read early); `stats` gathers groups and
//! `sort` holds rows (only as many as the result can show when nothing after it filters); whatever follows runs over those rows.

use std::collections::HashSet;

use super::ast::{Cmd, Expr, Parser, Query};
use super::eval::eval;
use super::record::{self, Event, Row};
use super::stats::Groups;
use super::value::{Val, compare};

/// The most rows a `sort` holds when it cannot keep only the best ones, and the most groups a `stats` makes: local bounds, past which
/// the query fails and says so.
const MAX_HELD: usize = 1_000_000;

/// A query's answer: each row its shown fields as text, in order.
#[derive(Debug, Default)]
pub(crate) struct Output {
    pub(crate) rows: Vec<Vec<(String, String)>>,
    /// Events that passed the filters.
    pub(crate) matched: u64,
    /// Why the query failed, if it did.
    pub(crate) failed: Option<String>,
}

enum Sink {
    Rows(Vec<Row>),
    Groups(Groups),
    Sorted { rows: Vec<Row>, keep: Option<usize> },
}

/// One run of a query.
pub(crate) struct Engine<'q> {
    q: &'q Query,
    /// The index of the first `stats` or `sort`, or the number of commands.
    split: usize,
    counts: Vec<usize>,
    seen: Vec<HashSet<Vec<String>>>,
    sink: Sink,
    cap: usize,
    matched: u64,
    full: bool,
    failed: Option<String>,
}

impl std::fmt::Debug for Engine<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("matched", &self.matched)
            .finish_non_exhaustive()
    }
}

impl<'q> Engine<'q> {
    /// A run whose answer has at most `cap` rows.
    pub(crate) fn new(q: &'q Query, cap: usize) -> Self {
        let split = q
            .cmds
            .iter()
            .position(|c| matches!(c, Cmd::Stats(_) | Cmd::Sort(_)))
            .unwrap_or(q.cmds.len());
        let after = q.cmds.get(split + 1..).unwrap_or_default();
        let sink = match q.cmds.get(split) {
            Some(Cmd::Stats(_)) => Sink::Groups(Groups::default()),
            Some(_) => Sink::Sorted {
                rows: Vec::new(),
                keep: keeps(after).map(|n| n.min(cap)),
            },
            None => Sink::Rows(Vec::new()),
        };
        Self {
            q,
            split,
            counts: vec![0; split],
            seen: vec![HashSet::new(); split],
            sink,
            cap,
            matched: 0,
            full: false,
            failed: None,
        }
    }

    /// Whether reading more events can change the answer.
    pub(crate) fn done(&self) -> bool {
        self.full || self.failed.is_some()
    }

    /// Takes one event.
    pub(crate) fn push(&mut self, e: &Event<'_>) {
        if self.done() {
            return;
        }
        let mut row = record::row(e);
        let pre = self.q.cmds.get(..self.split).unwrap_or_default();
        for (i, cmd) in pre.iter().enumerate() {
            match cmd {
                Cmd::Limit(n) => {
                    let count = self.counts.get_mut(i);
                    let Some(c) = count else { return };
                    *c += 1;
                    if *c >= *n {
                        self.full = true;
                    }
                }
                Cmd::Dedup(keys) => {
                    if let Some(key) = dedup_key(keys, &row)
                        && !self.seen.get_mut(i).is_some_and(|s| s.insert(key))
                    {
                        return;
                    }
                }
                other => {
                    if !step(other, &mut row) {
                        return;
                    }
                }
            }
        }
        self.matched += 1;
        self.sink(row);
    }

    fn sink(&mut self, row: Row) {
        match &mut self.sink {
            Sink::Rows(rows) => {
                rows.push(row);
                if rows.len() >= self.cap {
                    self.full = true;
                }
            }
            Sink::Groups(groups) => {
                if let Some(Cmd::Stats(s)) = self.q.cmds.get(self.split) {
                    groups.add(s, &row);
                }
                if groups.len() > MAX_HELD {
                    self.failed = Some(too_many("groups"));
                }
            }
            Sink::Sorted { rows, keep } => {
                rows.push(row);
                match *keep {
                    Some(n) if rows.len() > 2 * n + 1024 => {
                        if let Some(Cmd::Sort(keys)) = self.q.cmds.get(self.split) {
                            sort(rows, keys);
                        }
                        rows.truncate(n);
                    }
                    None if rows.len() > MAX_HELD => self.failed = Some(too_many("rows")),
                    _ => {}
                }
            }
        }
    }

    /// The answer.
    pub(crate) fn finish(self) -> Output {
        if let Some(why) = self.failed {
            return Output {
                failed: Some(why),
                matched: self.matched,
                ..Output::default()
            };
        }
        let mut rows = match (self.sink, self.q.cmds.get(self.split)) {
            (Sink::Groups(g), Some(Cmd::Stats(s))) => g.rows(s),
            (Sink::Sorted { mut rows, keep }, Some(Cmd::Sort(keys))) => {
                sort(&mut rows, keys);
                if let Some(n) = keep {
                    rows.truncate(n);
                }
                rows
            }
            (Sink::Rows(rows) | Sink::Sorted { rows, .. }, _) => rows,
            (Sink::Groups(_), _) => Vec::new(),
        };
        for cmd in self.q.cmds.get(self.split + 1..).unwrap_or_default() {
            rows = batch(cmd, rows);
        }
        let (names, aggregated) = shown(self.q);
        rows.truncate(self.cap);
        Output {
            rows: rows
                .iter()
                .map(|r| project(r, &names, aggregated))
                .collect(),
            matched: self.matched,
            failed: None,
        }
    }
}

fn too_many(what: &str) -> String {
    format!(
        "doze-aws holds at most {MAX_HELD} {what} for one query; narrow the time range or add a filter or a limit"
    )
}

/// How many rows a sort must keep when what follows it only shows or limits them.
fn keeps(after: &[Cmd]) -> Option<usize> {
    let plain = after.iter().all(|c| {
        matches!(
            c,
            Cmd::Fields(_) | Cmd::Display(_) | Cmd::Limit(_) | Cmd::Unmask
        )
    });
    if !plain {
        return None;
    }
    Some(
        after
            .iter()
            .filter_map(|c| match c {
                Cmd::Limit(n) => Some(*n),
                _ => None,
            })
            .min()
            .unwrap_or(usize::MAX),
    )
}

/// A command that runs row by row, on one row: whether the row stays.
fn step(cmd: &Cmd, row: &mut Row) -> bool {
    match cmd {
        Cmd::Fields(list) => {
            for n in list {
                if !matches!(&n.expr, Expr::Field(f) if *f == n.name) {
                    let v = eval(&n.expr, row, &[]);
                    row.set(&n.name, v);
                }
            }
            true
        }
        Cmd::Filter(e) => eval(e, row, &[]).truthy(),
        Cmd::Parse { source, how } => {
            let text = eval(source, row, &[]);
            if !text.is_null() {
                parsed(how, &text.text(), row);
            }
            true
        }
        _ => true,
    }
}

/// A command over rows already gathered.
fn batch(cmd: &Cmd, mut rows: Vec<Row>) -> Vec<Row> {
    match cmd {
        Cmd::Stats(s) => {
            let mut g = Groups::default();
            for r in &rows {
                g.add(s, r);
            }
            g.rows(s)
        }
        Cmd::Sort(keys) => {
            sort(&mut rows, keys);
            rows
        }
        Cmd::Limit(n) => {
            rows.truncate(*n);
            rows
        }
        Cmd::Dedup(keys) => {
            let mut seen = HashSet::new();
            rows.retain(|r| dedup_key(keys, r).is_none_or(|k| seen.insert(k)));
            rows
        }
        other => rows
            .into_iter()
            .filter_map(|mut r| step(other, &mut r).then_some(r))
            .collect(),
    }
}

/// What `dedup` compares a row by; `None` when a field is absent, which is never a duplicate. (memory)
fn dedup_key(keys: &[Expr], row: &Row) -> Option<Vec<String>> {
    keys.iter()
        .map(|k| {
            let v = eval(k, row, &[]);
            (!v.is_null()).then(|| v.text())
        })
        .collect()
}

fn sort(rows: &mut [Row], keys: &[(Expr, bool)]) {
    rows.sort_by(|a, b| {
        keys.iter()
            .map(|(k, desc)| {
                let o = compare(&eval(k, a, &[]), &eval(k, b, &[]));
                if *desc { o.reverse() } else { o }
            })
            .find(|o| o.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// `parse`: the fields a glob or a regular expression takes out of `text`; none when it does not match.
fn parsed(how: &Parser, text: &str, row: &mut Row) {
    match how {
        Parser::Regex(re) => {
            if let Some(caps) = re.captures(text) {
                for name in re.capture_names().flatten() {
                    if let Some(m) = caps.name(name) {
                        row.set(name, Val::Str(m.as_str().to_owned()));
                    }
                }
            }
        }
        Parser::Glob { pieces, names } => {
            if let Some(values) = glob(pieces, text) {
                for (n, v) in names.iter().zip(values) {
                    row.set(n, Val::Str(v.to_owned()));
                }
            }
        }
    }
}

/// The text between the pieces of a glob: the first piece anywhere, each `*` the shortest run up to the next piece, a trailing `*`
/// the rest. (memory)
fn glob<'t>(pieces: &[String], text: &'t str) -> Option<Vec<&'t str>> {
    let (first, rest) = pieces.split_first()?;
    let mut at = text.find(first.as_str())? + first.len();
    let mut out = Vec::new();
    for (i, piece) in rest.iter().enumerate() {
        let tail = text.get(at..)?;
        if piece.is_empty() && i + 1 == rest.len() {
            out.push(tail);
            at = text.len();
            continue;
        }
        let end = tail.find(piece.as_str())?;
        out.push(tail.get(..end)?);
        at += end + piece.len();
    }
    Some(out)
}

/// The fields the answer shows, and whether it is a stats answer. A query that names none shows `@timestamp` and `@message`. (memory)
fn shown(q: &Query) -> (Vec<String>, bool) {
    let mut names: Vec<String> = Vec::new();
    let mut aggregated = false;
    let add = |names: &mut Vec<String>, n: &str| {
        if !names.iter().any(|x| x == n) {
            names.push(n.to_owned());
        }
    };
    for c in &q.cmds {
        match c {
            Cmd::Fields(list) => list.iter().for_each(|n| add(&mut names, &n.name)),
            Cmd::Display(list) => names = list.iter().map(|n| n.name.clone()).collect(),
            Cmd::Parse {
                how: Parser::Glob { names: ns, .. },
                ..
            } => {
                ns.iter().for_each(|n| add(&mut names, n));
            }
            Cmd::Parse {
                how: Parser::Regex(re),
                ..
            } => {
                re.capture_names()
                    .flatten()
                    .for_each(|n| add(&mut names, n));
            }
            Cmd::Stats(s) => {
                names =
                    s.by.iter()
                        .chain(&s.items)
                        .map(|n| n.name.clone())
                        .collect();
                aggregated = true;
            }
            _ => {}
        }
    }
    if names.is_empty() && !aggregated {
        names = vec!["@timestamp".into(), "@message".into()];
    }
    (names, aggregated)
}

/// A row as the answer shows it: the shown fields it has, then `@ptr` for an event.
fn project(r: &Row, names: &[String], aggregated: bool) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = names
        .iter()
        .filter_map(|n| {
            r.get(n)
                .filter(|v| !v.is_null())
                .map(|v| (n.clone(), v.text()))
        })
        .collect();
    if !aggregated
        && let Some(p) = r.get("@ptr")
        && !names.iter().any(|n| n == "@ptr")
    {
        out.push(("@ptr".into(), p.text()));
    }
    out
}
