//! The key tree both modules show (protocol minor 2, a panel of kind "tree"), and the form that sets a key. Folders
//! are found with `SCAN … MATCH`, never `KEYS`, so a large keyspace is walked in steps and the server keeps serving;
//! the walk stops after [`MAX_KEYS`] keys or [`MAX_ROUNDS`] steps, and says so with a last node.

use crate::client::Client;
use crate::json::shown;
use crate::keyspace::{Found, Id, group, holding, human_secs, ids, under};
use crate::reply::Reply;
use doze_module_proto::msg::blocks::{CHILDREN_PAGE, Children, Field, TreeNode, field};
use doze_module_proto::msg::console::Action;
use doze_module_proto::msg::error::{Error, ErrorKind};
use std::collections::BTreeMap;
use std::io;
use tokio::io::{AsyncRead, AsyncWrite};

/// The tree panel's id, and its block's.
pub const PANEL: &str = "keys";
/// The most keys one look gathers.
pub const MAX_KEYS: usize = 50_000;
/// The most `SCAN` steps one look takes (each asks for [`STEP`] keys), so a filter that matches little in a huge
/// keyspace still answers quickly.
pub const MAX_ROUNDS: usize = 500;
/// The keys one `SCAN` step asks for.
pub const STEP: &str = "1000";

pub(crate) fn unavailable(e: io::Error) -> Error {
    Error::new(ErrorKind::Unavailable, e.to_string())
}

/// What one walk found, and whether it reached the end.
#[derive(Debug, Default)]
pub struct Scan {
    pub keys: Found,
    pub complete: bool,
    /// It stopped at [`MAX_KEYS`] keys (else, when not complete, after [`MAX_ROUNDS`] steps).
    pub full: bool,
}

/// Every key matching `pattern`, up to the limits.
pub async fn scan<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Client<S>,
    pattern: &[u8],
) -> Result<Scan, Error> {
    let mut out = Scan::default();
    let mut cursor = b"0".to_vec();
    for _ in 0..MAX_ROUNDS {
        let args: [&[u8]; 6] = [
            b"SCAN",
            &cursor,
            b"MATCH",
            pattern,
            b"COUNT",
            STEP.as_bytes(),
        ];
        let reply = c.call_bytes(&args).await.map_err(unavailable)?;
        let (next, keys) = cursor_and_items(reply)?;
        out.keys.extend(keys);
        if next == b"0" {
            out.complete = true;
            return Ok(out);
        }
        if out.keys.len() >= MAX_KEYS {
            out.full = true;
            return Ok(out);
        }
        cursor = next;
    }
    Ok(out)
}

/// A `SCAN`-family reply: the next cursor and the items.
pub(crate) fn cursor_and_items(reply: Reply) -> Result<(Vec<u8>, Vec<Vec<u8>>), Error> {
    let bad = |r: &Reply| {
        Error::new(
            ErrorKind::Internal,
            format!("the server's scan reply was not understood: {r:?}"),
        )
    };
    let items = match reply {
        Reply::Error(e) => return Err(Error::new(ErrorKind::Unavailable, e)),
        Reply::Array(Some(items)) => items,
        other => return Err(bad(&other)),
    };
    let mut it = items.into_iter();
    let (Some(Reply::Bulk(Some(next))), Some(Reply::Array(Some(keys)))) = (it.next(), it.next())
    else {
        return Err(Error::new(
            ErrorKind::Internal,
            "the server's scan reply was not understood",
        ));
    };
    let keys = keys
        .into_iter()
        .filter_map(|k| match k {
            Reply::Bulk(Some(b)) => Some(b),
            _ => None,
        })
        .collect();
    Ok((next, keys))
}

fn node(id: &Id, label: String, leaf: bool, info: BTreeMap<String, String>) -> TreeNode {
    TreeNode {
        id: id.encode(),
        label,
        leaf,
        info,
    }
}

/// The last node of a walk that stopped early, saying why: it had read as many keys as it reads at once, or it had
/// asked the server as many times as it asks (a sparse match in a big keyspace) with `found` keys to show.
fn more(prefix: &[u8], found: &Scan) -> TreeNode {
    let n = found.keys.len();
    let why = if found.full {
        format!("the walk stops at {MAX_KEYS} keys; filter to find the rest")
    } else {
        format!(
            "the walk stopped after {MAX_ROUNDS} scan steps with {n} keys found; filter to narrow it"
        )
    };
    node(
        &Id::More(prefix.to_vec()),
        "… more keys not scanned".into(),
        true,
        BTreeMap::from([("note".into(), why)]),
    )
}

/// The children of `parent` (an id this tree gave; "" for the top), or with `filter` the keys under it holding that
/// text, by their full names. One page of [`CHILDREN_PAGE`] after `cursor`; the keys on it carry their type and,
/// when they expire, the time left.
pub async fn children<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Client<S>,
    parent: &str,
    filter: &str,
    cursor: Option<&str>,
) -> Result<Children, Error> {
    let Some(Id::Folder(prefix)) = Id::parse(parent) else {
        return Ok(Children::default());
    };
    let mut all = Vec::new();
    let found = if filter.is_empty() {
        let found = scan(c, &under(&prefix)).await?;
        let levels = group(&prefix, &found.keys);
        for l in levels.iter().filter(|l| l.keys > 0) {
            let info = BTreeMap::from([("keys".to_string(), l.keys.to_string())]);
            all.push(node(
                &ids(&prefix, &l.segment).0,
                shown(&l.segment),
                false,
                info,
            ));
        }
        for l in levels.iter().filter(|l| l.key) {
            all.push(node(
                &ids(&prefix, &l.segment).1,
                shown(&l.segment),
                true,
                BTreeMap::new(),
            ));
        }
        found
    } else {
        let found = scan(c, &holding(&prefix, filter.as_bytes())).await?;
        for k in &found.keys {
            all.push(node(&Id::Key(k.clone()), shown(k), true, BTreeMap::new()));
        }
        found
    };
    if !found.complete {
        all.push(more(&prefix, &found));
    }
    let mut page = Children::page(all, cursor, CHILDREN_PAGE);
    describe(c, &mut page.nodes).await?;
    Ok(page)
}

