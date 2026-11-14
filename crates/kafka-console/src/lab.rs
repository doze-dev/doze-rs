//! The Lab: rehearsing a failure on purpose (docs/port/behaviours/lab.md, docs/port/phase9/lab.md).
//!
//! Rules live in the broker's fault table ([`kafka_server::fault`]) and nowhere else; this module is the console's side of them:
//! the words, the forms that make a rule, the views of the rules in force and over, and the rehearsals that happen once (a member's
//! session expires, a group rebalances, the cleaner runs now). Nothing here is reachable except through the console's own
//! pages, and every change is a POST the console's same-origin rule has judged.

pub mod act;
pub mod compact;
pub mod form;
mod handle;
pub mod recipes;
pub mod teach;
pub mod views;
pub mod words;

pub use handle::{Lab, MOST_ONCE};
