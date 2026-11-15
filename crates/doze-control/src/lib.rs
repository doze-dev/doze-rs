//! Control protocol 2 of the doze daemon, in Rust (docs/port/app/PLAN.md §4.2, §4.3).
//!
//! The daemon listens on `<project>/run/doze.admin.sock` and speaks newline-delimited JSON. A connection opens with a
//! hello each way (`{"hello":{"proto":2,"client":"app"}}`), then carries any number of requests at once, each answered
//! by frames tagged with the request's id: one frame for a unary op, an acknowledgement, topic frames and an end frame
//! for a stream. `{"id":N,"op":"cancel"}` ends a stream. This is Go's `internal/control`, port of every op, topic and
//! field; the wire is byte-compatible with Go's (see `tests/roundtrip.rs`).
//!
//! - [`types`]: the serde types: [`Request`], [`Frame`], [`Response`], [`InstanceView`], the console and journal.
//! - [`codec`]: the framing, sans-IO.
//! - [`Client`], [`Connection`], [`Stream`], [`Follow`]: a blocking client on a reader thread, frames delivered over
//!   `async_channel`, and a subscription that reconnects with back-off.
//! - [`conn`]: the transport behind a trait, so a Windows named pipe is a new dialer.
//! - `fixture` (feature `test-support`): a daemon that serves scripted frames on a real socket.

mod client;
pub mod codec;
pub mod conn;
mod duration;
mod error;
mod json;
pub mod time;
pub mod types;

#[cfg(feature = "test-support")]
pub mod fixture;

pub use client::{
    Backoff, CALL_TIMEOUT, Client, Connection, Follow, FollowEvent, HELLO_TIMEOUT, Pending, Stream,
    StreamEvent,
};
pub use duration::{format_go_duration, parse_go_duration};
pub use error::Error;
pub use time::{GoDuration, Timestamp};
pub use types::console::{
    CELL_RUNTIME, ConsoleColumn, ConsoleExtension, ConsoleFrame, ConsoleNode, ConsolePanel,
    ConsoleRow, ConsoleSpec, ConsoleTab,
};
pub use types::frame::{ActionView, EventFrame, Frame, Hello, LogFrame, ResourceView, Response};
pub use types::instance::{HolderView, InstanceView, ProcView, StatView, TaskView};
pub use types::journal::{Cause, Event};
pub use types::ops::{
    JOURNAL_FROM_START, LogQuery, Op, PROTO_VERSION, Snapshot, Subscription, Topic, Verb, fault,
};
pub use types::request::Request;
