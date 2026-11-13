//! The one action table (docs/port/app/PLAN.md §3 item 9). Every command is a gpui action, and this table generates
//! the keymap, the app menus, the ⌘K rows, the help sheet and the key chips, so no chip can name a key nothing binds
//! and no command lives in only one place.
//!
//! A row the app does not do yet (`built: false`) is still listed, so the menus and help show the whole app; its menu
//! item is disabled and its palette row says "soon".

use gpui::{
    Action, App, KeyBinding, KeyBindingContextPredicate, Keystroke, Menu, MenuItem, actions,
};
use gpui_component::kbd::Kbd;
use std::rc::Rc;

actions!(
    doze,
    [
        OpenPalette,
        CloseOverlay,
        ShowHelp,
        GoHome,
        OpenFolder,
        AllLogs,
        Activity,
        Settings,
        Back,
        Forward,
        ViewTiles,
        ViewList,
        ViewMap,
        ViewGrid,
        FindCard,
        FitMap,
        ZoomIn,
        ZoomOut,
        ActualSize,
        ToggleMinimap,
        MoveUp,
        MoveDown,
        MoveLeft,
        MoveRight,
        Confirm,
        NextPalette,
        CycleMode,
        CloseWindow,
        Quit,
        OpenStack,
        ToggleStack,
        RemoveStack,
        OpenService,
        ToggleService,
        FilterRows,
        CopyAddress,
        CycleDensity,
        UpAsLastTime,
        ChooseStart,
        OpenRowService,
    ]
);

/// Where a row shows in the help sheet, and the key context it binds in (`None`: everywhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    Global,
    Map,
    Overlay,
    /// The Library's table.
    Library,
    /// A stack's page: its tiles and its list.
    Overview,
    /// The Grid view.
    Grid,
    /// An instance's page.
    Instance,
    /// A logs page, or the logs tab.
    Logs,
    /// A module's console tab.
    Console,
}

impl Scope {
    /// The help sheet's heading.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Scope::Global => "Everywhere",
            Scope::Map => "On the Map",
            Scope::Overlay => "In a search box",
            Scope::Library => "In the Library",
            Scope::Overview => "On a stack's page",
            Scope::Grid => "In the Grid",
            Scope::Instance => "On a service's page",
            Scope::Logs => "In the logs",
            Scope::Console => "In a module's console",
        }
    }

    /// The key context a key of this scope binds in. A bare key (`?`, `1`) binds only on the page, so typing it into
    /// a search box types it; keys with a modifier, and escape, work everywhere. The map's keys bind on the map, whose
    /// search box sits outside it; the search boxes' own keys bind in gpui-component's `Input`.
    fn context(self, key: &str) -> Option<&'static str> {
        let bare = !["cmd-", "ctrl-", "alt-"].iter().any(|m| key.contains(m)) && key != "escape";
        match self {
            Scope::Global if bare => Some("Page && !Input"),
            Scope::Global => None,
            Scope::Map => Some("Map"),
            Scope::Overlay => Some("Input"),
            Scope::Library => Some("Library"),
            Scope::Overview => Some("Overview"),
            Scope::Grid => Some("Grid"),
            Scope::Instance => Some("Instance && !Input"),
            Scope::Logs => Some("Logs && !Input"),
            Scope::Console => Some("ConsoleTab && !Input"),
        }
    }
}

/// Which app menu a row is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuName {
    Doze,
    File,
    Go,
    View,
    Window,
    Help,
}

const MENUS: [(MenuName, &str); 6] = [
    (MenuName::Doze, "doze"),
    (MenuName::File, "File"),
    (MenuName::Go, "Go"),
    (MenuName::View, "View"),
    (MenuName::Window, "Window"),
    (MenuName::Help, "Help"),
];

/// One command.
#[derive(Clone, Copy)]
pub(crate) struct Spec {
    pub(crate) label: &'static str,
    /// What the help sheet and the palette say beside it.
    pub(crate) what: &'static str,
    pub(crate) make: fn() -> Box<dyn Action>,
    /// Its keys, in gpui's keystroke syntax (`cmd-k`); the first is the one chips show.
    pub(crate) keys: &'static [&'static str],
    pub(crate) scope: Scope,
    pub(crate) menu: Option<MenuName>,
    /// Offered in ⌘K.
    pub(crate) palette: bool,
    /// The app does it today.
    pub(crate) built: bool,
}

mod inside;
mod pages;
mod table;

pub(crate) use inside::*;
pub(crate) use table::TABLE;

/// Every row: the shell's and the Map's ([`TABLE`]), the stack pages' ([`pages::PAGES`]), then the pages' inside a
/// stack ([`INSIDE`]).
pub(crate) fn all() -> impl Iterator<Item = &'static Spec> {
    TABLE.iter().chain(pages::PAGES).chain(INSIDE)
}

