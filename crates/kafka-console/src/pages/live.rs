//! The Live pages: the tape, a request in full (and the refusals), the clients, the docks of a topic and a group, and the page a
//! new client lands on. Boards `live-wire`, `live-paused`, `live-quiet`, `live-load`, `request-detail`, `request-refused`,
//! `request-fenced`, `request-too-large`, `live-clients`, `connect-first-request`, `topic-wire`, `group-wire`.
//!
//! What the boards draw and the recorder cannot say, and what the page does instead (debt J151 and up):
//! - "waited 480 ms" on a Fetch: the recorder knows how long the broker took and how long the client was willing to wait, not
//!   how long it did wait: the line says "willing to wait".
//! - The rules of a refusal ("Record 0 has a null key"): no record is kept. The drawer shows the broker's own message, the rule
//!   in plain words, and what the topic is now (read from the broker when the drawer opens, and said to be as of now).

mod clients;
mod commands;
mod docks;
mod drawers;
mod filters;
mod first;
mod page;
mod parts;
mod refusals;
mod tape;

pub use clients::{clients_body, clients_drawer};
pub use commands::{FetchCommands, fetch_commands, shell_word};
pub use docks::{group_wire_page, topic_dock};
pub use drawers::{
    Kind as RequestKind, Limits as RequestLimits, detail_body, detail_heading_id, request_body,
    request_drawer,
};
pub use first::first_page;
pub use page::{live_fragment, live_page, state_crumb};
pub use parts::{LIVE_FRAGMENT, wire_url};
pub use refusals::{fenced_body, refused_body, too_large_body};
pub use tape::{Layout, table};
