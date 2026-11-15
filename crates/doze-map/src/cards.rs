//! A placed instance as a [`Card`]: its chips, its fold mark and its neighbours, the same in
//! every layout mode.

use crate::geom::Rect;
use crate::input::Flow;
use crate::output::{Card, CardId, Role};
use crate::text::{Lang, fit_chips};

/// The card for instance `v` at `rect`; `id` gives a placed instance's card.
pub(crate) fn card(
    f: &Flow<'_>,
    v: usize,
    rect: Rect,
    role: Role,
    id: &dyn Fn(usize) -> Option<CardId>,
) -> Card {
    let inst = f.inst(v);
    let kind = inst.map(|i| i.kind).unwrap_or_default();
    let ids = |l: &[usize]| l.iter().filter_map(|&x| id(x)).collect::<Vec<_>>();
    let mut c = Card {
        name: f.name(v).to_string(),
        rect,
        role,
        kind,
        lang: inst.and_then(|i| Lang::of(&i.run)),
        chips: Vec::new(),
        more: 0,
        quiet: false,
        fold: false,
        members: inst.map_or(1, |i| i.members.max(1)),
        calls: ids(f.calls(v)),
        callers: ids(f.callers(v)),
        uses: Vec::new(),
    };
    if role == Role::Shared {
        c.uses = ids(f.users(v));
        return c;
    }
    let uses: Vec<&str> = f.uses(v).iter().map(|&s| f.name(s)).collect();
    c.fold = f.folds(v);
    // Whole pixels: the cast is exact for any card the layout makes.
    #[allow(clippy::cast_possible_truncation)]
    let width = rect.w as i64;
    let (chips, more) = fit_chips(width, c.fold, &uses);
    c.chips = chips.into_iter().map(String::from).collect();
    c.more = more;
    c.quiet = uses.is_empty();
    c.uses = ids(f.uses(v));
    c
}
