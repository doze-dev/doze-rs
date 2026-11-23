//! SNS: topics and subscriptions.

mod subscriptions;
mod topics;

use super::Descriptor;

pub(super) static SNS: Descriptor = Descriptor {
    service: "sns",
    target: "sns",
    kinds: &[topics::TOPICS, subscriptions::SUBSCRIPTIONS],
};
