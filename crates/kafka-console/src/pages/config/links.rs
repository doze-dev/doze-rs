//! The address of the Config page for what it was asked to show, and the toggles that change one part of it. With no
//! script every expander, chip and filter is a plain link or a GET form, so the page is the whole state.

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

/// The same page with `token` opened if it was shut and shut if it was open.
#[must_use]
pub fn toggled(ask: &ConfigAsk, token: &str) -> ConfigAsk {
    let mut next = ask.clone();
    if let Some(at) = next.open.iter().position(|t| t == token) {
        next.open.remove(at);
    } else {
        next.open.push(token.to_owned());
    }
    next
}

/// The address of the page for `ask`, with the anchor of the row it is about.
#[must_use]
pub(super) fn url(base: &Base, topic: &str, ask: &ConfigAsk, anchor: Option<&str>) -> String {
    let open = ask.open.join(",");
    let query = encode_query(&[
        ("q", &ask.q),
        ("show", ask.show.word()),
        ("open", &open),
        ("replication", if ask.replication { "1" } else { "" }),
    ]);
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
            "/topics/orders/config?q=seg%20ment&show=changed&open=where%3Asegment.bytes&replication=1#c-segment.bytes"
        );
    }

    #[test]
    fn an_expander_opens_when_shut_and_shuts_when_open() {
        let a = ask();
        let shut = toggled(&a, "where:segment.bytes");
        assert!(shut.open.is_empty());
        let both = toggled(&a, "text:retention.ms");
        assert_eq!(both.open, ["where:segment.bytes", "text:retention.ms"]);
        assert_eq!(toggled(&both, "text:retention.ms"), a);
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
