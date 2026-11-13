//! The commands inside a stack: an instance's page, its logs, a module's console. Rows of the one table (see the
//! module doc), kept in their own file so the pages that own them change them in one place.

use super::{Scope, Spec};
use gpui::actions;

actions!(
    doze,
    [
        /// Try now when crashing; restart when awake (which asks first); run a task.
        RestartIt,
        /// Stop trying when crashing; sleep when awake (which asks first).
        SleepIt,
        WakeIt,
        PinIt,
        OpenAddress,
        CopyAddress,
        ShowLogs,
        RunTask,
        NextTab,
        PrevTab,
        FocusFilter,
        LevelDebug,
        LevelInfo,
        LevelWarn,
        LevelError,
        CopyShown,
        ToggleClock,
        ToggleFollow,
        OlderRun,
        NewerRun,
        ToggleFind,
        NextHit,
        PrevHit,
        NextTable,
        PrevTable,
        RowDown,
        RowUp,
        LogsTop,
        LogsEnd,
    ]
);

macro_rules! spec {
    ($label:literal, $what:literal, $a:expr, [$($k:literal),*], $scope:ident, $menu:expr, $pal:literal) => {
        Spec {
            label: $label,
            what: $what,
            make: || Box::new($a),
            keys: &[$($k),*],
            scope: Scope::$scope,
            menu: $menu,
            palette: $pal,
            built: true,
        }
    };
}

/// The rows.
pub(crate) const INSIDE: &[Spec] = &[
    spec!(
        "Open the card",
        "its page",
        super::Confirm,
        ["enter"],
        Map,
        None,
        false
    ),
    spec!(
        "Restart / try now",
        "a crashing one now, else restart; a task runs",
        RestartIt,
        ["r"],
        Instance,
        None,
        false
    ),
    spec!(
        "Sleep / stop trying",
        "sleep it, or stop trying a crashing one",
        SleepIt,
        ["s"],
        Instance,
        None,
        false
    ),
    spec!("Wake", "start it now", WakeIt, ["w"], Instance, None, false),
    spec!(
        "Pin awake",
        "keep it up past the idle time",
        PinIt,
        ["p"],
        Instance,
        None,
        false
    ),
    spec!(
        "Open in browser",
        "its address",
        OpenAddress,
        ["o"],
        Instance,
        None,
        false
    ),
    spec!(
        "Copy address",
        "the URL or name and port",
        CopyAddress,
        ["y"],
        Instance,
        None,
        false
    ),
    spec!(
        "Logs",
        "this service's logs",
        ShowLogs,
        ["l"],
        Instance,
        None,
        false
    ),
    spec!(
        "Run the task",
        "now, as doze task does",
        RunTask,
        ["enter"],
        Instance,
        None,
        false
    ),
    spec!(
        "Next tab",
        "overview, logs, console",
        NextTab,
        ["tab"],
        Instance,
        None,
        false
    ),
    spec!(
        "Tab before",
        "",
        PrevTab,
        ["shift-tab"],
        Instance,
        None,
        false
    ),
    spec!(
        "Filter the lines",
        "words, or /a pattern/",
        FocusFilter,
        ["/"],
        Logs,
        None,
        false
    ),
    spec!(
        "Debug and up",
        "every line",
        LevelDebug,
        ["1"],
        Logs,
        None,
        false
    ),
    spec!(
        "Info and up",
        "the default",
        LevelInfo,
        ["2"],
        Logs,
        None,
        false
    ),
    spec!(
        "Warnings and up",
        "warnings and errors",
        LevelWarn,
        ["3"],
        Logs,
        None,
        false
    ),
    spec!(
        "Errors only",
        "errors only",
        LevelError,
        ["4"],
        Logs,
        None,
        false
    ),
    spec!(
        "Copy the lines",
        "as shown, with their times",
        CopyShown,
        ["c"],
        Logs,
        None,
        false
    ),
    spec!(
        "Relative or clock",
        "how times read",
        ToggleClock,
        ["t"],
        Logs,
        None,
        false
    ),
    spec!(
        "Pause or follow",
        "new lines at the end",
        ToggleFollow,
        ["space"],
        Logs,
        None,
        false
    ),
    spec!(
        "Run before",
        "step back a run",
        OlderRun,
        ["["],
        Logs,
        None,
        false
    ),
    spec!(
        "Run after",
        "step forward a run",
        NewerRun,
        ["]"],
        Logs,
        None,
        false
    ),
    spec!(
        "Find or filter",
        "keep every line and step through the matches",
        ToggleFind,
        ["f"],
        Logs,
        None,
        false
    ),
    spec!("Next match", "", NextHit, ["n"], Logs, None, false),
    spec!("Match before", "", PrevHit, ["shift-n"], Logs, None, false),
    spec!(
        "To the top",
        "the oldest line kept",
        LogsTop,
        ["g", "home"],
        Logs,
        None,
        false
    ),
    spec!(
        "To the end",
        "and follow",
        LogsEnd,
        ["shift-g", "end"],
        Logs,
        None,
        false
    ),
    spec!("Next table", "", NextTable, ["]"], Console, None, false),
    spec!("Table before", "", PrevTable, ["["], Console, None, false),
    spec!(
        "Row below",
        "select",
        RowDown,
        ["down", "j"],
        Console,
        None,
        false
    ),
    spec!(
        "Row above",
        "select",
        RowUp,
        ["up", "k"],
        Console,
        None,
        false
    ),
];
