//! How fast each topic grows, from the console's own looks at the data directory: nothing samples on a timer. Each time a
//! page asks for the usage, its sizes are noted (at most one look a minute is kept, the last sixteen of them), and a topic's
//! growth is its size now against the oldest look kept that is at least a minute old. While nobody looks, nothing runs and
//! the looks kept are all there is.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};

/// The least time between two looks kept.
const SPACING_MS: i64 = 60_000;
/// How many looks are kept: a quarter of an hour, a minute apart.
const MOST_LOOKS: usize = 16;
/// The most topics a look notes: the largest (a topic past them shows no growth).
const MOST_TOPICS: usize = 500;
const HOUR_MS: i128 = 3_600_000;

#[derive(Debug)]
struct Look {
    at_ms: i64,
    bytes: HashMap<String, u64>,
}

/// The looks kept, oldest first, and the heads-up the last one called for (the top bar says it without looking again).
#[derive(Debug, Default)]
pub struct Growth {
    looks: Mutex<VecDeque<Look>>,
    alarm: Mutex<Option<crate::model::DiskAlarm>>,
}

/// What the looks say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rates {
    /// Each topic's growth in bytes an hour (negative: it shrank), for the topics the base look has.
    pub per_hour: HashMap<String, i64>,
    /// When the look it is measured from was taken; `None` until one is a minute old.
    pub since_ms: Option<i64>,
}

impl Growth {
    /// Notes a look taken at `now_ms` with each topic's bytes (largest first), and says how each grew since the oldest look
    /// kept that is at least a minute old.
    pub fn note(&self, now_ms: i64, topics: &[(String, u64)]) -> Rates {
        let mut looks = self.looks.lock().unwrap_or_else(PoisonError::into_inner);
        let base = looks
            .iter()
            .find(|l| now_ms.saturating_sub(l.at_ms) >= SPACING_MS);
        let rates = match base {
            Some(base) => {
                let span = i128::from(now_ms - base.at_ms);
                Rates {
                    per_hour: topics
                        .iter()
                        .filter_map(|(name, now)| {
                            let then = *base.bytes.get(name)?;
                            let delta = i128::from(*now) - i128::from(then);
                            let hourly = delta * HOUR_MS / span;
                            Some((name.clone(), i64::try_from(hourly).unwrap_or(i64::MAX)))
                        })
                        .collect(),
                    since_ms: Some(base.at_ms),
                }
            }
            None => Rates::default(),
        };
        let due = looks
            .back()
            .is_none_or(|l| now_ms.saturating_sub(l.at_ms) >= SPACING_MS);
        if due {
            looks.push_back(Look {
                at_ms: now_ms,
                bytes: topics.iter().take(MOST_TOPICS).cloned().collect(),
            });
            while looks.len() > MOST_LOOKS {
                looks.pop_front();
            }
        }
        rates
    }

    /// Forgets every look: after Empty all topics or Erase everything, the sizes before say nothing of growth.
    pub fn forget(&self) {
        self.looks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.set_alarm(None);
    }

    /// Keeps the heads-up the latest look called for.
    pub fn set_alarm(&self, alarm: Option<crate::model::DiskAlarm>) {
        *self.alarm.lock().unwrap_or_else(PoisonError::into_inner) = alarm;
    }

    /// The heads-up the latest look called for; `None` before any look.
    #[must_use]
    pub fn alarm(&self) -> Option<crate::model::DiskAlarm> {
        self.alarm
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(pairs: &[(&str, u64)]) -> Vec<(String, u64)> {
        pairs.iter().map(|(n, b)| ((*n).to_owned(), *b)).collect()
    }

    #[test]
    fn growth_is_measured_from_the_oldest_look_a_minute_old_and_said_per_hour() {
        let g = Growth::default();
        assert_eq!(
            g.note(0, &sizes(&[("a", 100)])),
            Rates::default(),
            "one look says nothing"
        );
        assert_eq!(
            g.note(30_000, &sizes(&[("a", 150)])).since_ms,
            None,
            "not a minute yet"
        );
        let r = g.note(120_000, &sizes(&[("a", 4_100), ("new", 5)]));
        assert_eq!(r.since_ms, Some(0));
        // 4,000 bytes in two minutes: 120,000 an hour. A topic the base look did not have has no growth yet.
        assert_eq!(r.per_hour.get("a"), Some(&120_000));
        assert_eq!(r.per_hour.get("new"), None);
        let shrunk = g.note(180_000, &sizes(&[("a", 0)]));
        assert!(shrunk.per_hour["a"] < 0, "an emptied topic shrank");
        g.forget();
        assert_eq!(g.note(240_000, &sizes(&[("a", 0)])), Rates::default());
    }

    #[test]
    fn at_most_sixteen_looks_are_kept_a_minute_apart() {
        let g = Growth::default();
        for minute in 0..40 {
            g.note(minute * 30_000, &sizes(&[("a", 1)]));
        }
        let looks = g.looks.lock().unwrap();
        assert_eq!(looks.len(), MOST_LOOKS);
        assert!(
            looks
                .iter()
                .zip(looks.iter().skip(1))
                .all(|(a, b)| b.at_ms - a.at_ms >= SPACING_MS)
        );
    }
}
