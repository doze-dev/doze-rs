//! The clients drawer, Home's strip of who has spoken, and the first-request page.

use super::live::WireRow;

/// One connected client.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ClientRow {
    pub client: String,
    pub tag: Option<String>,
    pub software: Option<String>,
    /// The address of its newest connection, and how many more it has.
    pub peer: Option<String>,
    pub more: usize,
    /// The APIs it has used, with the highest version of each, most used first.
    pub apis: Vec<(&'static str, i16)>,
    pub principal: String,
    pub last_ms: i64,
    pub requests: u64,
    pub errors: u64,
}

/// The clients drawer.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ClientsView {
    pub rows: Vec<ClientRow>,
    pub connections: usize,
    pub now_ms: i64,
}

/// Home's strip: who has spoken.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SpokeView {
    /// Distinct client ids seen since the broker started (those the ring still knows), and requests in the window.
    pub apps: usize,
    pub requests: u64,
    pub oldest_ms: Option<i64>,
    pub last_ms: Option<i64>,
    /// How long ago the last request landed, in milliseconds.
    pub ago_ms: Option<i64>,
    pub last_client: Option<String>,
}

/// The first-request page: the newest client the broker had not heard from before.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FirstView {
    pub client: String,
    pub software: Option<String>,
    pub peer: Option<String>,
    pub first_ms: i64,
    /// Its requests in order, oldest first, up to a dozen.
    pub rows: Vec<WireRow>,
    /// Whether every request of it was answered without an error.
    pub all_answered: bool,
    pub refused: usize,
    pub topics_listed: Option<u32>,
    pub profile: String,
    pub listen: String,
    pub versions: Vec<(&'static str, i16)>,
}
