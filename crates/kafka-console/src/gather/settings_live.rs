//! What the Settings page says that only the running broker knows, each from the place that knows it: which version a client
//! speaks (the request recorder, slice 9e), the cleaner's last round and whether a look now finds that log due (the log facts,
//! slice 9g), and when a group's committed offsets expire (the group's hidden facts, slice 9c). Each is left out where its
//! source has nothing: a console with no recorder names no client, a broker that has cleaned nothing names no round.

use super::group_read::listed;
use crate::backend::Backend;
use crate::format::{clock, span};
use crate::model::{SettingsHeard, SettingsRound, SettingsSpeaks};

/// The APIs a client is named for, in the order the sentence says them.
pub const SPOKEN: [kafka_wire::messages::ApiKey; 2] = [
    kafka_wire::messages::ApiKey::Fetch,
    kafka_wire::messages::ApiKey::Produce,
];

/// The most groups looked into for an expiry.
const MOST_GROUPS: usize = 200;

/// What the tape heard, with whether each version is inside what the profile offers (`offered`: name, lowest, highest).
#[must_use]
pub fn speaking(heard: &[SettingsHeard], offered: &[(String, i16, i16)]) -> Vec<SettingsSpeaks> {
    heard
        .iter()
        .map(|h| SettingsSpeaks {
            client: h.client.clone(),
            api: h.api,
            version: h.version,
            inside: offered
                .iter()
                .any(|(name, lo, hi)| name == h.api && (*lo..=*hi).contains(&h.version)),
        })
        .collect()
}

/// `21:31`, UTC.
#[must_use]
pub fn hour_minute(ms: i64) -> String {
    clock(ms).chars().take(5).collect()
}

/// The cleaner's last round: where and when, and whether a look now finds that partition due (the planner's own verdict,
/// `None` where the log could not be read).
pub async fn last_round(b: &Backend) -> Option<SettingsRound> {
    let (topic, partition, round) = b.last_cleaner_round()?;
    let due = b
        .log_facts(&topic, partition)
        .await
        .and_then(|f| f.compaction)
        .map(|c| c.verdict.is_some());
    Some(SettingsRound {
        at: hour_minute(round.at_ms),
        topic,
        partition,
        due,
    })
}

/// The group whose offsets the sweep takes first, and in how long from `now_ms`: `None` if no group has a date.
pub async fn soonest_expiry(b: &Backend, now_ms: i64) -> Option<(String, String)> {
    let mut soonest: Option<(i64, String)> = None;
    for g in listed(b).await.into_iter().take(MOST_GROUPS) {
        let Some(d) = b.group_detail(&g.id).await else {
            continue;
        };
        let Some(at) = d.offsets.iter().filter_map(|o| o.expires_ms).min() else {
            continue;
        };
        if soonest.as_ref().is_none_or(|(t, _)| at < *t) {
            soonest = Some((at, g.id));
        }
    }
    let (at, group) = soonest?;
    Some((group, span(((at - now_ms) / 1000).max(0))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(client: &str, api: &'static str, version: i16) -> SettingsHeard {
        SettingsHeard {
            client: client.to_owned(),
            api,
            version,
        }
    }

    fn offered() -> Vec<(String, i16, i16)> {
        vec![
            ("Fetch".to_owned(), 4, 12),
            ("Produce".to_owned(), 3, 9),
            ("Metadata".to_owned(), 0, 12),
        ]
    }

    #[test]
    fn what_was_heard_is_inside_when_the_version_is_in_the_range_offered_edges_included() {
        let got = speaking(
            &[
                heard("fulfilment", "Fetch", 12),
                heard("orders", "Produce", 9),
                heard("low", "Fetch", 4),
            ],
            &offered(),
        );
        assert!(got.iter().all(|s| s.inside), "{got:?}");
        assert_eq!(
            (got[0].client.as_str(), got[0].api, got[0].version),
            ("fulfilment", "Fetch", 12)
        );
    }

    #[test]
    fn a_version_outside_the_range_or_of_an_api_not_offered_is_not_inside() {
        let got = speaking(
            &[
                heard("a", "Fetch", 13),
                heard("b", "Produce", 2),
                heard("c", "Fetch", 3),
                heard("d", "Nothing", 1),
            ],
            &offered(),
        );
        assert!(got.iter().all(|s| !s.inside), "{got:?}");
        assert!(speaking(&[], &offered()).is_empty());
    }

    #[test]
    fn the_hour_and_minute_are_utc_with_two_digits_each() {
        assert_eq!(hour_minute(0), "00:00");
        assert_eq!(hour_minute(((21 * 60) + 31) * 60_000 + 59_999), "21:31");
    }
}
