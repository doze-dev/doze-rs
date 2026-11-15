//! What a client has used, measured as Kafka measures it, to the millisecond and the floating-point step.
//!
//! [`Rate`] is Kafka's `Rate` over a `WindowedSum` (`clients/.../metrics/stats/Rate.java`, `SampledStat.java`): the sum of
//! the values recorded in the last few windows, divided by the time those windows span. [`TokenBucket`] is
//! `TokenBucket.java`, what the controller mutation quota is held to (KIP-599). Each takes the time as an argument and
//! reads no clock, so a test drives it with any times it likes.

/// `quota.window.num` and `controller.quota.window.num`: how many windows a rate is measured over (Kafka's default).
pub(crate) const SAMPLES: usize = 11;
/// `quota.window.size.seconds` and `controller.quota.window.size.seconds`, in milliseconds (Kafka's default, one second).
pub(crate) const WINDOW_MS: i64 = 1000;

/// One window of a [`Rate`]: `SampledStat.Sample`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    start_ms: i64,
    last_event_ms: i64,
    value: f64,
    events: u64,
}

impl Sample {
    fn new(now: i64) -> Self {
        Self {
            start_ms: now,
            last_event_ms: now,
            value: 0.0,
            events: 0,
        }
    }

    /// `Sample.isComplete`: a window is over once it is a window old (the event-count bound is `Long.MAX_VALUE`).
    fn is_complete(&self, now: i64) -> bool {
        now - self.start_ms >= WINDOW_MS
    }
}

/// A rate per second over the last [`SAMPLES`] windows: `Rate` with a `WindowedSum`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Rate {
    /// At most `SAMPLES + 1`: Kafka keeps one more than the windows it measures, for the one that overlaps the start of
    /// the measured span (`SampledStat.advance`).
    samples: Vec<Sample>,
    current: usize,
}

impl Rate {
    /// `SampledStat.record`: adds `value` to the window `now` falls in, starting a new window if the current one is over.
    pub(crate) fn record(&mut self, value: f64, now: i64) {
        if self.current_is_complete(now) {
            self.advance(now);
        }
        if let Some(sample) = self.samples.get_mut(self.current) {
            sample.value += value;
            sample.events += 1;
            sample.last_event_ms = now;
        }
    }

    fn current_is_complete(&mut self, now: i64) -> bool {
        if self.samples.is_empty() {
            self.samples.push(Sample::new(now));
        }
        self.samples
            .get(self.current)
            .is_some_and(|s| s.is_complete(now))
    }

    fn advance(&mut self, now: i64) {
        self.current = (self.current + 1) % (SAMPLES + 1);
        match self.samples.get_mut(self.current) {
            Some(sample) => *sample = Sample::new(now),
            None => self.samples.push(Sample::new(now)),
        }
    }

    /// `Rate.measure`: the sum of the windows still in the span, per second of the span.
    pub(crate) fn measure(&mut self, now: i64) -> f64 {
        self.purge(now);
        let sum: f64 = self.samples.iter().map(|s| s.value).sum();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a span of milliseconds, far below 2^52, as Kafka divides it"
        )]
        let seconds = self.window_size(now) as f64 / 1000.0;
        sum / seconds
    }

    /// `Rate.windowSize`: the time from the start of the oldest window still in the span to now, and never less than
    /// `SAMPLES - 1` whole windows, so that a client's first burst is measured over the whole span and not over the
    /// moment it took.
    pub(crate) fn window_size(&mut self, now: i64) -> i64 {
        self.purge(now);
        if self.samples.is_empty() {
            self.samples.push(Sample::new(now));
        }
        let oldest = self.samples.iter().map(|s| s.start_ms).min().unwrap_or(now);
        let mut elapsed = now - oldest;
        let full = elapsed / WINDOW_MS;
        #[expect(clippy::cast_possible_wrap, reason = "SAMPLES is 11")]
        let least = SAMPLES as i64 - 1;
        if full < least {
            elapsed += (least - full) * WINDOW_MS;
        }
        elapsed.max(1)
    }

    /// `SampledStat.purgeObsoleteSamples`: a window with no event in the whole span starts again, empty, now.
    fn purge(&mut self, now: i64) {
        #[expect(clippy::cast_possible_wrap, reason = "SAMPLES is 11")]
        let expire = SAMPLES as i64 * WINDOW_MS;
        for sample in &mut self.samples {
            if now - sample.last_event_ms >= expire {
                *sample = Sample::new(now);
            }
        }
    }
}

/// How long a client must wait for its rate to come back to `bound`: `QuotaUtils.throttleTime`. If the rate measured is
/// `O` over a span `W`, the wait `X` is the one for which `O * W / (W + X)` is the bound: `X = (O - bound) / bound * W`,
/// rounded as Java's `Math.round` rounds.
pub(crate) fn throttle_ms(measured: f64, bound: f64, window_ms: i64) -> i64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a span of milliseconds, far below 2^52"
    )]
    let wait = (measured - bound) / bound * window_ms as f64;
    java_round(wait)
}

/// Java's `Math.round(double)`: the nearest whole number, halves rounded up (towards positive infinity), clamped to `long`.
pub(crate) fn java_round(x: f64) -> i64 {
    let floor = x.floor();
    let nearest = if x - floor >= 0.5 { floor + 1.0 } else { floor };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Math.round clamps to the long range; `as` saturates the same way and maps NaN to 0, as Java does"
    )]
    let rounded = nearest as i64;
    rounded
}

/// A token bucket: `TokenBucket.java`. Tokens come back at `bound` per second up to a burst of `SAMPLES` windows'
/// worth; a mutation takes as many tokens as it changes partitions, and the bucket may go below zero. The quota is
/// exhausted while it is below zero.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct TokenBucket {
    tokens: f64,
    last_update_ms: i64,
}

impl TokenBucket {
    /// `TokenBucket.measure`: the tokens there are now.
    pub(crate) fn measure(&mut self, bound: f64, now: i64) -> f64 {
        self.refill(bound, now);
        self.tokens
    }

    /// `TokenBucket.record`: takes `value` tokens.
    pub(crate) fn record(&mut self, value: f64, bound: f64, now: i64) {
        self.refill(bound, now);
        self.tokens = burst(bound).min(self.tokens - value);
    }

    fn refill(&mut self, bound: f64, now: i64) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "milliseconds since the last update, as Kafka converts them"
        )]
        let seconds = (now - self.last_update_ms) as f64 / 1000.0;
        self.tokens = burst(bound).min(self.tokens + bound * seconds);
        self.last_update_ms = now;
    }
}

/// `SAMPLES * window * bound`: the most tokens the bucket holds.
fn burst(bound: f64) -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "11 windows of 1000 ms")]
    let span = (SAMPLES as i64 * WINDOW_MS) as f64 / 1000.0;
    span * bound
}

/// How long until an exhausted bucket is back at zero: `ControllerMutationQuotaManager.throttleTimeMs`.
pub(crate) fn token_throttle_ms(tokens: f64, bound: f64) -> i64 {
    java_round(-tokens / bound * 1000.0)
}

#[cfg(test)]
mod tests;
