//! Settings › About's update line (Go `serve/update.go`): whether a newer doze is out, from GitHub's latest release.
//! It asks only when the page asks (About opens, Check now, the menu's Check for Updates…); nothing runs in the
//! background. An answer is kept six hours, or until a check is forced.
//!
//! The request itself is the app's to make, through doze's one HTTP client (`doze_cli::http`, the system's `curl` in the
//! app: docs/port/weight.md, "One HTTP stack"); this module only reads the answer. No `curl`, or no network, reads as
//! "could not reach GitHub".

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::page::time::{MONTHS, Zone, civil, secs};

/// doze's newest release.
pub const LATEST_URL: &str = "https://api.github.com/repos/doze-dev/doze/releases/latest";

/// How long an answer is kept.
pub const KEPT: Duration = Duration::from_secs(6 * 3600);

/// What About says of doze's own version (Go `view.Update`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Update {
    pub current: String,
    /// A development build: no release to compare with.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dev: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub latest: String,
    /// "10 Nov 2026".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub published: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub newer: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// "15:04".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub checked: String,
}

/// The parts of a release About reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Release {
    #[serde(default, rename = "tag_name")]
    pub tag: String,
    #[serde(default, rename = "html_url")]
    pub url: String,
    #[serde(default, rename = "published_at")]
    pub date: String,
}

/// A release, from GitHub's answer: its status and body. The error is what About says.
// fuzz:parser fuzz_release
pub fn parse_release(status: u16, body: &str) -> Result<Release, String> {
    let rel = (status == 200)
        .then(|| serde_json::from_str::<Release>(body).ok())
        .flatten()
        .filter(|r| !r.tag.is_empty());
    rel.ok_or_else(|| format!("GitHub answered {}", status_words(status)))
}

fn status_words(code: u16) -> String {
    let reason = match code {
        200 => "OK",
        301 => "Moved Permanently",
        302 => "Found",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "",
    };
    format!("{code} {reason}").trim_end().to_owned()
}

/// A version names a release: numbers between dots, an optional `v` first (Go `dozeversion.Released`).
pub fn released(v: &str) -> bool {
    let v = v.strip_prefix('v').unwrap_or(v);
    !v.is_empty() && v.split('.').all(|p| p.parse::<u64>().is_ok())
}

/// `a` is an older release than `b` (Go `dozeversion.Older`); a development build is older than nothing.
pub fn older(a: &str, b: &str) -> bool {
    if !released(a) || !released(b) {
        return false;
    }
    let nums = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (nums(a), nums(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x < y;
        }
    }
    false
}

/// What About says, given this doze's version and what GitHub answered (`None`: it could not be reached).
pub fn update_of(current: &str, got: Option<(u16, String)>, now: SystemTime, zone: Zone) -> Update {
    let mut u = Update {
        current: current.trim_start_matches('v').to_owned(),
        dev: !released(current),
        ..Update::default()
    };
    match got {
        None => u.error = "could not reach GitHub to check".into(),
        Some((status, body)) => match parse_release(status, &body) {
            Err(e) => u.error = e,
            Ok(rel) => {
                u.latest = rel.tag.trim_start_matches('v').to_owned();
                u.url = rel.url;
                u.published = rfc3339(&rel.date).map(|t| day(t, zone)).unwrap_or_default();
                u.newer = older(&u.current, &u.latest);
            }
        },
    }
    let hm = zone.clock(now);
    u.checked = hm.get(..5).unwrap_or(&hm).to_owned();
    u
}

/// "2 Jan 2006".
fn day(t: SystemTime, zone: Zone) -> String {
    let (y, m, d) = civil((secs(t) + i64::from(zone.0)).div_euclid(86_400));
    let month = MONTHS.get(m.saturating_sub(1) as usize).unwrap_or(&"");
    format!("{d} {month} {y}")
}

/// An RFC 3339 time (`2026-11-10T10:00:00Z`, `…+05:30`, fractions allowed).
fn rfc3339(s: &str) -> Option<SystemTime> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let rest = s
        .get(19..)?
        .trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let off = match rest {
        "Z" | "z" => 0,
        r if r.len() == 6 => {
            let sign = if r.starts_with('-') { -1 } else { 1 };
            sign * (r.get(1..3)?.parse::<i64>().ok()? * 3600
                + r.get(4..6)?.parse::<i64>().ok()? * 60)
        }
        _ => return None,
    };
    let days = days_from_civil(y, mo, d);
    let t = days * 86_400 + h * 3600 + mi * 60 + se - off;
    u64::try_from(t)
        .ok()
        .map(|t| UNIX_EPOCH + Duration::from_secs(t))
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The last answer, kept six hours.
#[derive(Debug, Clone, Default)]
pub struct Kept {
    last: Option<(SystemTime, Update)>,
}

impl Kept {
    /// The answer kept, unless it is old or a fresh one is asked for.
    pub fn get(&self, now: SystemTime, force: bool) -> Option<Update> {
        let (at, u) = self.last.as_ref()?;
        let young = now.duration_since(*at).is_ok_and(|d| d < KEPT);
        (!force && young).then(|| u.clone())
    }

    /// Keeps an answer.
    pub fn keep(&mut self, now: SystemTime, u: Update) {
        self.last = Some((now, u));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> SystemTime {
        rfc3339(s).unwrap()
    }

    #[test]
    fn versions_compare_as_go_compares_them() {
        for (a, b, want) in [
            ("0.1.1", "0.2.0", true),
            ("0.2.0", "0.2.0", false),
            ("v0.10.0", "0.9.9", false),
            ("0.2", "0.2.1", true),
            ("dev", "0.2.0", false),
            ("0.2.0", "dev", false),
        ] {
            assert_eq!(older(a, b), want, "{a} {b}");
        }
    }

    /// Go's `TestUpdateSaysWhenANewerDozeIsOut`, against a canned answer.
    #[test]
    fn says_when_a_newer_doze_is_out() {
        let body = r#"{"tag_name":"v0.3.0","html_url":"https://example/r","published_at":"2026-11-10T10:00:00Z"}"#;
        let now = at("2026-11-13T09:05:00Z");
        let u = update_of("0.2.1", Some((200, body.into())), now, Zone::UTC);
        assert!(u.newer && u.latest == "0.3.0" && u.url == "https://example/r");
        assert_eq!(u.published, "10 Nov 2026");
        assert_eq!(u.checked, "09:05");
        assert!(!update_of("0.3.0", Some((200, body.into())), now, Zone::UTC).newer);
        let dev = update_of("dev", Some((200, body.into())), now, Zone::UTC);
        assert!(dev.dev && !dev.newer);
        assert_eq!(
            update_of("0.2.1", Some((403, "{}".into())), now, Zone::UTC).error,
            "GitHub answered 403 Forbidden"
        );
        assert_eq!(
            update_of("0.2.1", None, now, Zone::UTC).error,
            "could not reach GitHub to check"
        );
        assert_eq!(
            rfc3339("2026-11-10T15:30:00+05:30"),
            Some(at("2026-11-10T10:00:00Z"))
        );
        assert!(rfc3339("2026-11-10").is_none());
    }

    #[test]
    fn an_answer_is_kept_six_hours() {
        let mut k = Kept::default();
        let now = at("2026-11-13T09:00:00Z");
        assert!(k.get(now, false).is_none());
        k.keep(now, Update::default());
        assert!(k.get(now + Duration::from_secs(3600), false).is_some());
        assert!(k.get(now + Duration::from_secs(3600), true).is_none());
        assert!(k.get(now + KEPT, false).is_none());
    }
}
