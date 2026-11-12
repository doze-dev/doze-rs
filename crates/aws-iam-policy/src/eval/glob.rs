//! Wildcard matching as IAM does it: `*` is any run of characters (also none), `?` is any one character, everything else is itself.

/// Whether `text` matches `pattern`. `ignore_case` folds ASCII case.
#[must_use]
pub(super) fn matches(pattern: &str, text: &str, ignore_case: bool) -> bool {
    let fold = |c: char| {
        if ignore_case {
            c.to_ascii_lowercase()
        } else {
            c
        }
    };
    let p: Vec<char> = pattern.chars().map(fold).collect();
    let t: Vec<char> = text.chars().map(fold).collect();
    let (mut pi, mut ti) = (0, 0);
    // Where the last `*` was, and how much of the text it has taken so far: the one place to go back to.
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some((pi, ti));
                pi += 1;
            }
            Some(c) if *c == '?' || Some(c) == t.get(ti) => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    p.get(pi..)
        .is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn wildcards() {
        for (p, t) in [
            ("s3:*", "s3:GetObject"),
            ("s3:Get*", "s3:GetObject"),
            ("*", ""),
            ("a?c", "abc"),
            ("*a*b*", "xxaxxbxx"),
            ("a*", "a"),
        ] {
            assert!(matches(p, t, false), "{p} {t}");
        }
        for (p, t) in [
            ("s3:Get*", "s3:PutObject"),
            ("a?c", "ac"),
            ("a", "A"),
            ("a*b", "a"),
            ("", "x"),
        ] {
            assert!(!matches(p, t, false), "{p} {t}");
        }
        assert!(matches("S3:GETOBJECT", "s3:GetObject", true));
        assert!(matches("*", "anything", false) && matches("", "", false));
    }

    #[test]
    fn a_pattern_of_many_stars_is_not_exponential() {
        let pattern = "*a".repeat(40);
        assert!(!matches(&pattern, &"a".repeat(60).replace('a', "b"), false));
        assert!(matches(&pattern, &"a".repeat(80), false));
    }
}
