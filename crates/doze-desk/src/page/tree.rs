//! A console tree as a person opens it (module protocol minor 2): the folders read so far, a page at a time, which
//! are open, what the filter found, and the leaf opened in the value viewer. Each folder's children are asked of the
//! module when it first opens (the hub's `children`, which never wakes an instance); this only keeps what came back
//! and lays it out as rows.

use super::value::Refusal;
use doze_control::{ChildrenView, TreeNodeView};
use std::collections::{HashMap, HashSet};

/// One folder's children as read: the pages so far, and where the next starts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Folder {
    pub nodes: Vec<TreeNodeView>,
    /// The next page's cursor; empty at the end.
    pub next: String,
    pub loading: bool,
    pub refused: Option<Refusal>,
}

/// A page to ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetch {
    pub parent: String,
    pub filter: String,
    pub cursor: String,
}

/// One row of the tree as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    Node {
        id: String,
        label: String,
        depth: usize,
        leaf: bool,
        open: bool,
        /// Its info in words: `12,431 rows · hash`.
        info: String,
    },
    /// "Show more": the folder `parent` has another page.
    More {
        parent: String,
        depth: usize,
    },
    Loading {
        depth: usize,
    },
    /// Why a folder could not be read, or that it is empty.
    Note {
        depth: usize,
        text: String,
        bad: bool,
    },
}

/// The tree.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeState {
    /// Each folder read, by its id ("" for the top).
    pub folders: HashMap<String, Folder>,
    pub open: HashSet<String>,
    /// What the filter box holds, and what the module found for it (a folder of its own).
    pub filter: String,
    pub found: Folder,
    /// The leaf opened in the value viewer.
    pub picked: Option<String>,
    /// The row the keys are on (a node's id): ↑↓ move it, → opens, ← closes or goes up, ⏎ opens a leaf.
    pub cursor: Option<String>,
}

impl TreeState {
    /// The top, asked for the first time (or again, after a refusal).
    pub fn start(&mut self) -> Option<Fetch> {
        let top = self.folders.entry(String::new()).or_default();
        if top.loading || (!top.nodes.is_empty() && top.refused.is_none()) {
            return None;
        }
        top.loading = true;
        top.refused = None;
        Some(fetch("", "", ""))
    }

    /// Forgets every folder read (the instance slept and woke, the module may have changed), keeping what is open, and
    /// asks for the top, each open folder and what the filter finds again.
    pub fn reread(&mut self) -> Vec<Fetch> {
        self.folders.clear();
        self.found = Folder::default();
        let mut out: Vec<Fetch> = self.start().into_iter().collect();
        // Each folder still open is read again too, or it would say "Reading…" for ever.
        let mut open: Vec<String> = self.open.iter().cloned().collect();
        open.sort();
        for id in open {
            let f = self.folders.entry(id.clone()).or_default();
            f.loading = true;
            out.push(fetch(&id, "", ""));
        }
        if !self.filter.is_empty() {
            self.found.loading = true;
            out.push(fetch("", &self.filter, ""));
        }
        out
    }

    /// Opens or closes a folder; a folder opened for the first time is asked for.
    pub fn toggle(&mut self, id: &str) -> Option<Fetch> {
        if self.open.remove(id) {
            return None;
        }
        self.open.insert(id.to_owned());
        self.open_one(id)
    }

    /// Opens a folder (→); asked for when not read yet.
    pub fn open_one(&mut self, id: &str) -> Option<Fetch> {
        self.open.insert(id.to_owned());
        let f = self.folders.entry(id.to_owned()).or_default();
        if f.loading || (!f.nodes.is_empty() && f.refused.is_none()) {
            return None;
        }
        f.loading = true;
        f.refused = None;
        Some(fetch(id, "", ""))
    }

    /// The next page of `parent` (`""` with a filter: of what was found).
    pub fn more(&mut self, parent: &str) -> Option<Fetch> {
        let filtering = parent.is_empty() && !self.filter.is_empty();
        let f = if filtering {
            &mut self.found
        } else {
            self.folders.get_mut(parent)?
        };
        if f.loading || f.next.is_empty() {
            return None;
        }
        f.loading = true;
        let filter = if filtering {
            self.filter.clone()
        } else {
            String::new()
        };
        Some(fetch(parent, &filter, &f.next))
    }

    /// The filter changed: what it finds is asked for (searching every folder, not only the open ones).
    pub fn set_filter(&mut self, filter: &str) -> Option<Fetch> {
        let filter = filter.trim();
        if filter == self.filter {
            return None;
        }
        filter.clone_into(&mut self.filter);
        self.found = Folder::default();
        if filter.is_empty() {
            return None;
        }
        self.found.loading = true;
        Some(fetch("", filter, ""))
    }

