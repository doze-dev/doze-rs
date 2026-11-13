//! The Map's two focus views, each a layout made from the stack's (so hover, selection, find, the detail card and the
//! camera work in it as on the whole map): a card's neighbourhood (`H` on a selected card: it and one to three calls
//! either way, in columns by path; `H` again steps the hops, `H` on another card centres there) and only trouble (`T`:
//! the cards in trouble and the shortest paths between them, laid out on their own). `esc` returns to the whole map
//! where the camera was.

use std::sync::Arc;

use doze_map::{Layout, MAX_HOPS, Options};
use gpui::Context;

use super::MapView;

/// A focus view.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Lens {
    /// A card and what it reaches in `hops` calls.
    Hood { centre: String, hops: u8 },
    /// The cards in trouble (`troubled` of them) and the `between` cards that join them; `hidden` left out.
    Trouble {
        troubled: usize,
        between: usize,
        hidden: usize,
    },
}

impl MapView {
    /// The focus view's layout, from the stack's; `None` when there is nothing to show.
    fn lens_layout(&self, lens: &Lens) -> Option<(Layout, Lens)> {
        let base = &self.base;
        match lens {
            Lens::Hood { centre, hops } => {
                let id = base.card_named(centre)?;
                let l = base.neighbourhood(id, *hops)?;
                Some((l, lens.clone()))
            }
            Lens::Trouble { .. } => {
                let bad: Vec<bool> = base
                    .cards
                    .iter()
                    .map(|c| {
                        let mut names = std::iter::once(&c.name)
                            .chain(base.groups.get(&c.name).into_iter().flatten());
                        names.any(|n| self.rows.get(n).is_some_and(|r| r.trouble))
                    })
                    .collect();
                let ids = base.trouble(&bad);
                let troubled = bad.iter().filter(|b| **b).count();
                let sub = doze_map::subset(&self.ins, &base.names(&ids));
                let l = doze_map::layout(&sub, &Options::default());
                let lens = Lens::Trouble {
                    troubled,
                    between: ids.len().saturating_sub(troubled),
                    hidden: base.cards.len().saturating_sub(ids.len()),
                };
                Some((l, lens))
            }
        }
    }

    /// Opens a focus view (or moves to another), keeping the whole map's camera to come back to.
    pub(crate) fn enter(&mut self, lens: Lens, cx: &mut Context<Self>) {
        let Some((l, lens)) = self.lens_layout(&lens) else {
            return;
        };
        if self.lens.is_none() {
            self.saved = Some(self.cam);
        }
        self.list = None;
        if let Lens::Hood { centre, .. } = &lens {
            self.show(Arc::new(l));
            self.lit = self.layout.card_named(centre);
        } else {
            self.show(Arc::new(l));
        }
        self.lens = Some(lens);
        self.cam.fitted = false;
        cx.notify();
    }

    /// The focus view made again from a new stack layout or new states; closed when nothing is left to show.
    pub(crate) fn relens(&mut self) {
        let Some(lens) = self.lens.clone() else {
            return;
        };
        match self.lens_layout(&lens) {
            Some((l, lens)) => {
                self.show(Arc::new(l));
                self.lens = Some(lens);
            }
            None => {
                self.lens = None;
                self.show(self.base.clone());
            }
        }
    }

    /// Back to the whole map, the camera where it was; says whether a focus view was open.
    pub(crate) fn leave(&mut self, cx: &mut Context<Self>) -> bool {
        if self.lens.take().is_none() {
            return false;
        }
        self.show(self.base.clone());
        if let Some(c) = self.saved.take() {
            self.cam = Camera::keep_size(c, self.cam.size);
        }
        cx.notify();
        true
    }

    /// `H`: a selected card's neighbourhood at two hops; in one, the selection becomes the centre, or the same
    /// centre steps through one, two and three hops.
    pub(crate) fn on_hood(&mut self, cx: &mut Context<Self>) {
        let lit = self
            .lit
            .and_then(|c| self.layout.card(c))
            .map(|c| c.name.clone())
            .filter(|_| !self.lit.is_some_and(|c| self.layout.folded(c)));
        let next = match (&self.lens, lit) {
            (Some(Lens::Hood { centre, hops }), Some(n)) if *centre == n => Lens::Hood {
                centre: n,
                hops: hops % MAX_HOPS + 1,
            },
            (Some(Lens::Hood { hops, .. }), Some(n)) => Lens::Hood {
                centre: n,
                hops: *hops,
            },
            (_, Some(n)) => Lens::Hood { centre: n, hops: 2 },
            (_, None) => return,
        };
        self.enter(next, cx);
    }

    /// Sets a neighbourhood's hops (the toolbar's 1 · 2 · 3).
    pub(crate) fn set_hops(&mut self, hops: u8, cx: &mut Context<Self>) {
        if let Some(Lens::Hood { centre, .. }) = self.lens.clone() {
            self.enter(Lens::Hood { centre, hops }, cx);
        }
    }

    /// `T`: only trouble, or back.
    pub(crate) fn on_trouble(&mut self, cx: &mut Context<Self>) {
        if matches!(self.lens, Some(Lens::Trouble { .. })) {
            self.leave(cx);
        } else {
            self.enter(
                Lens::Trouble {
                    troubled: 0,
                    between: 0,
                    hidden: 0,
                },
                cx,
            );
        }
    }
}

use super::Camera;

impl Camera {
    /// A kept camera, in today's viewport.
    fn keep_size(c: Camera, size: (f32, f32)) -> Camera {
        Camera {
            size,
            flight: None,
            ..c
        }
    }
}
