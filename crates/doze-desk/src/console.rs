//! A module's console as the hub holds it for one instance: the spec the module described, the rows its tables last
//! read (with rates worked out), the rows its stream produced. Subscribed to only while a page shows it (Go D14).

use crate::convert;
use doze_control::{ActionView, ConsoleFrame, ConsoleSpec};
use doze_display::{self as words, RateSample};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

/// The same rows as the module sent them (the worked-out `_rate:` cells aside).
fn same_rows(old: &[words::ConsoleRow], new: &[words::ConsoleRow]) -> bool {
    fn sent(r: &words::ConsoleRow) -> (Vec<(&String, &String)>, &str) {
        let mut c: Vec<(&String, &String)> = r
            .cells
            .iter()
            .filter(|(k, _)| !k.starts_with("_rate:"))
            .collect();
        c.sort();
        (c, r.tone.as_str())
    }
    old.len() == new.len() && old.iter().zip(new).all(|(a, b)| sent(a) == sent(b))
}

/// How many stream rows a panel keeps, as the Go dash and hub kept.
pub const STREAM_KEPT: usize = 300;

/// What a page draws a console from (Go `hub.ConsoleView`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConsoleView {
    /// The spec as the module sent it: its layout, extensions, notes and web console.
    pub spec: Option<Arc<ConsoleSpec>>,
    /// The same spec as the words read it.
    pub words: Option<Arc<words::ConsoleSpec>>,
    pub actions: Vec<ActionView>,
    /// The instance's state as the module last said it.
    pub state: String,
    /// The console's error, when the module could not answer or the subscription failed.
    pub err: String,
    /// The rows each table panel last read, by panel id, with a `_rate:<key>` cell for each rate worked out.
    pub tables: BTreeMap<String, Vec<words::ConsoleRow>>,
    /// The rows each stream panel produced, oldest first.
    pub streams: BTreeMap<String, Vec<words::ConsoleRow>>,
}

/// The totals a console's rates are worked out from.
pub(crate) type Rates = HashMap<String, RateSample>;

impl ConsoleView {
    /// Folds a console frame in, as Go's hub did: a spec replaces the spec and clears the error; rows of a table
    /// replace what was there, with rates worked out from `prev`; a row of a stream is appended. Returns whether it
    /// changed.
    ///
    /// `replay` says the frame is a new subscription's first word on a table already held (the console followed
    /// another stream panel): the daemon replays its last read, which is not a second sample, so the same rows change
    /// nothing and no rate is worked out from them ("0/s" where Go said "—").
    pub(crate) fn apply(
        &mut self,
        prev: &mut Rates,
        f: &ConsoleFrame,
        now: Instant,
        replay: bool,
    ) -> bool {
        let v = self;
        let mut changed = false;
        if let Some(spec) = &f.spec {
            v.words = Some(Arc::new(convert::console_spec(spec)));
            v.spec = Some(Arc::new(spec.clone()));
            v.actions.clone_from(&f.actions);
            v.state.clone_from(&f.state);
            v.err.clear();
            changed = true;
        }
        if !f.error.is_empty() && f.panel.is_empty() {
            v.err.clone_from(&f.error);
            changed = true;
        }
        if f.panel.is_empty() {
            return changed;
        }
        // Go's `rows` is `omitempty`: a frame with none says nothing about the table.
        if !f.rows.is_empty() {
            let mut rows: Vec<words::ConsoleRow> =
                f.rows.iter().map(convert::console_row).collect();
            if replay
                && v.tables
                    .get(&f.panel)
                    .is_some_and(|old| same_rows(old, &rows))
            {
                return false;
            }
            words::rates(v.words.as_deref(), &f.panel, &mut rows, prev, now);
            v.tables.insert(f.panel.clone(), rows);
            return true;
        }
        if let Some(row) = &f.row {
            let rows = v.streams.entry(f.panel.clone()).or_default();
            rows.push(convert::console_row(row));
            if rows.len() > STREAM_KEPT {
                let extra = rows.len() - STREAM_KEPT;
                rows.drain(..extra);
            }
            return true;
        }
        changed
    }

    /// The subscription failed: say why, in the words a page shows.
    pub(crate) fn failed(&mut self, error: &str) -> bool {
        let why = humanise(error);
        if self.err == why {
            return false;
        }
        why.clone_into(&mut self.err);
        true
    }
}

/// A connection error in a person's words. `doze-display` reads Go's error texts; `doze-control` words two of them
/// its own way, so those are read here: a failed dial ("cannot reach the daemon: …", with Rust's capitalised
/// "No such file") where Go said "connection refused" or "no such file", and a stream's end-of-file ("the daemon
/// hung up") where Go said "EOF".
pub(crate) fn humanise(error: &str) -> &str {
    if error.starts_with("cannot reach the daemon") {
        "the daemon is not reachable"
    } else if error.contains("hung up") {
        "the daemon closed the connection"
    } else {
        words::humanise(error)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use doze_control::{ConsoleColumn, ConsoleFrame, ConsolePanel, ConsoleRow, ConsoleSpec};

    use super::{ConsoleView, Rates};

    fn spec() -> ConsoleFrame {
        let col = |key: &str, role: &str| ConsoleColumn {
            key: key.to_owned(),
            title: key.to_owned(),
            role: role.to_owned(),
            ..ConsoleColumn::default()
        };
        ConsoleFrame {
            spec: Some(ConsoleSpec {
                panels: vec![ConsolePanel {
                    id: "topics".to_owned(),
                    kind: "table".to_owned(),
                    columns: vec![col("name", ""), col("messages", "rate")],
                    ..ConsolePanel::default()
                }],
                ..ConsoleSpec::default()
            }),
            ..ConsoleFrame::default()
        }
    }

    fn table(total: &str) -> ConsoleFrame {
        let cells = [("name", "orders"), ("messages", total)]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        ConsoleFrame {
            panel: "topics".to_owned(),
            rows: vec![ConsoleRow {
                cells: Some(cells),
                tone: String::new(),
            }],
            ..ConsoleFrame::default()
        }
    }

    fn rate(v: &ConsoleView) -> Option<&str> {
        v.tables
            .get("topics")
            .and_then(|t| t.first())
            .and_then(|r| r.cells.get("_rate:messages"))
            .map(String::as_str)
    }

    #[test]
    fn a_replayed_table_is_not_a_second_sample() {
        let (mut v, mut prev, t0) = (ConsoleView::default(), Rates::new(), Instant::now());
        v.apply(&mut prev, &spec(), t0, false);
        assert!(v.apply(&mut prev, &table("1,000"), t0, false));
        assert_eq!(rate(&v), None, "one total is no rate: the cell says —");
        let later = t0 + Duration::from_secs(3);
        assert!(
            !v.apply(&mut prev, &table("1,000"), later, true),
            "a replay changes nothing"
        );
        assert_eq!(rate(&v), None, "not 0/s");
        let next = later + Duration::from_secs(2);
        assert!(v.apply(&mut prev, &table("1,060"), next, false));
        assert_eq!(rate(&v), Some("12/s"), "from the first read, 5 s back");
        let quiet = next + Duration::from_secs(2);
        assert!(v.apply(&mut prev, &table("1,060"), quiet, false));
        assert_eq!(
            rate(&v),
            Some("0/s"),
            "a quiet module's next read is a real 0/s"
        );
    }
}
