//! The small groups a long context's keys are split into, by what the keys do. Each names the sheet's lines (after
//! folding) by their labels; a test holds every line of a long context to exactly one group.

use crate::actions::Scope;

/// A group's heading and the labels of its lines, in reading order.
pub(crate) type Named = (&'static str, &'static [&'static str]);

/// Each long context's groups.
pub(crate) const GROUPS: &[(Scope, &[Named])] = &[
    (
        Scope::Global,
        &[
            (
                "Go to",
                &[
                    "Home",
                    "A stack's tab",
                    "Back",
                    "Forward",
                    "All logs",
                    "Activity",
                    "Settings…",
                    "Needs you",
                ],
            ),
            (
                "Find and ask",
                &["Search or do…", "Keys and words", "Recent messages"],
            ),
            (
                "Views",
                &["Tiles · List · Map · Grid", "Dark, light or system"],
            ),
            (
                "Files and the window",
                &[
                    "Open folder…",
                    "New stack…",
                    "Edit doze.hcl",
                    "Close",
                    "Close the window",
                    "Quit doze",
                ],
            ),
        ],
    ),
    (
        Scope::Overview,
        &[
            (
                "Move and open",
                &["Move", "Filter", "Open the service", "Its logs", "Activity"],
            ),
            (
                "The service selected",
                &[
                    "Wake or sleep",
                    "Wake",
                    "Sleep",
                    "Restart",
                    "Pin awake",
                    "Copy the address",
                ],
            ),
            (
                "A stopped stack",
                &["Up as last time", "Choose what starts…"],
            ),
        ],
    ),
    (
        Scope::Instance,
        &[
            (
                "Act",
                &[
                    "Restart / try now",
                    "Sleep / stop trying",
                    "Wake",
                    "Pin awake",
                    "Run the task",
                ],
            ),
            (
                "Open",
                &["Open in browser", "Copy address", "Logs", "Next tab"],
            ),
        ],
    ),
    (
        Scope::Logs,
        &[
            (
                "Find",
                &[
                    "Filter the lines",
                    "Find or filter",
                    "Level: debug to error",
                ],
            ),
            (
                "Move",
                &["To the top", "To the end", "Run before", "Run after"],
            ),
            (
                "View",
                &["Pause or follow", "Relative or clock", "Copy the lines"],
            ),
        ],
    ),
    (
        Scope::Map,
        &[
            (
                "Move",
                &["The nearest card", "Open the card", "Find a card"],
            ),
            (
                "Zoom",
                &["Fit the map", "Zoom in", "Zoom out", "Actual size"],
            ),
            (
                "Show",
                &[
                    "Overview on or off",
                    "Every line or on demand",
                    "Neighbourhood",
                    "Only trouble",
                ],
            ),
        ],
    ),
];

/// A context's groups; none for a short one.
pub(crate) fn of(sc: Scope) -> &'static [Named] {
    GROUPS
        .iter()
        .find(|(s, _)| *s == sc)
        .map_or(&[], |(_, g)| *g)
}
