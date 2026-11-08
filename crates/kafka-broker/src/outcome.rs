use kafka_wire::api::Response;

/// What the connection does after a request has been handled.
#[derive(Debug)]
#[non_exhaustive]
pub enum Outcome {
    /// Send this response.
    Respond(Response),
    /// Send nothing and carry on (a Produce with `acks=0` is never answered).
    NoResponse,
    /// Close the connection: the request cannot be answered in a way the client could read.
    Close,
}
