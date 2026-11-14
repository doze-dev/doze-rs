//! The /etc/hosts block (Linux): every reserved apex name at its fixed address, for the machines whose resolv.conf
//! cannot route a domain, and as the floor everywhere else.
//!
//! The block is bounded by markers so it can be rewritten or removed exactly, without touching a line anyone else
//! owns. DDEV once decided it had no internet connection and appended to people's /etc/hosts until it was enormous; a
//! marked, idempotently replaced block cannot do that however many times it runs.

use crate::Name;
use crate::pool::{LOOPBACK, apex_services};
use std::path::Path;

pub const BEGIN: &str = "# BEGIN doze — managed, safe to delete";
pub const END: &str = "# END doze";

/// The managed block. Deliberately static: it does not reflect what is running, so it never needs rewriting as
/// services come and go, and a name whose service is stopped resolves to a refused connection rather than to "no such
/// host". In address order, so it is byte-stable (Check compares it with the file).
pub fn apex_block() -> String {
    let mut b = format!("{BEGIN}\n");
    for (service, offset) in apex_services() {
        b.push_str(&format!(
            "{}\t{}\n",
            LOOPBACK.at(offset),
            Name::apex(service).host
        ));
    }
    b.push_str(END);
    b.push('\n');
    b
}

/// `hosts` with the managed block replaced by `want`, or appended when it was not there; an empty `want` removes it.
///
/// Everything outside the markers is kept byte for byte: the file belongs to the person and to several other tools,
/// and the only lines that are ours are the ones between our own markers.
// fuzz:parser fuzz_setup
pub fn replace_block(hosts: &str, want: &str) -> String {
    let Some(start) = hosts.find(BEGIN) else {
        if want.is_empty() {
            return hosts.to_string();
        }
        let sep = if hosts.is_empty() || hosts.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        return format!("{hosts}{sep}\n{want}");
    };
    let end = match hosts.get(start..).and_then(|tail| tail.find(END)) {
        // The end marker went missing: truncating to the end of the file would delete whatever followed, so the block
        // is taken to be its first line and the rest is left alone.
        None => start + BEGIN.len(),
        // Offsets into the tail are relative to start; getting that wrong splices the file back together at the wrong
        // place and corrupts lines that were never ours.
        Some(rel) => {
            let end = start + rel + END.len();
            end + usize::from(hosts.get(end..).is_some_and(|t| t.starts_with('\n')))
        }
    };
    let (head, tail) = (
        hosts.get(..start).unwrap_or_default(),
        hosts.get(end..).unwrap_or_default(),
    );
    if want.is_empty() {
        // The blank line the block was separated by goes with it.
        let head = head.strip_suffix('\n').unwrap_or(head);
        let sep = if head.is_empty() || head.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        return format!("{head}{sep}{tail}");
    }
    format!("{head}{want}{tail}")
}

/// Whether the file at `path` carries exactly the block setup writes.
pub fn block_installed(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|h| h.contains(&apex_block()))
}

#[cfg(test)]
#[path = "hosts_tests.rs"]
mod tests;
