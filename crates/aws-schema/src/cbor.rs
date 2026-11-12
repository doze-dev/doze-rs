//! Encode a tree as CBOR (the RPC v2 CBOR protocol's body), driven by the schema like [`crate::enc`] is for JSON: member names
//! come from the tables, values from the tree. A timestamp is tag 1 over the seconds since the epoch, a blob is a byte string,
//! every length is definite.

use crate::arena::{Arena, NIL, Val};
use crate::json::Violation;
use crate::tables::{
    K_BLOB, K_BOOL, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STRUCT, K_TS, K_UNION, MemberRef,
    Tables,
};

type R = Result<(), Violation>;

/// Writes the structure `shape` held at `root`.
///
/// # Errors
/// `Violation::Table` when the tree does not fit the shape.
pub fn encode(t: &Tables, shape: u32, ar: &Arena, body: &str, root: u32, out: &mut Vec<u8>) -> R {
    let Some(n) = ar.node(root) else {
        return Err(Violation::Table);
    };
    object(t, shape, ar, body, n.val, out)
}

/// A CBOR head: the major type with its argument, in the shortest form.
fn head(out: &mut Vec<u8>, major: u8, arg: u64) {
    let m = major << 5;
    if arg < 24 {
        out.push(m | arg as u8);
    } else if let Ok(a) = u8::try_from(arg) {
        out.extend_from_slice(&[m | 24, a]);
    } else if let Ok(a) = u16::try_from(arg) {
        out.push(m | 25);
        out.extend_from_slice(&a.to_be_bytes());
    } else if let Ok(a) = u32::try_from(arg) {
        out.push(m | 26);
        out.extend_from_slice(&a.to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&arg.to_be_bytes());
    }
}

/// The header of a map of `n` pairs.
pub fn map_header(out: &mut Vec<u8>, n: usize) {
    head(out, 5, n as u64);
}

/// A text string.
pub fn text(out: &mut Vec<u8>, s: &str) {
    head(out, 3, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
}

fn int(out: &mut Vec<u8>, n: i64) {
    match u64::try_from(n) {
        Ok(u) => head(out, 0, u),
        Err(_) => head(out, 1, n.unsigned_abs() - 1),
    }
}

fn float(out: &mut Vec<u8>, f: f64) {
    out.push(0xfb);
    out.extend_from_slice(&f.to_be_bytes());
}

fn object(t: &Tables, shape: u32, ar: &Arena, body: &str, v: Val, out: &mut Vec<u8>) -> R {
    let Val::Obj(first) = v else {
        return Err(Violation::Table);
    };
    let sh = t.shape(shape).ok_or(Violation::Table)?;
    let count = members(ar, first);
    map_header(out, count);
    let mut at = first;
    while let Some(n) = ar.node(at).filter(|_| at != NIL) {
        let m = t
            .member(n.member)
            .filter(|_| n.member >= sh.first && n.member < sh.first + sh.count)
            .ok_or(Violation::Table)?;
        text(out, m.wire);
        value(t, &m, ar, body, n.val, out)?;
        at = n.next;
    }
    Ok(())
}

/// How many nodes the chain starting at `first` holds.
fn members(ar: &Arena, first: u32) -> usize {
    let (mut n, mut at) = (0, first);
    while let Some(node) = ar.node(at).filter(|_| at != NIL) {
        n += 1;
        at = node.next;
    }
    n
}

fn value(t: &Tables, m: &MemberRef<'_>, ar: &Arena, body: &str, v: Val, out: &mut Vec<u8>) -> R {
    match (m.kind, v) {
        (K_BOOL, Val::Bool(b)) => out.push(if b { 0xf5 } else { 0xf4 }),
        (K_INT, Val::Int(n)) => int(out, n),
        (K_FLOAT, Val::Float(f)) => float(out, f),
        #[allow(clippy::cast_precision_loss)]
        (K_FLOAT, Val::Int(n)) => float(out, n as f64),
        (K_TS, Val::Float(f)) => {
            head(out, 6, 1);
            float(out, f);
        }
        (K_TS, Val::Int(n)) => {
            head(out, 6, 1);
            int(out, n);
        }
        (K_STR, Val::Str(..) | Val::Esc(..)) => {
            text(out, ar.str_of(body, v).ok_or(Violation::Table)?)
        }
        (K_BLOB, Val::Blob(..)) => {
            let bytes = ar.bytes_of(v).ok_or(Violation::Table)?;
            head(out, 2, bytes.len() as u64);
            out.extend_from_slice(bytes);
        }
        (K_STRUCT | K_UNION, Val::Obj(_)) => object(t, m.target, ar, body, v, out)?,
        (K_LIST, Val::Seq(first)) => {
            let elem = t.member(m.target).ok_or(Violation::Table)?;
            head(out, 4, members(ar, first) as u64);
            let mut at = first;
            while let Some(n) = ar.node(at).filter(|_| at != NIL) {
                value(t, &elem, ar, body, n.val, out)?;
                at = n.next;
            }
        }
        (K_MAP, Val::Seq(first)) => {
            let val_m = t.member(m.target + 1).ok_or(Violation::Table)?;
            head(out, 5, (members(ar, first) / 2) as u64);
            let mut at = first;
            while let Some(k) = ar.node(at).filter(|_| at != NIL) {
                let vn = ar.node(k.next).ok_or(Violation::Table)?;
                text(out, ar.str_of(body, k.val).ok_or(Violation::Table)?);
                value(t, &val_m, ar, body, vn.val, out)?;
                at = vn.next;
            }
        }
        _ => return Err(Violation::Table),
    }
    Ok(())
}
