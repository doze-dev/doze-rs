//! Go's case mapping (`unicode.ToUpper`, `ToLower`, `ToTitle`): one character to one character, by
//! Unicode's simple mappings. Rust's `char::to_uppercase` gives the full mappings, which may be
//! several characters (`ß` → `SS`); where they are, Go keeps the character, except for the few
//! whose simple mapping differs from the character itself.

/// `unicode.ToUpper`.
pub(super) fn upper(c: char) -> char {
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => iota_subscript_title(c).unwrap_or(c),
    }
}

/// `unicode.ToLower`.
pub(super) fn lower(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        // Only U+0130 (İ) has a multi-character full lowercase; its simple mapping is `i`.
        _ if c == '\u{130}' => 'i',
        _ => c,
    }
}

/// `unicode.ToTitle`: the uppercase, except for the four digraphs (which have their own titlecase
/// forms) and Georgian Mkhedruli letters (whose titlecase is themselves).
pub(super) fn title(c: char) -> char {
    match c {
        '\u{1C4}'..='\u{1C6}' => '\u{1C5}',
        '\u{1C7}'..='\u{1C9}' => '\u{1C8}',
        '\u{1CA}'..='\u{1CC}' => '\u{1CB}',
        '\u{1F1}'..='\u{1F3}' => '\u{1F2}',
        '\u{10D0}'..='\u{10FA}' | '\u{10FD}'..='\u{10FF}' => c,
        _ => upper(c),
    }
}

/// Greek letters with ypogegrammeni: their full uppercase is two characters, their simple uppercase
/// is the titlecase form with prosgegrammeni.
fn iota_subscript_title(c: char) -> Option<char> {
    let u = u32::from(c);
    let t = match u {
        0x1F80..=0x1F87 | 0x1F90..=0x1F97 | 0x1FA0..=0x1FA7 => u + 8,
        0x1FB3 => 0x1FBC,
        0x1FC3 => 0x1FCC,
        0x1FF3 => 0x1FFC,
        _ => return None,
    };
    char::from_u32(t)
}

/// `strings.Title`: the title case of every letter that follows a separator (anything but an ASCII
/// letter, digit or underscore; outside ASCII, only white space).
pub(super) fn title_words(s: &str) -> String {
    let mut prev = ' ';
    s.chars()
        .map(|c| {
            let out = if is_separator(prev) { title(c) } else { c };
            prev = c;
            out
        })
        .collect()
}

fn is_separator(c: char) -> bool {
    if c.is_ascii() {
        return !(c.is_ascii_alphanumeric() || c == '_');
    }
    c.is_whitespace()
}
