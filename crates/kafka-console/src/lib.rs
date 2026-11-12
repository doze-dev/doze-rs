//! The web console: a server-rendered view of the broker, in the same process, reading and acting on it through the
//! same handlers and the same authorization as a Kafka client (as the principal `User:__console`).
//!
//! Pages are maud functions of view-models ([`model`]); view-models are made from broker answers ([`gather`]) or, for
//! tests and the design boards, from fixtures; the route table ([`routes`]) is the one list the router is built from.
//! The pure parts (formatting, the same-origin rule, the words of the pages) are plain functions with tables of cases.
//! The client layer (one small script of our own, no library) is behind [`ui::client`] alone, and every page is complete without it.

pub mod app;
pub mod assets;
pub mod backend;
pub mod base;
#[cfg(any(test, feature = "examples"))]
pub mod boards;
pub mod facts;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures_topics;
pub mod form;
pub mod format;
pub mod gather;
pub mod guard;
pub mod handlers;
pub mod headers;
pub mod model;
pub mod origin;
pub mod pages;
pub mod probe;
pub mod routes;
pub mod serve;
pub mod snippets;
pub mod source;
pub mod state;
pub mod stream;
pub mod tick;
pub mod ui;
pub mod words;

pub use backend::Backend;
pub use facts::Facts;
pub use serve::{Running, Settings, start, start_on};
pub use source::{Live, Source};
