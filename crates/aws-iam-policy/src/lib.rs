//! IAM policy documents as typed values. No I/O and no clock: a document is read from text and checked against the rules IAM applies
//! when one is stored, and the result is a [`Document`] a later evaluator can walk.
//!
//! Three kinds of document are stored: an **identity** policy (attached to a user, group or role; it names resources and no
//! principal), a **trust** policy (a role's `AssumeRolePolicyDocument`; it names principals and no resource) and a **resource** policy
//! (on a queue, a key, a secret; it names both). The checks differ and so do the words IAM answers with; the words that were recorded
//! (2026-11-09) are marked in `check.rs`.

mod check;
mod document;

pub use check::{Kind, PolicyError, check};
pub use document::{
    Condition, Document, Effect, Matcher, Principal, PrincipalKind, Principals, Statement, parse,
};
