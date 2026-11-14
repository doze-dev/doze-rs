//! The desktop app's brain, without GPUI (docs/port/app/PLAN.md §4.1–4.4). This first part is the hub: the app's one
//! view of every stack on this machine, a port of the Go app's `internal/desk/hub`.
//!
//! - Running stacks are found by their control sockets (`doze-projects`), stopped ones by their project records.
//! - Each running stack has one thread that owns a [`doze_control::Follow`]: the long-lived subscription
//!   to `instances` and `journal`, with `stats` and `logs` added while a page [`Hub::watch`]es them. The subscription
//!   is remade on the same connection when a watch count crosses zero. A module console a page shows gets its own
//!   subscription, on that thread too, for as long as it is shown.
//! - Frames are folded into the stack's state on that thread: instances, meters ([`Series`]), the journal, the log
//!   ring ([`LogRing`], 100,000 lines a stack), the consoles. Each fold says what it changed ([`Changed`]); the
//!   changes are coalesced ([`Coalescer`]) and reach the UI as at most one [`Batch`] per frame, none while hidden.
//! - Verbs and module actions go on a second, short-lived connection, so a long verb never blocks the stream.
//! - [`convert`] is the one place the daemon's wire types become `doze-display`'s inputs.
//!
//! What a page reads ([`StackView`], [`ConsoleView`], [`Series`]) is `PartialEq`, and its heavy parts are shared
//! (`Arc`), so a view is cheap to take and a page re-renders only when it changed.

pub mod act;
pub mod activity;
pub mod attention;
mod changed;
mod clock;
mod coalesce;
mod conn;
mod console;
pub mod convert;
mod error;
mod events;
mod hub;
pub mod library;
pub mod machine_names;
mod name;
pub mod notify;
pub mod overview;
pub mod page;
pub mod palette;
pub mod ports;
mod ring;
mod select;
mod series;
pub mod settings;
mod stack;
pub mod titles;
pub mod tray;
mod view;

pub use changed::{Batch, Changed, Parts};
pub use clock::Clock;
pub use coalesce::{Coalescer, FRAME, Policy};
pub use conn::{Conn, Link};
pub use console::{ConsoleView, STREAM_KEPT};
pub use error::Error;
pub use events::EVENTS_KEPT;
pub use hub::{ConsoleWatch, Hub, Options, VERB_TIMEOUT, Watch};
pub use name::name_from_key;
pub use ring::{BACKLOG, LINES_KEPT, LineRef, LogRing, TEXT_KEPT};
pub use series::{SAMPLES_KEPT, Series};
pub use view::StackView;

pub use doze_projects::StackKey;
