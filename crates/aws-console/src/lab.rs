//! The Lab's fault rules: the table the gateway asks about each request ([`aws_gateway::Faults`]) and everything it is made of.
//!
//! A rule makes the calls clients send to the doze-aws port slow, fail the way AWS fails, or drop. The console is the only way to
//! make one; they live in memory, and a restart clears them. Design: `docs/port/aws/lab.md`; behaviours: `docs/port/behaviours/aws-lab.md`.
//!
//! - [`rule`]: what a rule is and every bound of the Lab, with the typed refusal for going past it.
//! - [`glob`]: the pattern language of a rule's resource list.
//! - [`codes`]: the error each service answers each kind of failure with, with where each comes from.
//! - [`table`]: the table: bounded, in memory, with lazy expiry, precedence, and what a rule remembers.
//! - [`words`]: the sentences. [`teach`]: the lessons.

pub mod codes;
pub mod glob;
pub mod record;
pub mod rule;
pub mod table;
pub mod teach;
pub mod words;

pub use record::{Ended, EndedWhy, First, Found, Rule};
pub use rule::{
    AddError, Effect, Fail, Field, Kind, LONGEST_CODE, LONGEST_DELAY_MS, LONGEST_NAME,
    LONGEST_PATTERN, LONGEST_RULE, Matcher, MOST_CALLERS, MOST_ENDED, MOST_KEPT_CALLERS,
    MOST_OPERATIONS, MOST_REGIONS, MOST_RESOURCES, MOST_RULES, MOST_SERVICES, NTH, PERCENT,
    Pattern, SHORTEST_RULE, Share, Spec,
};
pub use table::{Clock, LabTable, SystemClock};
