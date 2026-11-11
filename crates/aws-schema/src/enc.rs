//! Encode a tree as JSON, driven by the schema: member names come from the tables, values from the tree.

use crate::arena::{Arena, NIL, Val};
use crate::json::Violation;
use crate::tables::{
    K_BLOB, K_BOOL, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STRUCT, K_TS, K_UNION, MemberRef,
    Tables,
};
use std::io::Write;

type R = Result<(), Violation>;

pub fn encode(t: &Tables, shape: u32, ar: &Arena, body: &str, root: u32, out: &mut Vec<u8>) -> R {
    let Some(n) = ar.node(root) else {
        return Err(Violation::Table);
    };
    object(t, shape, ar, body, n.val, out)
}

fn object(t: &Tables, shape: u32, ar: &Arena, body: &str, v: Val, out: &mut Vec<u8>) -> R {
    let Val::Obj(first) = v else {
        return Err(Violation::Table);
    };
    let sh = t.shape(shape).ok_or(Violation::Table)?;
    out.push(b'{');
    let mut at = first;
    let mut comma = false;
    while let Some(n) = ar.node(at).filter(|_| at != NIL) {
        let m = t
            .member(n.member)
            .filter(|_| n.member >= sh.first && n.member < sh.first + sh.count)
            .ok_or(Violation::Table)?;
        if comma {
            out.push(b',');
        }
        comma = true;
        string(out, m.wire);
        out.push(b':');
        value(t, &m, ar, body, n.val, out)?;
        at = n.next;
    }
    out.push(b'}');
    Ok(())
}

fn value(t: &Tables, m: &MemberRef<'_>, ar: &Arena, body: &str, v: Val, out: &mut Vec<u8>) -> R {
    match (m.kind, v) {
        (K_BOOL, Val::Bool(b)) => out.extend_from_slice(if b { b"true" } else { b"false" }),
        (K_INT, Val::Int(n)) => write!(out, "{n}").map_err(|_| Violation::Table)?,
        (K_FLOAT | K_TS, Val::Float(f)) => write!(out, "{f}").map_err(|_| Violation::Table)?,
        (K_FLOAT | K_TS, Val::Int(n)) => write!(out, "{n}").map_err(|_| Violation::Table)?,
        (K_STR | K_TS, Val::Str(..) | Val::Esc(..)) => {
            string(out, ar.str_of(body, v).ok_or(Violation::Table)?)
        }
        (K_BLOB, Val::Blob(..)) => base64(out, ar.bytes_of(v).ok_or(Violation::Table)?),
        (K_STRUCT | K_UNION, Val::Obj(_)) => object(t, m.target, ar, body, v, out)?,
        (K_LIST, Val::Seq(first)) => {
            let elem = t.member(m.target).ok_or(Violation::Table)?;
            out.push(b'[');
            let mut at = first;
            let mut comma = false;
            while let Some(n) = ar.node(at).filter(|_| at != NIL) {
                if comma {
                    out.push(b',');
                }
                comma = true;
                value(t, &elem, ar, body, n.val, out)?;
                at = n.next;
            }
            out.push(b']');
        }
        (K_MAP, Val::Seq(first)) => {
            let val_m = t.member(m.target + 1).ok_or(Violation::Table)?;
            out.push(b'{');
            let mut at = first;
            let mut comma = false;
            while let Some(k) = ar.node(at).filter(|_| at != NIL) {
                let vn = ar.node(k.next).ok_or(Violation::Table)?;
                if comma {
                    out.push(b',');
                }
                comma = true;
                string(out, ar.str_of(body, k.val).ok_or(Violation::Table)?);
                out.push(b':');
                value(t, &val_m, ar, body, vn.val, out)?;
                at = vn.next;
            }
            out.push(b'}');
        }
        _ => return Err(Violation::Table),
    }
    Ok(())
}

fn string(out: &mut Vec<u8>, s: &str) {
    out.push(b'"');
    for c in s.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                const H: &[u8; 16] = b"0123456789abcdef";
                out.extend_from_slice(b"\\u00");
                out.push(H.get(((c as usize) >> 4) & 0xf).copied().unwrap_or(b'0'));
                out.push(H.get((c as usize) & 0xf).copied().unwrap_or(b'0'));
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

fn base64(out: &mut Vec<u8>, b: &[u8]) {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    out.push(b'"');
    for ch in b.chunks(3) {
        let n = (u32::from(ch.first().copied().unwrap_or(0)) << 16)
            | (u32::from(ch.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(ch.get(2).copied().unwrap_or(0));
        for (i, sh) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= ch.len() {
                out.push(A.get(((n >> sh) & 63) as usize).copied().unwrap_or(b'A'));
            } else {
                out.push(b'=');
            }
        }
    }
    out.push(b'"');
}