/// The keymap the table gives. A keystroke that does not parse is left out (the test below proves none does).
pub(crate) fn keymap(cx: &App) -> Vec<KeyBinding> {
    let mut out = Vec::new();
    for s in all() {
        for k in s.keys {
            let ctx = s
                .scope
                .context(k)
                .and_then(|c| KeyBindingContextPredicate::parse(c).ok())
                .map(Rc::new);
            if let Ok(b) = KeyBinding::load(k, (s.make)(), ctx, false, None, cx) {
                out.push(b);
            }
        }
    }
    out
}

/// The app menus.
pub(crate) fn menus() -> Vec<Menu> {
    MENUS
        .iter()
        .map(|&(m, name)| {
            let items = all()
                .filter(|s| s.menu == Some(m))
                .map(|s| MenuItem::Action {
                    name: s.label.into(),
                    action: (s.make)(),
                    os_action: None,
                    checked: false,
                    disabled: !s.built,
                });
            Menu::new(name).items(items)
        })
        .collect()
}

/// The first key of a row, as a chip shows it (`⌘K`), or `None` for a row with no key.
pub(crate) fn chip(s: &Spec) -> Option<String> {
    s.keys.first().map(|k| key_words(k))
}

/// A key as a chip writes it: the platform's symbols (⌘K, ⇧A), with the words the Go app used for the keys whose
/// symbols read poorly at chip size (esc, space).
pub(crate) fn key_words(k: &'static str) -> String {
    match k {
        "escape" => "esc".into(),
        "space" => "space".into(),
        "tab" => "tab".into(),
        // A letter is the letter, as Go's chips wrote it (`y`, `o`; an `l` chip is not ⇧L); shifted, its capital (`A`).
        k if k.len() == 1 && k.bytes().all(|b| b.is_ascii_lowercase()) => k.into(),
        k if k
            .strip_prefix("shift-")
            .is_some_and(|l| l.len() == 1 && l.bytes().all(|b| b.is_ascii_lowercase())) =>
        {
            k.trim_start_matches("shift-").to_uppercase()
        }
        _ => match Keystroke::parse(k) {
            Ok(stroke) => Kbd::format(&stroke),
            Err(_) => k.to_owned(),
        },
    }
}

/// The rows ⌘K offers.
pub(crate) fn palette() -> impl Iterator<Item = &'static Spec> {
    all().filter(|s| s.palette)
}

/// The help sheet: each scope's rows with keys, in table order.
pub(crate) fn help() -> Vec<(Scope, Vec<&'static Spec>)> {
    [
        Scope::Global,
        Scope::Library,
        Scope::Overview,
        Scope::Grid,
        Scope::Map,
        Scope::Instance,
        Scope::Logs,
        Scope::Console,
        Scope::Overlay,
    ]
    .into_iter()
    .map(|sc| {
        (
            sc,
            all()
                .filter(|s| s.scope == sc && !s.keys.is_empty() && !s.what.is_empty())
                .collect(),
        )
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{MENUS, TABLE, all, chip, help, palette};
    use gpui::Keystroke;

    #[test]
    fn every_key_parses_and_every_row_has_a_way_in() {
        for s in all() {
            for k in s.keys {
                assert!(Keystroke::parse(k).is_ok(), "{}: {k}", s.label);
            }
            assert!(
                s.menu.is_some() || s.palette || !s.keys.is_empty(),
                "{} has no menu, no palette row and no key",
                s.label
            );
        }
    }

    #[test]
    fn chips_name_bound_keys() {
        for s in palette() {
            if let Some(c) = chip(s) {
                assert!(!c.is_empty(), "{}", s.label);
            }
        }
        let cmdk = TABLE
            .iter()
            .find(|s| s.label.starts_with("Search"))
            .and_then(chip);
        assert_eq!(cmdk.as_deref(), Some("⌘K"));
    }

    #[test]
    fn every_key_context_parses() {
        // A context that did not parse would bind its key everywhere, typing included.
        for s in super::all() {
            for k in s.keys {
                if let Some(c) = s.scope.context(k) {
                    assert!(
                        gpui::KeyBindingContextPredicate::parse(c).is_ok(),
                        "{}: {c}",
                        s.label
                    );
                }
            }
        }
    }

    #[test]
    fn help_and_menus_cover_the_table() {
        let listed: usize = help().iter().map(|(_, rows)| rows.len()).sum();
        assert!(listed > 20);
        assert!(
            TABLE
                .iter()
                .filter_map(|s| s.menu)
                .all(|m| MENUS.iter().any(|(n, _)| *n == m))
        );
    }
}
