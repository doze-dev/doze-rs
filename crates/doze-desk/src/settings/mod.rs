//! Settings' model, GPUI-free (docs/port/app/PLAN.md §2.8 rows 82–84): the Machine page (the setup part by part,
//! Repair, what stacks left behind) and About's update check. General's choices are the app's prefs.
//!
//! Nothing here acts on the machine by itself: a repair or a clean-up goes through [`Hands`], which the app makes
//! from [`SystemHands`] (the real CLI, the real administrator prompt) and a test or a fixture from a recorder.

pub mod cli;
mod hands;
pub mod leftovers;
pub mod machine;
pub mod plan;
#[cfg(test)]
mod record;
pub mod repair;
pub mod update;

pub use cli::Cli;
pub use hands::{Out, Said, SystemHands};
pub use leftovers::{Leftover, Machine};
pub use machine::{DozeAt, Part};
pub use plan::{Job, Planned};
pub use repair::{Fail, Hands, Outcome, Step};
pub use update::Update;

/// `s` as one single-quoted shell word.
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_quoted_word_is_one_word() {
        assert_eq!(super::quote("a b"), "'a b'");
        assert_eq!(super::quote("it's"), r"'it'\''s'");
    }
}
