//! The address of the Config page for what it was asked to show. The filter and the chips are plain links or a GET form, so
//! with no script the address is the state of the list. What is open (a row's "Where from", Kafka's text, the replication
//! section) is NOT in it: those are native disclosures that open in place; `?open=` and `?replication=1` are only deep links
//! that the server renders open.

use super::common::config_url;
use crate::base::Base;
use crate::form::encode_query;
use crate::model::{ConfigAsk, ConfigShow};

impl ConfigShow {
    /// The word the address carries; the whole list carries none.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::All => "",
            Self::Changed => "changed",
            Self::Compaction => "compaction",
            Self::Segments => "segments",
        }
    }

    /// The chip a word names; anything else is the whole list.
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "changed" => Self::Changed,
            "compaction" => Self::Compaction,
            "segments" => Self::Segments,
            _ => Self::All,
        }
    }
}

/// An expander's name in the address: `where:segment.bytes` or `text:retention.ms`.
#[must_use]
pub fn token(kind: &str, key: &str) -> String {
    format!("{kind}:{key}")
}

/// The address of the page for the filter in `ask` (what is open is not part of it), with the anchor of the row it is about.
#[must_use]
pub(super) fn url(base: &Base, topic: &str, ask: &ConfigAsk, anchor: Option<&str>) -> String {
    let query = encode_query(&[("q", &ask.q), ("show", ask.show.word())]);
    let tail = anchor.map_or_else(String::new, |a| format!("#c-{}", crate::base::segment(a)));
    format!("{}{query}{tail}", config_url(base, topic, ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask() -> ConfigAsk {
        ConfigAsk {
            q: "seg ment".to_owned(),
            show: ConfigShow::Changed,
            open: vec!["where:segment.bytes".to_owned()],
            replication: true,
        }
    }

    #[test]
    fn the_address_carries_what_was_asked_and_nothing_that_was_not() {
        let base = Base::root();
        assert_eq!(
            url(&base, "orders", &ConfigAsk::default(), None),
            "/topics/orders/config"
        );
        assert_eq!(
            url(&base, "orders", &ask(), Some("segment.bytes")),
            "/topics/orders/config?q=seg%20ment&show=changed#c-segment.bytes",
            "the filter is in the address; what is open (an expander) is not"
        );
    }

    #[test]
    fn a_chip_word_names_its_chip_and_any_other_word_is_the_whole_list() {
        for s in [
            ConfigShow::All,
            ConfigShow::Changed,
            ConfigShow::Compaction,
            ConfigShow::Segments,
        ] {
            assert_eq!(ConfigShow::from_word(s.word()), s);
        }
        assert_eq!(ConfigShow::from_word("everything"), ConfigShow::All);
    }
}
