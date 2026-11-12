//! The pages: each is `fn(&Model, &Base) -> Markup`, the content of `<main>` below the breadcrumb, written from the
//! board of its screen. The shell around it is `ui::shell`.

mod advertised;
mod common;
mod config;
mod connect;
pub mod groups;
mod home;
pub mod live;
mod notice;
pub mod topics;
mod topics_write;

pub use advertised::{advertised, probe_advertised};
pub use common::{PROBE_ADVERTISED, PROBE_CONNECT, PROBE_HOME, probe_fragment_url};
pub use config::{
    config_page, config_refused, config_saved, config_url, dialog_title as config_dialog_title,
    edit_dialog as config_edit_dialog, edit_overlay as config_edit_overlay,
    edit_url as config_edit_url, no_config as config_no_key, reset_dialog as config_reset_dialog,
    reset_url as config_reset_url, toggled as config_toggled, token as config_token,
    with_overlay as config_with_overlay,
};
pub use connect::{connect, probe_connect};
pub use home::{HOME_LIVE, callout_body, home, live_fragment, probe_home};
pub use notice::{not_found, planned};
pub use topics_write::{
    add_partitions_dialog, create_page, create_refused, created, delete_dialog,
    delete_records_dialog, delete_records_refused, deleted_page, no_topic, produce_dialog,
    produce_refused, produced,
};
