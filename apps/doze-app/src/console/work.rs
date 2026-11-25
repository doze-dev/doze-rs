//! The working blocks' state on a console tab (module protocol minor 2): what each value viewer shows and has read,
//! each tree's folders. Values and folders are asked of the daemon as a person looks, off the UI thread, never with
//! `wake` unless the person pressed to open a value on an instance that sleeps. A tree is only listed: listing never
//! wakes anything, and an instance that sleeps says so where the tree would be until it wakes.

use std::collections::{BTreeMap, HashMap, HashSet};

use doze_desk::BlockAsk;
use doze_desk::page::console::Node;
use doze_desk::page::json;
use doze_desk::page::tree::{Fetch, TreeState};
use doze_desk::page::value::{Refusal, ValueDoc};
use doze_display::NodeKind;
use gpui::{AppContext as _, Context, Entity, Window};
use gpui_component::input::{InputEvent, InputState};

use super::ConsoleTabView;

/// What a value viewer is to show: the panel asked, the row (a table's cells or `{"id": leaf}`), its identity and
/// its name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Want {
    pub(crate) panel: String,
    pub(crate) row: BTreeMap<String, String>,
    pub(crate) ident: String,
    pub(crate) label: String,
}

/// One value viewer.
#[derive(Default)]
pub(crate) struct ValueSlot {
    pub(crate) want: Option<Want>,
    pub(crate) doc: Option<ValueDoc>,
    pub(crate) refused: Option<Refusal>,
    pub(crate) loading: bool,
    /// The JSON containers folded, by line.
    pub(crate) folded: HashSet<usize>,
    /// Long text lines wrap (on by default; off keeps a log line one line).
    pub(crate) nowrap: bool,
    /// When Copy was pressed: it says "Copied" a moment.
    pub(crate) copied: Option<std::time::Instant>,
    /// Counts the asks, so an answer to one no longer wanted is dropped.
    asked: u64,
}

/// One tree.
pub(crate) struct TreeSlot {
    pub(crate) state: TreeState,
    pub(crate) panel: String,
    pub(crate) filter: Option<Entity<InputState>>,
}

/// Every working block on the tab.
#[derive(Default)]
pub(crate) struct Work {
    pub(crate) values: HashMap<String, ValueSlot>,
    pub(crate) trees: HashMap<String, TreeSlot>,
    /// The instance was awake when last looked: a tree refused while it slept is read again as it wakes.
    awake: bool,
}

/// The value viewer a tree shows its leaves in when no value block on the tab follows it.
pub(crate) fn own_viewer(tree: &str) -> String {
    format!("{tree}#value")
}

impl ConsoleTabView {
    /// Brings the working blocks up to the page just resolved: a new pick or leaf is asked for, a tree is listed the
    /// first time it shows, and what was refused while the instance slept is asked again when it wakes.
    pub(super) fn sync_work(&mut self, cx: &mut Context<Self>) {
        let awake = self.page.awake;
        let woke = awake && !self.work.awake;
        self.work.awake = awake;
        let nodes: Vec<Node> = self.page.body.walk().into_iter().cloned().collect();
        self.sync_trees(&nodes, woke, cx);
        self.sync_values(&nodes, woke, cx);
    }