/// Each key's type and time left, asked in one round trip.
async fn describe<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Client<S>,
    nodes: &mut [TreeNode],
) -> Result<(), Error> {
    let keys: Vec<(usize, Vec<u8>)> = nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| match Id::parse(&n.id) {
            Some(Id::Key(k)) => Some((i, k)),
            _ => None,
        })
        .collect();
    if keys.is_empty() {
        return Ok(());
    }
    let mut cmds = Vec::with_capacity(keys.len() * 2);
    for (_, k) in &keys {
        cmds.push(vec![&b"TYPE"[..], k]);
        cmds.push(vec![&b"TTL"[..], k]);
    }
    let replies = c.pipeline(&cmds).await.map_err(unavailable)?;
    for ((i, _), pair) in keys.iter().zip(replies.chunks(2)) {
        let Some(n) = nodes.get_mut(*i) else { continue };
        if let Some(Reply::Simple(t)) = pair.first()
            && t != "none"
        {
            n.info.insert("type".into(), t.clone());
        }
        if let Some(Reply::Integer(s)) = pair.get(1)
            && *s > 0
        {
            n.info.insert("ttl".into(), human_secs(s.unsigned_abs()));
        }
    }
    Ok(())
}

/// The action that sets a key, with its form.
pub fn set_key_action() -> Action {
    let f = |id: &str, label: &str, kind: &str, required: bool| Field {
        id: id.into(),
        label: label.into(),
        kind: kind.into(),
        required,
        ..Field::default()
    };
    Action {
        // Its id gives its key (the first letter): `K`, as `s` is valkey's save.
        id: "key-set".into(),
        label: "Set a key".into(),
        kind: "server".into(),
        destructive: false,
        input_hint: Some("key value".into()),
        fields: vec![
            f("key", "Key", field::TEXT, true),
            f("value", "Value", field::TEXT, true),
            Field {
                default: Some("0".into()),
                min: Some(0),
                max: Some(31_536_000),
                help: Some("Seconds; 0 keeps it forever".into()),
                ..f("ttl", "Expires after", field::NUMBER, false)
            },
        ],
    }
}

/// Sets the key the form names (`key`, `value`, `ttl` in seconds; 0 or none keeps it forever). Without the form's
/// values, `input` is read as `key value`.
pub async fn set_key<S: AsyncRead + AsyncWrite + Unpin>(
    c: &mut Client<S>,
    values: &BTreeMap<String, String>,
    input: Option<&str>,
) -> Result<String, Error> {
    let (key, value, ttl) = match values.get("key") {
        Some(k) => (
            k.as_str(),
            values.get("value").map(String::as_str).unwrap_or_default(),
            values.get("ttl").map(String::as_str).unwrap_or("0"),
        ),
        None => {
            let line = input.unwrap_or_default().trim_start();
            match line.split_once(char::is_whitespace) {
                Some((k, v)) => (k, v, "0"),
                None => {
                    return Err(Error::new(
                        ErrorKind::Invalid,
                        "type the key, a space, then the value",
                    ));
                }
            }
        }
    };
    if key.is_empty() {
        return Err(Error::for_field("key", "a key needs a name"));
    }
    // A form sends every field, blanks included: a blank time to live keeps the key for ever, as 0 does.
    let ttl: u64 = match ttl.trim() {
        "" => 0,
        t => t.parse().map_err(|_| {
            Error::for_field("ttl", "Expires after must be a whole number of seconds")
        })?,
    };
    let secs = ttl.to_string();
    let mut args: Vec<&[u8]> = vec![b"SET", key.as_bytes(), value.as_bytes()];
    if ttl > 0 {
        args.extend([&b"EX"[..], secs.as_bytes()]);
    }
    match c.call_bytes(&args).await.map_err(unavailable)? {
        Reply::Error(e) => Err(Error::new(ErrorKind::Invalid, e)),
        _ if ttl > 0 => Ok(format!("set {key}; it expires in {}", human_secs(ttl))),
        _ => Ok(format!("set {key}")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_scan_reply_gives_its_cursor_and_keys() {
        let r = Reply::Array(Some(vec![
            Reply::Bulk(Some(b"17".to_vec())),
            Reply::Array(Some(vec![Reply::Bulk(Some(b"a".to_vec()))])),
        ]));
        assert_eq!(
            cursor_and_items(r).unwrap(),
            (b"17".to_vec(), vec![b"a".to_vec()])
        );
        assert!(cursor_and_items(Reply::Error("ERR no".into())).is_err());
        assert!(cursor_and_items(Reply::Integer(1)).is_err());
    }

    #[test]
    fn the_form_declares_key_value_and_ttl() {
        let a = set_key_action();
        let ids: Vec<&str> = a.fields.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["key", "value", "ttl"]);
        assert!(a.input_hint.is_some(), "an older app shows one box");
        assert_eq!(a.fields[2].max, Some(31_536_000));
    }
}
