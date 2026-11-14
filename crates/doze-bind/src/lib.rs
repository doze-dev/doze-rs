//! Where each instance of a stack is reached, decided by the core and told to the module (doze
//! `docs/design/next/module-names.md`, agreed 2026-11-07).
//!
//! A module describes its listeners (this port is TCP, that one HTTP, these host shapes go to that port); doze decides
//! where they end up. Two halves, kept apart on purpose:
//!
//! - [`plan`] is a pure function of the stack's configuration: no I/O, no running process. `doze env`, `doze status`
//!   and `endpoints.yaml` are computed from it by the CLI without the daemon, and come out the same as what the daemon
//!   serves. Every mistake a person can make (two listeners on one port without names, a sibling route colliding with
//!   another instance's name) is found here, before anything runs, with both parties named.
//! - [`bind`] carries a plan out against the machine's shared names registry: it claims each instance's name, twins
//!   the child and sibling routes onto it, opens every public port (through [`doze_names::Lease::listen`], so a Mac's
//!   translated addresses and Linux's bound ones look the same), publishes the routes to the front door, and returns
//!   the grant each module is told. Dropping the result gives everything back.

mod apply;
mod plan;

pub use apply::{Bound, BoundInstance, BoundListener, bind};
pub use plan::{Decl, InstancePlan, ListenerPlan, Plan, RouteHost, plan};

/// The address every listener is on when names are off or the machine is not set up.
pub const LOOPBACK: std::net::Ipv4Addr = std::net::Ipv4Addr::LOCALHOST;
