//! What the process knows about itself, where it can be read without `unsafe`: Linux's `/proc`, and the list of open
//! files, which Linux and macOS both keep as a directory.

use std::fs;

/// Memory, threads and open files of this process; `None` where the platform does not say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Process {
    /// Resident memory in bytes (Linux only).
    pub resident_bytes: Option<u64>,
    /// Threads (Linux only).
    pub threads: Option<u32>,
    /// Files and sockets open now.
    pub open_files: Option<usize>,
}

impl Process {
    /// Reads what there is to read.
    #[must_use]
    pub fn read() -> Self {
        Self {
            resident_bytes: fs::read_to_string("/proc/self/statm")
                .ok()
                .and_then(|s| resident_bytes(&s)),
            threads: fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|s| threads(&s)),
            open_files: ["/proc/self/fd", "/dev/fd"]
                .iter()
                .find_map(|d| fs::read_dir(d).ok())
                .map(Iterator::count),
        }
    }
}

/// The resident set of a `/proc/self/statm` text: its second number is in pages of 4 KiB (the page size of every
/// platform this broker is built for that has a `/proc`).
fn resident_bytes(statm: &str) -> Option<u64> {
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    pages.checked_mul(4096)
}

/// The `Threads:` line of a `/proc/self/status` text.
fn threads(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("Threads:"))?
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resident_set_is_the_second_number_in_pages() {
        assert_eq!(
            resident_bytes("2100 1100 300 10 0 800 0"),
            Some(1100 * 4096)
        );
        assert_eq!(resident_bytes("2100"), None);
        assert_eq!(resident_bytes("a b"), None);
        assert_eq!(resident_bytes(""), None);
    }

    #[test]
    fn the_thread_count_is_its_own_line_of_the_status() {
        assert_eq!(threads("Name:\tdoze\nThreads:\t3\nVmRSS: 1 kB\n"), Some(3));
        assert_eq!(threads("Name:\tdoze\n"), None);
        assert_eq!(threads("Threads:\tmany\n"), None);
    }
}
