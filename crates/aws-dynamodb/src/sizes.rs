//! How many entries and bytes lie under a key prefix (a table's items, an index's entries), counted off the serving thread.
//!
//! `DescribeTable` shows a table's and each index's item count and size. AWS refreshes them about every six hours; here they are
//! exact, which takes a read of every item: 77 ms at 100,000 items, on the one thread that serves every service. So the count runs
//! before the call, in one job on the blocking pool ([`Disk::count_off_loop`]), and is kept with the number of writes it was made
//! at: while no write follows, a waiter that asks every few seconds costs a lookup. `doze-aws data`'s usage counts the same way.

use doze_store::StoreError;

use crate::core::{Core, Cx};
use crate::disk::{Disk, def};
use crate::keys::{Space, prefix_end};
use crate::meta::Table;

/// A count: entries and the bytes their keys and values hold.
pub(crate) type Count = (u64, u64);

impl Disk {
    /// The count under `prefix` in `space`, if one was made since the last write.
    pub(crate) fn counted(&self, space: Space, prefix: &[u8]) -> Option<Count> {
        let at = self.writes();
        self.counted
            .get(&(space, prefix.to_vec()))
            .filter(|(made, _)| *made == at)
            .map(|(_, c)| *c)
    }

    /// Counts each prefix of `wanted` that has no count since the last write, in one job on the blocking pool, and keeps the counts
    /// (only those of the current write count: the map never holds more than the last call asked for).
    pub(crate) async fn count_off_loop(
        &mut self,
        wanted: Vec<(Space, Vec<u8>)>,
    ) -> Result<(), StoreError> {
        let at = self.writes();
        self.counted.retain(|_, (made, _)| *made == at);
        let wanted: Vec<(Space, Vec<u8>)> = wanted
            .into_iter()
            .filter(|(s, p)| !self.counted.contains_key(&(*s, p.clone())))
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let counts = self
            .store()?
            .read_off_loop(move |read| {
                let mut out = Vec::with_capacity(wanted.len());
                for (space, prefix) in wanted {
                    let table = read.open_table(def(space))?;
                    let (mut n, mut bytes) = (0, 0);
                    for row in
                        table.range::<&[u8]>(prefix.as_slice()..prefix_end(&prefix).as_slice())?
                    {
                        let (k, v) = row?;
                        n += 1;
                        bytes += (k.value().len() + v.value().len()) as u64;
                    }
                    out.push(((space, prefix), (n, bytes)));
                }
                Ok(out)
            })
            .await?;
        for (key, count) in counts {
            self.counted.insert(key, (at, count));
        }
        Ok(())
    }
}

/// The prefixes `DescribeTable` shows counts of: the table's items and each index's entries.
pub(crate) fn shown(t: &Table) -> Vec<(Space, Vec<u8>)> {
    let mut out = vec![(Space::Items, t.id.to_vec())];
    for ix in t.gsis.iter().chain(&t.lsis) {
        out.push((Space::Index, crate::stage::index_prefix(t, ix)));
    }
    out
}

impl Core {
    /// What `doze-aws data`'s usage measures, to be counted off the serving thread first: each table's items, index entries,
    /// stream records and recovery log, and each backup's items.
    pub(crate) fn counted_in_usage(&self) -> Vec<(Space, Vec<u8>)> {
        let mut out = Vec::new();
        for (_, t) in self.tables.iter() {
            for space in [Space::Items, Space::Index, Space::Pitr] {
                out.push((space, t.id.to_vec()));
            }
            if let Some(s) = &t.stream {
                out.push((Space::Stream, s.id.to_vec()));
            }
        }
        for (_, e) in self.extras.with_prefix("backup/") {
            if let crate::extras::Extra::Backup(b) = e {
                out.push((Space::Backup, b.id.to_vec()));
            }
        }
        out
    }

    /// Before a `DescribeTable` (any other call returns at once): counts what it will show, off the serving thread. A table that
    /// is not there, or a failed read, is left to the call itself to report.
    pub(crate) async fn count_ahead(&mut self, cx: &Cx<'_>) {
        if aws_model_dynamodb::Operation::from_index(cx.call.op)
            != Some(aws_model_dynamodb::Operation::DescribeTable)
        {
            return;
        }
        let Some(t) = cx
            .table_name("TableName")
            .ok()
            .and_then(|name| self.tables.get(&name).map(shown))
        else {
            return;
        };
        // WHY: on a failed count the call counts on the serving thread, as before, and reports what that read says.
        drop(self.disk.count_off_loop(t).await);
    }
}
