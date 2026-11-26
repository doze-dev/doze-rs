//! A key's value for the console's value viewer (protocol minor 2), by its type: a string as JSON, text or bytes (as it
//! is, never made into text it is not), a big one a part at a time; a hash's fields and a sorted set's members with
//! their scores as pairs; a list, a set and a stream's last entries as JSON. Collections show their first
//! [`MAX_ITEMS`] and say how many there are.

use crate::browse::{cursor_and_items, unavailable};
use crate::client::Client;
use crate::json::{array, is_json, quote, shown};
use crate::keyspace::Id;
use crate::reply::Reply;
use doze_module_proto::msg::blocks::{VALUE_CHUNK, ValueAnswer, media};
use doze_module_proto::msg::error::{Error, ErrorKind};
use std::collections::BTreeMap;
use tokio::io::{AsyncRead, AsyncWrite};

/// The most fields, items or members a value shows.
pub const MAX_ITEMS: usize = 1000;
/// The most stream entries a value shows, newest first.
pub const STREAM_ENTRIES: usize = 100;

/// What a string's bytes are: JSON when the whole value is a JSON document, text when UTF-8, bytes otherwise. `part`
/// says these are part of a bigger value, so a character may be cut at either end, and JSON is not told (a part of a
/// document is not one).
pub fn media_of(b: &[u8], part: bool) -> &'static str {
    if !part {
        return if is_json(b) {
            media::JSON
        } else if std::str::from_utf8(b).is_ok() {
            media::TEXT
        } else {
            media::BYTES
        };
    }
    // At most three bytes of a character cut at the start (continuation bytes), and of one cut at the end.
    let lead = b.iter().take(3).take_while(|&&x| x & 0xC0 == 0x80).count();
    let rest = b.get(lead..).unwrap_or_default();
    match std::str::from_utf8(rest) {
        Ok(_) => media::TEXT,
        Err(e) if e.error_len().is_none() => media::TEXT,
        Err(_) => media::BYTES,
    }
}

type Conn<S> = Client<S>;

async fn ask<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Conn<S>,
    args: &[&[u8]],
) -> Result<Reply, Error> {
    match c.call_bytes(args).await.map_err(unavailable)? {
        Reply::Error(e) => Err(Error::new(ErrorKind::Unavailable, e)),
        r => Ok(r),
    }
}

async fn count<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Conn<S>,
    cmd: &[u8],
    key: &[u8],
) -> Result<u64, Error> {
    Ok(match ask(c, &[cmd, key]).await? {
        Reply::Integer(n) => n.unsigned_abs(),
        _ => 0,
    })
}

fn items(r: Reply) -> Vec<Vec<u8>> {
    match r {
        Reply::Array(Some(v)) => v
            .into_iter()
            .filter_map(|i| match i {
                Reply::Bulk(Some(b)) => Some(b),
                Reply::Integer(n) => Some(n.to_string().into_bytes()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `items` as JSON: an array when it is every item, else `{"length": n, "shown": k, "items": […]}`.
fn listed(items: &[Vec<u8>], length: u64) -> String {
    let arr = array(items.iter().map(Vec::as_slice));
    if items.len() as u64 >= length {
        arr
    } else {
        format!(
            "{{\"length\":{length},\"shown\":{},\"items\":{arr}}}",
            items.len()
        )
    }
}

/// Every member of a set or field and value of a hash up to [`MAX_ITEMS`], walked with `SSCAN` or `HSCAN`.
async fn walk<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Conn<S>,
    cmd: &[u8],
    key: &[u8],
    per: usize,
) -> Result<Vec<Vec<u8>>, Error> {
    let mut out = Vec::new();
    let mut cursor = b"0".to_vec();
    loop {
        let args: [&[u8]; 5] = [cmd, key, &cursor, b"COUNT", b"500"];
        let reply = c.call_bytes(&args).await.map_err(unavailable)?;
        let (next, got) = cursor_and_items(reply)?;
        out.extend(got);
        if next == b"0" || out.len() >= MAX_ITEMS * per {
            out.truncate(MAX_ITEMS * per);
            return Ok(out);
        }
        cursor = next;
    }
}

fn pairs_of(flat: Vec<Vec<u8>>) -> Vec<(String, String)> {
    let mut it = flat.into_iter();
    let mut out = Vec::new();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        out.push((shown(&k), shown(&v)));
    }
    out
}

fn pairs(mut pairs: Vec<(String, String)>, length: u64, what: &str) -> ValueAnswer {
    if (pairs.len() as u64) < length {
        pairs.push((
            "…".into(),
            format!("{} of {length} {what} shown", pairs.len()),
        ));
    }
    ValueAnswer {
        media: media::PAIRS.into(),
        pairs,
        ..ValueAnswer::default()
    }
}

/// The value of the tree's leaf `row["id"]`, from byte `offset`, `max` bytes at most.
pub async fn value<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Conn<S>,
    row: &BTreeMap<String, String>,
    offset: u64,
    max: u32,
) -> Result<ValueAnswer, Error> {
    let id = row.get("id").map(String::as_str).unwrap_or_default();
    let key = match Id::parse(id) {
        Some(Id::Key(k)) => k,
        Some(Id::More(_)) => {
            let note = format!(
                "Only the first {} keys were looked at, to keep the server quick. Type in the filter to find the others.",
                crate::browse::MAX_KEYS
            );
            return Ok(ValueAnswer::slice(
                media::TEXT,
                note.as_bytes(),
                offset,
                max,
            ));
        }
        _ => {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("{id:?} is not a key"),
            ));
        }
    };
    of_key(c, &key, offset, max).await
}

