//! The web console: a server-rendered view of the broker, in the same process, reading and acting on it through the
//! same handlers and the same authorization as a Kafka client (as the principal `User:__console`).
//!
//! Pages are maud functions of view-models ([`model`]); view-models are made from broker answers ([`gather`]) or, for
//! tests and the design boards, from fixtures; the route table ([`routes`]) is the one list the router is built from.
//! The pure parts (formatting, the same-origin rule, the words of the pages) are plain functions with tables of cases.
//! The client layer (one small script of our own, no library) is behind [`ui::client`] alone, and every page is complete without it.

pub mod access_words;
pub mod acl_words;
pub mod actions;
pub mod app;
pub mod assets;
pub mod backend;
pub mod base;
#[cfg(any(test, feature = "examples"))]
pub mod boards;
#[cfg(any(test, feature = "examples"))]
pub mod boards_access;
#[cfg(any(test, feature = "examples"))]
pub mod boards_config;
#[cfg(any(test, feature = "examples"))]
pub mod boards_log;
#[cfg(any(test, feature = "examples"))]
pub mod boards_settings;
#[cfg(any(test, feature = "examples"))]
pub mod boards_write;
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
pub mod handlers;
pub mod headers;
pub mod lag;
pub mod limits;
pub mod live_ids;
pub mod log_ruler;
pub mod log_source;
pub mod log_words;
pub mod model;
pub mod origin;
pub mod pages;
pub mod partitioner;
pub mod phrases;
pub mod probe;
pub mod producers_words;
pub mod recorder;
pub mod refusal;
pub mod routes;
pub mod serve;
pub mod settings_words;
pub mod snippets;
pub mod source;
pub mod state;
pub mod story;
pub mod story_ids;
pub mod stream;
pub mod stream_groups;
pub mod stream_log;
pub mod teach;
pub mod tick;
pub mod trail;
pub mod ui;
pub mod validate;
pub mod words;
pub mod write_source;

pub use backend::Backend;
pub use facts::Facts;
pub use limits::{Limits, MAX_CONNECTIONS, MAX_STREAMS};
pub use recorder::Recorder;
pub use serve::{Running, Settings, start, start_on, start_with};
pub use source::{Live, Source};
