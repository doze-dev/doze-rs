//! Protocol versions (protocol.md row 1): a major both sides must share, and a minor each side knows up to. Within a
//! major, fields and calls are only ever added; a side uses a feature only when the agreed minor includes it.

use thiserror::Error;

/// The protocol this build speaks.
pub const CURRENT: Version = Version { major: 2, minor: 0 };

/// A protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

/// Why two sides cannot talk.
#[derive(Debug, Error, PartialEq, Eq)]
#[error("this side speaks protocol {ours_major}, the other speaks {theirs_major}: {advice}")]
pub struct Mismatch {
    pub ours_major: u16,
    pub theirs_major: u16,
    pub advice: &'static str,
}

/// The version both sides use: the shared major and the smaller minor.
pub fn negotiate(ours: Version, theirs: Version, ours_is_host: bool) -> Result<Version, Mismatch> {
    if ours.major != theirs.major {
        let host_newer = (ours.major > theirs.major) == ours_is_host;
        let advice = if host_newer {
            "update the module (doze modules upgrade)"
        } else {
            "update doze"
        };
        return Err(Mismatch {
            ours_major: ours.major,
            theirs_major: theirs.major,
            advice,
        });
    }
    Ok(Version {
        major: ours.major,
        minor: ours.minor.min(theirs.minor),
    })
}

impl Version {
    /// Whether a feature introduced in minor `since` may be used under this agreed version.
    pub fn has(self, since: u16) -> bool {
        self.minor >= since
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_older_minor_wins_and_a_different_major_says_who_should_update() {
        let v = |major, minor| Version { major, minor };
        assert_eq!(negotiate(v(2, 3), v(2, 1), true), Ok(v(2, 1)));
        assert_eq!(negotiate(v(2, 0), v(2, 5), false), Ok(v(2, 0)));
        assert!(v(2, 1).has(1) && !v(2, 1).has(2));
        let host_newer = negotiate(v(3, 0), v(2, 4), true).unwrap_err();
        assert_eq!(
            host_newer.advice,
            "update the module (doze modules upgrade)"
        );
        let module_newer = negotiate(v(2, 0), v(3, 0), true).unwrap_err();
        assert_eq!(module_newer.advice, "update doze");
        // Seen from the module, the same mismatch gives the same advice.
        assert_eq!(
            negotiate(v(2, 4), v(3, 0), false).unwrap_err().advice,
            "update the module (doze modules upgrade)"
        );
    }
}
