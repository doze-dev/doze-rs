//! The Amazon States Language, without I/O: a definition read and checked as AWS checks it ([`def`]), JSONPath ([`path`]), the
//! intrinsic functions ([`intrinsic`]), JSONata ([`jsonata`], on `jlens-core`), and an interpreter of executions ([`exec`]) that
//! changes an execution's plain data and says what happened (history events, task calls, the end). The service around it does the
//! calls, keeps the clock and the disk.
//!
//! Bounded: definitions nest at most 32 deep, intrinsic calls 16, one call of the interpreter runs at most 25,000 states, and a
//! JSONata expression runs at most five seconds.

pub mod def;
pub mod env;
pub mod errors;
pub mod exec;
pub mod intrinsic;
pub mod jackson;
pub mod jsonata;
pub mod path;
pub mod time;

pub use def::{Definition, Diagnostic, parse};
pub use env::Env;
pub use errors::Failure;
pub use exec::{Dispatch, End, Event, Exec, ITEM_READER, Outcome, Start, Status, Step, parks};
