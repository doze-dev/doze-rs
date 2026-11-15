//! How the palette filters: each word typed must begin a word of the label, in order. "re" finds restart, reset
//! and rerun and not "wake worker", whose letters are there but nowhere a word starts; "reset d" finds reset db.

use crate::gonum::lower;

/// Whether `query` matches `label` by word starts, and if so the positions (in characters) of the letters matched,
/// for drawing them. An empty query matches, with nothing drawn. (Go: `FuzzyMatch`.)
#[must_use]
pub fn fuzzy_match(query: &str, label: &str) -> Option<Vec<usize>> {
    let label: Vec<char> = label.chars().map(lower).collect();
    let mut at = Vec::new();
    let mut pos = 0;
    for term in query.split_whitespace() {
        let term: Vec<char> = term.chars().map(lower).collect();
        let start = (pos..label.len()).find(|&p| {
            let word_start = p == 0 || label.get(p - 1) == Some(&' ');
            word_start && label.get(p..p + term.len()) == Some(term.as_slice())
        })?;
        at.extend(start..start + term.len());
        pos = start + term.len();
    }
    Some(at)
}

/// Ranks matches, lower being better: letters together, and near the front, first. (Go: `FuzzyScore`.)
#[must_use]
pub fn fuzzy_score(at: &[usize]) -> usize {
    let Some(&first) = at.first() else {
        return 0;
    };
    first
        + at.windows(2)
            .map(|w| if let [a, b] = w { b - a - 1 } else { 0 })
            .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Go: TestPaletteMatchesWordStarts. The palette matches word starts, not scattered letters.
    #[test]
    fn palette_matches_word_starts() {
        for (query, label, want) in [
            ("re", "restart api", true),
            ("re", "reset db", true),
            ("re", "rerun task migrate", true),
            ("re", "wake worker", false),
            ("reset d", "reset db", true),
            ("reset db", "reset db", true),
            ("db", "reset db", true),
            ("rdb", "reset db", false),
            ("", "anything", true),
            ("res", "restart api", true),
            ("resx", "restart api", false),
        ] {
            assert_eq!(
                fuzzy_match(query, label).is_some(),
                want,
                "fuzzy_match({query:?}, {label:?})"
            );
        }
    }
}
