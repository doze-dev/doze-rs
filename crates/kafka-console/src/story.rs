//! The story of a group: why it rebalanced, one generation at a time, and whether it is flapping. Pure: it reads what the
//! broker's ring of notes says (`ConsoleView::group_events`) and what the group page already knows, and makes the plain
//! values (`model::story`) the Story pages are written from. The words, the flapping rule and the arithmetic are here so
//! that each has a table of cases.

mod build;
mod flap;
mod words;

pub use build::{calm_of, causes_of, facts_of, generations_of, now_of, view_of};
pub use flap::{NEEDS, WINDOW_MS, cycles, flap_of};
pub use words::{
    ago_near, cause_help, cause_phrase, cause_title, holds_phrase, last_clause, lasted,
    member_label, parts_phrase,
};

#[cfg(test)]
mod tests;
