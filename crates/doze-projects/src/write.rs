//! Writing the record, as Go's `projects.Save` and `projects.Update` write it: the same bytes for the same record, so
//! a stack the Go doze and this one both use keeps one file whichever wrote it last.
//!
//! Go's bytes are `json.Marshal`'s: the fields in the struct's order (`schema`, `config`, then `name`, `last_up`,
//! `last_down`, `last_set` when set), strings with `<`, `>`, `&`, U+2028 and U+2029 escaped, times as RFC 3339 with
//! the nanoseconds trimmed and the offset they were read with. When the record has fields a newer doze wrote, Go
//! re-marshals through a map, so every key, ours and theirs, comes out sorted, and their values compacted. Then a
//! newline. The file is written beside the record and renamed over it, so a reader never sees half of one, and it is
//! created private (Go's `os.CreateTemp` makes it 0600).

use crate::record::{FILE_NAME, Project, SCHEMA, load};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Loads the record in `dir`, applies `change`, and saves it (Go's `projects.Update`). A record that cannot be read
/// is set aside as `project.json.bad` and a fresh one written, so one broken file does not warn on every run; when it
/// cannot even be set aside, the read's error is given and nothing is written.
pub fn update(dir: &Path, change: impl FnOnce(&mut Project)) -> Result<(), crate::Error> {
    let mut p = match load(dir) {
        Ok(Some(p)) => p,
        Ok(None) => Project::fresh(dir),
        Err(e) => {
            let file = dir.join(FILE_NAME);
            let mut bad = file.clone().into_os_string();
            bad.push(".bad");
            if std::fs::rename(&file, &bad).is_err() {
                return Err(e);
            }
            Project::fresh(dir)
        }
    };
    change(&mut p);
    save(dir, &p).map_err(crate::Error::Io)
}

/// Writes `p` as the record in `dir`, atomically (Go's `projects.Save`). A newer doze's record keeps its version.
pub fn save(dir: &Path, p: &Project) -> io::Result<()> {
    let mut bytes = encode(p)?;
    bytes.push(b'\n');
    std::fs::create_dir_all(dir)?;
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let tmp = dir.join(format!(
        "{FILE_NAME}.{}-{}-{nanos}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let written = doze_os::fs::write_private(&tmp, &bytes)
        .and_then(|()| std::fs::rename(&tmp, dir.join(FILE_NAME)));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// The record's bytes as Go's `json.Marshal` gives them (no newline). Fails only for a time Go cannot write either (a
/// year past 9999, an offset with seconds).
pub fn encode(p: &Project) -> io::Result<Vec<u8>> {
    let mut ours: Vec<(&str, String)> = vec![
        ("schema", p.schema.max(SCHEMA).to_string()),
        ("config", string(&p.config)),
    ];
    if !p.name.is_empty() {
        ours.push(("name", string(&p.name)));
    }
    for (key, t) in [("last_up", p.last_up), ("last_down", p.last_down)] {
        if let Some(t) = t {
            ours.push((key, time(t)?));
        }
    }
    if !p.last_set.is_empty() {
        let items: Vec<String> = p.last_set.iter().map(|s| string(s)).collect();
        ours.push(("last_set", format!("[{}]", items.join(","))));
    }
    let ours = ours
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.into_bytes()));
    let pairs: Vec<(String, Vec<u8>)> = if p.extra.is_empty() {
        ours.collect()
    } else {
        // A newer doze's values are written as their bytes, compacted and escaped as Go's `json.RawMessage` is.
        let mut all: BTreeMap<String, Vec<u8>> = p
            .extra
            .iter()
            .map(|(k, v)| (k.clone(), go_escape(&compact(v.bytes()))))
            .collect();
        all.extend(ours);
        all.into_iter().collect()
    };
    let mut out = vec![b'{'];
    for (n, (k, v)) in pairs.iter().enumerate() {
        if n > 0 {
            out.push(b',');
        }
        out.extend_from_slice(string(k).as_bytes());
        out.push(b':');
        out.extend_from_slice(v);
    }
    out.push(b'}');
    Ok(out)
}

/// A JSON string as Go writes it.
fn string(s: &str) -> String {
    let json = serde_json::to_string(s).unwrap_or_default();
    String::from_utf8_lossy(&go_escape(json.as_bytes())).into_owned()
}

/// A Go `time.Time` in JSON: RFC 3339 with the fraction's trailing zeros dropped (Go's `RFC3339Nano`).
fn time(t: OffsetDateTime) -> io::Result<String> {
    if !(0..=9999).contains(&t.year()) || t.offset().seconds_past_minute() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{t}: not a time a record can hold"),
        ));
    }
    t.format(&Rfc3339)
        .map(|s| format!("\"{s}\""))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Escapes as Go's `json.Marshal` does beyond what serde_json does: `<`, `>`, `&`, U+2028 and U+2029, which in JSON
/// can only be inside strings. Other bytes, UTF-8 or not, pass as they are (Go's compact of a raw value keeps them).
fn go_escape(json: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(json.len());
    let mut i = 0;
    while let Some(&c) = json.get(i) {
        let sep = match json.get(i..i + 3) {
            Some([0xE2, 0x80, 0xA8]) => Some(&b"\\u2028"[..]),
            Some([0xE2, 0x80, 0xA9]) => Some(&b"\\u2029"[..]),
            _ => None,
        };
        if let Some(sep) = sep {
            out.extend_from_slice(sep);
            i += 3;
            continue;
        }
        match c {
            b'<' => out.extend_from_slice(b"\\u003c"),
            b'>' => out.extend_from_slice(b"\\u003e"),
            b'&' => out.extend_from_slice(b"\\u0026"),
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// A JSON value without the spaces between its tokens (Go compacts a raw value it writes).
fn compact(json: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(json.len());
    let (mut in_string, mut escaped) = (false, false);
    for &c in json {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
        } else if !matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
            in_string = c == b'"';
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_keeps_what_is_inside_strings() {
        assert_eq!(
            compact(b" { \"a b\" : [ 1 , \"x\\\" y\xff\" ] }\n"),
            b"{\"a b\":[1,\"x\\\" y\xff\"]}"
        );
    }

    #[test]
    fn escapes_what_go_escapes() {
        assert_eq!(
            string("<a&b>\u{2028}"),
            "\"\\u003ca\\u0026b\\u003e\\u2028\""
        );
    }
}
