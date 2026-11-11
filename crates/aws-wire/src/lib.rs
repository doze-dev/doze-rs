//! AWS wire protocols, sans-IO.
//!
//! - [`ApiError`]: a failure as a service states it; each protocol words it its own way.
//! - [`RequestId`]: the id every answer carries.
//! - [`json`]: the awsJson1.0 and 1.1 protocols: route by `X-Amz-Target`, answer, and fail.
//! - [`service`]: the contract between a service and the gateway (a decoded [`service::Call`] in, an [`service::Output`] out).
//! - [`from_auth`]: authentication failures, worded per protocol family.
//!
//! The Query, REST and CBOR protocols join as the services that speak them are built.

mod auth_error;
mod error;
pub mod json;
mod request_id;
pub mod service;

pub use auth_error::{Family, from_auth};
pub use error::{ApiError, Fault};
pub use request_id::RequestId;
