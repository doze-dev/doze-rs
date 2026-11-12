//! Broker answers to view-models: the code that decides what a page will say, from real data. No HTML here.

mod attention;
mod config;
mod config_edit;
mod config_refuse;
mod config_reset;
mod create;
mod delete;
mod forms;
pub mod group;
pub mod group_read;
mod group_write;
pub mod groups;
mod home;
mod partitions;
mod produce;
pub mod protocol;
mod records;

pub use attention::{Looked, MAX_DESCRIBED, near_timeout, nearest, nearest_capped};
pub use home::{choose, counts, instance_view};

pub use config::{ConfigMissing, page as config_page};
pub use config_edit::{
    ConfigFields, EditState, EditWorld, changes_of as config_changes, checked_of as config_checked,
    form as config_form, input_of as config_input, missing as config_missing, save as config_save,
    shape as config_shape, view_of as config_view,
};
pub use config_reset::{form as config_reset_form, reset as config_reset};
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

pub use group::{group, lag_state};
pub use group_write::{
    Bounds, DeleteGroupResult, OffsetDeleteResult, ResetResult, delete as delete_group,
    delete_form as delete_group_form, offset_delete, offset_delete_form, parse_mode, plan_rows,
    reset as reset_offsets, reset_form,
};
pub use groups::{MOST_ROWS as GROUPS_MOST_ROWS, groups};
