//! "Why is it this big": what a topic's configs keep, in a sentence, for the console's Data page and `doze-kafka data`.

use super::usage::Keeps;

const SECOND: i64 = 1_000;
const MINUTE: i64 = 60 * SECOND;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

fn unit(n: i64, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// A configured time as it was meant: `7 days`, `1 hour`, `90 minutes`, `250 ms` (the largest unit it is a whole number of).
#[must_use]
pub fn duration_words(ms: i64) -> String {
    for (size, name) in [
        (DAY, "day"),
        (HOUR, "hour"),
        (MINUTE, "minute"),
        (SECOND, "second"),
    ] {
        if ms >= size && ms % size == 0 {
            return unit(ms / size, name);
        }
    }
    format!("{ms} ms")
}

/// A wait, rounded as a person would say it: `about 3 days`, `about 5 hours`, `a few minutes`.
fn wait_words(ms: i64) -> String {
    if ms >= 2 * DAY {
        format!("about {}", unit((ms + DAY / 2) / DAY, "day"))
    } else if ms >= 2 * HOUR {
        format!("about {}", unit((ms + HOUR / 2) / HOUR, "hour"))
    } else if ms >= 2 * MINUTE {
        format!("about {}", unit((ms + MINUTE / 2) / MINUTE, "minute"))
    } else {
        "a minute or so".to_owned()
    }
}

/// Bytes as a person reads them, in powers of 1024: `512 B`, `1.5 KiB`, `128 MiB`.
#[must_use]
pub fn bytes_words(n: u64) -> String {
    const NAMES: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut whole = n;
    let mut i = 0;
    while whole >= 1024 * 1024 && i < NAMES.len() - 2 {
        whole /= 1024;
        i += 1;
    }
    if n < 1024 {
        return format!("{n} B");
    }
    // One step more, with a tenth (in integers: the tenths of `whole / 1024`).
    let tenths = (whole * 10 + 512) / 1024;
    let name = NAMES.get(i + 1).copied().unwrap_or("TiB");
    if tenths >= 100 {
        format!("{} {name}", (tenths + 5) / 10)
    } else {
        format!("{}.{} {name}", tenths / 10, tenths % 10)
    }
}

impl Keeps {
    /// Why a topic holds what it holds, in a sentence. `due_in_ms` is how long until retention's next pass could take its
    /// oldest segment, where the last pass worked that out (negative: due already, at the next pass).
    #[must_use]
    pub fn why(&self, due_in_ms: Option<i64>) -> String {
        if self.compact && !self.delete {
            return "Compacted: it keeps the latest value of every key, so it grows with the number of keys, not with time."
                .to_owned();
        }
        let limit = match (self.max_age_ms, self.max_bytes) {
            (None, None) if self.compact => {
                return "Compacted with delete, but no retention limit: it keeps the latest value of every key, for ever."
                    .to_owned();
            }
            (None, None) => {
                return "No retention limit: it keeps everything written to it, for ever."
                    .to_owned();
            }
            (Some(ms), None) => format!("Retention {}", duration_words(ms)),
            (None, Some(b)) => format!("Retention {} per partition", bytes_words(b)),
            (Some(ms), Some(b)) => format!(
                "Retention {} or {} per partition, whichever comes first",
                duration_words(ms),
                bytes_words(b)
            ),
        };
        let what = if self.compact {
            "compacted, and whole segments past it are deleted"
        } else {
            "whole segments past it are deleted"
        };
        let when = match (self.max_age_ms, due_in_ms) {
            (Some(_), Some(ms)) if ms <= 0 => " The oldest is due at the next sweep.".to_owned(),
            (Some(_), Some(ms)) => format!(" The oldest goes in {}.", wait_words(ms)),
            _ => String::new(),
        };
        format!("{limit}: {what}.{when}")
    }
}
