//! The gateway's question to the table: what to do with this call.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use aws_gateway::{Ask, Faults, Tag, Verdict};

use super::{LabTable, choose, record};
use crate::lab::codes;
use crate::lab::record::EndedWhy;
use crate::lab::rule::Effect;

impl Faults for LabTable {
    fn armed(&self) -> bool {
        self.switched_on.load(Ordering::Relaxed) > 0
    }

    fn judge(&self, ask: &Ask<'_>) -> Verdict {
        if !self.armed() {
            return Verdict::Pass;
        }
        let now = self.clock.mono_ms();
        let mut inner = self.lock();
        self.sweep(&mut inner, now);
        let Some((index, (_, wait_ms))) = choose(&mut inner, ask) else {
            return Verdict::Pass;
        };
        let wall = self.clock.wall_ms();
        let Some(l) = inner.rules.get_mut(index) else {
            return Verdict::Pass;
        };
        record(&mut l.rule, ask, wall);
        let tag = Tag {
            rule: l.rule.id,
            name: Arc::clone(&l.rule.name),
        };
        let effect = l.rule.spec.effect.clone();
        let spent = l
            .rule
            .spec
            .max_hits
            .is_some_and(|m| l.rule.hits >= u64::from(m));
        if spent {
            inner.end(index, EndedWhy::Spent, wall);
            self.recount(&inner);
        }
        drop(inner);
        match effect {
            Effect::Slow { .. } => Verdict::Slow {
                delay: Duration::from_millis(wait_ms),
                tag,
            },
            Effect::Fail(fail) => Verdict::Fail(Box::new(codes::failure(&fail, ask, tag))),
            Effect::Drop => Verdict::Drop { tag },
        }
    }
}
