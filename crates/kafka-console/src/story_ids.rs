//! The fragment ids of the Story pages: the part of the stream's closed vocabulary that slice 9f adds. `stream::parse_watch`
//! accepts these as well as its own.

/// The sentence under the group's name on the Story page.
pub const STORY_SENTENCE: &str = "story-sentence";
/// The Now card: the rebalance going on and the time the members have left.
pub const STORY_NOW: &str = "story-now";

/// What the stream may be asked to watch for these pages.
pub const WATCHABLE: &[&str] = &[STORY_SENTENCE, STORY_NOW];

/// What a group's Story page watches.
pub const STORY_IDS: &[&str] = &[STORY_SENTENCE, STORY_NOW];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_story_watches_is_in_the_vocabulary_and_the_stream_accepts_it() {
        for id in STORY_IDS {
            assert!(WATCHABLE.contains(id), "{id}");
        }
        assert!(crate::stream::parse_watch("story-now,story-sentence").is_ok());
        assert_eq!(crate::stream::parse_watch("story-nope"), Err("story-nope"));
    }
}
