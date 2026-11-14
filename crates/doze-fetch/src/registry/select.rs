//! Which release of a module this doze would take: the newest that speaks its module protocol, that it is new enough
//! to run (each release records its oldest doze), and that runs the engine majors asked for, preferring the stable
//! channel's head. The same policy as the Go `modindex.SelectFor`, so an older doze keeps resolving after a module
//! moves on, and says which doze to get when nothing is left.

use super::index::{Index, Release};
use std::cmp::Ordering;

/// The doze asking.
#[derive(Debug, Clone, Copy)]
pub struct Here<'a> {
    pub protocol: i64,
    pub doze: &'a str,
}

/// What the choice came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fit {
    /// This version.
    Release(String),
    /// Every release that would fit needs a newer doze: the oldest of those.
    NeedsDoze(String),
    /// No release speaks this doze's protocol (the protocols they do speak).
    Protocol { min: i64, max: i64 },
    /// No release runs the engine majors asked for.
    Engines,
    /// The index has no releases.
    Empty,
}

/// Numeric per dotted segment, text where a segment is not a number: 0.10.0 is newer than 0.9.1.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.split('.'), b.split('.'));
    loop {
        let o = match (x.next(), y.next()) {
            (None, None) => return Ordering::Equal,
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (Some(p), Some(q)) => match (p.parse::<u64>(), q.parse::<u64>()) {
                (Ok(m), Ok(n)) => m.cmp(&n),
                _ => p.cmp(q),
            },
        };
        if o != Ordering::Equal {
            return o;
        }
    }
}

/// A released doze's version: dotted numbers, with or without a leading `v`. A development build is not one.
pub fn is_doze_version(v: &str) -> bool {
    let v = v.strip_prefix('v').unwrap_or(v);
    !v.is_empty()
        && v.split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether a release runs every one of `majors` (a release with no list runs any).
pub fn supports(r: &Release, majors: &[String]) -> bool {
    r.engines.is_empty() || majors.iter().all(|m| m.is_empty() || r.engines.contains(m))
}

/// The release this doze would take for `majors`.
pub fn newest_that_fits(idx: &Index, here: Here<'_>, majors: &[String]) -> Fit {
    if idx.releases.is_empty() {
        return Fit::Empty;
    }
    let released = is_doze_version(here.doze);
    let doze = here.doze.strip_prefix('v').unwrap_or(here.doze);
    let too_new =
        |r: &Release| released && !r.doze.is_empty() && compare_versions(&r.doze, doze).is_gt();
    let speaks = |r: &Release| r.protocol == here.protocol;
    let mut fits: Vec<&str> = idx
        .releases
        .iter()
        .filter(|(_, r)| speaks(r) && supports(r, majors) && !too_new(r))
        .map(|(v, _)| v.as_str())
        .collect();
    fits.sort_by(|a, b| compare_versions(b, a));
    if let Some(head) = idx.stable().filter(|h| fits.contains(h)) {
        return Fit::Release(head.to_string());
    }
    if let Some(v) = fits.first() {
        return Fit::Release((*v).to_string());
    }
    let need = idx
        .releases
        .values()
        .filter(|r| speaks(r) && supports(r, majors) && too_new(r))
        .map(|r| r.doze.as_str())
        .min_by(|a, b| compare_versions(a, b));
    if let Some(need) = need {
        return Fit::NeedsDoze(need.to_string());
    }
    if idx.releases.values().any(speaks) {
        return Fit::Engines;
    }
    let protocols = idx.releases.values().map(|r| r.protocol);
    Fit::Protocol {
        min: protocols.clone().min().unwrap_or_default(),
        max: protocols.max().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn rel(protocol: i64, engines: &[&str], doze: &str) -> Release {
        Release {
            protocol,
            engines: engines.iter().map(|e| e.to_string()).collect(),
            artifacts: BTreeMap::new(),
            doze: doze.into(),
        }
    }

    fn index(rs: &[(&str, Release)], stable: &str) -> Index {
        Index {
            module: "valkey".into(),
            namespace: "doze".into(),
            releases: rs.iter().map(|(v, r)| (v.to_string(), r.clone())).collect(),
            channels: Some(BTreeMap::from([("stable".into(), stable.into())])),
            signature: String::new(),
        }
    }

    /// The selection policy, case by case: it decides what a stack runs, and the Go doze applies the same.
    #[test]
    fn picks_as_the_go_doze_does() {
        let here = Here {
            protocol: 2,
            doze: "0.4.0",
        };
        let nine = ["9".to_string()];
        let idx = index(
            &[
                ("0.2.1", rel(1, &["8", "9"], "")),
                ("0.3.0", rel(2, &["8", "9"], "0.3.0")),
                ("0.10.0", rel(2, &["9"], "0.4.0")),
                ("0.11.0", rel(2, &["9", "10"], "0.5.0")),
            ],
            "0.11.0",
        );
        // The stable head needs doze 0.5: the newest this doze runs is taken instead.
        assert_eq!(
            newest_that_fits(&idx, here, &nine),
            Fit::Release("0.10.0".into())
        );
        // Engine 8 is only in 0.3.0.
        assert_eq!(
            newest_that_fits(&idx, here, &["8".into()]),
            Fit::Release("0.3.0".into())
        );
        assert_eq!(
            newest_that_fits(&idx, here, &["10".into()]),
            Fit::NeedsDoze("0.5.0".into())
        );
        assert_eq!(newest_that_fits(&idx, here, &["7".into()]), Fit::Engines);
        // A development build runs every release.
        let dev = Here {
            protocol: 2,
            doze: "dev",
        };
        assert_eq!(
            newest_that_fits(&idx, dev, &nine),
            Fit::Release("0.11.0".into())
        );
        let old = index(&[("0.2.1", rel(1, &[], ""))], "0.2.1");
        assert_eq!(
            newest_that_fits(&old, here, &[]),
            Fit::Protocol { min: 1, max: 1 }
        );
        assert_eq!(compare_versions("0.10.0", "0.9.9"), Ordering::Greater);
        assert_eq!(compare_versions("1.2", "1.2.0"), Ordering::Less);
    }
}
