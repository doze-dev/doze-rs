//! The keywords of strings, numbers and enums, and the order Java's `HashMap` gives names in.

use super::{Limit, Out};
use crate::jsonschema::node::{Num, Str};
use crate::jsonschema::number::Dec;

pub(super) fn limit_change(l: &Limit, o: Option<&Dec>, u: Option<&Dec>, path: &str, out: &mut Out) {
    let kind = match (o, u) {
        (None, None) => return,
        (None, Some(_)) => l.added,
        (Some(_), None) => l.removed,
        (Some(o), Some(u)) => match o.cmp(u) {
            std::cmp::Ordering::Less => l.increased,
            std::cmp::Ordering::Greater => l.decreased,
            std::cmp::Ordering::Equal => return,
        },
    };
    out.add(kind, path, l.keyword);
}

pub(super) fn string(o: &Str, u: &Str, path: &str, out: &mut Out) {
    limit_change(
        &limit!("maxLength", "MAX_LENGTH"),
        o.max_len.as_ref(),
        u.max_len.as_ref(),
        path,
        out,
    );
    limit_change(
        &limit!("minLength", "MIN_LENGTH"),
        o.min_len.as_ref(),
        u.min_len.as_ref(),
        path,
        out,
    );
    match (&o.pattern, &u.pattern) {
        (None, Some(_)) => out.add("PATTERN_ADDED", path, "/pattern"),
        (Some(_), None) => out.add("PATTERN_REMOVED", path, "/pattern"),
        (Some(a), Some(b)) if a != b => out.add("PATTERN_CHANGED", path, "/pattern"),
        _ => {}
    }
}

pub(super) fn number(o: &Num, u: &Num, path: &str, out: &mut Out) {
    // The limits first, then whether it became integer only (Confluent's order, recorded: "random 114").
    limit_change(
        &limit!("maximum", "MAXIMUM"),
        o.max.as_ref(),
        u.max.as_ref(),
        path,
        out,
    );
    limit_change(
        &limit!("minimum", "MINIMUM"),
        o.min.as_ref(),
        u.min.as_ref(),
        path,
        out,
    );
    limit_change(
        &limit!("exclusiveMaximum", "EXCLUSIVE_MAXIMUM"),
        o.ex_max.as_ref(),
        u.ex_max.as_ref(),
        path,
        out,
    );
    limit_change(
        &limit!("exclusiveMinimum", "EXCLUSIVE_MINIMUM"),
        o.ex_min.as_ref(),
        u.ex_min.as_ref(),
        path,
        out,
    );
    match (&o.multiple_of, &u.multiple_of) {
        (None, Some(_)) => out.add("MULTIPLE_OF_ADDED", path, "/multipleOf"),
        (Some(_), None) => out.add("MULTIPLE_OF_REMOVED", path, "/multipleOf"),
        (Some(a), Some(b)) if a != b => {
            let kind = if b.is_multiple_of(a) {
                "MULTIPLE_OF_EXPANDED"
            } else if a.is_multiple_of(b) {
                "MULTIPLE_OF_REDUCED"
            } else {
                "MULTIPLE_OF_CHANGED"
            };
            out.add(kind, path, "/multipleOf");
        }
        _ => {}
    }
    match (o.integer, u.integer) {
        (true, false) => out.add("TYPE_EXTENDED", path, ""),
        (false, true) => out.add("TYPE_NARROWED", path, ""),
        _ => {}
    }
}

pub(super) fn enumeration(o: &[String], u: &[String], path: &str, out: &mut Out) {
    let o_in_u = o.iter().all(|v| u.contains(v));
    let u_in_o = u.iter().all(|v| o.contains(v));
    let kind = match (o_in_u, u_in_o) {
        (true, true) => return,
        (true, false) => "ENUM_ARRAY_EXTENDED",
        (false, true) => "ENUM_ARRAY_NARROWED",
        (false, false) => "ENUM_ARRAY_CHANGED",
    };
    out.add(kind, path, "/enum");
}

/// The order a `java.util.HashMap` built from these names iterates them in: by bucket (`String.hashCode`, spread,
/// masked by a capacity that grows from 16 at three quarters full), and within a bucket in the order they were put in,
/// taken here to be by name.
pub(super) fn java_hash_order(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    java_hash_inserted(names)
}

/// [`java_hash_order`] for names inserted in the order given.
pub(super) fn java_hash_inserted(mut names: Vec<String>) -> Vec<String> {
    let mut capacity: u32 = 16;
    while names.len() > (capacity as usize) * 3 / 4 {
        capacity = capacity.saturating_mul(2);
    }
    let bucket = |s: &String| {
        let h = s
            .encode_utf16()
            .fold(0_i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)));
        let u = h.cast_unsigned();
        (u ^ (u >> 16)) & (capacity - 1)
    };
    names.sort_by_key(bucket);
    names
}
