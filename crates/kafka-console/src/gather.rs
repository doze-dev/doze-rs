//! Broker answers to view-models: the code that decides what a page will say, from real data. No HTML here.

mod attention;
mod configs;
pub mod decode;
mod home;
mod messages;
mod meta;
mod read;
mod readers;
mod record;
mod topic;
mod topics;
mod txns;
mod walk;

pub use attention::{near_timeout, nearest};
pub use home::{choose, counts, instance_view};
pub use messages::page as messages;
pub use record::one as record;
pub use topic::head as topic_head;
pub use topics::list as topics;
