//! Broker answers to view-models: the code that decides what a page will say, from real data. No HTML here.

mod attention;
mod home;

pub use attention::{Looked, MAX_DESCRIBED, near_timeout, nearest, nearest_capped};
pub use home::{choose, counts, instance_view};
