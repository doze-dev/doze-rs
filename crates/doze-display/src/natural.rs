//! Names in the order a person reads them: `api-2`, `api-7`, `api-26`, not `api-2`, `api-26`, `api-7`. Every surface
//! that lists services or stacks by name orders them with [`natural_cmp`], so the sidebar, the List, the Tiles, the
//! Grid and the Library agree.

use std::cmp::Ordering;

/// Compares two names, runs of ASCII digits by their value and the rest character by character. Equal values with
/// different zero padding (`a-01`, `a-1`) fall back to the plain order, so the order is total and stable.
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a, b);
    loop {
        match (x.chars().next(), y.chars().next()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let (dx, rx) = split_digits(x);
                let (dy, ry) = split_digits(y);
                let ord = by_value(dx, dy);
                if ord != Ordering::Equal {
                    return ord;
                }
                (x, y) = (rx, ry);
            }
            (Some(p), Some(q)) => {
                if p != q {
                    return p.cmp(&q);
                }
                (x, y) = (
                    x.get(p.len_utf8()..).unwrap_or_default(),
                    y.get(q.len_utf8()..).unwrap_or_default(),
                );
            }
        }
    }
}

/// The leading run of ASCII digits, and the rest.
fn split_digits(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    s.split_at_checked(end).unwrap_or((s, ""))
}

/// Two runs of digits by value, however long: fewer significant digits is smaller.
fn by_value(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::natural_cmp;

    #[test]
    fn numbers_in_names_read_by_value() {
        let mut names = vec![
            "api-26", "api-7", "api-2", "api", "api-10b", "api-10a", "web-1", "Api-3",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            [
                "Api-3", "api", "api-2", "api-7", "api-10a", "api-10b", "api-26", "web-1"
            ]
        );
    }

    #[test]
    fn padding_and_long_runs_stay_total() {
        let mut names = vec![
            "a-01",
            "a-1",
            "a-99999999999999999999999",
            "a-100000000000000000000000",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            [
                "a-01",
                "a-1",
                "a-99999999999999999999999",
                "a-100000000000000000000000"
            ]
        );
    }
}
