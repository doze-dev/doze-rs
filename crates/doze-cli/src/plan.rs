//! `--dry-run`: a command does its reading and planning, fills a [`Plan`], and the plan is printed instead of acted on.
//! Nothing is changed and `doze.lock` is never written. Object by object, as Terraform's plan: what would be created,
//! changed, removed or done, grouped by what it belongs to.

use crate::paint::{Paint, pad, width};
use serde::Serialize;

/// What one step would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Op {
    /// `+`: something new.
    #[serde(rename = "+")]
    Add,
    /// `~`: something that exists, changed.
    #[serde(rename = "~")]
    Change,
    /// `-`: something removed (data deleted, a pin dropped).
    #[serde(rename = "-")]
    Remove,
    /// `→`: an action (wake, sleep, install, run).
    #[serde(rename = "→")]
    Do,
}

impl Op {
    fn mark(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Change => "~",
            Op::Remove => "-",
            Op::Do => "→",
        }
    }
}

/// One step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub op: Op,
    pub subject: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// The steps for one thing (a service, a module, doze itself).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Group {
    pub title: String,
    pub changes: Vec<Change>,
}

/// What a command would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// The command that does it for real, for the last line.
    pub applies_with: String,
    pub groups: Vec<Group>,
}

impl Plan {
    pub fn new(applies_with: impl Into<String>) -> Plan {
        Plan {
            applies_with: applies_with.into(),
            groups: Vec::new(),
        }
    }

    /// Adds a step under `title`, opening the group the first time.
    pub fn step(
        &mut self,
        title: &str,
        op: Op,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) {
        let change = Change {
            op,
            subject: subject.into(),
            detail: detail.into(),
        };
        match self.groups.iter_mut().find(|g| g.title == title) {
            Some(g) => g.changes.push(change),
            None => self.groups.push(Group {
                title: title.to_string(),
                changes: vec![change],
            }),
        }
    }

    /// Records that `title` was looked at and needs nothing.
    pub fn unchanged(&mut self, title: &str) {
        if !self.groups.iter().any(|g| g.title == title) {
            self.groups.push(Group {
                title: title.to_string(),
                changes: Vec::new(),
            });
        }
    }

    /// The plan as a person reads it.
    pub(crate) fn render(&self, p: Paint) -> String {
        // Every detail starts in one column, so a long plan reads down.
        let col = self
            .groups
            .iter()
            .flat_map(|g| {
                let own = g.changes.is_empty().then(|| width(&g.title) + 2);
                let steps = g
                    .changes
                    .iter()
                    .filter(|c| !c.detail.is_empty())
                    .map(|c| 4 + width(&c.subject) + 2);
                own.into_iter().chain(steps)
            })
            .max()
            .unwrap_or(0);
        let mut out = String::new();
        for g in &self.groups {
            if g.changes.is_empty() {
                out.push_str(&format!(
                    "{}{}\n",
                    p.bold(&pad(&g.title, col)),
                    p.dim("nothing to change")
                ));
                continue;
            }
            out.push_str(&format!("{}\n", p.bold(&g.title)));
            for c in &g.changes {
                let mark = match c.op {
                    Op::Add => p.ok(c.op.mark()),
                    Op::Remove => p.fail(c.op.mark()),
                    Op::Change | Op::Do => p.warn(c.op.mark()),
                };
                if c.detail.is_empty() {
                    out.push_str(&format!("  {mark} {}\n", c.subject));
                } else {
                    let subject = pad(&c.subject, col.saturating_sub(4));
                    out.push_str(&format!("  {mark} {subject}{}\n", c.detail));
                }
            }
        }
        out.push_str(&format!(
            "  {} · {} applies this\n",
            p.dim("dry run: nothing was changed"),
            p.cmd(&self.applies_with)
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The proposal's `sync --dry-run`, drawn from a plan.
    #[test]
    fn draws_the_proposal_sync_plan() {
        let mut plan = Plan::new("doze sync");
        plan.step("db (postgres)", Op::Change, "role app", "password changed");
        plan.step("db (postgres)", Op::Add, "extension pgvector", "");
        plan.unchanged("cache (valkey)");
        assert_eq!(
            plan.render(Paint::new(false)),
            "db (postgres)\n  ~ role app    password changed\n  + extension pgvector\ncache (valkey)  nothing to change\n  dry run: nothing was changed · doze sync applies this\n"
        );
    }
}
