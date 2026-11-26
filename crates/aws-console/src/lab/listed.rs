//! The errors AWS's model lists for an operation: what a Fail rule's "an error I choose" may answer with.
//!
//! A person picks the error from what the service's own model says the operation can answer, with the status the model gives it, and
//! never types one the model does not list for it. The model is the vendored one (`aws_schema::tables`): for each operation, its error
//! shapes in model order. A rule that fits several operations offers the errors every one of them lists.
//!
//! The code is what a client reads: the shape's name for a service that speaks JSON, CBOR or REST, and the Query code (`awsQueryError`,
//! else the shape's name) for a service that speaks only Query. That is the rule `codes` follows for its own rows.

use aws_schema::meta::{Protocol, ServiceMeta};
use aws_schema::tables::Tables;

/// One error an operation lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listed {
    /// The code a client reads.
    pub code: &'static str,
    /// The HTTP status the model gives it.
    pub status: u16,
    /// Its shape in the model: the gateway words the answer from it.
    pub shape: &'static str,
    /// A server fault (the 5xx class) rather than a client one.
    pub server: bool,
}

/// An API of the build: its meta and its model.
pub type Model = (&'static ServiceMeta, &'static Tables);

/// Whether the service names a code by its Query code alone.
fn query_only(meta: &ServiceMeta) -> bool {
    meta.speaks(Protocol::AwsQuery)
        && !meta.speaks(Protocol::AwsJson10)
        && !meta.speaks(Protocol::AwsJson11)
        && !meta.speaks(Protocol::Rpcv2Cbor)
}

/// The errors of operation `op` in one model, in model order; none when the model has no such operation.
fn of_operation(model: Model, op: &str) -> Option<Vec<Listed>> {
    let (meta, tables) = model;
    let found = tables.op(tables.find_op(op)?)?;
    let mut out: Vec<Listed> = Vec::new();
    for e in tables.op_errors(&found) {
        let Some(shape) = tables.shape(e.shape) else {
            continue;
        };
        let listed = Listed {
            code: if query_only(meta) { e.code } else { shape.name },
            status: e.status,
            shape: shape.name,
            server: e.server,
        };
        if !out.contains(&listed) {
            out.push(listed);
        }
    }
    Some(out)
}

/// The errors every one of `operations` lists, in every model that has it (a model that has none of them is not asked); in the order
/// of the first. Empty when there is nothing in common, or nothing to ask about.
#[must_use]
pub fn common(models: &[Model], operations: &[&str]) -> Vec<Listed> {
    let mut lists = operations
        .iter()
        .flat_map(|op| models.iter().filter_map(move |m| of_operation(*m, op)));
    let Some(mut first) = lists.next() else {
        return Vec::new();
    };
    for next in lists {
        first.retain(|e| next.contains(e));
    }
    first
}

/// The error `code` is among `listed` (by the code a client reads, or by the shape's name, which a Query service's people may type).
#[must_use]
pub fn find<'a>(listed: &'a [Listed], code: &str) -> Option<&'a Listed> {
    listed
        .iter()
        .find(|e| e.code == code)
        .or_else(|| listed.iter().find(|e| e.shape == code))
}

#[cfg(test)]
mod tests;
