//! `application/x-www-form-urlencoded`, as a query string and a form body are written: the few lines it takes, so the
//! console carries no serde and no decoder crate for two fields.

/// The most fields read; the rest are ignored.
const MOST_FIELDS: usize = 64;

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// One component: `+` is a space, `%XX` is a byte, a `%` that is not followed by two hex digits stays as it is; the
/// bytes are read as UTF-8, and what is not is replaced.
#[must_use]
pub fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let pair = bytes
            .get(i + 1)
            .copied()
            .and_then(hex)
            .zip(bytes.get(i + 2).copied().and_then(hex));
        match (b, pair) {
            (b'%', Some((hi, lo))) => {
                out.push(hi << 4 | lo);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The fields of a query string or a form body, in order.
#[must_use]
pub fn parse(input: &str) -> Vec<(String, String)> {
    input
        .split('&')
        .filter(|p| !p.is_empty())
        .take(MOST_FIELDS)
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

/// A query part (with its `?`) from fields; a field with an empty value is left out, and nothing at all is nothing.
#[must_use]
pub fn encode_query(fields: &[(&str, &str)]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}={}", crate::base::segment(v)))
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

/// The first value of a field.
#[must_use]
pub fn get<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_component_is_decoded_as_a_browser_wrote_it() {
        for (raw, want) in [
            ("", ""),
            ("home", "home"),
            ("a+b", "a b"),
            ("a%20b", "a b"),
            ("%41%42", "AB"),
            ("%e2%9c%93", "✓"),
            ("100%", "100%"),
            ("%zz", "%zz"),
            ("%4", "%4"),
            ("%ff", "\u{fffd}"),
            ("a%2Bb", "a+b"),
        ] {
            assert_eq!(decode(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn fields_are_split_at_ampersands_and_at_the_first_equals() {
        let f = parse("back=home&x=1=2&flag&&y=%26");
        assert_eq!(get(&f, "back"), Some("home"));
        assert_eq!(get(&f, "x"), Some("1=2"));
        assert_eq!(get(&f, "flag"), Some(""));
        assert_eq!(get(&f, "y"), Some("&"));
        assert_eq!(get(&f, "z"), None);
        assert_eq!(f.len(), 4);
    }

    #[test]
    fn a_query_part_leaves_out_what_is_empty_and_encodes_the_rest() {
        assert_eq!(encode_query(&[("q", ""), ("internal", "")]), "");
        assert_eq!(encode_query(&[("q", "a b&c"), ("internal", "1")]), "?q=a%20b%26c&internal=1");
        assert_eq!(decode("a%20b%26c"), "a b&c");
    }

    #[test]
    fn the_first_value_of_a_repeated_field_wins_and_the_count_is_bounded() {
        let f = parse("a=1&a=2");
        assert_eq!(get(&f, "a"), Some("1"));
        let many: String = (0..500).map(|i| format!("k{i}=v&")).collect();
        assert_eq!(parse(&many).len(), MOST_FIELDS);
    }

    #[test]
    fn decoding_never_panics_on_any_byte_string() {
        for len in 0..4 {
            let mut bytes = vec![0_u8; len];
            for seed in 0..2000_u32 {
                for (i, b) in bytes.iter_mut().enumerate() {
                    *b = [b'%', b'+', b'0', b'f', b'G', 0xc3, 0xff, b'&', b'=']
                        [((seed >> (i * 3)) % 9) as usize];
                }
                let s = String::from_utf8_lossy(&bytes).into_owned();
                let _ = parse(&s);
            }
        }
    }
}
