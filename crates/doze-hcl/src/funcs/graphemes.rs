//! Grapheme clusters as Go's `go-textseg/v15` finds them (Unicode 15.0). Newer Unicode adds rule
//! GB9c, which joins a consonant to the cluster before it when a virama (`InCB=Linker`) precedes
//! it; textseg breaks there. So the clusters of `unicode-segmentation` are split again
//! before any character that follows a virama (through extending characters only) and would not
//! itself extend a cluster.

use unicode_segmentation::UnicodeSegmentation;

/// Whether `c` is `Indic_Conjunct_Break=Linker` (a virama) in this crate's Unicode data: it joins two
/// consonants (KA, `c`, KA) into one cluster.
fn is_linker(c: char) -> bool {
    one_cluster(&['\u{915}', c, '\u{915}'])
}

/// Whether these characters, in order, make a single grapheme cluster.
fn one_cluster(chars: &[char]) -> bool {
    let s: String = chars.iter().collect();
    s.graphemes(true).nth(1).is_none()
}

/// Whether `c` extends a cluster on its own account (Extend, ZWJ, SpacingMark): `a` then `c` is one
/// cluster.
fn extends(c: char) -> bool {
    one_cluster(&['a', c])
}

/// The clusters with their byte offsets.
pub(super) fn clusters(s: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    for (at, g) in s.grapheme_indices(true) {
        if g.chars().nth(1).is_none() || !g.chars().any(is_linker) {
            out.push((at, g));
            continue;
        }
        let mut start = 0;
        let mut after_linker = false;
        for (i, c) in g.char_indices() {
            if i > 0 && after_linker && !extends(c) {
                out.push((at + start, g.get(start..i).unwrap_or("")));
                start = i;
                after_linker = false;
            }
            after_linker = is_linker(c) || (after_linker && extends(c));
        }
        out.push((at + start, g.get(start..).unwrap_or("")));
    }
    out
}

pub(super) fn count(s: &str) -> usize {
    clusters(s).len()
}
