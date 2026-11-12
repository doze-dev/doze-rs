//! A topic's wire page (board `topic-wire`): the topic's head and tabs with the dock opened up.

use maud::{Markup, html};

use super::common::{Acts, topic_head, topic_tabs};
use crate::base::Base;
use crate::model::TopicHead;

/// The topic's page with `dock` (made by `pages::live::topic_dock`, expanded) under its tabs.
#[must_use]
pub fn topic_wire_page(h: &TopicHead, dock: Markup, base: &Base) -> Markup {
    html! {
        (topic_head(h, base, Acts::Full))
        (topic_tabs(h, base))
        (dock)
    }
}
