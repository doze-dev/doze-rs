//! EventBridge API destinations: the list, a destination's page, and what the pages do (create one and its connection).

use super::super::Format::{Mono, Number, Text as Plain, Time};
use super::super::{Delete, Id, Kind, List, Step, Text, col, field, ops, state};

pub(super) const API_DESTINATIONS: Kind = Kind {
    id: "api-destinations",
    title: "API destinations",
    one: "API destination",
    about: "HTTP endpoints a rule can send events to, each with a connection that holds its credentials.",
    members: &[],
    list: List {
        step: Step::new(
            "ListApiDestinations",
            r#"{"Limit":100,"NextToken":"{token}"}"#,
        ),
        items: "ApiDestinations",
        item_as: "Name",
        token_out: "NextToken",
        id: "Name",
        name: Text::at("Name"),
        label: None,
        split: None,
    },
    elsewhere: "",
    each: &[],
    columns: &[
        state("State", "ApiDestinationState", Plain),
        col("Endpoint", "InvocationEndpoint", Mono),
        col("HTTP method", "HttpMethod", Plain),
        col("Connection", "ConnectionArn", Mono),
        col(
            "Rate limit per second",
            "InvocationRateLimitPerSecond",
            Number,
        ),
    ],
    id_of: Id::Name,
    detail: &[Step::new("DescribeApiDestination", r#"{"Name":"{id}"}"#)],
    fields: &[
        field("ARN", "ApiDestinationArn", Mono),
        field("Description", "Description", Plain),
        field("State", "ApiDestinationState", super::STATES),
        field("Endpoint", "InvocationEndpoint", Mono),
        field("HTTP method", "HttpMethod", Plain),
        field("Connection", "ConnectionArn", Mono),
        field(
            "Rate limit per second",
            "InvocationRateLimitPerSecond",
            Number,
        ),
        field("Created", "CreationTime", Time),
        field("Last modified", "LastModifiedTime", Time),
    ],
    subs: &[],
    create: None,
    delete: Some(Delete {
        verb: "Delete",
        step: Step::new("DeleteApiDestination", r#"{"Name":"{id}"}"#),
        words: "Deleting removes the destination. A rule that has it as a target keeps the target and can no longer deliver to it.",
    }),
    reveal: None,
    deep: &[],
    // Create API destination and Create connection (pages of their own), and the connections the form picks from.
    ops: &[
        ops::free("CreateApiDestination"),
        ops::free("CreateConnection"),
        ops::free("ListConnections"),
        ops::free("DescribeConnection"),
        ops::free("ListApiDestinations"),
    ],
    ..Kind::BARE
};
