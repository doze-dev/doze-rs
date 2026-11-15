//! A stack's name when its daemon does not say one.

use std::path::Path;

/// A state folder's name without its hash: `shop-918fa699` is `shop`. A daemon from an older doze says no name and
/// no folder; its state folder still carries the name. A folder that does not end in a dash and eight lowercase hex
/// digits is its own name. (Go `nameFromKey`.)
pub fn name_from_key(key: &Path) -> String {
    let base = key
        .file_name()
        .map_or_else(|| key.to_string_lossy(), |n| n.to_string_lossy())
        .into_owned();
    match base.rsplit_once('-') {
        Some((name, hash))
            if !name.is_empty()
                && hash.len() == 8
                && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) =>
        {
            name.to_owned()
        }
        _ => base,
    }
}
