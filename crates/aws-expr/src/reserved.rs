//! DynamoDB's reserved words: a bare attribute name in an expression may not be one of these (in any case); the way to say it anyway
//! is an expression attribute name. The list is AWS's published one, 573 words, misspellings included (FLATTERN, INNTER, LOGED are
//! theirs). It is a text file of one upper-case word a line, compiled in; nothing is built at start, and a lookup reads the lines.

/// The reserved words, upper case, one to a line.
static WORDS: &str = include_str!("reserved.txt");

/// Whether `word` is reserved.
#[must_use]
pub fn is_reserved(word: &str) -> bool {
    WORDS.lines().any(|w| w.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_whole_and_sorted() {
        assert_eq!(WORDS.lines().count(), 573);
        let words: Vec<&str> = WORDS.lines().collect();
        assert!(words.windows(2).all(|w| w[0] < w[1]));
        for w in ["name", "owner", "status", "timestamp", "Count"] {
            assert!(is_reserved(w), "{w}");
        }
        assert!(!is_reserved("pk") && !is_reserved("title"));
    }
}
