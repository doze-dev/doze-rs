//! Protocol 2 between doze and its modules (docs/port/modules/protocol.md): the frame codec, version negotiation,
//! stream credit, the messages, and the peer that runs them over a `doze-os` channel.
//!
//! The sans-IO parts ([`frame`], [`version`], [`flow`]) hold the rules; the peer only moves bytes.

pub mod flow;
pub mod frame;
pub mod method;
pub mod msg;
pub mod peer;
pub mod version;
