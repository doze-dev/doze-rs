//! How a task went, said when its record changes (Go `reportTasks`): "migrate done in 1.2s", "seed failed (exit 1)
//! after 0.4s". Every task on every stack is watched, whoever ran it; a record seen for the first time only primes.

use std::collections::HashMap;

use doze_display as words;
use gpui::Context;

use super::Root;

/// The last record of each task, by stack folder and name: when it ran and how.
#[derive(Debug, Default)]
pub(crate) struct Tasks(HashMap<String, String>);

impl Root {
    /// Says how each task whose record changed since the last batch went.
    pub(crate) fn report_tasks(&mut self, cx: &mut Context<Self>) {
        let mut said = Vec::new();
        for st in self.hub.stacks() {
            for v in &st.words().instances {
                let Some(rec) = v.task.as_ref().filter(|_| v.kind == words::Kind::Task) else {
                    continue;
                };
                let key = format!("{}/{}", st.dir, v.name);
                let sig = format!("{:?}{:?}", rec.ran_at, rec.outcome);
                let prev = self.tasks.0.insert(key, sig.clone());
                if prev.is_some_and(|p| p != sig) {
                    said.push(words::task_result(&v.name, rec));
                }
            }
        }
        for s in said {
            self.acts.update(cx, |a, cx| a.say(s.text, s.bad, cx));
        }
    }
}
