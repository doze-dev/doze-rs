//! Broker answers to view-models: the code that decides what a page will say, from real data. No HTML here.

mod attention;
mod home;

pub use attention::{near_timeout, nearest};
pub use home::{choose, counts, instance_view};