    /// Each tree listed the first time it shows, and again as its instance wakes if it was refused.
    fn sync_trees(&mut self, nodes: &[Node], woke: bool, cx: &mut Context<Self>) {
        let mut fetches = Vec::new();
        for n in nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Tree && n.missing.is_empty())
        {
            let slot = self
                .work
                .trees
                .entry(n.id.clone())
                .or_insert_with(|| TreeSlot {
                    state: TreeState::default(),
                    panel: n.panel.clone(),
                    filter: None,
                });
            let refused = slot.state.folders.values().any(|f| f.refused.is_some());
            if woke && refused {
                fetches.extend(slot.state.reread().into_iter().map(|f| (n.id.clone(), f)));
            } else if !slot.state.folders.contains_key("")
                && let Some(f) = slot.state.start()
            {
                fetches.push((n.id.clone(), f));
            }
        }
        for (tree, f) in fetches {
            self.fetch_children(&tree, f, cx);
        }
    }

    /// Each value viewer asked for what it is to show now (a new pick or leaf), and again as its instance wakes if
    /// it was refused for sleeping.
    fn sync_values(&mut self, nodes: &[Node], woke: bool, cx: &mut Context<Self>) {
        let mut wants: Vec<(String, Option<Want>)> = Vec::new();
        for n in nodes.iter().filter(|n| n.missing.is_empty()) {
            match n.kind {
                NodeKind::Value if n.follows_tree => {
                    wants.push((n.id.clone(), self.leaf_want(&n.follows, &n.panel)));
                }
                NodeKind::Value => wants.push((
                    n.id.clone(),
                    (!n.pick.is_empty()).then(|| Want {
                        panel: n.panel.clone(),
                        row: n.row.clone(),
                        ident: n.pick.clone(),
                        label: n.pick_name.clone(),
                    }),
                )),
                NodeKind::Tree if !n.paired => {
                    wants.push((own_viewer(&n.id), self.leaf_want(&n.id, &n.panel)));
                }
                _ => {}
            }
        }
        for (id, want) in wants {
            let slot = self.work.values.entry(id.clone()).or_default();
            let same = slot.want.as_ref().map(|w| &w.ident) == want.as_ref().map(|w| &w.ident);
            // The same value, named better now its folder has been read.
            if same && let (Some(w), Some(new)) = (slot.want.as_mut(), want.as_ref()) {
                w.label.clone_from(&new.label);
            }
            let retry = woke
                && slot
                    .refused
                    .as_ref()
                    .is_some_and(|r| r.asleep() || r.starting());
            if same && !retry {
                continue;
            }
            if !same {
                *slot = ValueSlot {
                    want,
                    nowrap: slot.nowrap,
                    asked: slot.asked,
                    ..ValueSlot::default()
                };
            }
            if slot.want.is_some() {
                self.fetch_value(&id, 0, false, cx);
            }
        }
    }

    /// The leaf opened in tree `tree`, asked of `panel`.
    fn leaf_want(&self, tree: &str, panel: &str) -> Option<Want> {
        let slot = self.work.trees.get(tree)?;
        let leaf = slot.state.picked.clone()?;
        // Its place in the tree, the last two steps of it (`orders › rows`); its id until its folder is read.
        let path = slot.state.path(&leaf);
        let label = if path.is_empty() {
            leaf.clone()
        } else {
            path.get(path.len().saturating_sub(2)..)
                .unwrap_or_default()
                .join(" › ")
        };
        Some(Want {
            panel: if panel.is_empty() {
                slot.panel.clone()
            } else {
                panel.to_owned()
            },
            row: BTreeMap::from([("id".to_owned(), leaf.clone())]),
            ident: format!("{tree}\u{1f}{leaf}"),
            label,
        })
    }

    /// Asks for the value of viewer `id` from `offset` ("Load the rest" asks from where the body ends); `wake` only
    /// when the person pressed to open it on an instance that sleeps.
    pub(crate) fn fetch_value(
        &mut self,
        id: &str,
        offset: u64,
        wake: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.work.values.get_mut(id) else {
            return;
        };
        let Some(want) = slot.want.clone() else {
            return;
        };
        slot.asked += 1;
        slot.loading = true;
        if offset == 0 {
            slot.refused = None;
        }
        let asked = slot.asked;
        let (hub, key, id) = (self.hub.clone(), self.key.clone(), id.to_owned());
        let at = BlockAsk {
            instance: self.name.clone(),
            panel: want.panel.clone(),
        };
        cx.spawn(async move |this, cx| {
            let got = cx
                .background_executor()
                .spawn(async move { hub.value(&key, &at, want.row, offset, wake) })
                .await;
            let _ = this.update(cx, |v, cx| {
                let Some(slot) = v.work.values.get_mut(&id).filter(|s| s.asked == asked) else {
                    return;
                };
                slot.loading = false;
                match got {
                    Ok(part) if offset > 0 => {
                        if let Some(d) = &mut slot.doc {
                            d.add(&part);
                        }
                    }
                    Ok(part) => {
                        let d = ValueDoc::first(&part);
                        slot.folded = d.json.as_deref().map(json::first_folds).unwrap_or_default();
                        slot.doc = Some(d);
                    }
                    Err(e) => slot.refused = Some(Refusal::of(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Asks for a page of tree `tree`'s folder.
    pub(crate) fn fetch_children(&mut self, tree: &str, f: Fetch, cx: &mut Context<Self>) {
        let Some(slot) = self.work.trees.get(tree) else {
            return;
        };
        let (hub, key, tree) = (self.hub.clone(), self.key.clone(), tree.to_owned());
        let at = BlockAsk {
            instance: self.name.clone(),
            panel: slot.panel.clone(),
        };
        cx.spawn(async move |this, cx| {
            let asked = f.clone();
            let got = cx
                .background_executor()
                .spawn(async move { hub.children(&key, &at, &f.parent, &f.filter, &f.cursor) })
                .await;
            let _ = this.update(cx, |v, cx| {
                if let Some(slot) = v.work.trees.get_mut(&tree) {
                    match got {
                        Ok(page) => slot.state.got(&asked, page),
                        Err(e) => slot.state.refused(&asked, Refusal::of(&e)),
                    }
                }
                // A leaf opened before its folder was read gets its name.
                v.sync_work(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// A folder's chevron: it opens (asked for the first time) or closes.
    pub(crate) fn toggle_folder(&mut self, tree: &str, id: &str, cx: &mut Context<Self>) {
        self.focused = Some(tree.to_owned());
        let f = self
            .work
            .trees
            .get_mut(tree)
            .and_then(|s| s.state.toggle(id));
        if let Some(f) = f {
            self.fetch_children(tree, f, cx);
        }
        cx.notify();
    }

    /// "Show more" under a folder.
    pub(crate) fn more(&mut self, tree: &str, parent: &str, cx: &mut Context<Self>) {
        let f = self
            .work
            .trees
            .get_mut(tree)
            .and_then(|s| s.state.more(parent));
        if let Some(f) = f {
            self.fetch_children(tree, f, cx);
        }
    }

    /// A leaf opens in the value viewer that follows the tree (or the tree's own).
    pub(crate) fn open_leaf(&mut self, tree: &str, id: &str, cx: &mut Context<Self>) {
        self.focused = Some(tree.to_owned());
        if let Some(s) = self.work.trees.get_mut(tree) {
            s.state.picked = Some(id.to_owned());
        }
        self.sync_work(cx);
        cx.notify();
    }

    /// Each tree's filter box, made the first time the tree is drawn (a box needs the window).
    pub(super) fn ensure_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let missing: Vec<String> = self
            .work
            .trees
            .iter()
            .filter(|(_, s)| s.filter.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        for id in missing {
            let i = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
            let tree = id.clone();
            cx.subscribe_in(
                &i,
                window,
                move |v: &mut ConsoleTabView, i, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Change => {
                        let words = i.read(cx).value().to_string();
                        let f = v
                            .work
                            .trees
                            .get_mut(&tree)
                            .and_then(|s| s.state.set_filter(&words));
                        if let Some(f) = f {
                            v.fetch_children(&tree, f, cx);
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => window.focus(&v.focus, cx),
                    _ => {}
                },
            )
            .detach();
            if let Some(s) = self.work.trees.get_mut(&id) {
                s.filter = Some(i);
            }
        }
    }
}
