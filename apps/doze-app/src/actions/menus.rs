//! The native menu bar (Go `app/menu.go`, completed): doze, File, Edit, Go, View, Window, Help, each item a row of the
//! action table, so its accelerator is the key the keymap binds. The layout (order, separators, the system's own
//! items) is here; which rows exist, their words and keys are the table's. A row given a menu but not placed below
//! still shows, at the end of its menu, so no command can be left out of the bar.

use gpui::{Menu, MenuItem, OsAction, SystemMenuType};
use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};

use super::{MenuName, Spec, all};

/// One place in a menu.
enum Place {
    /// The table's row with this label.
    Row(&'static str),
    Sep,
    /// macOS's Services submenu.
    Services,
}

use Place::{Row, Sep, Services};

/// The bar, menu by menu.
const BAR: [(MenuName, &str, &[Place]); 6] = [
    (
        MenuName::Doze,
        "doze",
        &[
            Row("About doze"),
            Row("Check for Updates…"),
            Sep,
            Row("Settings…"),
            Sep,
            Services,
            Sep,
            Row("Hide doze"),
            Row("Hide Others"),
            Row("Show All"),
            Sep,
            Row("Quit doze"),
        ],
    ),
    (
        MenuName::File,
        "File",
        &[
            Row("Open folder…"),
            Row("Edit doze.hcl"),
            Sep,
            Row("Close the window"),
        ],
    ),
    (
        MenuName::Go,
        "Go",
        &[
            Row("Home"),
            Row("Back"),
            Row("Forward"),
            Sep,
            Row("All logs"),
            Row("Search or do…"),
            Row("Needs you"),
            Row("Activity"),
            Sep,
            Row("Tab 1"),
            Row("Tab 2"),
            Row("Tab 3"),
            Row("Tab 4"),
            Row("Tab 5"),
            Row("Tab 6"),
            Row("Tab 7"),
            Row("Tab 8"),
            Row("Tab 9"),
        ],
    ),
    (
        MenuName::View,
        "View",
        &[
            Row("Tiles"),
            Row("List"),
            Row("Map"),
            Row("Grid"),
            Sep,
            Row("Find a card"),
            Row("Fit the map"),
            Row("Zoom in"),
            Row("Zoom out"),
            Row("Actual size"),
            Row("Overview on or off"),
            Sep,
            Row("Next colour theme"),
            Row("Dark, light or system"),
            Row("Row density"),
            Sep,
            Row("Enter Full Screen"),
        ],
    ),
    (
        MenuName::Window,
        "Window",
        &[Row("Minimize"), Row("Zoom"), Sep, Row("Close the window")],
    ),
    (
        MenuName::Help,
        "Help",
        &[Row("Keys and words"), Row("doze Docs")],
    ),
];

fn item(s: &Spec) -> MenuItem {
    MenuItem::Action {
        name: s.label.into(),
        action: (s.make)(),
        os_action: None,
        checked: false,
        disabled: !s.built,
    }
}

/// The Edit menu's text commands, which the system routes to the focused box.
fn edit() -> Vec<MenuItem> {
    vec![
        MenuItem::os_action("Undo", Undo, OsAction::Undo),
        MenuItem::os_action("Redo", Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action("Cut", Cut, OsAction::Cut),
        MenuItem::os_action("Copy", Copy, OsAction::Copy),
        MenuItem::os_action("Paste", Paste, OsAction::Paste),
        MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
    ]
}

/// The menu bar.
pub(super) fn menus() -> Vec<Menu> {
    let mut out = Vec::new();
    for (m, name, places) in &BAR {
        let mut items = Vec::new();
        let mut placed: Vec<&str> = Vec::new();
        for p in *places {
            match p {
                Row(label) => {
                    // The Window menu's Close repeats File's; any other label is placed once.
                    if let Some(s) = all().find(|s| s.label == *label) {
                        items.push(item(s));
                        placed.push(label);
                    }
                }
                Sep => items.push(MenuItem::separator()),
                Services => items.push(MenuItem::os_submenu("Services", SystemMenuType::Services)),
            }
        }
        // A row given this menu that the layout does not place: at the end, so nothing is left out.
        let rest: Vec<&Spec> = all()
            .filter(|s| s.menu == Some(*m) && !placed.contains(&s.label))
            .collect();
        if !rest.is_empty() {
            items.push(MenuItem::separator());
            items.extend(rest.into_iter().map(item));
        }
        out.push(Menu::new(*name).items(items));
        if *m == MenuName::File {
            out.push(Menu::new("Edit").items(edit()));
        }
    }
    out
}

/// The labels the layout places, for the test that every menu row is in the bar.
#[cfg(test)]
pub(super) fn placed() -> Vec<&'static str> {
    BAR.iter()
        .flat_map(|(_, _, places)| places.iter())
        .filter_map(|p| match p {
            Row(l) => Some(*l),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{menus, placed};
    use crate::actions::all;

    #[test]
    fn every_global_command_with_a_key_or_a_menu_is_in_the_bar() {
        let names: Vec<String> = menus().iter().map(|m| m.name.to_string()).collect();
        assert_eq!(
            names,
            ["doze", "File", "Edit", "Go", "View", "Window", "Help"]
        );
        let placed = placed();
        for s in all().filter(|s| s.menu.is_some()) {
            assert!(
                placed.contains(&s.label),
                "{} has a menu but no place in the bar",
                s.label
            );
        }
        for l in &placed {
            assert!(all().any(|s| s.label == *l), "{l} names no row");
        }
    }
}
