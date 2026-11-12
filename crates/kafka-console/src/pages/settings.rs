//! The Settings page (slice 9h), written from the boards in `design/boards/settings/`: the five dynamic broker configs, the
//! version profile, the intervals, the weight, the data directory and the flags in one page of sections; the edit dialog of
//! a broker config and its saved and refused answers.
//!
//! What a board shows and the console does differently, and why (each is an allow-list entry or a debt row):
//! - "today fulfilment speaks Fetch v12 and orders Produce v9" (the wire, slice 9e) and "its last round was on inventory.levels"
//!   (the cleaner's rounds, slice 9g) and "analytics' offsets expire in 3 d 2 h" (a group's own clock, Groups): the sentences say
//!   what the broker holds now and name none of them.
//! - "Idle memory 4.5 MB, 3 threads, read on Linux only": measured by the process itself where it can be read (Linux), and
//!   says so where it cannot (macOS) instead of a number.
//! - The Intervals table's defaults and the Flags table are the flags of the binary (`facts_start`): a broker started by a
//!   test has none, and the tables say "unknown" and "not known" and invent nothing.

mod configs;
mod data;
mod edit;
mod flags;
mod intervals;
mod page;
mod profile;
mod states;
mod weight;

pub use edit::{edit_dialog, edit_overlay, edit_title, with_overlay};
pub use page::settings_page;
pub use states::{settings_refused, settings_saved};

use crate::base::Base;
use crate::form::encode_query;

/// The address of the edit dialog of one config.
#[must_use]
pub fn edit_url(base: &Base, key: &str) -> String {
    base.url(&format!(
        "/settings/config/edit{}",
        encode_query(&[("key", key)])
    ))
}

/// The address of the page, or one of its sections.
#[must_use]
pub fn settings_url(base: &Base) -> String {
    base.url("/settings")
}

#[cfg(test)]
mod tests;
