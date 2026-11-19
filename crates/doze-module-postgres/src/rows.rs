//! psql's unaligned, tuples-only output read as rows, with nothing else of the crate in it so the fuzz target can take it
//! whole.

// fuzz:parser fuzz_rows

/// Separates fields in rows read back (psql's `-F`): the unit separator, which no catalog value holds.
pub(crate) const SEP: char = '\x1f';

/// Rows of `n` fields from psql's unaligned output; a line of any other width (a value holding a line break) is
/// left out rather than read as fields it does not have.
pub(crate) fn split(out: &str, n: usize) -> Vec<Vec<String>> {
    out.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.split(SEP).map(str::to_owned).collect::<Vec<_>>())
        .filter(|f| f.len() == n)
        .collect()
}
