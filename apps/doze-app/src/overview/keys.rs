//! The keys bar of a stack's page, for the view in force: each chip from the action table.

use gpui::IntoElement;

use super::{OverviewView, View};
use crate::kit::{chip_of, keys_bar};
use crate::theme::Tokens;

/// The keys bar for the view in force.
/// `held` is the Map's selection and whether a focus view (a neighbourhood, only trouble) is open.
pub(super) fn keys(t: &Tokens, v: &OverviewView, (lit, lens): (bool, bool)) -> impl IntoElement {
    let arrows = |up: &str, down: &str| format!("{}{}", chip_of(up), chip_of(down));
    let (keys, note): (Vec<(String, &str)>, &str) = if v.home.stopped.is_some() {
        (
            vec![
                (arrows("Service above", "Service below"), "move"),
                (chip_of("Wake or sleep"), "start with this one"),
                (chip_of("Up as last time"), "up as last time"),
                (chip_of("Choose what starts…"), "choose what starts"),
                (chip_of("Filter"), "filter"),
                (chip_of("Close"), "home"),
            ],
            "nothing runs while the stack is down",
        )
    } else {
        match v.view {
            View::Map => map_keys(lit, lens),
            View::Grid => (
                vec![
                    (
                        ["Cell above", "Cell below", "Cell left", "Cell right"]
                            .iter()
                            .map(|l| chip_of(l))
                            .collect::<String>(),
                        "move",
                    ),
                    (chip_of("Wake or sleep"), "wake / sleep the row"),
                    (chip_of("Open what the cell names"), "open"),
                    (chip_of("Open the row"), "open the row"),
                    (chip_of("Filter rows"), "filter rows"),
                ],
                "reads like a table: a row needs each column it has a dot in",
            ),
            _ => (
                vec![
                    (arrows("Service above", "Service below"), "move"),
                    (chip_of("Wake or sleep"), "wake / sleep"),
                    (chip_of("Open the service"), "open"),
                    (chip_of("Copy the address"), "copy address"),
                    (chip_of("Filter"), "filter"),
                    (chip_of("Close"), "home"),
                ],
                "from Home: click a stack",
            ),
        }
    };
    keys_bar(t, &keys, note)
}

/// The Map's keys and note: what esc does depends on what it holds (a focus view, a selection).
fn map_keys(lit: bool, lens: bool) -> (Vec<(String, &'static str)>, &'static str) {
    let arrows = ["Card above", "Card below", "Card left", "Card right"]
        .iter()
        .map(|l| chip_of(l))
        .collect::<String>();
    let esc = match (lens, lit) {
        (true, _) => "back to the whole map",
        (_, true) => "let go",
        _ => "close",
    };
    let note = if lens {
        "a focus view: the rest of the map is hidden until esc"
    } else if lit {
        "selected: its calls stay lit, the rest steps back"
    } else {
        "hover a card to see its calls · click to keep them"
    };
    (
        vec![
            (arrows, "move"),
            (chip_of("Find a card"), "find"),
            (chip_of("Fit the map"), "fit"),
            (chip_of("Overview on or off"), "overview"),
            (chip_of("Close"), esc),
        ],
        note,
    )
}
