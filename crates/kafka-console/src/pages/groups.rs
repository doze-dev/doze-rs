//! The Groups pages: the list, a group (classic, KIP-848, rebalancing), its members and a member, its lag in four states,
//! and the dialogs that reset or delete its offsets or the group.
//!
//! What the boards show and the broker cannot say, and what the page does instead (debt J91 to J120):
//! - the "Story" tab and its count, and the "Why it began" lines: the cause of a rebalance is not recorded until the group
//!   event ring of slice 9f; the tab is drawn without a link and the sentences say only what the group machine says now;
//! - the "What just happened" wire dock: the request recorder (slice 9e); it is not drawn;
//! - "the last rebalance was 6 h ago" and a member that "left 3 s ago": no record of either (9f);
//! - a member's generation `held` and its "Left" state are the machine's present state only.

mod common;
mod detail;
mod dialogs;
mod lag;
mod list;
mod members;
mod refused;

pub(crate) use common::tone;
pub use common::{GROUP_STATS, group_url, stats_fragment as group_stats_fragment};
pub(crate) use common::{
    head as group_head, story_tabs as group_story_tabs, switch as group_switch,
};
pub use detail::group_page;
pub use dialogs::{delete_dialog, offset_delete_dialog, reset_dialog};
pub(crate) use lag::cell_lag;
pub use lag::{LAG_TABLE, lag_page, lag_table_fragment};
pub use list::{
    GROUPS_LIVE, GROUPS_TABLE, groups_live_fragment, groups_page, groups_table_fragment,
};
pub use members::{member_drawer, members_page, members_page_open};
pub use refused::{delete_refused_page, no_group, reset_refused_page};
