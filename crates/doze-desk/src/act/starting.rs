//! What Up would start, by the CLI's rule (Go `startingSet`, the CLI's `chooseUpTargets`): the chosen ones, or last
//! time's set still declared, or else every enabled instance, tasks included; each with what it needs, all the way
//! down. And the start sheet (Go `StartSheet`): tick the apps to work on, see what comes with them.

use std::collections::{BTreeSet, HashMap};

use crate::overview::Decl;

/// The set Up would start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Starting<'a> {
    /// Every instance that starts, roots first, each followed by what it needs.
    pub decls: Vec<&'a Decl>,
    /// What was asked for, by name.
    pub names: Vec<String>,
    /// What comes because something needs it.
    pub deps: Vec<String>,
    /// Needed by another in the set.
    pub needed: BTreeSet<String>,
}

/// The set `action` (`up-last`, `up`, `up-all`) starts from `decls`, given last time's set and the names chosen.
pub fn starting_set<'a>(
    decls: &'a [Decl],
    last: &[String],
    action: &str,
    chosen: &[String],
) -> Starting<'a> {
    let by: HashMap<&str, &Decl> = decls.iter().map(|d| (d.name.as_str(), d)).collect();
    let keep = |names: &[String]| -> Vec<String> {
        names
            .iter()
            .filter(|n| by.get(n.as_str()).is_some_and(|d| d.enabled))
            .cloned()
            .collect()
    };
    let mut roots = match action {
        "up" => keep(chosen),
        "up-last" => keep(last),
        _ => Vec::new(),
    };
    if roots.is_empty() {
        roots = decls
            .iter()
            .filter(|d| d.enabled)
            .map(|d| d.name.clone())
            .collect();
    }
    let mut out = Starting::default();
    let mut on = BTreeSet::new();
    for r in &roots {
        mark(&by, &roots, r, true, &mut on, &mut out);
    }
    out
}

fn mark<'a>(
    by: &HashMap<&str, &'a Decl>,
    roots: &[String],
    name: &str,
    root: bool,
    on: &mut BTreeSet<String>,
    out: &mut Starting<'a>,
) {
    let Some(d) = by.get(name).copied().filter(|d| d.enabled) else {
        return;
    };
    if !root {
        out.needed.insert(name.to_owned());
    }
    if !on.insert(name.to_owned()) {
        return;
    }
    out.decls.push(d);
    if roots.iter().any(|r| r == name) {
        out.names.push(name.to_owned());
    } else {
        out.deps.push(name.to_owned());
    }
    for n in &d.needs {
        mark(by, roots, n, false, on, out);
    }
}

/// One line of the start sheet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pick {
    pub name: String,
    /// "needs api, db", or "needs nothing".
    pub needs: String,
    /// It was in last time's set.
    pub last: bool,
}

/// The start sheet: what can be ticked, what is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartSheet {
    pub stack: String,
    pub picks: Vec<Pick>,
    pub ticked: Vec<bool>,
    /// The line under the keys.
    pub at: usize,
    needs: HashMap<String, Vec<String>>,
}

/// What the sheet's Start button says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartWord {
    All,
    One,
    These,
}

impl StartSheet {
    /// The sheet for a stopped stack: its apps and services that can start (not tasks, not modules, not paused),
    /// ticked as last time, or every one when nothing is remembered.
    pub fn new(stack: &str, decls: &[Decl], last: &[String]) -> StartSheet {
        let picks: Vec<Pick> = decls
            .iter()
            .filter(|d| d.enabled && d.kind != "task" && d.kind != "module")
            .map(|d| Pick {
                name: d.name.clone(),
                needs: if d.needs.is_empty() {
                    "needs nothing".to_owned()
                } else {
                    format!("needs {}", d.needs.join(", "))
                },
                last: last.contains(&d.name),
            })
            .collect();
        let ticked = picks.iter().map(|p| last.is_empty() || p.last).collect();
        StartSheet {
            stack: stack.to_owned(),
            picks,
            ticked,
            at: 0,
            needs: decls
                .iter()
                .map(|d| (d.name.clone(), d.needs.clone()))
                .collect(),
        }
    }

    /// The names ticked.
    pub fn chosen(&self) -> Vec<String> {
        self.picks
            .iter()
            .zip(&self.ticked)
            .filter(|(_, t)| **t)
            .map(|(p, _)| p.name.clone())
            .collect()
    }

