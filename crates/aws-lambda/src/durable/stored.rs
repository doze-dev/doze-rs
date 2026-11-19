//! A function's durable executions as `doze-aws data` sees them: their rows count with the function, a note says how many there
//! are, and `--before` removes the closed ones that ended before a time (retention removes them anyway, after
//! `RetentionPeriodInDays`).

use aws_wire::storage::{Emptied, Note, Prune};

use super::{WHAT, closed};
use crate::core::Core;
use crate::data::encoded;

impl Core {
    /// The bytes of a function's executions' rows, and the note that says what they are.
    pub(crate) fn execution_usage(&self, function: &str) -> (u64, Option<Note>) {
        let prefix = format!("{function}/");
        let (mut bytes, mut count, mut open) = (0, 0, 0);
        for (k, e) in self.durable.with_prefix(&prefix) {
            bytes += encoded(WHAT, k, e);
            count += 1;
            if !closed(&e.status) {
                open += 1;
            }
        }
        if count == 0 {
            return (0, None);
        }
        let note = Note {
            bytes,
            text: format!(
                "in {count} durable execution{} ({open} running): each closed one is kept for its RetentionPeriodInDays; --before removes closed ones sooner",
                if count == 1 { "" } else { "s" }
            ),
            prune: Some(Prune {
                before_ms: Some(self.now_ms()),
                ..Prune::default()
            }),
        };
        (bytes, Some(note))
    }

    /// Removes the closed executions of `function` that ended before `before`.
    pub(crate) fn forget_executions(
        &mut self,
        function: &str,
        before: i64,
        dry_run: bool,
    ) -> Emptied {
        let prefix = format!("{function}/");
        let gone: Vec<(String, u64)> = self
            .durable
            .with_prefix(&prefix)
            .filter(|(_, e)| closed(&e.status) && e.end_ms.is_some_and(|t| t < before))
            .map(|(k, e)| (k.to_owned(), encoded(WHAT, k, e)))
            .collect();
        let mut done = Emptied::default();
        for (k, bytes) in gone {
            done.items += 1;
            done.bytes += bytes;
            if !dry_run {
                self.durable.remove(&k);
            }
        }
        if !dry_run && done.items > 0 {
            self.live.changed = true;
            if let Ok(Some(ack)) = self.flush() {
                // WHY: the rows are written in the background; a failed write is the writer's to report.
                drop(ack);
            }
        }
        done
    }
}
