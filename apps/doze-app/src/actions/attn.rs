//! The rows of ⌘K, Needs you, the tabs and the way back (M2 steps 2.3, 2.4, 2.7): Go's keys for them (PLAN.md §2.10),
//! and the intents ⌘K and Needs you hand to the verbs (`Verb`, `StackVerb`), whose handlers are the verbs' step.

use gpui::{Action, actions};
use serde::Deserialize;

use super::{MenuName, Scope, Spec};

actions!(
    doze,
    [
        /// Needs you: what is broken or worth doing.
        OpenBell,
        /// ⌘K: into the highlighted stack or service.
        ScopeIn,
        /// ⌘K: one step out (a service → its stack → everything).
        ScopeOut,
        /// ⌘K: ⌫ on an empty box steps out; otherwise it is the box's.
        BackOut,
        /// ⌘K: the highlighted row's first action.
        FirstAct,
        /// Needs you: the highlighted item's first fix.
        BellFix,
        /// Needs you: the page that explains the highlighted item.
        BellShow,
        /// Needs you: put the highlighted item aside until what it says changes.
        BellNotNow,
        /// Logs of the selected row.
        RowLogs,
        /// About doze: the keys sheet, with the version.
        AboutDoze,
        /// The docs, in the browser.
        OpenDocs,
        /// Settings › About, asking for a new version now (M3).
        CheckUpdates,
        HideDoze,
        HideOthers,
        ShowAll,
        Minimize,
        Zoom,
        FullScreen,
    ]
);

/// Goes to the stack tab `n` (1–9), as ⌘1–⌘9 do.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Action)]
#[action(namespace = doze, no_json)]
pub(crate) struct GoTab {
    pub(crate) n: usize,
}

/// A verb on a service (`target`), or on the whole stack when `target` is empty: `sleep`, `wake`, `restart`, `pin`,
/// `off`, `on`, `reset`, `task`. `ask` says it asks first. Sent by ⌘K and Needs you; done by the verbs' step.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Action)]
#[action(namespace = doze, no_json)]
pub(crate) struct Verb {
    pub(crate) stack: String,
    pub(crate) verb: String,
    pub(crate) target: String,
    pub(crate) ask: bool,
}

/// A stack action: `up-last`, `up-all`, `restart`, `down`, `remove`, `choose` (the start sheet), `dismiss` (a failed
/// start put away), `holder:PID` (stop what holds a port, then start `target`). Done by the verbs' step.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Action)]
#[action(namespace = doze, no_json)]
pub(crate) struct StackVerb {
    pub(crate) stack: String,
    pub(crate) verb: String,
    pub(crate) target: String,
}

macro_rules! row {
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

use MenuName::{Doze, Go, Help, View, Window};

/// The rows.
#[rustfmt::skip]
pub(crate) const ATTN: &[Spec] = &[
    row!("Needs you", "what is broken or worth doing", OpenBell, ["cmd-shift-n"], Global, Some(Go), false, true),
    row!("Tab 1", "a stack's tab", GoTab { n: 1 }, ["cmd-1"], Global, Some(Go), false, true),
    row!("Tab 2", "a stack's tab", GoTab { n: 2 }, ["cmd-2"], Global, Some(Go), false, true),
    row!("Tab 3", "a stack's tab", GoTab { n: 3 }, ["cmd-3"], Global, Some(Go), false, true),
    row!("Tab 4", "a stack's tab", GoTab { n: 4 }, ["cmd-4"], Global, Some(Go), false, true),
    row!("Tab 5", "a stack's tab", GoTab { n: 5 }, ["cmd-5"], Global, Some(Go), false, true),
    row!("Tab 6", "a stack's tab", GoTab { n: 6 }, ["cmd-6"], Global, Some(Go), false, true),
    row!("Tab 7", "a stack's tab", GoTab { n: 7 }, ["cmd-7"], Global, Some(Go), false, true),
    row!("Tab 8", "a stack's tab", GoTab { n: 8 }, ["cmd-8"], Global, Some(Go), false, true),
    row!("Tab 9", "a stack's tab", GoTab { n: 9 }, ["cmd-9"], Global, Some(Go), false, true),
    row!("Its logs", "the selected service's", RowLogs, ["l"], Overview, None, false, true),
    row!("Its logs", "the row's service's", RowLogs, ["l"], Grid, None, false, true),
    row!("Into it", "a stack's or a service's commands", ScopeIn, ["tab"], Palette, None, false, true),
    row!("Step out", "a service → its stack → everything", ScopeOut, ["shift-tab"], Palette, None, false, true),
    row!("Step out", "a service → its stack → everything", BackOut, ["backspace"], Palette, None, false, true),
    row!("Its first action", "copy, open, sleep…", FirstAct, ["cmd-enter"], Palette, None, false, true),
    row!("Next item", "move", super::MoveDown, ["down", "j"], Bell, None, false, true),
    row!("Item above", "move", super::MoveUp, ["up", "k"], Bell, None, false, true),
    row!("First fix", "the highlighted item's", BellFix, ["enter"], Bell, None, false, true),
    row!("Show", "the page that explains it", BellShow, ["cmd-enter"], Bell, None, false, true),
    row!("Not now", "until what it says changes", BellNotNow, ["n"], Bell, None, false, true),
    row!("About doze", "the version, the keys", AboutDoze, [], Global, Some(Doze), false, true),
    row!("Check for Updates…", "a newer doze", CheckUpdates, [], Global, Some(Doze), true, true),
    row!("Hide doze", "", HideDoze, ["cmd-h"], Global, Some(Doze), false, true),
    row!("Hide Others", "", HideOthers, ["alt-cmd-h"], Global, Some(Doze), false, true),
    row!("Show All", "", ShowAll, [], Global, Some(Doze), false, true),
    row!("Minimize", "", Minimize, ["cmd-m"], Global, Some(Window), false, true),
    row!("Zoom", "", Zoom, [], Global, Some(Window), false, true),
    row!("Enter Full Screen", "", FullScreen, ["ctrl-cmd-f"], Global, Some(View), false, true),
    row!("doze Docs", "in the browser", OpenDocs, [], Global, Some(Help), true, true),
];
