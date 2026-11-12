//! View-models: what a page says, as plain data. No HTML, no broker types, nothing a page could write with.
//!
//! A page is `fn(&Model) -> Markup`. Everything that decides what the person reads (which state of Home, whether the
//! advertised address answers) is decided before, from real data, and arrives here as a value.

mod connect;
mod home;

pub use connect::{Access, ClientSnippet, ConnectView, ProbeView, Rule, Span, Tok, Verdict};
pub use home::{Attention, HeldBack, HomeState, HomeView, InstanceView, StopView};
