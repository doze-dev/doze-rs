//! What the interpreter needs from the world besides a definition: the time and a source of randomness (retry jitter, `States.UUID`,
//! `States.MathRandom`, task tokens). Both are handed in, so an execution is a pure function of its inputs and a test can fix them.

use std::cell::Cell;

/// The clock and the random numbers of one step of an execution.
#[derive(Debug)]
pub struct Env {
    /// Now, in milliseconds since the epoch.
    pub now_ms: i64,
    rng: Cell<u64>,
    /// Whether task tokens may be made (an Express execution cannot wait for one).
    pub tokens: bool,
    /// What every Wait and retry interval is multiplied by (1 is AWS; a local-testing setting).
    pub wait_scale: f64,
    /// The region and account, for ARNs the interpreter writes (a Map Run's).
    pub region: String,
    pub account: String,
}

impl Env {
    /// An environment at `now_ms`, its random numbers drawn from `seed` (any value; zero is moved off).
    #[must_use]
    pub fn new(now_ms: i64, seed: u64) -> Self {
        Self {
            now_ms,
            rng: Cell::new(seed | 1),
            tokens: true,
            wait_scale: 1.0,
            region: String::new(),
            account: String::new(),
        }
    }

    /// The next random 64 bits (xorshift64*: enough for jitter and identifiers, not for secrets).
    #[must_use]
    pub fn next_u64(&self) -> u64 {
        let mut x = self.rng.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng.set(x);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `[0, 1)`.
    #[must_use]
    pub fn unit(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let x = (self.next_u64() >> 11) as f64;
        x / (1u64 << 53) as f64
    }

    /// A version 4 UUID.
    #[must_use]
    pub fn uuid(&self) -> String {
        let (a, b) = (self.next_u64(), self.next_u64());
        let mut bytes = [0u8; 16];
        let (left, right) = bytes.split_at_mut(8);
        left.copy_from_slice(&a.to_be_bytes());
        right.copy_from_slice(&b.to_be_bytes());
        if let Some(v) = bytes.get_mut(6) {
            *v = (*v & 0x0f) | 0x40;
        }
        if let Some(v) = bytes.get_mut(8) {
            *v = (*v & 0x3f) | 0x80;
        }
        let hex = aws_core::hex::encode(&bytes);
        let part = |r: std::ops::Range<usize>| hex.get(r).unwrap_or_default();
        format!(
            "{}-{}-{}-{}-{}",
            part(0..8),
            part(8..12),
            part(12..16),
            part(16..20),
            part(20..32)
        )
    }

    /// A task token: opaque, long, and not guessable from the execution (AWS's are base64 text of about 600 characters).
    #[must_use]
    pub fn token(&self) -> String {
        let mut bytes = Vec::with_capacity(48);
        for _ in 0..6 {
            bytes.extend_from_slice(&self.next_u64().to_be_bytes());
        }
        format!("AQ{}", aws_core::base64::encode(&bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::Env;

    #[test]
    fn identifiers_look_right() {
        let env = Env::new(0, 42);
        let u = env.uuid();
        assert_eq!(u.len(), 36);
        assert_eq!(u.chars().nth(14), Some('4'));
        assert_ne!(env.uuid(), u);
        let x = env.unit();
        assert!((0.0..1.0).contains(&x));
        assert!(env.token().starts_with("AQ"));
    }
}
