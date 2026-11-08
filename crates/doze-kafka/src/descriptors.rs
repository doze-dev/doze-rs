//! File descriptors: asking for as many as the system allows, and not letting open partitions use them all.

/// The most descriptors worth asking for. macOS refuses a soft limit above 10240 whatever the hard limit says; the
/// `rlimit` crate lowers the ask there.
const WANTED: u64 = 65_536;
/// Kept back for connections, the lock, the metadata store, the log and everything else the process opens.
const RESERVED: u64 = 256;
/// Descriptors an open partition may hold: its log, its index, one more while a directory is flushed, and one of
/// headroom.
const PER_PARTITION: u64 = 4;

/// Raises the soft limit on descriptors toward the hard one, and returns the limit now in force.
pub(crate) fn raise() -> u64 {
    rlimit::increase_nofile_limit(WANTED).unwrap_or_else(|e| {
        tracing::warn!("the limit on open files could not be raised: {e}");
        rlimit::getrlimit(rlimit::Resource::NOFILE).map_or(1024, |(soft, _)| soft)
    })
}

/// How many partitions may stay open at `limit` descriptors, given the `configured` number (0: no bound of its own).
/// Never fewer than eight, so that a broker with a tiny limit still works, one partition at a time.
pub(crate) fn open_partitions(configured: usize, limit: u64) -> usize {
    let fit = usize::try_from(limit.saturating_sub(RESERVED) / PER_PARTITION)
        .unwrap_or(usize::MAX)
        .max(8);
    if configured == 0 {
        fit
    } else {
        configured.min(fit)
    }
}

#[cfg(test)]
mod tests {
    use super::open_partitions;

    #[test]
    fn the_configured_number_stands_when_the_limit_has_room_for_it() {
        assert_eq!(open_partitions(512, 65_536), 512);
    }

    #[test]
    fn a_low_limit_lowers_it_and_leaves_room_for_everything_else() {
        assert_eq!(open_partitions(512, 1024), 192, "(1024 - 256) / 4");
        assert_eq!(open_partitions(512, 256), 8, "never below eight");
    }

    #[test]
    fn unbounded_is_bounded_by_the_limit() {
        assert_eq!(open_partitions(0, 10_240), 2496);
    }
}
