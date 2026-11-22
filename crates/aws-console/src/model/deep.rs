//! The deep pages' own answers (most of what a deep page reads is the service's answer as its model has it).

use serde::Serialize;

/// `POST /api/v1/resources/lambda/functions/<name>/op/Invoke`: what a synchronous invoke answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Invoked {
    /// Lambda's `StatusCode` (200 for a synchronous invoke, the function's error included).
    pub status: u16,
    /// `Unhandled` when the function threw: the call succeeded and the payload is its error document.
    pub function_error: Option<String>,
    pub executed_version: Option<String>,
    /// The function's answer, as text.
    pub payload: String,
    /// The last 4 KB of the run's log, when it was asked for (`LogType: Tail`).
    pub log: Option<String>,
}