    /// A page came back. One for a filter no longer in the box is dropped.
    pub fn got(&mut self, f: &Fetch, page: ChildrenView) {
        let Some(folder) = self.folder_of(f) else {
            return;
        };
        if f.cursor.is_empty() {
            folder.nodes.clear();
        }
        folder.nodes.extend(page.nodes);
        folder.next = page.next;
        folder.loading = false;
        folder.refused = None;
    }

    /// A page could not be read.
    pub fn refused(&mut self, f: &Fetch, r: Refusal) {
        if let Some(folder) = self.folder_of(f) {
            folder.loading = false;
            folder.refused = Some(r);
        }
    }

    fn folder_of(&mut self, f: &Fetch) -> Option<&mut Folder> {
        if f.filter.is_empty() {
            Some(self.folders.entry(f.parent.clone()).or_default())
        } else {
            (f.filter == self.filter).then_some(&mut self.found)
        }
    }

    /// Where node `id` is, as the labels of the folders down to it and its own (`["public", "orders", "rows"]`);
    /// empty when it is not among the rows drawn.
    pub fn path(&self, id: &str) -> Vec<String> {
        let rows = self.rows();
        let Some(at) = rows
            .iter()
            .position(|r| matches!(r, TreeRow::Node { id: i, .. } if i == id))
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut want = usize::MAX;
        for r in rows.iter().take(at + 1).rev() {
            if let TreeRow::Node { label, depth, .. } = r
                && *depth < want
            {
                out.push(label.clone());
                want = *depth;
            }
        }
        out.reverse();
        out
    }

    /// Something was refused (asleep, starting, failed): a folder, or what the filter found.
    pub fn any_refused(&self) -> bool {
        self.found.refused.is_some() || self.folders.values().any(|f| f.refused.is_some())
    }

    /// The rows as drawn: the top (or what the filter found), each open folder's children under it.
    pub fn rows(&self) -> Vec<TreeRow> {
        let mut out = Vec::new();
        if self.filter.is_empty() {
            if let Some(top) = self.folders.get("") {
                self.lay(top, "", 0, &mut out);
            }
        } else {
            self.lay(&self.found, "", 0, &mut out);
        }
        out
    }

    fn lay(&self, f: &Folder, parent: &str, depth: usize, out: &mut Vec<TreeRow>) {
        for n in &f.nodes {
            let open = !n.leaf && self.open.contains(&n.id);
            out.push(TreeRow::Node {
                id: n.id.clone(),
                label: n.label.clone(),
                depth,
                leaf: n.leaf,
                open,
                info: info(n),
            });
            if open {
                match self.folders.get(&n.id) {
                    Some(c) => self.lay(c, &n.id, depth + 1, out),
                    None => out.push(TreeRow::Loading { depth: depth + 1 }),
                }
            }
        }
        if !f.next.is_empty() && !f.loading {
            out.push(TreeRow::More {
                parent: parent.to_owned(),
                depth,
            });
        }
        if f.loading {
            out.push(TreeRow::Loading { depth });
        } else if let Some(r) = &f.refused {
            // Listing never wakes anything: an instance asleep or starting is listed when it is up, and says so;
            // one off or failed says why in the daemon's words.
            let text = if r.asleep() {
                "Asleep: listed again as it wakes. Listing never wakes it.".to_owned()
            } else if r.starting() {
                "Starting: listed once it is up.".to_owned()
            } else {
                r.message.clone()
            };
            out.push(TreeRow::Note {
                depth,
                text,
                bad: r.state().is_none(),
            });
        } else if f.nodes.is_empty() {
            let text = if depth == 0 && !self.filter.is_empty() {
                format!("Nothing matches “{}”.", self.filter)
            } else {
                "Empty.".to_owned()
            };
            out.push(TreeRow::Note {
                depth,
                text,
                bad: false,
            });
        }
    }
}

fn fetch(parent: &str, filter: &str, cursor: &str) -> Fetch {
    Fetch {
        parent: parent.to_owned(),
        filter: filter.to_owned(),
        cursor: cursor.to_owned(),
    }
}

/// A node's info in words, in the module's order: a figure is counted with its word (`12,431 rows`, `1 key`), a
/// size in bytes in its unit, anything else as it is.
pub fn info(n: &TreeNodeView) -> String {
    n.info
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| match v.parse::<u64>() {
            Ok(b) if k == "bytes" || k == "size" => super::value::size(b),
            Ok(1) => format!("1 {}", k.strip_suffix('s').unwrap_or(k)),
            Ok(c) => format!("{} {k}", thousands(c)),
            Err(_) => v.clone(),
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// `12431` as `12,431`.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;
