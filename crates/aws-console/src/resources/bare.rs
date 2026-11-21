//! [`Kind::BARE`]: a kind with nothing in it, for a descriptor to write `..Kind::BARE` after the members it sets (the console's
//! newer members, [`Kind::ops`] and the rest, are then written only by the kinds that have them).

use super::{Id, Kind, List, Page, Step, Text};

impl Kind {
    /// Every member empty. A descriptor sets `id`, `title` and the rest of what it has, then `..Kind::BARE`.
    pub const BARE: Self = Self {
        id: "",
        title: "",
        one: "",
        about: "",
        members: &[],
        list: List {
            step: Step::new("", "{}"),
            items: "",
            item_as: "",
            token_out: "",
            id: "",
            name: Text::at(""),
            label: None,
            split: None,
        },
        elsewhere: "",
        each: &[],
        columns: &[],
        id_of: Id::Name,
        detail: &[],
        fields: &[],
        subs: &[],
        create: None,
        delete: None,
        reveal: None,
        deep: &[],
        ops: &[],
        tags: None,
        blockers: &[],
        kept: &[],
        confirm: "",
        params: &[],
        page: Page::NONE,
    };
}
