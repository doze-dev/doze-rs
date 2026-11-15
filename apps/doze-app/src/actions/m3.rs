//! The rows of Activity and Settings (M3 steps 3.3 and 3.4): Go's keys for them (PLAN.md §2.10), and the keys the
//! Rust pages add (a span shorter or longer, a service picked from the keyboard, the report copied).

use gpui::{Action, actions};
use serde::Deserialize;

use super::{Scope, Spec};

actions!(
    doze,
    [
        /// Activity: the span before (a day → an hour).
        SpanShorter,
        /// Activity: the span after (a day → a week).
        SpanLonger,
        /// Settings › Machine: the report, with the fixes, on the clipboard.
        CopyReport,
        /// Settings › Machine's Repair, from anywhere.
        RepairMachine,
        /// Settings › Machine.
        MachineSettings,
    ]
);

/// Settings, at one of its parts: `general`, `machine`, `about`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Action)]
#[action(namespace = doze, no_json)]
pub(crate) struct SettingsAt {
    pub(crate) section: String,
}

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

/// The rows.
#[rustfmt::skip]
pub(crate) const M3: &[Spec] = &[
    row!("Shorter span", "an hour, a day, a week", SpanShorter, ["["], Activity, None, false),
    row!("Longer span", "an hour, a day, a week", SpanLonger, ["]"], Activity, None, false),
    row!("Service above", "move", super::MoveUp, ["up", "k"], Activity, None, false),
    row!("Service below", "move", super::MoveDown, ["down", "j"], Activity, None, false),
    row!("That service alone", "or every service again", super::Confirm, ["enter"], Activity, None, false),
    row!("Find a service", "by name", super::FilterRows, ["/"], Activity, None, false),
    row!("Part above", "General, Machine, About", super::MoveUp, ["up", "k"], Settings, None, false),
    row!("Part below", "General, Machine, About", super::MoveDown, ["down", "j"], Settings, None, false),
    row!("Copy report", "the machine's, with the fixes", CopyReport, ["c"], Settings, None, false),
];