/// A key's value by its type.
async fn of_key<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Conn<S>,
    key: &[u8],
    offset: u64,
    max: u32,
) -> Result<ValueAnswer, Error> {
    let gone = || Error::new(ErrorKind::NotFound, format!("no key {}", shown(key)));
    let kind = match ask(c, &[b"TYPE", key]).await? {
        Reply::Simple(t) => t,
        r => r.text().unwrap_or_default(),
    };
    match kind.as_str() {
        "none" => Err(gone()),
        "string" => string(c, key, offset, max).await?.ok_or_else(gone),
        "ReJSON-RL" => match ask(c, &[b"JSON.GET", key]).await? {
            Reply::Bulk(Some(b)) => Ok(ValueAnswer::slice(media::JSON, &b, offset, max)),
            _ => Err(gone()),
        },
        "hash" | "zset" => pairs_value(c, &kind, key).await,
        "list" | "set" | "stream" => {
            let body = json_value(c, &kind, key).await?;
            Ok(ValueAnswer::slice(
                media::JSON,
                body.as_bytes(),
                offset,
                max,
            ))
        }
        other => {
            let note = format!(
                "{} is a {other}; the console does not show this kind of value yet.",
                shown(key)
            );
            Ok(ValueAnswer::slice(
                media::TEXT,
                note.as_bytes(),
                offset,
                max,
            ))
        }
    }
}

/// A hash's fields (sorted by name) or a sorted set's members (by score), with their values or scores.
async fn pairs_value<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Conn<S>,
    kind: &str,
    key: &[u8],
) -> Result<ValueAnswer, Error> {
    if kind == "hash" {
        let n = count(c, b"HLEN", key).await?;
        let mut p = pairs_of(walk(c, b"HSCAN", key, 2).await?);
        p.sort();
        return Ok(pairs(p, n, "fields"));
    }
    let n = count(c, b"ZCARD", key).await?;
    let last = (MAX_ITEMS - 1).to_string();
    let r = ask(c, &[b"ZRANGE", key, b"0", last.as_bytes(), b"WITHSCORES"]).await?;
    Ok(pairs(pairs_of(items(r)), n, "members"))
}

/// A list's first items, a set's members (sorted) or a stream's last entries, as JSON.
async fn json_value<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Conn<S>,
    kind: &str,
    key: &[u8],
) -> Result<String, Error> {
    let last = (MAX_ITEMS - 1).to_string();
    match kind {
        "list" => {
            let n = count(c, b"LLEN", key).await?;
            let r = ask(c, &[b"LRANGE", key, b"0", last.as_bytes()]).await?;
            Ok(listed(&items(r), n))
        }
        "set" => {
            let n = count(c, b"SCARD", key).await?;
            let mut m = walk(c, b"SSCAN", key, 1).await?;
            m.sort();
            Ok(listed(&m, n))
        }
        _ => {
            let n = count(c, b"XLEN", key).await?;
            let entries = STREAM_ENTRIES.to_string();
            let args: [&[u8]; 6] = [b"XREVRANGE", key, b"+", b"-", b"COUNT", entries.as_bytes()];
            Ok(stream(ask(c, &args).await?, n))
        }
    }
}

