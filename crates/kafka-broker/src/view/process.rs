//! What the process knows about itself, where it can be read without `unsafe`: Linux's `/proc`, and the list of open
//! files, which Linux and macOS both keep as a directory.

use std::fs;

/// Memory, threads and open files of this process; `None` where the platform does not say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Process {
    /// Resident memory in bytes (Linux only; read from `VmRSS`, which is in kB whatever the page size).
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
        let status = fs::read_to_string("/proc/self/status").ok();
        Self {
            resident_bytes: status.as_deref().and_then(resident_bytes),
            threads: status.as_deref().and_then(threads),
            open_files: ["/proc/self/fd", "/dev/fd"]
                .iter()
                .find_map(|d| fs::read_dir(d).ok())
                .map(open_files_in),
        }
    }
}

/// The descriptors a listing of the open-files directory counts: one entry per open descriptor, and the listing's own
/// handle is one of them (it is open while the directory is read), which is not the process's.
fn open_files_in<T>(listing: impl Iterator<Item = T>) -> usize {
    listing.count().saturating_sub(1)
}

/// The `VmRSS:` line of a `/proc/self/status` text, in bytes. The kernel writes it in kB; `/proc/self/statm` counts pages,
/// whose size is 4 KiB on x86_64 and may be 16 or 64 KiB on aarch64 Linux, and nothing here knows which.
fn resident_bytes(status: &str) -> Option<u64> {
    let value = status.lines().find_map(|l| l.strip_prefix("VmRSS:"))?;
    let mut words = value.split_whitespace();
    let number: u64 = words.next()?.parse().ok()?;
    let unit: u64 = match words.next()?.to_ascii_lowercase().as_str() {
        "kb" => 1024,
        "mb" => 1024 * 1024,
        "gb" => 1024 * 1024 * 1024,
        _ => return None,
    };
    number.checked_mul(unit)
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
    fn the_resident_set_is_the_vmrss_line_in_kb_whatever_the_page_size() {
        let status = "Name:\tdoze-kafka\nVmPeak:\t  900000 kB\nVmRSS:\t    5936 kB\nRssAnon:\t    2000 kB\nThreads:\t3\n";
        assert_eq!(resident_bytes(status), Some(5936 * 1024));
        // The line that only starts with the same letters is another line.
        assert_eq!(resident_bytes("RssAnon:\t 7 kB\nVmRSSX:\t 8 kB\n"), None);
        assert_eq!(resident_bytes("VmRSS:\t 12 kB"), Some(12 * 1024));
        assert_eq!(resident_bytes("VmRSS:\t 3 mB"), Some(3 * 1024 * 1024));
        // No unit, a unit that is not known, a number that is not one, no line: nothing is claimed.
        assert_eq!(resident_bytes("VmRSS:\t 12\n"), None);
        assert_eq!(resident_bytes("VmRSS:\t 12 pages\n"), None);
        assert_eq!(resident_bytes("VmRSS:\t many kB\n"), None);
        assert_eq!(resident_bytes("VmRSS:\t 18446744073709551615 kB\n"), None);
        assert_eq!(resident_bytes(""), None);
        // A 64 KiB page kernel and a 4 KiB one say the same kB for the same memory: the old statm reading did not.
        assert_eq!(resident_bytes("VmRSS:\t 64 kB\n"), Some(64 * 1024));
    }

    #[test]
    fn the_listing_of_the_open_files_does_not_count_its_own_handle() {
        assert_eq!(open_files_in(["0", "1", "2", "3"].into_iter()), 3);
        assert_eq!(open_files_in(std::iter::empty::<u8>()), 0);
        assert_eq!(open_files_in(["3"].into_iter()), 0);
    }

    #[test]
    fn the_thread_count_is_its_own_line_of_the_status() {
        assert_eq!(threads("Name:\tdoze\nThreads:\t3\nVmRSS: 1 kB\n"), Some(3));
        assert_eq!(threads("Name:\tdoze\n"), None);
        assert_eq!(threads("Threads:\tmany\n"), None);
    }
}
