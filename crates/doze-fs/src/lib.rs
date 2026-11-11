//! The file-system seam shared by every doze service.
//!
//! All disk access goes through [`fs::Fs`], so a service's storage runs unchanged on the real disk
//! ([`fs::RealFs`]) and on a simulated one ([`sim::SimFs`]) that can cut power at any operation, tear a
//! write, or run out of space. Extracted from `kafka-storage` (A1); nothing here knows about any service.

pub mod fs;
#[cfg(any(test, feature = "sim"))]
pub mod sim;
