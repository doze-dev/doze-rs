//! The top-level members of a record as the bytes they were written in. Go keeps a newer doze's fields as
//! `json.RawMessage`, which is the file's own bytes: invalid UTF-8 and lone surrogate escapes (`\ud800`) included. The
//! decode reads a mended copy of the text (serde_json takes neither); this finds each member's original bytes, so a
//! field kept for a newer doze is written back as it was. Syntax is checked by the decode, not here: on bytes that are
//! not an object of members this gives `None`, and the mended text is kept instead.

/// Each member's value as its bytes, in the file's order.
// fuzz:parser fuzz_rawscan
pub(crate) fn members(b: &[u8]) -> Option<Vec<&[u8]>> {
    let mut i = ws(b, 0);
    if b.get(i) != Some(&b'{') {
        return None;
    }
    i = ws(b, i + 1);
    let mut out = Vec::new();
    if b.get(i) == Some(&b'}') {
        return Some(out);
    }
    loop {
        i = ws(b, string(b, i)?);
        if b.get(i) != Some(&b':') {
            return None;
        }
        let start = ws(b, i + 1);
        let end = value(b, start)?;
        out.push(b.get(start..end)?);
        i = ws(b, end);
        match b.get(i) {
            Some(b',') => i = ws(b, i + 1),
            Some(b'}') => return Some(out),
            _ => return None,
        }
    }
}

fn ws(b: &[u8], mut i: usize) -> usize {
    while matches!(b.get(i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        i += 1;
    }
    i
}

/// The end of the string starting at `i` (past its closing quote).
fn string(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'"') {
        return None;
    }
    let mut j = i + 1;
    loop {
        match b.get(j)? {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
}

/// The end of the value starting at `i`: a string, a bracketed value (strings inside skipped), or a scalar up to the
/// next delimiter.
fn value(b: &[u8], i: usize) -> Option<usize> {
    match b.get(i)? {
        b'"' => string(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            loop {
                match b.get(j)? {
                    b'"' => {
                        j = string(b, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth = depth.checked_sub(1)?;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        _ => {
            let mut j = i;
            while !matches!(
                b.get(j),
                None | Some(b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
            ) {
                j += 1;
            }
            (j > i).then_some(j)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_each_members_bytes() {
        let b =
            b" { \"a\\\"}\" : [1, \"]\\\"\" , {\"x\":\"\xff\"} ] ,\"b\":-1.5e3, \"c\":\"\\ud800\"}";
        let got = members(b).unwrap_or_default();
        assert_eq!(
            got,
            [
                &b"[1, \"]\\\"\" , {\"x\":\"\xff\"} ]"[..],
                b"-1.5e3",
                b"\"\\ud800\""
            ]
        );
        assert_eq!(members(b"{}"), Some(Vec::new()));
        for bad in [
            &b"null"[..],
            b"{\"a\"}",
            b"{\"a\":}",
            b"{\"a\":1",
            b"{\"a\":[}",
        ] {
            assert_eq!(members(bad), None, "{:?}", String::from_utf8_lossy(bad));
        }
    }
}
