//! The Config pages of a topic (slice 9d): the list of configs, the edit dialogs, the saved and refused states, and the
//! reset dialog. Each is written from its board in `design/boards/config/`.

mod common;
mod dialog;
mod links;
mod page;
mod refused;
mod reset;
mod rows;
mod saved;

pub use common::{config_url, edit_url, reset_url};
pub use dialog::edit_dialog;
pub use links::{toggled, token};
pub use page::{config_page, no_config};
pub use refused::config_refused;
pub use reset::reset_dialog;
pub use saved::config_saved;

#[cfg(test)]
mod tests;
