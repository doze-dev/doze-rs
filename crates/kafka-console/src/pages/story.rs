//! The Story of a group (slice 9f): why it rebalanced, one generation at a time (`group-story`), a rebalance opened as a
//! drawer over it (`story-rebalance`), and the flapping rule held against the group (`story-flapping`).
//!
//! Everything is the group's own ring of notes (`ConsoleView::group_events`): 200 events a group, 100 groups, emptied at a
//! restart, and the page says so. What the boards draw and the broker cannot say, and what the page does instead (debt
//! J291 to J300):
//! - a classic group cannot say "a topic changed": a client sees the change and its leader joins again, so the cause is
//!   "Leader rejoined"; a consumer-protocol group is told by the broker and says it;
//! - the drawer's "last told to rejoin" is "last heard from": the machine does not keep when a member was told.

mod cards;
mod drawer;
mod flapping;
mod generation;
mod page;

pub use drawer::rebalance_drawer;
pub use flapping::flapping_page;
pub use page::{no_story, story_page, story_url};