/// A string value: whole when it fits one part (so JSON is told), else the part asked for with `GETRANGE`. `None`
/// when the key went away meanwhile.
async fn string<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Conn<S>,
    key: &[u8],
    offset: u64,
    max: u32,
) -> Result<Option<ValueAnswer>, Error> {
    let size = count(c, b"STRLEN", key).await?;
    if size <= u64::from(VALUE_CHUNK) {
        return Ok(match ask(c, &[b"GET", key]).await? {
            Reply::Bulk(Some(b)) => Some(ValueAnswer::slice(media_of(&b, false), &b, offset, max)),
            _ => None,
        });
    }
    let max = if max == 0 {
        VALUE_CHUNK
    } else {
        max.min(VALUE_CHUNK)
    };
    let end = offset.saturating_add(u64::from(max)).min(size);
    if offset >= end {
        return Ok(Some(ValueAnswer {
            media: media::TEXT.into(),
            size,
            ..ValueAnswer::default()
        }));
    }
    let (from, to) = (offset.to_string(), (end - 1).to_string());
    let body = match ask(c, &[b"GETRANGE", key, from.as_bytes(), to.as_bytes()]).await? {
        Reply::Bulk(Some(b)) => b,
        _ => return Ok(None),
    };
    Ok(Some(ValueAnswer {
        media: media_of(&body, true).into(),
        more: offset + (body.len() as u64) < size,
        body,
        pairs: Vec::new(),
        size,
    }))
}

/// A stream's entries (newest first) as JSON: `{"length": n, "entries": [{"id": …, "fields": {…}}]}`.
fn stream(r: Reply, length: u64) -> String {
    let Reply::Array(Some(entries)) = r else {
        return format!("{{\"length\":{length},\"entries\":[]}}");
    };
    let mut out = Vec::new();
    for e in entries {
        let Reply::Array(Some(parts)) = e else {
            continue;
        };
        let mut it = parts.into_iter();
        let (Some(Reply::Bulk(Some(id))), Some(fields)) = (it.next(), it.next()) else {
            continue;
        };
        let fields: Vec<String> = pairs_of(items(fields))
            .iter()
            .map(|(k, v)| format!("{}:{}", quote(k), quote(v)))
            .collect();
        out.push(format!(
            "{{\"id\":{},\"fields\":{{{}}}}}",
            quote(&shown(&id)),
            fields.join(",")
        ));
    }
    format!("{{\"length\":{length},\"entries\":[{}]}}", out.join(","))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_string_is_json_text_or_bytes() {
        assert_eq!(media_of(br#"{"a":1}"#, false), media::JSON);
        assert_eq!(media_of(b"hello", false), media::TEXT);
        assert_eq!(media_of("é".as_bytes(), false), media::TEXT);
        assert_eq!(media_of(b"\x89PNG\x00\xff", false), media::BYTES);
        assert_eq!(
            media_of(br#"{"a":1}"#, true),
            media::TEXT,
            "a part of JSON is not JSON"
        );
        // A character cut at either end of a part is still text.
        let e = "é".as_bytes();
        assert_eq!(media_of(&[b'a', e[0]], true), media::TEXT);
        assert_eq!(media_of(&[e[1], b'a'], true), media::TEXT);
        assert_eq!(media_of(b"a\xffb", true), media::BYTES);
    }

    #[test]
    fn collections_say_when_they_show_only_some() {
        let items = vec![b"a".to_vec(), b"b".to_vec()];
        assert_eq!(listed(&items, 2), r#"["a","b"]"#);
        assert_eq!(
            listed(&items, 5),
            r#"{"length":5,"shown":2,"items":["a","b"]}"#
        );
        assert!(is_json(listed(&items, 5).as_bytes()));
        let p = pairs(vec![("f".into(), "v".into())], 3, "fields");
        assert_eq!(p.pairs.last().unwrap().1, "1 of 3 fields shown");
        assert_eq!(pairs(vec![], 0, "fields").pairs.len(), 0);
    }

    #[test]
    fn a_stream_reads_as_json() {
        let bulk = |s: &str| Reply::Bulk(Some(s.as_bytes().to_vec()));
        let entry = Reply::Array(Some(vec![
            bulk("1-0"),
            Reply::Array(Some(vec![bulk("k"), bulk("v\"")])),
        ]));
        let s = stream(Reply::Array(Some(vec![entry])), 1);
        assert_eq!(
            s,
            r#"{"length":1,"entries":[{"id":"1-0","fields":{"k":"v\""}}]}"#
        );
        assert!(is_json(s.as_bytes()));
    }
}
