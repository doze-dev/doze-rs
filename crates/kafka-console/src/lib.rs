//! The web console: a view of the broker, in the same process, reading and acting on it through the same handlers and the
//! same authorization as a Kafka client (as the principal `User:__console`).
//!
//! The page layer is the React app in `web/` (`docs/port/console/react-plan.md`), built into `web-dist/`, embedded and served
//! by [`web`]; it talks to the JSON API in [`api`]. What the API answers is view-models ([`model`]), made from broker answers
//! ([`gather`]) or, for tests, from fixtures. The pure parts (formatting, the same-origin rule, the words Rust decides) are plain
//! functions with tables of cases.

pub mod access_words;
pub mod acl_words;
pub mod api;
pub mod app;
pub mod backend;
pub mod base;
pub mod calls;
pub mod calls_groups;
pub mod config_calls;
pub mod config_effects;
pub mod config_pass;
pub mod config_source;
pub mod config_teach;
pub mod config_words;
pub mod facts;
pub mod facts_start;
pub mod files;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures_access;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures_config;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures_groups;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures_live;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures_log;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures_settings;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures_story;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures_topics;
#[cfg(any(test, feature = "examples"))]
pub mod fixtures_write;
pub mod form;
pub mod format;
pub mod gather;
pub mod groups_words;
pub mod guard;
pub mod headers;
pub mod lab;
pub mod lag;
pub mod limits;
pub mod live_words;
pub mod log_ruler;
pub mod log_source;
pub mod log_words;
pub mod model;
pub mod origin;
pub mod partitioner;
pub mod phrases;
pub mod probe;
pub mod producers_words;
pub mod recorder;
pub mod refusal;
pub mod serve;
pub mod settings_words;
pub mod snippets;
pub mod source;
pub mod state;
pub mod story;
pub mod stream;
pub mod teach;
pub mod validate;
pub mod web;
pub mod words;
pub mod write_source;
pub mod write_words;

pub use backend::Backend;
pub use facts::Facts;
pub use limits::{Limits, MAX_CONNECTIONS, MAX_STREAMS};
pub use recorder::Recorder;
pub use serve::{Running, Settings, start, start_on, start_with};
pub use source::{Live, Source};
