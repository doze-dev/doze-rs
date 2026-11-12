//! The words of the Live pages. A sentence that held a number is gone (CON-805): the numbers are metric slots now
//! (`pages/live/page.rs`), and what is left here is how a time is written.

use crate::format::civil;

/// `19:12`, UTC.
#[must_use]
pub fn hhmm(ms: i64) -> String {
    let c = civil(ms);
    format!("{:02}:{:02}", c.hour, c.minute)
}

#[cfg(test)]
mod tests;
