//! View-models: what a page says, as plain data. No HTML, no broker types, nothing a page could write with.
//!
//! A page is `fn(&Model) -> Markup`. Everything that decides what the person reads (which state of Home, whether the
//! advertised address answers) is decided before, from real data, and arrives here as a value.

mod connect;
mod home;
mod messages;
mod query;
mod topic;
mod topics;

pub use connect::{Access, ClientSnippet, ConnectView, ProbeView, Rule, Span, Tok, Verdict};
pub use home::{Attention, HeldBack, HomeState, HomeView, InstanceView, StopView};
pub use messages::{
    HeaderRow, Hold, Kind, MessagesView, Reach, RecordRow, RecordTxn, RecordView, Shown, Standing,
};
pub use query::{DEFAULT_LIMIT, MOST_KEY, MOST_LIMIT, MessageQuery, Reading};
pub use topic::{CallVersions, OpenTxn, PartitionRow, TabCounts, TopicHead};
pub use topics::{Cleanup, TopicRow, TopicsState, TopicsView};
