//! Components of the Kafka pages: the title row, the tabs of a topic, empty states, sections, fields of a record, the
//! call a page makes, the pager and the filter box. Each is a function of plain values; none takes markup that came from
//! data except as an escaped string.

mod chrome;
mod record;
mod ruler;

pub use chrome::{Tab, apicall, empty, filter_box, pager, sec, tabs, title_row};
pub use record::{field, headers};
pub use ruler::{Mark, ruler};
