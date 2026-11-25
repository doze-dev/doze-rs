//! The Lab's forced events: rehearsals that are not rules. Production does these things on a timer (a message's visibility timeout runs
//! out, a schedule's time comes, an idle service is closed, a table's expired items are swept, a function's process is replaced) and a
//! client has to be ready for them; here the person asks for one now, and the service carries it out by the same function its own timer
//! or request path runs, behind the same check, never by a transition of its own. When there is nothing to do it says so in words.
//!
//! - [`kinds`]: the five kinds, in the order of their value, and whether this run can carry each out.
//! - [`form`]: what there is to choose in a region, read through the bus (a service that rests is described as resting, not called).
//! - [`act`]: the carrying out, as the console, which is the account's root.
//! - [`rest`]: resting a service, which is the host's register of lazy services, not a service's own step.
//! - [`History`]: what was forced, the last 200.
//! - [`teach`]: the lesson of each kind.
//!
//! The services' own steps are `aws_wire::forced::Forced`, reached through [`aws_wire::service::Service::forced`] on the bus's targets.

pub mod act;
pub mod form;
mod history;
pub mod kinds;
pub mod rest;
pub mod teach;

pub use history::History;

use crate::model::LabOnce;
use crate::state::Shared;

/// What a forced event came to, before it is told to the page.
#[derive(Debug)]
pub(crate) struct Done {
    pub(crate) ok: bool,
    pub(crate) sentence: String,
    pub(crate) subject: String,
}

/// The forced events kept so far, newest first: what `GET /api/v1/lab/rules` lists as `once`.
#[must_use]
pub fn recent(s: &Shared) -> Vec<LabOnce> {
    s.once.recent()
}
