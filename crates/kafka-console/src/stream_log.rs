//! The fragments the Log and Producers pages ask the stream to watch: a closed vocabulary, registered with the stream by one line.

/// What a topic's Log page watches.
pub const LOG_IDS: &[&str] = &["log-sentence", "log-ruler", "log-cards"];
/// What a topic's Producers page watches.
pub const PRODUCERS_IDS: &[&str] = &["producers-sentence", "producers-table"];
/// Every id above: the stream's vocabulary for these pages.
pub const WATCHABLE: &[&str] = &[
    "log-sentence",
    "log-ruler",
    "log-cards",
    "producers-sentence",
    "producers-table",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_a_page_watches_is_in_the_vocabulary_and_the_pages_use_these_constants() {
        for id in LOG_IDS.iter().chain(PRODUCERS_IDS) {
            assert!(WATCHABLE.contains(id), "{id}");
        }
        assert!(WATCHABLE.contains(&crate::pages::LOG_SENTENCE));
        assert!(WATCHABLE.contains(&crate::pages::LOG_RULER));
        assert!(WATCHABLE.contains(&crate::pages::LOG_CARDS));
        assert!(WATCHABLE.contains(&crate::pages::PRODUCERS_SENTENCE));
        assert!(WATCHABLE.contains(&crate::pages::PRODUCERS_TABLE));
    }
}
