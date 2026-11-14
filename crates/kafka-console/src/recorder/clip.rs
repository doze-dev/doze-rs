//! Clipping: nothing a client sent is kept longer than the recorder says, and never in the middle of a character.

/// The longest a name is kept (a client id, a topic, a group, a member).
pub const NAME: usize = 128;
/// The longest free text is kept (a reason, a broker's own message, a note).
pub const TEXT: usize = 256;

/// `s` cut to at most `max` bytes at a character boundary, with a marker when it was cut.
#[must_use]
pub fn clip(s: &str, max: usize) -> Box<str> {
    if s.len() <= max {
        return s.into();
    }
    let mut end = max.saturating_sub(3);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 3);
    out.push_str(s.get(..end).unwrap_or(""));
    out.push_str("...");
    out.into_boxed_str()
}

/// [`clip`] of a name.
#[must_use]
pub fn name(s: &str) -> Box<str> {
    clip(s, NAME)
}

/// [`clip`] of free text, and `None` for text that is empty.
#[must_use]
pub fn text(s: &str) -> Option<Box<str>> {
    (!s.is_empty()).then(|| clip(s, TEXT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_string_is_kept_and_a_long_one_is_cut_with_a_marker_within_the_bound() {
        assert_eq!(&*clip("orders", 8), "orders");
        assert_eq!(&*clip("12345678", 8), "12345678");
        let cut = clip("123456789", 8);
        assert_eq!(&*cut, "12345...");
        assert!(cut.len() <= 8);
    }

    #[test]
    fn a_cut_never_lands_inside_a_character() {
        for max in 0..24 {
            let cut = clip("a\u{1F600}b\u{E9}c\u{1F600}\u{1F600}d", max);
            assert!(cut.len() <= max.max(3), "{max}: {cut:?}");
            assert!(std::str::from_utf8(cut.as_bytes()).is_ok());
        }
    }

    #[test]
    fn a_900_kib_text_is_kept_at_the_text_bound() {
        let big = "r".repeat(900 << 10);
        assert_eq!(text(&big).map(|t| t.len()), Some(TEXT));
        assert_eq!(text(""), None);
    }
}
