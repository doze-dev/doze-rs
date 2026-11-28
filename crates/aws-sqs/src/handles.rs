//! Receipt handles: what one is made of, and finding the message it names. A handle carries its message's `ord`, so a delete
//! or a change of visibility is a lookup, not a search through the queue.

use aws_wire::ApiError;

use crate::core::invalid_parameter;
use crate::messages::ranged;
use crate::model::{Message, Queue};

/// How many of its latest receipt handles a message keeps. AWS deletes a message with an earlier receive's handle (recorded,
/// `test_an_old_receipt_handle`) and does not say for how long; its documentation allows that such a delete may not delete. Keeping
/// a few bounds what a message received again and again costs.
pub(crate) const KEPT_HANDLES: usize = 8;

impl Queue {
    /// Deletes the message the receipt handle names. A handle that is well formed but names nothing succeeds: the delete is
    /// idempotent.
    pub(crate) fn delete_handle(&mut self, handle: &str) -> Result<(), ApiError> {
        if !well_formed(handle) {
            return Err(bad_handle());
        }
        if let Some(ord) = self.holder(handle, |m| m.handles.iter().any(|h| h == handle)) {
            self.messages.remove(ord);
        }
        Ok(())
    }

    /// The message `handle` was given to, if `names` accepts it. A handle carries its message's `ord`, so this is a lookup; one
    /// given before handles did (its `ord` part is past every `ord` given) is looked for among all the messages.
    fn holder(&self, handle: &str, names: impl Fn(&Message) -> bool) -> Option<u64> {
        let ord = ord_of(handle);
        if ord < self.messages.next_ord() {
            return self.messages.get(ord).filter(|m| names(m)).map(|m| m.ord);
        }
        self.messages.iter().find(|m| names(m)).map(|m| m.ord)
    }

    /// Moves the deadline of the in-flight message the handle names to `timeout` seconds from `now_ms`.
    pub(crate) fn change_visibility(
        &mut self,
        handle: &str,
        timeout: i64,
        now_ms: i64,
    ) -> Result<(), ApiError> {
        ranged(
            Some(timeout),
            0,
            43_200,
            0,
            "VisibilityTimeout",
            "VisibilityTimeout must be an integer between 0 and 43200.",
        )?;
        if !well_formed(handle) {
            return Err(bad_handle());
        }
        // A message that is gone, or not in flight, is the same answer: the handle is of no use.
        let refused = || {
            invalid_parameter(format!(
                "Value {handle} for parameter ReceiptHandle is invalid. Reason: Message does not exist or is not available for visibility timeout change."
            ))
        };
        // Only the latest handle: AWS refuses an earlier receive's (recorded, `test_an_old_receipt_handle`).
        let ord = self
            .holder(handle, |m| {
                m.handles.last().is_some_and(|h| h == handle) && m.in_flight(now_ms)
            })
            .ok_or_else(refused)?;
        let received_ms = self.messages.get(ord).map_or(now_ms, |m| m.received_ms);
        if now_ms + timeout * 1000 > received_ms + 43_200_000 {
            return Err(invalid_parameter(format!(
                "Value {timeout} for parameter VisibilityTimeout is invalid. Reason: Total VisibilityTimeout for the message is beyond the limit [43200 seconds]."
            )));
        }
        self.messages
            .update(ord, |m| m.visible_at_ms = now_ms + timeout * 1000);
        Ok(())
    }
}

/// A new handle for `m`, kept with the latest few it was given.
pub(crate) fn give_handle(m: &mut Message) -> String {
    let handle = new_handle(m.ord);
    if m.handles.len() >= KEPT_HANDLES {
        m.handles.remove(0);
    }
    m.handles.push(handle.clone());
    handle
}

/// A receipt handle: 48 hex digits, the message's `ord` (16) and 128 random bits (32). AWS's are longer and opaque; what matters
/// is that a client cannot make one up (the random part is checked against the handles the message was given), and the `ord`
/// makes a delete a lookup rather than a search.
pub(crate) fn new_handle(ord: u64) -> String {
    format!("{ord:016x}{}", random_hex::<16>())
}

/// `N` random bytes as hex.
pub(crate) fn random_hex<const N: usize>() -> String {
    let mut bytes = [0u8; N];
    // WHY: with no randomness the bytes are all zeros; a collision costs one wrong-message delete in a test environment that has
    // no entropy source, which is not a place anyone runs this.
    if getrandom::fill(&mut bytes).is_err() {
        bytes = [0; N];
    }
    aws_core::hex::encode(&bytes)
}

/// The `ord` a (well-formed) handle carries.
fn ord_of(handle: &str) -> u64 {
    handle
        .get(..16)
        .and_then(|h| u64::from_str_radix(h, 16).ok())
        .unwrap_or(u64::MAX)
}

fn well_formed(handle: &str) -> bool {
    handle.len() == 48 && handle.bytes().all(|b| b.is_ascii_hexdigit())
}

fn bad_handle() -> ApiError {
    ApiError::modeled(
        "ReceiptHandleIsInvalid",
        "The input receipt handle is invalid.",
    )
}
