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

// fuzz:parser form
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

/// The fields of a form body as it arrives: bytes, of which what is not UTF-8 is replaced (so a hostile or broken body
/// is text like any other, never an error).
#[must_use]
pub fn parse_body(body: &[u8]) -> Vec<(String, String)> {
    parse(&String::from_utf8_lossy(body))
}

/// The first value of a field, or the empty text when it is absent: a form left a field out the way it leaves one empty.
#[must_use]
pub fn text<'a>(fields: &'a [(String, String)], name: &str) -> &'a str {
    get(fields, name).unwrap_or("")
}

/// The most fields a form is read for (see [`parse`]).
pub const FIELD_LIMIT: usize = MOST_FIELDS;

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
        assert_eq!(
            encode_query(&[("q", "a b&c"), ("internal", "1")]),
            "?q=a%20b%26c&internal=1"
        );
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

#[cfg(test)]
mod body_tests {
    use super::*;

    #[test]
    fn a_body_that_is_not_utf8_is_text_with_replacements() {
        let f = parse_body(b"a=\xff\xfe&b=%ff&c=ok&d=%e2%9c");
        assert_eq!(text(&f, "a"), "\u{fffd}\u{fffd}");
        assert_eq!(text(&f, "b"), "\u{fffd}");
        assert_eq!(text(&f, "c"), "ok");
        assert_eq!(
            text(&f, "d"),
            "\u{fffd}",
            "a cut multi-byte sequence is one replacement"
        );
        assert_eq!(parse_body(b""), Vec::new());
    }

    #[test]
    fn an_absent_field_is_empty_and_the_first_of_a_repeat_wins() {
        let f = parse_body(b"name=first&name=second&empty=");
        assert_eq!(text(&f, "name"), "first");
        assert_eq!(text(&f, "empty"), "");
        assert_eq!(text(&f, "missing"), "");
        assert_eq!(get(&f, "missing"), None);
        assert_eq!(get(&f, "empty"), Some(""));
    }

    #[test]
    fn percent_escapes_plus_and_control_bytes_decode_the_way_a_browser_wrote_them() {
        let f = parse_body(b"k=a%2Bb+c%20d%0D%0Ae%00f&%6Eame=x");
        assert_eq!(text(&f, "k"), "a+b c d\r\ne\0f");
        assert_eq!(text(&f, "name"), "x", "a name is decoded too");
        // An escape that is not one stays as it is.
        assert_eq!(text(&parse_body(b"k=%&j=%g1&l=%1"), "k"), "%");
        assert_eq!(text(&parse_body(b"j=%g1"), "j"), "%g1");
    }

    #[test]
    fn a_field_as_large_as_the_body_limit_is_read_whole() {
        // The body limit is 1 MiB (guard::MAX_BODY): a value that fills it is decoded without loss and without a panic.
        let mut body = b"value=".to_vec();
        body.extend(std::iter::repeat_n(b'a', (1 << 20) - 6));
        let f = parse_body(&body);
        assert_eq!(text(&f, "value").len(), (1 << 20) - 6);
        let mut escaped = b"value=".to_vec();
        for _ in 0..((1 << 20) / 3) {
            escaped.extend_from_slice(b"%41");
        }
        assert_eq!(text(&parse_body(&escaped), "value").len(), (1 << 20) / 3);
    }

    #[test]
    fn fields_past_the_limit_are_dropped_not_errors() {
        let body: String = (0..FIELD_LIMIT + 50).map(|i| format!("f{i}=v&")).collect();
        let f = parse_body(body.as_bytes());
        assert_eq!(f.len(), FIELD_LIMIT);
        assert_eq!(text(&f, "f0"), "v");
        assert_eq!(
            text(&f, &format!("f{}", FIELD_LIMIT)),
            "",
            "the first field past the limit is gone"
        );
        // A field before the limit still wins over a later repeat of it.
        let dup = format!("a=1&{}a=2", "x=1&".repeat(10));
        assert_eq!(text(&parse_body(dup.as_bytes()), "a"), "1");
    }
}
