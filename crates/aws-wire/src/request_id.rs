//! The id of a request: a random UUID, lower case, as `x-amzn-RequestId` carries it.

use std::fmt;

/// A request's id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestId(String);

impl RequestId {
    /// A fresh random id (UUID version 4).
    ///
    /// If the operating system has no randomness to give, the id is all zeros apart from its version bits: a request is
    /// not failed for want of an identifier.
    #[must_use]
    pub fn new() -> Self {
        let mut b = [0u8; 16];
        // WHY: a failed read leaves the zeros above, which is a valid (if unremarkable) id.
        if getrandom::fill(&mut b).is_err() {
            b = [0; 16];
        }
        if let Some(x) = b.get_mut(6) {
            *x = (*x & 0x0f) | 0x40;
        }
        if let Some(x) = b.get_mut(8) {
            *x = (*x & 0x3f) | 0x80;
        }
        let hex = aws_core::hex::encode(&b);
        let part = |r: std::ops::Range<usize>| hex.get(r).unwrap_or_default().to_owned();
        Self(format!(
            "{}-{}-{}-{}-{}",
            part(0..8),
            part(8..12),
            part(12..16),
            part(16..20),
            part(20..32)
        ))
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_a_version_4_uuid_and_differs_each_time() {
        let a = RequestId::new();
        let s = a.as_str();
        assert_eq!(s.len(), 36);
        assert_eq!(s.as_bytes().get(14), Some(&b'4'));
        assert!(matches!(
            s.as_bytes().get(19),
            Some(b'8' | b'9' | b'a' | b'b')
        ));
        assert_ne!(a, RequestId::new());
    }
}
