//! The `wildcard` operator of a filter policy (SNS Developer Guide, "String value matching": `{"wildcard": "*ball"}` and
//! `{"anything-but": {"wildcard": "*ball"}}`): `*` stands for any run of characters, `\*` for an asterisk and `\\` for a backslash. The
//! rule and the refusals' words are EventBridge's (whose pattern language SNS shares; the words are from memory); a copy of
//! `aws-eventbridge`'s matcher, kept here so SNS does not depend on EventBridge for forty lines.

/// The literal pieces between the wildcards of `pattern`.
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
/// Why it is not.
pub(crate) fn check(pattern: &str) -> Result<(), String> {
    pieces(pattern).map(drop)
}

/// Whether `value` matches `pattern`.
pub(crate) fn matches(pattern: &str, value: &str) -> bool {
    let Ok(pieces) = pieces(pattern) else {
        return false;
    };
    let [first, middle @ .., last] = pieces.as_slice() else {
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
    rest.len() >= last.len() && rest.ends_with(last.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_stand_for_any_run_and_escapes_for_themselves() {
        assert!(matches("*ball", "baseball"));
        assert!(!matches("*ball", "hockey"));
        assert!(matches("a*c*e", "abcde"));
        assert!(matches("a\\*", "a*"));
        assert!(!matches("a\\*", "ab"));
        assert!(matches("exact", "exact"));
        assert!(check("a**b").is_err());
        assert!(check("a\\b").is_err());
    }
}
