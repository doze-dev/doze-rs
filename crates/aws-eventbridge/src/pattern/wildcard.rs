//! The `wildcard` condition: `*` stands for any run of characters, `\*` for an asterisk and `\\` for a backslash.

/// The literal pieces between the wildcards of `pattern`, in order. A pattern with `n` wildcards has `n + 1` pieces; a piece is empty
/// where a wildcard is first, last, or beside another.
fn pieces(pattern: &str) -> Result<Vec<String>, String> {
    let mut out = vec![String::new()];
    let mut chars = pattern.chars().enumerate();
    let mut after_star = false;
    while let Some((i, c)) = chars.next() {
        match c {
            '*' => {
                if after_star {
                    return Err(format!("Consecutive wildcard characters at pos {i}"));
                }
                after_star = true;
                out.push(String::new());
                continue;
            }
            '\\' => match chars.next() {
                Some((_, e @ ('*' | '\\'))) => push(&mut out, e),
                _ => return Err(format!("Invalid escape character at pos {i}")),
            },
            c => push(&mut out, c),
        }
        after_star = false;
    }
    Ok(out)
}

fn push(out: &mut [String], c: char) {
    if let Some(last) = out.last_mut() {
        last.push(c);
    }
}

/// Whether the pattern is well formed.
///
/// # Errors
/// The reason, in the words EventBridge uses (from memory).
pub(super) fn check(pattern: &str) -> Result<(), String> {
    pieces(pattern).map(drop)
}

/// Whether `value` matches `pattern`. A pattern that is malformed matches nothing (it was refused when it was put).
pub(super) fn matches(pattern: &str, value: &str) -> bool {
    let Ok(pieces) = pieces(pattern) else {
        return false;
    };
    let [first, middle @ .., last] = pieces.as_slice() else {
        // No wildcard: the pattern is one literal piece.
        return pieces.first().is_some_and(|p| p == value);
    };
    let Some(mut rest) = value.strip_prefix(first.as_str()) else {
        return false;
    };
    for piece in middle {
        match rest.find(piece.as_str()) {
            Some(at) => rest = rest.get(at + piece.len()..).unwrap_or_default(),
            None => return false,
        }
    }
    rest.ends_with(last.as_str())
}
