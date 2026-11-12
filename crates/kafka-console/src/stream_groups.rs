//! The fragments the Groups pages ask the stream to watch: a closed vocabulary, registered with the stream by one line.

/// What the Groups list watches.
pub const GROUPS_IDS: &[&str] = &["groups-live", "groups-table"];
/// What a group's Members page watches.
pub const GROUP_IDS: &[&str] = &["group-sentence"];
/// What a group's Lag tab watches.
pub const LAG_IDS: &[&str] = &["group-lag-sentence", "group-lag-table"];
/// Every id above: the stream's vocabulary for these pages.
pub const WATCHABLE: &[&str] = &[
    "groups-live",
    "groups-table",
    "group-sentence",
    "group-lag-sentence",
    "group-lag-table",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_a_page_watches_is_in_the_vocabulary_and_the_pages_use_these_constants() {
        for id in GROUPS_IDS.iter().chain(GROUP_IDS).chain(LAG_IDS) {
            assert!(WATCHABLE.contains(id), "{id}");
        }
        assert!(WATCHABLE.contains(&crate::pages::groups::GROUPS_LIVE));
        assert!(WATCHABLE.contains(&crate::pages::groups::GROUPS_TABLE));
        assert!(WATCHABLE.contains(&crate::pages::groups::GROUP_SENTENCE));
        assert!(WATCHABLE.contains(&crate::pages::groups::LAG_SENTENCE));
        assert!(WATCHABLE.contains(&crate::pages::groups::LAG_TABLE));
    }
}
