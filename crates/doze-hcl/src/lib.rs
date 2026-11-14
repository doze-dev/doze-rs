//! HCL as a language, with the semantics of Go's `hashicorp/hcl` and `go-cty` (the versions doze
//! pins): hcl-edit for syntax and spans, and everything above it written here, checked against Go's
//! output by the oracle corpus in `testdata/`.
//!
//! It knows nothing of doze: no `module`, no ports. `doze-config` puts doze's schema on top.

pub mod check;
pub mod convert;
pub mod diag;
pub mod eval;
pub mod function;
pub mod lex;
pub mod number;
pub mod syntax;
pub mod typeexpr;
pub mod types;
pub mod value;

pub use number::Number;
pub use types::Type;
pub use value::Value;
