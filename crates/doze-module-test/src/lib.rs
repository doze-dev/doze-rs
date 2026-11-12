//! The harness module tests run on (docs/port/modules/PLAN.md step 4): literal HCL to the values the core sends, a
//! sandboxed doze home, the core's boot sequence, and a plan executor with readiness checks and graceful stops.

pub mod accept;
pub mod boot;
pub mod hcl;
pub mod oracle;
pub mod run;
pub mod sandbox;

pub use accept::acceptance;
pub use boot::{Booted, Harness, target};
