//! The table itself: every command, its words, its keys, where it shows. Each row makes its action, so the keymap,
//! the menus, ⌘K and the help sheet are generated from these rows and nothing else.

use super::*;

macro_rules! spec {
    ($label:literal, $what:literal, $a:expr, [$($k:literal),*], $scope:ident, $menu:expr, $pal:literal, $built:literal) => {
        Spec {
            label: $label,
            what: $what,
            make: || Box::new($a),
            keys: &[$($k),*],
            scope: Scope::$scope,
            menu: $menu,
            palette: $pal,
            built: $built,
        }
    };
}

use MenuName::{Doze, File, Go, Help, View, Window};

/// Every command.
pub(crate) const TABLE: &[Spec] = &[
    spec!(
        "Search or do…",
        "the command palette",
        OpenPalette,
        ["cmd-k"],
        Global,
        Some(Go),
        false,
        true
    ),
    spec!(
        "Keys and words",
        "this sheet",
        ShowHelp,
        ["?", "cmd-/"],
        Global,
        Some(Help),
        false,
        true
    ),
    spec!(
        "Close",
        "the palette, the sheet, the selection",
        CloseOverlay,
        ["escape"],
        Global,
        None,
        false,
        true
    ),
    spec!(
        "Home",
        "every stack",
        GoHome,
        ["cmd-0"],
        Global,
        Some(Go),
        true,
        true
    ),
    spec!(
        "Open folder…",
        "a stack's folder",
        OpenFolder,
        ["cmd-o"],
        Global,
        Some(File),
        true,
        true
    ),
    spec!(
        "All logs",
        "the stack's merged logs",
        AllLogs,
        ["cmd-l", "shift-l"],
        Global,
        Some(Go),
        false,
        true
    ),
    spec!(
        "Activity",
        "what happened, when",
        Activity,
        ["shift-a"],
        Global,
        Some(Go),
        true,
        false
    ),
    spec!(
        "Settings…",
        "theme, density, the machine",
        Settings,
        ["cmd-,"],
        Global,
        Some(Doze),
        true,
        false
    ),
    spec!(
        "Back",
        "the page before",
        Back,
        ["cmd-["],
        Global,
        Some(Go),
        false,
        true
    ),
    spec!(
        "Forward",
        "the page after",
        Forward,
        ["cmd-]"],
        Global,
        Some(Go),
        false,
        true
    ),
    spec!(
        "Tiles",
        "the stack as tiles",
        ViewTiles,
        ["1"],
        Global,
        Some(View),
        true,
        true
    ),
    spec!(
        "List",
        "the stack as a list",
        ViewList,
        ["2"],
        Global,
        Some(View),
        true,
        true
    ),
    spec!(
        "Map",
        "who calls whom",
        ViewMap,
        ["3"],
        Global,
        Some(View),
        true,
        true
    ),
    spec!(
        "Grid",
        "services by what they use",
        ViewGrid,
        ["4"],
        Global,
        Some(View),
        true,
        true
    ),
    spec!(
        "Next colour theme",
        "violet, teal, burgundy, amber, slate",
        NextPalette,
        ["cmd-shift-t"],
        Global,
        Some(View),
        true,
        true
    ),
    spec!(
        "Dark, light or system",
        "the mode",
        CycleMode,
        ["cmd-shift-l"],
        Global,
        Some(View),
        true,
        true
    ),
    spec!(
        "Find a card",
        "jump to it",
        FindCard,
        ["/", "cmd-f"],
        Map,
        Some(View),
        true,
        true
    ),
    spec!(
        "Fit the map",
        "all of it in view",
        FitMap,
        ["0"],
        Map,
        Some(View),
        true,
        true
    ),
    spec!(
        "Zoom in",
        "closer",
        ZoomIn,
        ["=", "cmd-="],
        Map,
        Some(View),
        true,
        true
    ),
    spec!(
        "Zoom out",
        "further",
        ZoomOut,
        ["-", "cmd--"],
        Map,
        Some(View),
        true,
        true
    ),
    spec!(
        "Actual size",
        "1:1",
        ActualSize,
        ["shift-0"],
        Map,
        Some(View),
        true,
        true
    ),
    spec!(
        "Overview on or off",
        "the minimap",
        ToggleMinimap,
        ["m"],
        Map,
        Some(View),
        true,
        true
    ),
    spec!(
        "Card above",
        "the nearest",
        MoveUp,
        ["up", "k"],
        Map,
        None,
        false,
        true
    ),
    spec!(
        "Card below",
        "the nearest",
        MoveDown,
        ["down", "j"],
        Map,
        None,
        false,
        true
    ),
    spec!(
        "Card left",
        "the nearest",
        MoveLeft,
        ["left", "h"],
        Map,
        None,
        false,
        true
    ),
    spec!(
        "Card right",
        "the nearest",
        MoveRight,
        ["right", "l"],
        Map,
        None,
        false,
        true
    ),
    spec!(
        "Choose",
        "the highlighted row",
        Confirm,
        ["enter"],
        Overlay,
        None,
        false,
        true
    ),
    spec!(
        "Next row",
        "",
        MoveDown,
        ["down"],
        Overlay,
        None,
        false,
        true
    ),
    spec!("Row above", "", MoveUp, ["up"], Overlay, None, false, true),
    spec!(
        "Close the window",
        "doze stays in the menu bar",
        CloseWindow,
        ["cmd-w"],
        Global,
        Some(Window),
        false,
        true
    ),
    spec!(
        "Quit doze",
        "stacks keep running",
        Quit,
        ["cmd-q"],
        Global,
        Some(Doze),
        false,
        true
    ),
];
