//! The regions of each page: the parts a person's Refresh fetches again (CON-803, `docs/port/console/live-design.md`).
//!
//! A region is an element with an id and a `data-frag` address ([`crate::ui::client::fragment`]); the page lists its regions when
//! it is made, and the shell draws the Refresh control only for a page that lists some. A page that lists none is a snapshot with
//! nothing to refresh (Config, Settings, Access, Connect). The ids are stable and name what the region is (`topics-table`);
//! a region holds data (a strip of metrics, a table), never a header, a toolbar or a sentence.

/// Home: the strip, the transaction region and the cards.
pub const HOME: &[&str] = &["home-live"];
/// The topics list.
pub const TOPICS: &[&str] = &["topics-stats", "topics-table"];
/// A topic's Messages page.
pub const TOPIC: &[&str] = &["topic-stats", "topic-messages"];
/// A topic's Messages page with the wire dock under it.
pub const TOPIC_DOCKED: &[&str] = &["topic-stats", "topic-messages", "topic-wire"];
/// A topic's wire page.
pub const TOPIC_WIRE: &[&str] = &["topic-stats", "topic-wire"];
/// A topic's Log page, with the wire dock.
pub const LOG: &[&str] = &["log-stats", "log-ruler", "log-cards", "topic-wire"];
/// A topic's Producers page, with the wire dock.
pub const PRODUCERS: &[&str] = &["producers-stats", "producers-table", "topic-wire"];
/// The groups list.
pub const GROUPS: &[&str] = &["groups-live", "groups-table"];
/// A group's Members page.
pub const GROUP: &[&str] = &["group-stats"];
/// A group's Lag tab.
pub const LAG: &[&str] = &["group-stats", "group-lag-table"];
/// A group's Story page.
pub const STORY: &[&str] = &["story-stats", "story-now"];
/// A group's wire page.
pub const GROUP_WIRE: &[&str] = &["group-wire"];
/// The Live page.
pub const LIVE: &[&str] = &["live-stats", "live-wire"];

/// The tail regions: the only ones that can stream, and only when a person turns the tail on.
pub const LIVE_WIRE: &str = "live-wire";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_lists_each_region_once() {
        for page in [
            HOME,
            TOPICS,
            TOPIC,
            TOPIC_DOCKED,
            TOPIC_WIRE,
            LOG,
            PRODUCERS,
            GROUPS,
            GROUP,
            LAG,
            STORY,
            GROUP_WIRE,
            LIVE,
        ] {
            let mut seen = std::collections::HashSet::new();
            assert!(page.iter().all(|id| seen.insert(*id)), "{page:?}");
        }
    }
}
