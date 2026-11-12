//! The HTTP front of doze-aws.
//!
//! A [`Gateway`] owns the listening socket and a set of [`aws_wire::service::Service`]s. A request is routed by the service
//! named in its SigV4 credential scope, authenticated (verified, or trusted: see [`Auth`]), read into memory up to a limit,
//! matched to an operation, decoded and validated against the model, handed to the service as an owned
//! [`aws_wire::service::Call`], and answered. Nothing here knows what any service does.
//!
//! The awsJson and awsQuery protocols are served so far; a service that speaks another is refused by name with a 501.

mod body;
mod cbor;
mod config;
mod pipeline;
mod presign;
mod query;
mod rest;
mod s3host;
mod server;
mod target;
mod upload;
mod web;

pub use config::{Auth, Config};
pub use server::{Gateway, Running};
