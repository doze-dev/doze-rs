//! The rows of the Library, a stack's page and its Grid (M1 steps 1.1, 1.2, 1.4), beside the shell's. The keys are
//! Go's (docs/port/app/PLAN.md §2.10). The verbs act through `crate::act` (M2).

use super::*;

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

use MenuName::View;

/// The pages' rows.
#[rustfmt::skip]
pub(crate) const PAGES: &[Spec] = &[
    row!("Open the stack", "its page", OpenStack, ["enter"], Library, None, false, true),
    row!("Stack above", "move", MoveUp, ["up", "k"], Library, None, false, true),
    row!("Stack below", "move", MoveDown, ["down", "j"], Library, None, false, true),
    row!("Up or down", "the stack's switch", ToggleStack, ["space"], Library, None, false, true),
    row!("Remove from doze…", "stops it; every file stays", RemoveStack, ["backspace"], Library, None, false, true),
    row!("Service above", "move", MoveUp, ["up", "k"], Overview, None, false, true),
    row!("Service below", "move", MoveDown, ["down", "j"], Overview, None, false, true),
    row!("Tile left", "move", MoveLeft, ["left"], Overview, None, false, true),
    row!("Tile right", "move", MoveRight, ["right"], Overview, None, false, true),
    row!("Filter", "the services", FilterRows, ["/"], Overview, None, true, true),
    row!("Copy the address", "of the one selected", CopyAddress, ["y"], Overview, None, true, true),
    row!("Open the service", "its page", OpenService, ["enter"], Overview, None, false, true),
    row!("Wake or sleep", "its switch", ToggleService, ["space"], Overview, None, false, true),
    row!("Up as last time", "a stopped stack", UpAsLastTime, ["u"], Overview, None, true, true),
    row!("Choose what starts…", "a stopped stack", ChooseStart, ["c"], Overview, None, true, true),
    row!("Row density", "compact, cosy, roomy", CycleDensity, [], Global, Some(View), true, true),
    row!("Cell above", "move", MoveUp, ["up", "k"], Grid, None, false, true),
    row!("Cell below", "move", MoveDown, ["down", "j"], Grid, None, false, true),
    row!("Cell left", "move", MoveLeft, ["left"], Grid, None, false, true),
    row!("Cell right", "move", MoveRight, ["right"], Grid, None, false, true),
    row!("Open what the cell names", "the service it needs", OpenService, ["enter"], Grid, None, false, true),
    row!("Open the row", "the service on the left", OpenRowService, ["o"], Grid, None, false, true),
    row!("Filter rows", "the services", FilterRows, ["/"], Grid, None, false, true),
];
