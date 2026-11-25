//! One finished call as the ring holds it, and the pieces of it (what made a call between services; the words of an error).

use std::num::NonZeroU32;

use aws_peers::summary::Summary;

use super::RECORD_BYTES;

/// One finished call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Its number: 1 for the first call of this process.
    pub seq: u64,
    /// When it was answered, in milliseconds since 1970.
    pub at_ms: u64,
    /// From its head being read to its answer being made, in microseconds (a call over 71 minutes says 71 minutes).
    pub took_us: u32,
    /// The HTTP status answered.
    pub status: u16,
    /// `--iam soft` would have refused it and let it through.
    pub would_be_denied: bool,
    /// The service's name in a credential scope; empty when the request named no service of this build.
    pub signing_name: &'static str,
    /// The service's SDK id (`DynamoDB Streams`).
    pub sdk_id: &'static str,
    /// The operation; empty when the request was refused before one was found.
    pub action: &'static str,
    pub request_id: Box<str>,
    /// The access key id the request was signed with.
    pub caller: Box<str>,
    pub region: Box<str>,
    /// The error code answered; empty for a success.
    pub code: Box<str>,
    /// The input member that named what was acted on (`QueueUrl`); empty for an S3 path and when nothing was named.
    pub member: &'static str,
    /// What was acted on, when it was named; empty otherwise.
    pub resource: Box<str>,
    /// A call one service made to another on the bus, not a request from the wire: who made it and why.
    pub via: Option<Box<Via>>,
    /// The client that sent it, from its `User-Agent` ([`crate::agent::label`]: `boto3 1.34.69`); empty for a call between
    /// services and for a request that named no client.
    pub sdk: Box<str>,
    /// What an error answer or a failed function says, held only by the calls that have any (a success holds nothing of it, so the
    /// slot stays small).
    pub extra: Option<Box<Extra>>,
    /// The length the answer declared, in bytes, at most 65,534 (a longer one says so); [`u16::MAX`] when it declared none (a streamed
    /// answer, a call between services).
    pub answer_bytes: u16,
    /// What the call did, in a few numbers (`3 messages`); [`Summary::NONE`] for an operation that has none and for a call that failed.
    pub summary: Summary,
}

/// What a Lab rule did to a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabDid {
    /// The call waited this many milliseconds before it was served.
    Delayed(u32),
    /// It was answered with an error and the service never saw it.
    Failed,
    /// The connection was closed with no answer.
    Dropped,
}

/// The Lab rule that touched a call: kept on its row, never folded into another. The rule's name is not kept here (a ring row is
/// bounded to under a mebibyte in all): the API asks the table for it. Eight bytes: the rule's number, and what it did packed into a
/// number of milliseconds (a wait) or one of two values no wait reaches (an error, a closed connection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lab {
    rule: NonZeroU32,
    packed: u32,
}

const FAILED: u32 = u32::MAX;
const DROPPED: u32 = u32::MAX - 1;

impl Lab {
    /// The mark of rule `rule` having done `did`; `None` for rule 0, which no table makes.
    #[must_use]
    pub fn new(rule: u32, did: LabDid) -> Option<Self> {
        let packed = match did {
            LabDid::Delayed(ms) => ms.min(DROPPED - 1),
            LabDid::Failed => FAILED,
            LabDid::Dropped => DROPPED,
        };
        Some(Self {
            rule: NonZeroU32::new(rule)?,
            packed,
        })
    }

    /// The rule's number.
    #[must_use]
    pub fn rule(self) -> u32 {
        self.rule.get()
    }

    /// What it did.
    #[must_use]
    pub fn did(self) -> LabDid {
        match self.packed {
            FAILED => LabDid::Failed,
            DROPPED => LabDid::Dropped,
            ms => LabDid::Delayed(ms),
        }
    }
}

/// The words of an error answer, a function invocation's own failure, and the Lab rule that touched the call.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Extra {
    /// The words an error answer carried (the service's own). On a call that succeeded, where there are none, the short text its
    /// summary carries (an index, a session name, the role an attach was for): the two never meet, so one slot holds either.
    pub message: Box<str>,
    /// `Unhandled` or `Handled` when a function invocation answered 200 and the function failed.
    pub function_error: Box<str>,
    /// The Lab rule that touched the call.
    pub lab: Option<Lab>,
}

/// What made a call between services.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Via {
    /// The service that made it, as it named itself on the bus (`sns`); empty for one made as the caller itself (a Step
    /// Functions task as its role).
    pub by: Box<str>,
    /// The id of the request from the wire being answered when it was made; empty when none was (a rule, a schedule).
    pub request: Box<str>,
    /// The rule, subscription, mapping or schedule that made it; empty when the service did not say.
    pub name: Box<str>,
}

impl Via {
    fn bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.by.len() + self.request.len() + self.name.len()
    }
}

impl Call {
    /// The bytes this record holds: its slot and its text.
    #[must_use]
    pub fn bytes(&self) -> usize {
        RECORD_BYTES
            + self.request_id.len()
            + self.caller.len()
            + self.region.len()
            + self.code.len()
            + self.resource.len()
            + self.via.as_ref().map_or(0, |v| v.bytes())
            + self.sdk.len()
            + self.extra.as_ref().map_or(0, |e| {
                std::mem::size_of::<Extra>() + e.message.len() + e.function_error.len()
            })
    }
}

/// `text`, cut at the last character boundary at or before `most` bytes.
pub(super) fn cut(text: &str, most: usize) -> Box<str> {
    if text.len() <= most {
        return text.into();
    }
    let mut end = most;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default().into()
}
