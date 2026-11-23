//! What a call did: read from its input as it is decoded and from its answer as it is made, for the observer.

use aws_peers::summary::{self, Summary, TreeFields};
use http::request::Parts;

use super::note;
use crate::pipeline::Entry;

/// What a call did: the summary, and the short text some summaries carry (an index, a session name, the role an attach was for).
#[derive(Debug, Clone, Copy)]
pub struct Did<'a> {
    pub summary: Summary,
    pub detail: &'a str,
}

impl Did<'_> {
    /// No result.
    pub const NONE: Did<'static> = Did {
        summary: Summary::NONE,
        detail: "",
    };
}

/// The service's answer to the request is the decoded tree `root` of `arena`.
pub(crate) fn output(parts: &Parts, entry: &Entry, output: &aws_wire::service::Output) {
    note(parts, |n| {
        let fields = TreeFields {
            tables: entry.service.tables(),
            arena: &output.arena,
            body: "",
            root: output.root,
        };
        summary::from_output(
            entry.service.meta().sdk_id,
            n.action,
            &fields,
            &mut n.summary,
        );
    });
}

/// A REST service's answer to the request is `json`.
pub(crate) fn output_json(parts: &Parts, entry: &Entry, json: &serde_json::Value) {
    note(parts, |n| {
        summary::from_output(entry.service.meta().sdk_id, n.action, json, &mut n.summary);
    });
}
