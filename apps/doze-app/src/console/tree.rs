//! The tree (module protocol minor 2): folders that open as you go, each folder's children asked of the module a page
//! at a time ("Show more"), a filter that asks the module (so it searches every folder, not only the open ones), and
//! leaves that open in the value viewer. Listing never wakes anything: an instance that sleeps says so where its
//! folders would be, and they are read as it wakes. A tree with no value block following it shows the leaf itself,
//! beside its folders.

use doze_desk::page::console::Node;
use doze_desk::page::tree::TreeRow;
use gpui::{AnyElement, IntoElement, SharedString, div, prelude::*, px, relative};
use gpui_component::input::Input;

use super::blocks::{Draw, share};
use super::value::chevron;
use super::work::own_viewer;
use crate::instance::ui::{heading, text};
use crate::theme::{Tokens, ty};
use crate::widgets::Mono as _;

/// A tree row's height.
const ROW: f32 = 26.;
/// Each level's indent.
const INDENT: f32 = 16.;

impl Draw<'_> {
    pub(super) fn tree(&self, n: &Node) -> AnyElement {
        let folders = self.folders(n);
        if n.paired || self.work.is_none() {
            return folders;
        }
        // No value block follows it: the leaf opens beside the folders, a third and two thirds.
        let viewer = Node {
            id: own_viewer(&n.id),
            weight: 2,
            ..n.clone()
        };
        div()
            .flex()
            .flex_row()
            .gap(px(14.))
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_basis(relative(0.))
                    .flex_grow(1.)
                    .min_w_0()
                    .min_h_0()
                    .child(folders),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_basis(relative(0.))
                    .flex_grow(2.)
                    .min_w_0()
                    .min_h_0()
                    .child(self.value(&viewer, Some(&viewer.id))),
            )
            .into_any_element()
    }

    /// The folders' card: its title, the filter, the rows.
    fn folders(&self, n: &Node) -> AnyElement {
        let t = self.t;
        let slot = self.work.and_then(|w| w.trees.get(&n.id));
        let rows = slot.map(|s| s.state.rows()).unwrap_or_default();
        let picked = slot.and_then(|s| s.state.picked.clone());
        let cursor = slot.and_then(|s| s.state.cursor.clone());
        let filter = slot.and_then(|s| s.filter.clone()).map(|i| {
            let focused = self.live.as_ref().is_some_and(|_| {
                self.filter
                    .as_ref()
                    .is_some_and(|(id, _, k)| *id == n.id && *k)
            });
            crate::widgets::framed_input(
                t,
                &format!("tree-filter-{}", n.id),
                focused,
                Input::new(&i),
            )
        });
        let list = if rows.is_empty() {
            vec![self.note_row(0, "Reading…", false)]
        } else {
            rows.iter()
                .enumerate()
                .map(|(i, r)| self.tree_row(n, i, r, (picked.as_deref(), cursor.as_deref())))
                .collect()
        };
        share(self.frame(), n)
            .min_h(px(160.))
            .child(heading(t, &n.title, None))
            .children(filter)
            .child(
                div()
                    .id(SharedString::from(format!("tree-rows-{}", n.id)))
                    .flex_1()
                    .min_h_0()
                    .mx(px(-8.))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .when_some(slot.map(|s| s.scroll.clone()), |d, h| d.track_scroll(&h))
                    .children(list),
            )
            .into_any_element()
    }

    fn note_row(&self, depth: usize, words: &str, bad: bool) -> AnyElement {
        let t = self.t;
        div()
            .pl(px(8. + INDENT * depth as f32 + 18.))
            .pr(px(8.))
            .py(px(4.))
            .text_size(px(ty::SM))
            .text_color(if bad { t.bad } else { t.dim })
            .child(text(words))
            .into_any_element()
    }

    fn tree_row(
        &self,
        n: &Node,
        i: usize,
        r: &TreeRow,
        marks: (Option<&str>, Option<&str>),
    ) -> AnyElement {
        let t = self.t;
        let tree = n.id.clone();
        match r {
            TreeRow::Note {
                depth,
                text: w,
                bad,
            } => self.note_row(*depth, w, *bad),
            TreeRow::Loading { depth } => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(ROW))
                .pl(px(8. + INDENT * *depth as f32 + 18.))
                .text_size(px(ty::SM))
                .text_color(t.dim)
                .child(crate::widgets::spinner(
                    SharedString::from(format!("tree-spin-{tree}-{i}")),
                    11.,
                    t.dim,
                ))
                .child("Reading…")
                .into_any_element(),
            TreeRow::More { parent, depth } => {
                let mut d = div()
                    .id(SharedString::from(format!("tree-more-{tree}-{i}")))
                    .flex()
                    .items_center()
                    .h(px(ROW))
                    .pl(px(8. + INDENT * *depth as f32 + 18.))
                    .text_size(px(ty::SM))
                    .font_weight(ty::MEDIUM)
                    .text_color(t.accent)
                    .child("Show more");
                if let Some(e) = self.live.clone() {
                    let parent = parent.clone();
                    d = d
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .on_click(move |_, _, cx| e.update(cx, |v, cx| v.more(&tree, &parent, cx)));
                }
                d.into_any_element()
            }
            TreeRow::Node {
                id,
                label,
                depth,
                leaf,
                open,
                info,
            } => self.node_row(&tree, (id, label, info), *depth, (*leaf, *open), marks),
        }
    }

    /// A folder (its chevron turns as it opens) or a leaf (picked: the accent bar), its info on the right.
    fn node_row(
        &self,
        tree: &str,
        (id, label, info): (&str, &str, &str),
        depth: usize,
        (leaf, open): (bool, bool),
        (picked, cursor): (Option<&str>, Option<&str>),
    ) -> AnyElement {
        let t = self.t;
        let tree = tree.to_owned();
        let on = leaf && picked == Some(id);
        // The keys' row: lit like a focused table's selection (the picked leaf keeps its bar).
        let at = cursor == Some(id);
        let mut row = div()
            .id(SharedString::from(format!("tree-{tree}-{id}")))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .h(px(ROW))
            .pl(px(8. + INDENT * depth as f32))
            .pr(px(10.))
            .rounded(px(6.))
            .child(
                div()
                    .size(px(12.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(!leaf, |d| {
                        d.child(chevron(t, &format!("tree-{tree}-{id}"), open))
                    }),
            )
            .children(name_and_info(t, label, info, leaf, on))
            .when(on, |d| d.bg(t.sel).child(crate::widgets::sel_bar(t, 4.)))
            .when(at, |d| d.bg(t.pick));
        if let Some(e) = self.live.clone() {
            let id = id.to_owned();
            row = row
                .cursor_pointer()
                .when(!on && !at, |d| d.hover(|s| s.bg(t.sel)))
                .on_click(move |_, _, cx| {
                    e.update(cx, |v, cx| {
                        if leaf {
                            v.open_leaf(&tree, &id, cx);
                        } else {
                            v.toggle_folder(&tree, &id, cx);
                        }
                    });
                });
        }
        row.into_any_element()
    }
}

/// A node's name, which keeps its room (up to most of the row), and its info beside it, which gives way first.
fn name_and_info(t: &Tokens, label: &str, info: &str, leaf: bool, on: bool) -> [gpui::Div; 2] {
    let name = div()
        .flex_shrink_0()
        .max_w(relative(0.72))
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .mono_face()
        .text_size(px(ty::SM))
        .text_color(if on || !leaf { t.bright } else { t.text })
        .when(!leaf, |d| d.font_weight(ty::MEDIUM))
        .child(text(label));
    let info = div()
        .flex_1()
        .min_w_0()
        .flex()
        .justify_end()
        .when(!info.is_empty(), |d| {
            d.child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(ty::XS))
                    .font_features(ty::tabular())
                    .text_color(t.dim)
                    .child(text(info)),
            )
        });
    [name, info]
}