    /// What the chosen ones need, all the way down, that was not chosen itself, in the order it is found.
    pub fn brought(&self) -> Vec<String> {
        let chosen = self.chosen();
        let mut out: Vec<String> = Vec::new();
        for n in &chosen {
            self.walk(n, &chosen, &mut out);
        }
        out
    }

    /// Go's `walk`: each need in the order written, then what it needs, before the next.
    fn walk(&self, n: &str, chosen: &[String], out: &mut Vec<String>) {
        for d in self.needs.get(n).into_iter().flatten() {
            if !out.contains(d) && !chosen.contains(d) {
                out.push(d.clone());
                self.walk(d, chosen, out);
            }
        }
    }

    /// Ticks or unticks line `i` (space on the line, or a click).
    pub fn toggle(&mut self, i: usize) {
        if let Some(t) = self.ticked.get_mut(i) {
            *t = !*t;
        }
    }

    /// Moves the line under the keys, kept in the list.
    pub fn step(&mut self, by: isize) {
        let n = self.picks.len();
        if n > 0 {
            self.at = self.at.saturating_add_signed(by).min(n - 1);
        }
    }

    /// The Start button's word.
    pub fn word(&self) -> StartWord {
        match self.chosen().len() {
            n if n == self.picks.len() => StartWord::All,
            1 => StartWord::One,
            _ => StartWord::These,
        }
    }

    /// The line under the picks: what else starts, or that nothing does, or that nothing is ticked.
    pub fn also(&self) -> (Option<String>, bool) {
        if self.chosen().is_empty() {
            return (None, true);
        }
        let b = self.brought();
        ((!b.is_empty()).then(|| b.join(", ")), false)
    }
}

#[cfg(test)]
mod tests {
    use super::{StartSheet, StartWord, starting_set};
    use crate::overview::Decl;

    fn d(name: &str, kind: &str, needs: &[&str]) -> Decl {
        Decl {
            name: name.into(),
            kind: kind.into(),
            enabled: true,
            needs: needs.iter().map(|s| (*s).to_owned()).collect(),
            ..Decl::default()
        }
    }

    fn decls() -> Vec<Decl> {
        vec![
            d("api", "app", &["db"]),
            d("web", "app", &["api"]),
            d("worker", "app", &[]),
            d("db", "module", &[]),
            d("migrate", "task", &["db"]),
        ]
    }

    fn names(s: &super::Starting<'_>) -> String {
        s.decls
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }

    // Go: TestStartingSet.
    #[test]
    fn the_set_up_starts_as_go_reads_it() {
        let ds = decls();
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            names(&starting_set(&ds, &s(&["web"]), "up-last", &[])),
            "web,api,db"
        );
        assert_eq!(
            names(&starting_set(&ds, &[], "up-last", &[])),
            "api,db,web,worker,migrate"
        );
        assert_eq!(
            names(&starting_set(&ds, &s(&["gone"]), "up-last", &[])),
            "api,db,web,worker,migrate"
        );
        let set = starting_set(&ds, &[], "up", &s(&["web", "worker"]));
        assert!(set.needed.contains("api"));
        assert!(!set.needed.contains("worker") && !set.needed.contains("web"));
        assert_eq!(set.deps, s(&["api", "db"]));
    }

    #[test]
    fn the_sheet_ticks_last_time_and_says_what_comes_too() {
        let ds = decls();
        let mut sheet = StartSheet::new("shop", &ds, &["web".into()]);
        let names: Vec<&str> = sheet.picks.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["api", "web", "worker"], "no modules, no tasks");
        assert_eq!(sheet.chosen(), ["web"]);
        assert_eq!(sheet.brought(), ["api", "db"]);
        assert_eq!(sheet.word(), StartWord::One);
        assert_eq!(sheet.also(), (Some("api, db".into()), false));
        sheet.toggle(1);
        assert_eq!(sheet.also(), (None, true), "nothing ticked");
        let all = StartSheet::new("shop", &ds, &[]);
        assert_eq!(all.word(), StartWord::All, "nothing remembered: every one");
        assert_eq!(all.brought(), ["db"]);
        sheet.step(5);
        assert_eq!(sheet.at, 2);
        sheet.step(-9);
        assert_eq!(sheet.at, 0);
    }
}
