//! A topic pattern a member subscribes with (ConsumerGroupHeartbeat v1). Kafka matches the whole topic name against an
//! RE2/J expression; `regex-lite` takes the same syntax without Unicode classes, which topic names (ASCII) never need.

use regex_lite::Regex;

/// A compiled subscription pattern and the text it came from.
#[derive(Debug, Clone)]
pub(crate) struct Pattern {
    text: String,
    whole: Regex,
}

impl Pattern {
    /// `Err` is the engine's words for what is wrong with it.
    pub(crate) fn compile(text: &str) -> Result<Self, String> {
        let whole = Regex::new(&format!("^(?:{text})$")).map_err(|e| e.to_string())?;
        Ok(Self {
            text: text.to_owned(),
            whole,
        })
    }

    #[must_use]
    pub(crate) fn matches(&self, topic: &str) -> bool {
        self.whole.is_match(topic)
    }

    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_matches_the_whole_name_and_a_bad_one_is_refused() {
        let p = Pattern::compile("orders-.*").unwrap();
        assert!(p.matches("orders-eu") && !p.matches("my-orders-eu") && !p.matches("orders"));
        assert!(Pattern::compile("a|b").unwrap().matches("b"));
        assert!(
            !Pattern::compile("a|b").unwrap().matches("ab"),
            "the alternation does not escape the anchors"
        );
        assert!(Pattern::compile("(").is_err());
    }
}
