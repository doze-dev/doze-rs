//! EventBridge: event buses, rules (each with its targets), archives, replays and API destinations.
//!
//! A rule is named in the console's addresses as its ARN names it: `on-order` on the default bus, `orders/on-order` on the bus
//! `orders` (`{name:head}` and `{name:last}` in the bodies). The pages the console's app draws for these make their own calls
//! through the kinds' `ops`: a rule with its targets and tags, a bus with an archive, a replay, a connection.

mod archives;
mod buses;
mod destinations;
mod replays;
mod rules;

use super::Descriptor;

pub(super) static EVENTBRIDGE: Descriptor = Descriptor {
    service: "eventbridge",
    target: "events",
    kinds: &[
        rules::RULES,
        buses::BUSES,
        archives::ARCHIVES,
        replays::REPLAYS,
        destinations::API_DESTINATIONS,
    ],
};

/// What a tag call names: a rule, by its ARN (`rule/<bus>/<name>`, or `rule/<name>` on the default bus).
const RULE_ARN: &str = r#"{"ResourceARN":"arn:aws:events:{region}:{account}:rule/{name}"}"#;
/// The same for a bus.
const BUS_ARN: &str = r#"{"ResourceARN":"arn:aws:events:{region}:{account}:event-bus/{id}"}"#;

/// The retention column of an archive list: `0` days is how EventBridge says the events are kept indefinitely.
const fn retention() -> super::Column {
    super::col_or(
        "Retention (days)",
        "RetentionDays",
        super::Format::Named(&[("0", "Indefinite")]),
        "Indefinite",
    )
}
