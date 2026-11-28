//! The rows of Settings (M3 step 3.4): Go's keys for it (PLAN.md §2.10), and the key the Rust page adds (the report
//! copied).

use gpui::{Action, actions};
use serde::Deserialize;

use super::{Scope, Spec};

actions!(
    doze,
    [
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
    row!("Part above", "General, Machine, About", super::MoveUp, ["up", "k"], Settings, None, false),
    row!("Part below", "General, Machine, About", super::MoveDown, ["down", "j"], Settings, None, false),
    row!("Copy report", "the machine's, with the fixes", CopyReport, ["c"], Settings, None, false),
];
