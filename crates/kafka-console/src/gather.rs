//! Broker answers to view-models: the code that decides what a page will say, from real data. No HTML here.

mod attention;
mod create;
mod delete;
mod forms;
mod home;
mod partitions;
mod produce;
mod records;

pub use attention::{near_timeout, nearest};
pub use home::{choose, counts, instance_view};

pub use create::{
    CLEANUP, Inputs, RETENTION, create, errors, inputs as create_inputs, landing, landing_view,
    template_values, view_at as create_view,
};
pub use delete::{DeleteResult, LIST_MOST, delete, form as delete_form};
pub use partitions::{AddResult, add, form as add_form};
pub use produce::{form as produce_form, send as produce};
pub use records::{RecordsResult, delete as delete_records, form as delete_records_form};
mod configs;
pub mod decode;
mod messages;
mod meta;
mod read;
mod readers;
mod record;
mod topic;
mod topics;
mod txns;
mod walk;

pub use messages::page as messages;
pub use record::one as record;
pub use topic::head as topic_head;
pub use topics::list as topics;
