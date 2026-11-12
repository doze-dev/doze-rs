//! The Topics pages: the list, a topic with its Messages, one record, its partitions, and the offline state.
//!
//! What the boards show and the broker cannot say, and what the page does instead (each is a debt row, J21 to J40):
//! - "written by <client>" on the list and "wire" docks on the topic page: the request recorder (slice 9e). The list says
//!   "read by" only; the dock is not drawn.
//! - The record count on the list rows: ListOffsets would open every partition just to look at a list, so the list shows
//!   what the files say (empty, size, last write) and the topic page shows the count.
//! - When a partition went offline: the broker keeps the cause, not the time.
//! - "Producers N" and "Consumers N" are counts of active producers and of groups with a committed offset on the topic.

mod bar;
mod committed;
mod common;
mod consumers;
mod list;
mod messages;
mod offline;
mod partitions;
mod record;
mod table;
mod wire;

pub use common::{Acts, TopicTab, acts_of, topic_frame, topic_tabs_on, topic_url, unknown};
pub use common::{TOPIC_SENTENCE, sentence_fragment as topic_sentence_fragment};
pub use consumers::{CONSUMERS_REGION, consumers_page};
pub use list::{LIST_FRAGMENT, LIST_SENTENCE, list_fragment, list_page, sentence_fragment};
pub use messages::{MESSAGES_FRAGMENT, messages_fragment, messages_page};
pub use partitions::partitions_page;
pub use record::{no_record, record_drawer};
pub use wire::topic_wire_page;
