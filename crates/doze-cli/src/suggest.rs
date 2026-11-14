//! "Did you mean": the nearest known name to a typo, close enough to be the same word misspelt.

/// The candidate nearest to `typo` by edit distance, if it is within a third of the word (at least one edit).
pub(crate) fn nearest<'a>(typo: &str, candidates: &'a [String]) -> Option<&'a str> {
    let limit = (typo.chars().count() / 3).max(1);
    candidates
        .iter()
        .filter(|c| c.as_str() != typo)
        .map(|c| (distance(typo, c), c))
        .filter(|(d, _)| *d <= limit)
        .min_by(|(d1, c1), (d2, c2)| d1.cmp(d2).then_with(|| c1.cmp(c2)))
        .map(|(_, c)| c.as_str())
}

/// Levenshtein distance, by characters, in one row.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diag = i;
        let mut left = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let up = row.get(j + 1).copied().unwrap_or(usize::MAX);
            let cost = usize::from(ca != *cb);
            let here = (diag + cost).min(up + 1).min(left + 1);
            diag = up;
            if let Some(slot) = row.get_mut(j + 1) {
                *slot = here;
            }
            left = here;
        }
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
    }
    row.last().copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typo_finds_its_word_and_a_stranger_finds_none() {
        let known: Vec<String> = ["valkey", "postgres", "kvrocks"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(nearest("valky", &known), Some("valkey"));
        assert_eq!(nearest("postgre", &known), Some("postgres"));
        assert_eq!(nearest("mongo", &known), None);
        assert_eq!(distance("kitten", "sitting"), 3);
    }
}
