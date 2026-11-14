//! Rule 6, one model and three renderings: each command builds a typed result once, and it is drawn for a terminal
//! (colour), for a pipe (the same words without it) and as JSON (the envelope's `data`). Plain and JSON are the stable
//! contracts the transcript tests pin. `data` is each result serialised in its own field order, as Go's structs are.

use crate::cmd::{docs, doctor, logs, modules, report, setup, status, task, verify, version, why};
use crate::paint::Paint;
use crate::plan::Plan;
use serde::{Serialize, Serializer};

/// A command's result.
#[derive(Debug)]
pub enum View {
    /// Printed as it is (a completion script).
    Raw(String),
    /// Nothing to print (the background refresh).
    Silent,
    /// Already written as it happened (`logs -f`, `status -w`): nothing more, and no envelope after NDJSON.
    Streamed,
    /// A page was opened in the browser.
    Opened(String),
    Version(version::Info),
    Plan(Plan),
    Docs(Box<docs::Page>),
    Modules(modules::Listing),
    Verify(verify::Report),
    Doctor(doctor::Report),
    /// The script a privileged command would run (`--dry-run`).
    Script(setup::Script),
    /// What a privileged command did, and each part as it stands after.
    Ran(setup::Ran),
    Status(Box<status::Report>),
    Graph(Box<status::Report>),
    /// What a verb did; each line was printed as it happened, so nothing is drawn after.
    Verbs(report::Report),
    Tasks(task::Listing),
    Logs(logs::Report),
    Why(Box<why::Why>),
}

impl View {
    /// For a person: `p` says whether stdout takes colour.
    pub(crate) fn human(&self, p: Paint) -> String {
        match self {
            View::Raw(s) => s.clone(),
            View::Silent | View::Streamed | View::Verbs(_) => String::new(),
            View::Opened(url) => format!("→ opened {url} in your browser\n"),
            View::Version(v) => v.human(),
            View::Plan(plan) => plan.render(p),
            View::Docs(page) => page.human(p),
            View::Modules(l) => l.human(p),
            View::Verify(r) => r.human(p),
            View::Doctor(r) => r.human(p),
            View::Script(s) => s.human(),
            View::Ran(r) => r.human(p),
            View::Status(r) => r.human(p),
            View::Graph(r) => r.graph(p),
            View::Tasks(t) => t.human(p),
            View::Logs(l) => l.human(p),
            View::Why(w) => w.human(p),
        }
    }

    /// Whether the envelope carries no `data` for it.
    pub(crate) fn is_empty(&self) -> bool {
        matches!(self, View::Raw(_) | View::Silent | View::Streamed)
    }
}

#[derive(Serialize)]
struct Opened<'a> {
    opened: &'a str,
}

#[derive(Serialize)]
struct DryRun<'a> {
    dry_run: bool,
    applies_with: &'a str,
    plan: &'a [crate::plan::Group],
}

impl Serialize for View {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            View::Raw(_) | View::Silent | View::Streamed => s.serialize_none(),
            View::Opened(url) => Opened { opened: url }.serialize(s),
            View::Version(v) => v.serialize(s),
            View::Plan(plan) => DryRun {
                dry_run: true,
                applies_with: &plan.applies_with,
                plan: &plan.groups,
            }
            .serialize(s),
            View::Docs(page) => page.serialize(s),
            View::Modules(l) => l.serialize(s),
            View::Verify(r) => r.serialize(s),
            View::Doctor(r) => r.serialize(s),
            View::Script(x) => x.serialize(s),
            View::Ran(r) => r.serialize(s),
            View::Status(r) | View::Graph(r) => r.json().serialize(s),
            View::Verbs(r) => r.serialize(s),
            View::Tasks(t) => t.serialize(s),
            View::Logs(l) => l.serialize(s),
            View::Why(w) => w.serialize(s),
        }
    }
}
