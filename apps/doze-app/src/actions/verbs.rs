//! The rows of acting (M2 steps 2.1, 2.2, 2.5, 2.6): a stack page's verbs on the selected service, a dialog's keys,
//! the start sheet's and the checks' sheet's, the empty folder's, New stack, Recent, and the stack actions the
//! palette and the menus offer. Rows of the one table (see the module doc), in their own file.

use super::{MenuName, Scope, Spec};
use gpui::actions;

actions!(
    doze,
    [
        /// `y`: a restart's yes.
        DialogYes,
        /// `n`: a restart's no.
        DialogNo,
        /// Tick or untick the line under the keys.
        ToggleTick,
        /// Start everything (the start sheet's Everything).
        StartEverything,
        /// Start anyway (the checks' sheet).
        GoAnyway,
        /// A new stack: pick a folder, then set it up.
        NewStack,
        /// What was said lately.
        ShowRecent,
        /// Restart the stack shown.
        RestartStack,
        /// Stop the stack shown.
        StopStack,
        /// Remove the stack shown from doze.
        RemoveShown,
        /// The empty folder's three ways.
        Way1,
        Way2,
        Way3,
        /// Put a failed stack action away.
        DismissFailure,
        /// Try a failed start again.
        TryAgain,
        /// Edit the stack's doze.hcl.
        EditConfig,
    ]
);

macro_rules! row {
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

use super::{
    CloseOverlay, Confirm, MoveDown, MoveLeft, MoveRight, MoveUp, PinIt, RestartIt, SleepIt, WakeIt,
};
use MenuName::{File, Go};

/// The rows.
#[rustfmt::skip]
pub(crate) const VERBS: &[Spec] = &[
    row!("New stack…", "pick a folder; doze writes its doze.hcl", NewStack, ["cmd-n"], Global, Some(File), true),
    row!("Recent messages", "the last twenty things said", ShowRecent, ["cmd-shift-m"], Global, Some(Go), true),
    row!("Restart the stack", "down, then up as last time", RestartStack, [], Global, Some(File), false),
    row!("Stop the stack", "every service stops; data stays", StopStack, [], Global, Some(File), false),
    row!("Remove the stack from doze…", "it stops; every file stays", RemoveShown, [], Global, Some(File), false),
    row!("Edit doze.hcl", "in your editor", EditConfig, ["cmd-e"], Global, Some(File), false),
    row!("Wake", "the service selected", WakeIt, ["w"], Overview, None, false),
    row!("Sleep", "the service selected (asks when in use)", SleepIt, ["s"], Overview, None, false),
    row!("Restart", "the selected one; a task runs", RestartIt, ["r"], Overview, None, false),
    row!("Pin awake", "the selected one stays up", PinIt, ["p"], Overview, None, false),
    row!("Try again", "a start that failed", TryAgain, [], Overview, None, true),
    row!("Dismiss the failure", "put it away", DismissFailure, [], Overview, None, true),
    row!("Next button", "", MoveRight, ["right", "tab"], Dialog, None, false),
    row!("Button before", "", MoveLeft, ["left", "shift-tab"], Dialog, None, false),
    row!("Press it", "the highlighted button", Confirm, ["enter"], Dialog, None, false),
    row!("Yes", "a restart", DialogYes, ["y"], Dialog, None, false),
    row!("No", "a restart", DialogNo, ["n"], Dialog, None, false),
    row!("Cancel", "nothing is done", CloseOverlay, ["escape"], Dialog, None, false),
    row!("Line below", "", MoveDown, ["down", "j"], Sheet, None, false),
    row!("Line above", "", MoveUp, ["up", "k"], Sheet, None, false),
    row!("Tick", "the line", ToggleTick, ["space"], Sheet, None, false),
    row!("Start", "these, or the first way out", Confirm, ["enter"], Sheet, None, false),
    row!("Everything", "start every one", StartEverything, ["e"], Sheet, None, false),
    row!("Anyway", "start as asked", GoAnyway, ["a"], Sheet, None, false),
    row!("Next way", "", MoveRight, ["right", "down"], Empty, None, false),
    row!("Way before", "", MoveLeft, ["left", "up"], Empty, None, false),
    row!("Choose it", "write doze.hcl that way", Confirm, ["enter"], Empty, None, false),
    row!("Set it up from this repo", "doze init", Way1, ["1"], Empty, None, false),
    row!("Add one thing", "doze add", Way2, ["2"], Empty, None, false),
    row!("Start with an empty file", "a starter doze.hcl", Way3, ["3"], Empty, None, false),
];
