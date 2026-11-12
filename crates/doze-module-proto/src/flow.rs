//! Credit-based flow control for streams (protocol.md row 10). The receiver grants items; the sender may only send
//! while it holds credit. Sans-IO: the peer turns grants into `credit` frames and waits on [`Outbound::take`].

/// How many items a receiver grants when a stream opens, and tops up to.
pub const WINDOW: u32 = 64;

/// The sender's side of one stream.
#[derive(Debug, Default)]
pub struct Outbound {
    credit: u32,
}

impl Outbound {
    /// A stream opens with the receiver's initial window.
    pub fn new() -> Outbound {
        Outbound { credit: WINDOW }
    }

    /// The receiver granted `n` more.
    pub fn grant(&mut self, n: u32) {
        self.credit = self.credit.saturating_add(n);
    }

    /// Takes one item's credit; `false` means wait for a grant.
    pub fn take(&mut self) -> bool {
        if self.credit == 0 {
            return false;
        }
        self.credit -= 1;
        true
    }
}

/// The receiver's side of one stream.
#[derive(Debug)]
pub struct Inbound {
    outstanding: u32,
}

impl Default for Inbound {
    fn default() -> Inbound {
        Inbound::new()
    }
}

impl Inbound {
    /// The initial window is granted implicitly when the stream opens.
    pub fn new() -> Inbound {
        Inbound {
            outstanding: WINDOW,
        }
    }

    /// An item arrived. Returns `false` if the sender exceeded its credit (a protocol violation).
    pub fn arrived(&mut self) -> bool {
        if self.outstanding == 0 {
            return false;
        }
        self.outstanding -= 1;
        true
    }

    /// The consumer took an item off the queue. Returns a grant to send once half the window is used up, so a fast
    /// consumer sends one credit frame per half window and a stalled one sends none.
    pub fn consumed(&mut self, queued: u32) -> Option<u32> {
        let in_flight = self.outstanding + queued;
        if in_flight <= WINDOW / 2 {
            let grant = WINDOW - in_flight;
            self.outstanding += grant;
            return Some(grant);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A producer far faster than its consumer never has more than one window in flight, and never stalls for good.
    #[test]
    fn a_fast_sender_is_held_to_the_window_and_a_slow_reader_keeps_it_moving() {
        let (mut tx, mut rx) = (Outbound::new(), Inbound::new());
        let mut queue: u32 = 0;
        let (mut sent, mut read, mut max_queue) = (0u32, 0u32, 0u32);
        for step in 0..100_000u32 {
            if read == 5_000 {
                break;
            }
            // The sender pushes as hard as credit allows.
            while sent < 5_000 && tx.take() {
                assert!(rx.arrived(), "sent beyond credit");
                queue += 1;
                sent += 1;
            }
            max_queue = max_queue.max(queue);
            // The reader takes one item every third step.
            if step % 3 == 0 && queue > 0 {
                queue -= 1;
                read += 1;
                if let Some(n) = rx.consumed(queue) {
                    tx.grant(n);
                }
            }
        }
        assert_eq!(read, 5_000, "the stream stalled");
        assert!(max_queue <= WINDOW, "queue reached {max_queue}");
    }

    #[test]
    fn an_item_without_credit_is_a_violation() {
        let mut rx = Inbound::new();
        for _ in 0..WINDOW {
            assert!(rx.arrived());
        }
        assert!(!rx.arrived());
    }
}
