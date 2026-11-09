//! File descriptors: asking for as many as the system allows, and not letting open partitions use them all.

/// The most descriptors worth asking for. macOS refuses a soft limit above 10240 whatever the hard limit says; the
/// `rlimit` crate lowers the ask there.
const WANTED: u64 = 65_536;
/// Kept back for the lock, the metadata store, the log and everything else the process opens besides connections.
const RESERVED: u64 = 256;
/// Descriptors an open partition may hold: its log, its index, one more while a directory is flushed, and one of
/// headroom.
const PER_PARTITION: u64 = 4;

/// Raises the soft limit on descriptors toward the hard one, and returns the limit now in force.
#[cfg(unix)]
pub(crate) fn raise() -> u64 {
    rlimit::increase_nofile_limit(WANTED).unwrap_or_else(|e| {
        tracing::warn!("the limit on open files could not be raised: {e}");
        rlimit::getrlimit(rlimit::Resource::NOFILE).map_or(1024, |(soft, _)| soft)
    })
}

/// Windows has no descriptor limit of this kind to raise (handles are bounded by memory), so the ask is the answer.
#[cfg(not(unix))]
pub(crate) fn raise() -> u64 {
    WANTED
}

/// How many files the open partitions may hold open between them at `limit` descriptors, given that `connections` may be
/// open as well, each holding one: what is left after those and the reserve, and never fewer than 64.
pub(crate) fn open_files(limit: u64, connections: usize) -> usize {
    let spoken_for = RESERVED.saturating_add(u64::try_from(connections).unwrap_or(u64::MAX));
    usize::try_from(limit.saturating_sub(spoken_for))
        .unwrap_or(usize::MAX)
        .max(64)
}

/// How many partitions may stay open at `limit` descriptors, given the `configured` number (0: no bound of its own)
/// and that `connections` may be open as well, each holding one. Never fewer than eight, so that a broker with a tiny
/// limit still works, one partition at a time.
pub(crate) fn open_partitions(configured: usize, limit: u64, connections: usize) -> usize {
    let spoken_for = RESERVED.saturating_add(u64::try_from(connections).unwrap_or(u64::MAX));
    let fit = usize::try_from(limit.saturating_sub(spoken_for) / PER_PARTITION)
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
    use super::{open_files, open_partitions};

    #[test]
    fn the_configured_number_stands_when_the_limit_has_room_for_it() {
        assert_eq!(open_partitions(512, 65_536, 0), 512);
    }

    #[test]
    fn a_low_limit_lowers_it_and_leaves_room_for_everything_else() {
        assert_eq!(open_partitions(512, 1024, 0), 192, "(1024 - 256) / 4");
        assert_eq!(open_partitions(512, 256, 0), 8, "never below eight");
    }

    #[test]
    fn every_connection_that_may_be_open_is_a_descriptor_too() {
        // 10,240 less 256 less 2,048 connections, a quarter of what is left.
        assert_eq!(open_partitions(100_000, 10_240, 2_048), 1_984);
        assert_eq!(
            open_partitions(100_000, 1_000, 2_048),
            8,
            "and never below eight"
        );
    }

    #[test]
    fn the_files_left_for_partitions_are_what_the_limit_leaves_after_the_reserve_and_the_connections()
     {
        assert_eq!(open_files(10_240, 2_048), 7_936);
        assert_eq!(open_files(300, 2_048), 64, "and never below sixty-four");
    }

    #[test]
    fn unbounded_is_bounded_by_the_limit() {
        assert_eq!(open_partitions(0, 10_240, 0), 2496);
    }
}
