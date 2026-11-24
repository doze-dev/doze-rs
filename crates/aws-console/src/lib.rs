//! The doze-aws web console: a view of the running doze-aws, in the same process, on a listener of its own.
//!
//! Its own port, not the gateway's: the gateway serves objects out of local S3, and a page served from a bucket on the console's
//! origin could script the console. On its own origin the console answers only its own pages.
//!
//! - [`serve`] binds nothing itself: it is handed a listener, serves it under limits ([`limits`]), and stops when told.
//! - [`app`] is the router: the JSON API ([`api`], under `/api/v1/`), the embedded bundle ([`web`], `crates/aws-console/web-dist/`),
//!   and `index.html` for every other GET (client routing). In front of all of it: security headers ([`headers`]), the `Host`
//!   guard ([`guard`]) and a body limit; the API also asks for its proof header and, for anything but a read, the same-origin
//!   proof ([`origin`]).
//! - [`ring`] is the Live record: a bounded ring of finished calls, fed by the gateway on its request path
//!   ([`aws_gateway::Observer`]), with no timer of its own. A reader is woken by a call, never by a clock.
//! - [`resources`] is the Resources space: each service's descriptor, read and changed through the peer bus as the caller
//!   `console`.
//! - [`model`] is the contract: the view-models the API answers with, from which a test writes the TypeScript types of
//!   `console/apps/aws/src/api/types/` (a stale file fails it).
//!
//! `files`, `origin`, `headers`, `limits` and the connection loop of `serve` are doze-kafka's console code made product-neutral;
//! they belong in a crate both consoles share (see the note in each).

pub mod agent;
pub mod api;
pub mod app;
pub mod conns;
pub mod facts;
pub mod files;
pub mod guard;
pub mod headers;
pub mod iam;
pub mod limits;
pub mod model;
pub mod origin;
pub mod regions;
pub mod resources;
pub mod ring;
pub mod serve;
pub mod state;
pub mod web;

pub use facts::{Facts, Flag, LocalData, ServiceFacts};
pub use limits::Limits;
pub use ring::Recorder;
pub use serve::{Running, Settings, start_on, start_with};
