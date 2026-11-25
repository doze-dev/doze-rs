//! The pattern language of a rule's resource list: `*` fits any run of characters (none too), `?` fits any one character, and
//! everything else fits itself. Nothing is escaped: a name with a literal `*` or `?` in it is not one a rule can single out.
//!
//! The walk keeps one backtrack point (the last `*` seen), so it takes at most the length of the pattern times the length of the
//! name steps, and never recurses: a hostile pattern such as `*a*a*a*a*b` cannot make a match slow the way a backtracking
//! expression engine would.

/// Whether `name` fits `pattern`.
///
/// It works on characters, not bytes, so `?` fits `é` as it fits `e`.
// fuzz:parser fuzz_lab_glob
#[must_use]
pub fn fits(pattern: &str, name: &str) -> bool {
    let (mut p, mut n) = (pattern, name);
    // Where the last `*` left off: the rest of the pattern after it, and the rest of the name it has not yet been given.
    let mut star: Option<(&str, &str)> = None;
    loop {
        let mut pc = p.chars();
        match pc.next() {
            Some('*') => {
                p = pc.as_str();
                star = Some((p, n));
                continue;
            }
            Some(c) => {
                let mut nc = n.chars();
                if let Some(x) = nc.next()
                    && (c == '?' || c == x)
                {
                    p = pc.as_str();
                    n = nc.as_str();
                    continue;
                }
            }
            None if n.is_empty() => return true,
            None => {}
        }
        // A mismatch: let the last `*` take one more character of the name and try the rest of the pattern again.
        let Some((after, at)) = star else {
            return false;
        };
        let mut ac = at.chars();
        if ac.next().is_none() {
            return false;
        }
        let rest = ac.as_str();
        star = Some((after, rest));
        p = after;
        n = rest;
    }
}

/// The last name in what a call names: `orders` in a queue URL `http://localhost:4566/000000000000/orders` and in the ARN
/// `arn:aws:sqs:us-east-1:000000000000:orders`, the file of an S3 `bucket/dir/file.png`. A pattern fits the whole text or this.
#[must_use]
pub fn last_name(resource: &str) -> &str {
    resource
        .rsplit(['/', ':'])
        .find(|part| !part.is_empty())
        .unwrap_or(resource)
}

#[cfg(test)]
mod tests {
    use super::{fits, last_name};

    #[test]
    fn a_pattern_fits_by_its_stars_and_question_marks() {
        for (pattern, name, want) in [
            ("orders", "orders", true),
            ("orders", "order", false),
            ("orders*", "orders-dlq", true),
            ("*-dlq", "orders-dlq", true),
            ("*-dlq", "orders", false),
            ("*", "", true),
            ("*", "anything at all", true),
            ("", "", true),
            ("", "x", false),
            ("a?c", "abc", true),
            ("a?c", "ac", false),
            ("a?c", "abbc", false),
            ("a*b*c", "a--b--c", true),
            ("a*b*c", "a--b--", false),
            ("**", "x", true),
            ("?*", "", false),
            ("?*", "x", true),
            ("*?", "x", true),
            ("img/*.png", "img/a/b.png", true),
            ("img/*.png", "img/a/b.jpg", false),
        ] {
            assert_eq!(fits(pattern, name), want, "{pattern:?} against {name:?}");
        }
    }

    #[test]
    fn a_question_mark_fits_a_character_not_a_byte() {
        assert!(fits("caf?", "café"));
        assert!(!fits("caf?", "cafés"));
        assert!(fits("caf??", "cafés"));
        assert!(fits("*é", "café"));
        assert!(fits("日?語", "日本語"));
    }

    #[test]
    fn a_hostile_pattern_is_linear_not_exponential() {
        // A backtracking engine takes 2^40 steps over this; the walk takes at most 80 x 80.
        let pattern = format!("{}b", "*a".repeat(40));
        let name = "a".repeat(80);
        let started = std::time::Instant::now();
        assert!(!fits(&pattern, &name));
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        // And the longest pattern and name a rule can be asked about.
        let pattern = format!("{}b", "*a".repeat(128));
        let name = "a".repeat(1024);
        let started = std::time::Instant::now();
        assert!(!fits(&pattern, &name));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn what_a_call_names_has_a_last_name() {
        assert_eq!(
            last_name("http://localhost:4566/000000000000/orders"),
            "orders"
        );
        assert_eq!(
            last_name("arn:aws:sqs:us-east-1:000000000000:orders"),
            "orders"
        );
        assert_eq!(last_name("images/2026/cat.png"), "cat.png");
        assert_eq!(last_name("orders"), "orders");
        assert_eq!(last_name("images/"), "images");
        assert_eq!(last_name("/"), "/");
        assert_eq!(last_name(""), "");
    }
}
